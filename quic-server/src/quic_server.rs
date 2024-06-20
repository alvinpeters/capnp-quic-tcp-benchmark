use std::sync::Arc;
use quinn::crypto::rustls::QuicServerConfig;
use quinn::{Endpoint, Incoming, RecvStream, SendStream};
use rustls::crypto::aws_lc_rs;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use protos::key_cert_bytes::{CERT, KEY};
use anyhow::{Error, Result};
use crate::{Opt};

pub async fn get_quic_server(ctx_opts: &Opt) -> Result<Endpoint> {
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
    Ok(Endpoint::server(server_config, ctx_opts.listen)?)
}

pub async fn handle_conn(conn: Incoming) -> Result<Option<(SendStream, RecvStream)>> {
    let connection = conn.await?;
    println!("established");
    loop {
        // Wait for connection
        return match connection.accept_bi().await {
            Err(quinn::ConnectionError::ApplicationClosed { .. }) => {
                println!("connection closed");
                Ok(None)
            }
            Err(e) => {
                Err(Error::from(e))
            }
            Ok(s) => Ok(Some(s)),
        }
    }
}