use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use anyhow::{Result, Error};
use capnp_futures::serialize::read_message;
use proto::key_cert_bytes::*;
use clap::Parser;
use quinn::crypto::rustls::QuicServerConfig;
use quinn::{Incoming, RecvStream, SendStream};
use rustls::crypto::aws_lc_rs;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio_util::compat::{TokioAsyncReadCompatExt};
use proto::proto_capnp::{address_book, person};

#[derive(Parser, Debug)]
#[clap(name = "server")]
struct Opt {
    /// file to log TLS keys to for debugging
    #[clap(long = "keylog")]
    keylog: bool,
    /// Enable stateless retries
    #[clap(long = "stateless-retry")]
    stateless_retry: bool,
    /// Address to listen on
    #[clap(long = "listen", default_value = "127.0.0.1:4433")]
    listen: SocketAddr,
    /// Maximum number of concurrent connections to allow
    #[clap(long = "connection-limit")]
    connection_limit: Option<usize>,
}

#[tokio::main]
async fn main() {
    println!("Starting server!");
    let opt = Opt::parse();
    let code = {
        if let Err(e) = listen(opt).await {
            eprintln!("ERROR: {e}");
            1
        } else {
            0
        }
    };
    std::process::exit(code);
}

async fn listen(ctx_opts: Opt) -> Result<()> {
    let certs = vec![CertificateDer::try_from(CERT).unwrap()];
    let key = PrivateKeyDer::try_from(KEY).unwrap();
    aws_lc_rs::default_provider().install_default().unwrap();
    let mut server_crypto = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;
    server_crypto.alpn_protocols = vec![b"h3".to_vec()];
    if ctx_opts.keylog {
        server_crypto.key_log = Arc::new(rustls::KeyLogFile::new());
    }
    let mut server_config =
        quinn::ServerConfig::with_crypto(Arc::new(QuicServerConfig::try_from(server_crypto)?));
    let transport_config = Arc::get_mut(&mut server_config.transport).unwrap();
    transport_config.max_concurrent_uni_streams(0_u8.into());
    let endpoint = quinn::Endpoint::server(server_config, ctx_opts.listen)?;
    println!("listening on {}", endpoint.local_addr()?);
    while let Some(conn) = endpoint.accept().await {
        if ctx_opts
            .connection_limit
            .map_or(false, |n| endpoint.open_connections() >= n)
        {
            println!("refusing due to open connection limit");
            conn.refuse();
        } else if ctx_opts.stateless_retry && !conn.remote_address_validated() {
            println!("requiring connection to validate its address");
            conn.retry().unwrap();
        } else {
            println!("accepting connection");
            let fut = handle_connection(conn);
            tokio::spawn(async move {
                if let Err(e) = fut.await {
                    eprintln!("connection failed: {reason}", reason = e.to_string())
                }
            });
        }
    }
    Ok(())
}

async fn handle_connection(conn: Incoming) -> Result<()> {
    let connection = conn.await?;
    println!("established");
    loop {
        // Wait for connection
        let streams = match connection.accept_bi().await {
            Err(quinn::ConnectionError::ApplicationClosed { .. }) => {
                println!("connection closed");
                return Ok(());
            }
            Err(e) => {
                return Err(Error::from(e));
            }
            Ok(s) => s,
        };
        let fut = print_addressbook(streams);
        tokio::spawn(
            async move {
                if let Err(e) = fut.await {
                    eprintln!("failed: {reason}", reason = e.to_string());
                }
            }
        );
    }
}

async fn print_addressbook((mut send_stream, mut receive_stream): (SendStream, RecvStream)) -> Result<()> {

    let message_reader
        = read_message(&mut receive_stream.compat(), capnp::message::ReaderOptions::new()).await?;
    let address_book = message_reader.get_root::<address_book::Reader>()?;
    for person in address_book.get_people()? {
        println!(
            "{}: {}",
            person.get_name()?.to_str()?,
            person.get_email()?.to_str()?
        );
        for phone in person.get_phones()? {
            let type_name = match phone.get_type() {
                Ok(person::phone_number::Type::Mobile) => "mobile",
                Ok(person::phone_number::Type::Home) => "home",
                Ok(person::phone_number::Type::Work) => "work",
                Err(::capnp::NotInSchema(_)) => "UNKNOWN",
            };
            println!("  {} phone: {}", type_name, phone.get_number()?.to_str()?);
        }
        match person.get_employment().which() {
            Ok(person::employment::Unemployed(())) => {
                println!("  unemployed");
            }
            Ok(person::employment::Employer(employer)) => {
                println!("  employer: {}", employer?.to_str()?);
            }
            Ok(person::employment::School(school)) => {
                println!("  student at: {}", school?.to_str()?);
            }
            Ok(person::employment::SelfEmployed(())) => {
                println!("  self-employed");
            }
            Err(::capnp::NotInSchema(_)) => {}
        }
    }
    Ok(())
}
