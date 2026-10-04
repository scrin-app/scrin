//! Cryptographic building blocks for scrin.
//!
//! - [`identity`]: the per-install Ed25519 device key (also the iroh endpoint id).
//! - [`code`]: one-time connection codes shown by the host.
//! - [`pake`]: SPAKE2 over the already-encrypted QUIC channel, bound to both
//!   endpoint ids, so a short code can't be attacked offline and the
//!   rendezvous server can't sit in the middle.
//! - [`sas`]: short authentication string (emoji) both users can compare.
//! - [`trust`]: MAC-sealed list of controllers allowed unattended access.

pub mod code;
pub mod identity;
pub mod pake;
pub mod sas;
pub mod trust;

/// Protocol label bound into every derivation. Changing it breaks pairing
/// with older peers on purpose.
pub const PROTOCOL: &str = "scrin/1";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("random source unavailable: {0}")]
    Random(String),
    #[error("pairing failed: wrong code or tampered handshake")]
    PairingFailed,
    #[error("malformed input: {0}")]
    Malformed(&'static str),
    #[error("trust store integrity check failed")]
    TrustStoreTampered,
    #[error("serialization: {0}")]
    Serde(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn random_bytes<const N: usize>() -> Result<[u8; N]> {
    let mut buf = [0u8; N];
    getrandom::fill(&mut buf).map_err(|e| Error::Random(e.to_string()))?;
    Ok(buf)
}
