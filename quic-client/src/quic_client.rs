use std::net::ToSocketAddrs;
use std::sync::Arc;
use anyhow::{anyhow, Result};
use quinn::crypto::rustls::QuicClientConfig;
use quinn::Endpoint;
use rustls::pki_types::CertificateDer;
use protos::key_cert_bytes::CERT;
use crate::Opt;

pub(crate) async fn get_quic_client(ctx_opts: &Opt) -> Result<Endpoint> {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(CertificateDer::from(CertificateDer::from(CERT)))?;
    let mut client_crypto = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    client_crypto.alpn_protocols = vec![b"h3".to_vec()];
    if ctx_opts.keylog {
        client_crypto.key_log = Arc::new(rustls::KeyLogFile::new());
    }

    let client_config =
        quinn::ClientConfig::new(Arc::new(QuicClientConfig::try_from(client_crypto)?));
    let mut endpoint = Endpoint::client(ctx_opts.bind)?;
    endpoint.set_default_client_config(client_config);
    Ok(endpoint)
}