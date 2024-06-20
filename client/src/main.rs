//! This example demonstrates an HTTP client that requests files from a server.
//!
//! Checkout the `README.md` for guidance.
use capnp_futures::{serialize, serialize_packed};
use proto::proto_capnp::*;


use std::{
    fs,
    io::{self, Write},
    net::{SocketAddr, ToSocketAddrs},
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Result};
use clap::Parser;
use quinn::crypto::rustls::QuicClientConfig;
use quinn::SendStream;
use rustls::crypto::aws_lc_rs;
use rustls::pki_types::CertificateDer;
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};
use tracing::{error, info};
use url::Url;
use proto::key_cert_bytes::CERT;

/// HTTP/0.9 over QUIC client
#[derive(Parser, Debug)]
#[clap(name = "client")]
struct Opt {
    /// Perform NSS-compatible TLS key logging to the file specified in `SSLKEYLOGFILE`.
    #[clap(long = "keylog")]
    keylog: bool,
    url: Url,

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
    let url = options.url;
    let url_host = url.host_str().unwrap();
    let remote = (url_host, url.port().unwrap_or(4433))
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| anyhow!("couldn't resolve to an address"))?;

    let mut roots = rustls::RootCertStore::empty();
    roots.add(CertificateDer::from(CertificateDer::from(CERT)))?;
    let mut client_crypto = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();


    client_crypto.alpn_protocols = vec![b"h3".to_vec()];
    if options.keylog {
        client_crypto.key_log = Arc::new(rustls::KeyLogFile::new());
    }

    let client_config =
        quinn::ClientConfig::new(Arc::new(QuicClientConfig::try_from(client_crypto)?));
    let mut endpoint = quinn::Endpoint::client(options.bind)?;
    endpoint.set_default_client_config(client_config);

    let start = Instant::now();
    let rebind = options.rebind;
    let host = options.host.as_deref().unwrap_or(url_host);

    eprintln!("connecting to {host} at {remote}");
    let conn = endpoint
        .connect(remote, host)?
        .await
        .map_err(|e| anyhow!("failed to connect: {}", e))?;
    eprintln!("connected at {:?}", start.elapsed());
    let (mut send, mut recv) = conn
        .open_bi()
        .await
        .map_err(|e| anyhow!("failed to open stream: {}", e))?;
    if rebind {
        let socket = std::net::UdpSocket::bind("[::]:0").unwrap();
        let addr = socket.local_addr().unwrap();
        eprintln!("rebinding to {addr}");
        endpoint.rebind(socket).expect("rebind failed");
    }
    let mut compat_send = send.compat_write();
    write_addressbook(&mut compat_send).await?;
    compat_send.get_mut().finish().unwrap();
    let response_start = Instant::now();
    eprintln!("request sent at {:?}", response_start - start);
    let resp = recv
        .read_to_end(usize::MAX)
        .await
        .map_err(|e| anyhow!("failed to read response: {}", e))?;
    let duration = response_start.elapsed();
    eprintln!(
        "response received in {:?} - {} KiB/s",
        duration,
        resp.len() as f32 / (duration_secs(&duration) * 1024.0)
    );
    conn.close(0u32.into(), b"done");

    // Give the server a fair chance to receive the close packet
    endpoint.wait_idle().await;

    Ok(())
}

fn duration_secs(x: &Duration) -> f32 {
    x.as_secs() as f32 + x.subsec_nanos() as f32 * 1e-9
}

async fn write_addressbook(mut stream: &mut Compat<SendStream>) -> Result<()> {
    let mut message = ::capnp::message::Builder::new_default();
    {
        let address_book = message.init_root::<address_book::Builder>();

        let mut people = address_book.init_people(2);
        {
            let mut alice = people.reborrow().get(0);
            alice.set_id(123);
            alice.set_name("Alice");
            alice.set_email("alice@example.com");
            {
                let mut alice_phones = alice.reborrow().init_phones(1);
                alice_phones.reborrow().get(0).set_number("555-1212");
                alice_phones
                    .reborrow()
                    .get(0)
                    .set_type(person::phone_number::Type::Mobile);
            }
            alice.get_employment().set_school("MIT");
        }

        {
            let mut bob = people.get(1);
            bob.set_id(456);
            bob.set_name("Bob");
            bob.set_email("bob@example.com");
            {
                let mut bob_phones = bob.reborrow().init_phones(2);
                bob_phones.reborrow().get(0).set_number("555-4567");
                bob_phones
                    .reborrow()
                    .get(0)
                    .set_type(person::phone_number::Type::Home);
                bob_phones.reborrow().get(1).set_number("555-7654");
                bob_phones
                    .reborrow()
                    .get(1)
                    .set_type(person::phone_number::Type::Work);
            }
            bob.get_employment().set_unemployed(());
        }
    }

    Ok(serialize::write_message(&mut stream, &message).await?)
}
