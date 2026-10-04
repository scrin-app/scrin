//! Device identity: one Ed25519 key per install.
//!
//! The 32-byte seed is the only secret. Callers persist it through the OS
//! keystore (DPAPI, Android Keystore, non-extractable `WebCrypto`); this module
//! never writes it anywhere.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use zeroize::Zeroizing;

use crate::{Error, Result, random_bytes};

/// Public half of a device identity. Equal to the iroh `EndpointId` bytes.
#[derive(
    Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct DeviceId(pub [u8; 32]);

impl DeviceId {
    #[must_use]
    pub fn to_hex(&self) -> String {
        data_encoding::HEXLOWER.encode(&self.0)
    }

    /// Short human fingerprint (`xxxx-xxxx-xxxx-xxxx`) for settings screens.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        let h = blake3::derive_key("scrin device fingerprint v1", &self.0);
        let s = data_encoding::BASE32_NOPAD.encode(&h[..10]).to_lowercase();
        s.as_bytes()
            .chunks(4)
            .map(|c| String::from_utf8_lossy(c).into_owned())
            .collect::<Vec<_>>()
            .join("-")
    }

    pub fn verify(&self, msg: &[u8], sig: &[u8; 64]) -> Result<()> {
        let key = VerifyingKey::from_bytes(&self.0).map_err(|_| Error::Malformed("device key"))?;
        key.verify(msg, &Signature::from_bytes(sig))
            .map_err(|_| Error::Malformed("signature"))
    }
}

impl std::fmt::Debug for DeviceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DeviceId({})", &self.to_hex()[..12])
    }
}

/// A device's secret identity. The seed is zeroized on drop.
pub struct Identity {
    seed: Zeroizing<[u8; 32]>,
    key: SigningKey,
}

impl Identity {
    pub fn generate() -> Result<Self> {
        Ok(Self::from_seed(random_bytes::<32>()?))
    }

    #[must_use]
    pub fn from_seed(seed: [u8; 32]) -> Self {
        let key = SigningKey::from_bytes(&seed);
        Self {
            seed: Zeroizing::new(seed),
            key,
        }
    }

    /// The seed, for sealing into the OS keystore. Handle with care.
    #[must_use]
    pub fn seed(&self) -> &[u8; 32] {
        &self.seed
    }

    #[must_use]
    pub fn device_id(&self) -> DeviceId {
        DeviceId(self.key.verifying_key().to_bytes())
    }

    #[must_use]
    pub fn sign(&self, msg: &[u8]) -> [u8; 64] {
        self.key.sign(msg).to_bytes()
    }

    /// Key for MAC-sealing local state (trust store), derived from the seed.
    #[must_use]
    pub(crate) fn local_mac_key(&self, context: &str) -> Zeroizing<[u8; 32]> {
        Zeroizing::new(blake3::derive_key(context, self.seed.as_slice()))
    }
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identity")
            .field("device_id", &self.device_id())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_and_verify_round_trip() {
        let id = Identity::generate().expect("rng");
        let sig = id.sign(b"hello");
        id.device_id().verify(b"hello", &sig).expect("valid");
        assert!(id.device_id().verify(b"hellO", &sig).is_err());
    }

    #[test]
    fn seed_restores_same_identity() {
        let a = Identity::generate().expect("rng");
        let b = Identity::from_seed(*a.seed());
        assert_eq!(a.device_id(), b.device_id());
    }

    #[test]
    fn fingerprint_is_grouped() {
        let id = Identity::from_seed([7; 32]);
        let fp = id.device_id().fingerprint();
        assert_eq!(fp.len(), 19);
        assert_eq!(fp.matches('-').count(), 3);
    }
}
