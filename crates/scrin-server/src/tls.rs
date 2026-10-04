//! TLS material for the TCP listener and the WebTransport (QUIC) listener.
//!
//! - Self-signed: one ECDSA P-256 certificate, valid 14 days, shared by both
//!   listeners. Browsers accept it through `serverCertificateHashes`, which
//!   requires exactly these properties (P-256, validity <= 14 days).
//! - Manual: PEM chain + key from disk, both listeners.
//! - ACME: TLS-ALPN-01 on the TCP listener; the QUIC listener resolves the
//!   same certificate through the ACME resolver once it is issued.

use std::path::Path;
use std::sync::Arc;

use rustls::ServerConfig;
use rustls::server::ResolvesServerCert;
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use sha2::{Digest, Sha256};

/// ALPN the TCP listener offers (the relay and API speak HTTP/1.1 only).
pub const HTTP1_ALPN: &[u8] = b"http/1.1";
/// WebTransport over HTTP/3.
pub const H3_ALPN: &[u8] = b"h3";

#[derive(Debug, thiserror::Error)]
pub enum TlsError {
    #[error("certificate generation: {0}")]
    Generate(#[from] rcgen::Error),
    #[error("reading {path}: {source}")]
    Pem {
        path: String,
        source: rustls_pki_types::pem::Error,
    },
    #[error("tls config: {0}")]
    Rustls(#[from] rustls::Error),
    #[error("no certificate in {0}")]
    Empty(String),
}

/// A certificate chain + key, cloneable for both listeners.
#[derive(Debug)]
pub struct CertKey {
    pub chain: Vec<CertificateDer<'static>>,
    pub key: PrivateKeyDer<'static>,
}

impl Clone for CertKey {
    fn clone(&self) -> Self {
        Self {
            chain: self.chain.clone(),
            key: self.key.clone_key(),
        }
    }
}

impl CertKey {
    /// SHA-256 of the leaf certificate (for `serverCertificateHashes`).
    #[must_use]
    pub fn leaf_sha256(&self) -> Option<[u8; 32]> {
        self.chain
            .first()
            .map(|c| Sha256::digest(c.as_ref()).into())
    }
}

/// Self-signed P-256 certificate for `names` (DNS names or IP literals), valid
/// from one hour ago for 13 days (browsers cap hash-pinned certs at 14).
pub fn self_signed(names: &[String]) -> Result<CertKey, TlsError> {
    let mut params = rcgen::CertificateParams::new(names.to_vec())?;
    let now = time::OffsetDateTime::now_utc();
    params.not_before = now - time::Duration::hours(1);
    params.not_after = now + time::Duration::days(13);
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "scrin-server dev");
    let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)?;
    let cert = params.self_signed(&key)?;
    Ok(CertKey {
        chain: vec![cert.der().clone()],
        key: PrivateKeyDer::try_from(key.serialize_der())
            .map_err(|e| TlsError::Rustls(rustls::Error::General(e.to_owned())))?,
    })
}

pub fn load_pem(cert: &Path, key: &Path) -> Result<CertKey, TlsError> {
    let chain = CertificateDer::pem_file_iter(cert)
        .and_then(Iterator::collect::<Result<Vec<_>, _>>)
        .map_err(|source| TlsError::Pem {
            path: cert.display().to_string(),
            source,
        })?;
    if chain.is_empty() {
        return Err(TlsError::Empty(cert.display().to_string()));
    }
    let key = PrivateKeyDer::from_pem_file(key).map_err(|source| TlsError::Pem {
        path: key.display().to_string(),
        source,
    })?;
    Ok(CertKey { chain, key })
}

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// TLS 1.2/1.3 server config for the TCP listener (HTTP/1.1).
pub fn tcp_config(ck: &CertKey) -> Result<ServerConfig, TlsError> {
    let mut cfg = ServerConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(ck.chain.clone(), ck.key.clone_key())?;
    cfg.alpn_protocols = vec![HTTP1_ALPN.to_vec()];
    Ok(cfg)
}

/// TLS 1.3-only server config for WebTransport (ALPN `h3`).
pub fn quic_config(ck: &CertKey) -> Result<ServerConfig, TlsError> {
    let mut cfg = ServerConfig::builder_with_provider(provider())
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_no_client_auth()
        .with_single_cert(ck.chain.clone(), ck.key.clone_key())?;
    cfg.alpn_protocols = vec![H3_ALPN.to_vec()];
    Ok(cfg)
}

/// TLS 1.3-only config for WebTransport that takes certificates from a
/// resolver (the ACME resolver).
pub fn quic_config_with_resolver(
    resolver: Arc<dyn ResolvesServerCert>,
) -> Result<ServerConfig, TlsError> {
    let mut cfg = ServerConfig::builder_with_provider(provider())
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_no_client_auth()
        .with_cert_resolver(resolver);
    cfg.alpn_protocols = vec![H3_ALPN.to_vec()];
    Ok(cfg)
}

/// `aa:bb:...` form of a hash (what browsers' devtools show).
#[must_use]
pub fn colon_hex(h: &[u8]) -> String {
    h.iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_signed_cert_builds_both_configs() {
        let ck = self_signed(&["localhost".into(), "127.0.0.1".into()]).expect("cert");
        assert_eq!(ck.chain.len(), 1);
        assert!(ck.leaf_sha256().is_some());
        let tcp = tcp_config(&ck).expect("tcp");
        assert_eq!(tcp.alpn_protocols, vec![HTTP1_ALPN.to_vec()]);
        let quic = quic_config(&ck).expect("quic");
        assert_eq!(quic.alpn_protocols, vec![H3_ALPN.to_vec()]);
    }

    #[test]
    fn colon_hex_formats() {
        assert_eq!(colon_hex(&[0, 0xab, 0x10]), "00:ab:10");
    }
}
