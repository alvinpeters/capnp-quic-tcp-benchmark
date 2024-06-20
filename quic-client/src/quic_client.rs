use std::sync::Arc;
use anyhow::{Result};
use quinn::crypto::rustls::QuicClientConfig;
use quinn::Endpoint;
use rustls::pki_types::CertificateDer;
use protos::key_cert_bytes::CERT;
use crate::Opt;

pub(crate) async fn get_quic_client(ctx_opts: &Opt) -> Result<Endpoint> {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(CertificateDer::from(CertificateDer::from(CERT)))?;
    let client_crypto = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();

    let client_config =
        quinn::ClientConfig::new(Arc::new(QuicClientConfig::try_from(client_crypto)?));
    let mut endpoint = Endpoint::client(ctx_opts.bind)?;
    endpoint.set_default_client_config(client_config);
    Ok(endpoint)
}