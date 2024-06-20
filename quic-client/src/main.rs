//! This example demonstrates an HTTP client that requests files from a server.
//!
//! Checkout the `README.md` for guidance.
mod quic_client;
mod capnp_client;

use capnp_futures::{serialize};
use protos::addressbook_capnp::{address_book, person};


use std::{
    net::{SocketAddr, ToSocketAddrs},
    path::PathBuf,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Result};
use clap::Parser;
use quinn::SendStream;
use rustls::crypto::aws_lc_rs;
use tokio::task::{LocalSet};
use tokio_util::compat::{Compat};
use url::Url;
use crate::capnp_client::connect_rpc_server;
use crate::quic_client::get_quic_client;

/// HTTP/0.9 over QUIC client
#[derive(Parser, Debug)]
#[clap(name = "client")]
pub(crate) struct Opt {
    /// Perform NSS-compatible TLS key logging to the file specified in `SSLKEYLOGFILE`.
    #[clap(long = "keylog")]
    keylog: bool,
    url: Option<String>,

    /// Override hostname used for certificate verification
    #[clap(long = "host")]
    host: Option<String>,

    /// Custom certificate authority to trust, in DER format
    #[clap(long = "ca")]
    ca: Option<PathBuf>,

    /// Simulate NAT rebinding after connecting
    #[clap(long = "rebind")]
    rebind: bool,

    /// Address to bind on
    #[clap(long = "bind", default_value = "[::]:0")]
    bind: SocketAddr,
}

#[tokio::main]
async fn main() -> Result<()> {
    aws_lc_rs::default_provider().install_default().unwrap();
    let options = Opt::parse();
    let url_host_str = options.url.clone().unwrap_or_else(|| "https://localhost:4433".to_string());
    let url = Url::parse(&url_host_str).unwrap();
    let url_host = url.host().unwrap().to_string();
    let remote = (url_host.clone(), url.port().unwrap_or(4433))
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| anyhow!("couldn't resolve to an address"))?;
    let endpoint = get_quic_client(&options).await?;

    //let start = Instant::now();
    let rebind = options.rebind;
    let host = options.host.as_deref().unwrap_or(&url_host);

    eprintln!("connecting to {host} at {remote}");
    let conn_timer = tokio::time::Instant::now();
    let conn = endpoint
        .connect(remote, host)?
        .await
        .map_err(|e| anyhow!("failed to connect: {}", e))?;
    //eprintln!("connected at {:?}", start.elapsed());

    let (send, recv) = conn
        .open_bi()
        .await
        .map_err(|e| anyhow!("failed to open stream: {}", e))?;
    if rebind {
        let socket = std::net::UdpSocket::bind("[::]:0").unwrap();
        let addr = socket.local_addr().unwrap();
        eprintln!("rebinding to {addr}");
        endpoint.rebind(socket).expect("rebind failed");
    }
    println!("Connection established. Took {}ms", conn_timer.elapsed().as_micros());
    let local = LocalSet::new();
    local.run_until(connect_rpc_server(send, recv)).await?;
    local.await;
    conn.close(0u32.into(), b"done");
    // Give the server a fair chance to receive the close packet
    endpoint.wait_idle().await;

    Ok(())
}
