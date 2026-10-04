//! Signed rendezvous requests.
//!
//! Every mutating RPC is signed by the device key it concerns. The signed
//! message is a canonical byte string, never the JSON request itself:
//!
//! ```text
//! label (ASCII) || 0x00 || device_pub (32) || timestamp_secs (u64 BE)
//!               || body_len (u32 BE) || body
//! ```
//!
//! `label` is per operation (see the `LABEL_*` constants) so a signature for
//! one operation can never be replayed as another. `body` is operation
//! specific and documented next to each request type in [`crate::api`].
//! Timestamps more than [`MAX_SKEW_SECS`] away from the server clock are
//! rejected, which bounds replay of a captured request to that window.

use std::time::{SystemTime, UNIX_EPOCH};

use scrin_crypto::identity::DeviceId;

pub const LABEL_REGISTER: &str = "scrin rendezvous register v1";
pub const LABEL_PRESENCE: &str = "scrin rendezvous presence v1";
pub const LABEL_RESOLVE: &str = "scrin rendezvous resolve v1";
pub const LABEL_REPORT_FAILURE: &str = "scrin rendezvous report-failure v1";
pub const LABEL_ABUSE: &str = "scrin rendezvous abuse v1";
/// `POST /v1/locator` (D24); body is empty.
pub const LABEL_LOCATOR: &str = "scrin rendezvous locator v1";
/// `POST /v1/locator/release` (D24); body is empty.
pub const LABEL_LOCATOR_RELEASE: &str = "scrin rendezvous locator-release v1";

/// Accepted clock skew between client and server, both directions.
pub const MAX_SKEW_SECS: u64 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    #[error("malformed {0}")]
    Malformed(&'static str),
    #[error("timestamp outside the accepted window")]
    Expired,
    #[error("bad signature")]
    BadSignature,
}

/// The exact bytes a client signs for `label`.
#[must_use]
pub fn canonical(label: &str, device: &DeviceId, timestamp: u64, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(label.len() + 1 + 32 + 8 + 4 + body.len());
    out.extend_from_slice(label.as_bytes());
    out.push(0);
    out.extend_from_slice(&device.0);
    out.extend_from_slice(&timestamp.to_be_bytes());
    // Bodies are request-sized (a few KB at most); the API caps them first.
    let len = u32::try_from(body.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(body);
    out
}

#[must_use]
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Checks the timestamp window, then the Ed25519 signature.
pub fn verify(
    label: &str,
    device: &DeviceId,
    timestamp: u64,
    body: &[u8],
    signature: &[u8; 64],
    now: u64,
) -> Result<(), AuthError> {
    if timestamp.abs_diff(now) > MAX_SKEW_SECS {
        return Err(AuthError::Expired);
    }
    device
        .verify(&canonical(label, device, timestamp, body), signature)
        .map_err(|_| AuthError::BadSignature)
}

/// Lowercase hex of exactly 32 bytes.
pub fn parse_key(hex: &str) -> Result<DeviceId, AuthError> {
    let raw = data_encoding::HEXLOWER_PERMISSIVE
        .decode(hex.as_bytes())
        .map_err(|_| AuthError::Malformed("device key"))?;
    let bytes: [u8; 32] = raw
        .try_into()
        .map_err(|_| AuthError::Malformed("device key"))?;
    Ok(DeviceId(bytes))
}

/// Lowercase hex of exactly 64 bytes.
pub fn parse_sig(hex: &str) -> Result<[u8; 64], AuthError> {
    let raw = data_encoding::HEXLOWER_PERMISSIVE
        .decode(hex.as_bytes())
        .map_err(|_| AuthError::Malformed("signature"))?;
    raw.try_into()
        .map_err(|_| AuthError::Malformed("signature"))
}

#[cfg(test)]
mod tests {
    use scrin_crypto::identity::Identity;

    use super::*;

    fn sign(id: &Identity, label: &str, ts: u64, body: &[u8]) -> [u8; 64] {
        id.sign(&canonical(label, &id.device_id(), ts, body))
    }

    #[test]
    fn good_signature_verifies() {
        let id = Identity::generate().expect("rng");
        let now = 1_800_000_000;
        let sig = sign(&id, LABEL_REGISTER, now, b"body");
        verify(LABEL_REGISTER, &id.device_id(), now, b"body", &sig, now).expect("valid");
        verify(
            LABEL_REGISTER,
            &id.device_id(),
            now,
            b"body",
            &sig,
            now + 299,
        )
        .expect("skew ok");
    }

    #[test]
    fn tampered_body_or_wrong_key_fails() {
        let id = Identity::generate().expect("rng");
        let other = Identity::generate().expect("rng");
        let now = 1_800_000_000;
        let sig = sign(&id, LABEL_REGISTER, now, b"body");
        assert_eq!(
            verify(LABEL_REGISTER, &id.device_id(), now, b"bodY", &sig, now),
            Err(AuthError::BadSignature)
        );
        assert_eq!(
            verify(LABEL_REGISTER, &other.device_id(), now, b"body", &sig, now),
            Err(AuthError::BadSignature)
        );
    }

    #[test]
    fn signature_does_not_cross_operations() {
        let id = Identity::generate().expect("rng");
        let now = 1_800_000_000;
        let sig = sign(&id, LABEL_PRESENCE, now, b"body");
        assert_eq!(
            verify(LABEL_REGISTER, &id.device_id(), now, b"body", &sig, now),
            Err(AuthError::BadSignature)
        );
    }

    #[test]
    fn expired_and_future_timestamps_fail() {
        let id = Identity::generate().expect("rng");
        let now = 1_800_000_000;
        let old = now - 301;
        let sig = sign(&id, LABEL_REGISTER, old, b"");
        assert_eq!(
            verify(LABEL_REGISTER, &id.device_id(), old, b"", &sig, now),
            Err(AuthError::Expired)
        );
        let future = now + 301;
        let sig = sign(&id, LABEL_REGISTER, future, b"");
        assert_eq!(
            verify(LABEL_REGISTER, &id.device_id(), future, b"", &sig, now),
            Err(AuthError::Expired)
        );
    }

    #[test]
    fn hex_parsing_checks_length() {
        assert!(parse_key(&"ab".repeat(32)).is_ok());
        assert!(parse_key(&"ab".repeat(31)).is_err());
        assert!(parse_key("zz").is_err());
        assert!(parse_sig(&"ab".repeat(64)).is_ok());
        assert!(parse_sig(&"ab".repeat(63)).is_err());
    }
}
