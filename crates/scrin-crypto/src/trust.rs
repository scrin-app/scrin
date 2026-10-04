//! Trust store: controllers allowed to connect without a one-time code.
//!
//! Persisted as JSON + a keyed BLAKE3 MAC derived from the device seed. A bad
//! MAC is reported as [`Error::TrustStoreTampered`]; callers then start from an
//! empty store (fail closed) and tell the user.

use serde::{Deserialize, Serialize};

use crate::identity::{DeviceId, Identity};
use crate::{Error, Result};

const MAC_CONTEXT: &str = "scrin trust-store v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    ViewOnly,
    Support,
    Full,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustedPeer {
    pub device: DeviceId,
    pub label: String,
    pub profile: Profile,
    /// Unix seconds.
    pub added_at: u64,
    /// Optional expiry (Unix seconds) for time-boxed access.
    pub expires_at: Option<u64>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustStore {
    peers: Vec<TrustedPeer>,
}

impl TrustStore {
    #[must_use]
    pub fn peers(&self) -> &[TrustedPeer] {
        &self.peers
    }

    /// Returns the entry if `device` is trusted and not expired at `now`.
    #[must_use]
    pub fn lookup(&self, device: &DeviceId, now: u64) -> Option<&TrustedPeer> {
        self.peers
            .iter()
            .find(|p| &p.device == device && p.expires_at.is_none_or(|e| now < e))
    }

    pub fn upsert(&mut self, peer: TrustedPeer) {
        self.peers.retain(|p| p.device != peer.device);
        self.peers.push(peer);
    }

    pub fn revoke(&mut self, device: &DeviceId) -> bool {
        let before = self.peers.len();
        self.peers.retain(|p| &p.device != device);
        before != self.peers.len()
    }

    /// Serialises and seals: `mac(32) || json`.
    pub fn seal(&self, id: &Identity) -> Result<Vec<u8>> {
        let json = serde_json::to_vec(self)?;
        let key = id.local_mac_key(MAC_CONTEXT);
        let mac = blake3::keyed_hash(&key, &json);
        let mut out = Vec::with_capacity(32 + json.len());
        out.extend_from_slice(mac.as_bytes());
        out.extend_from_slice(&json);
        Ok(out)
    }

    pub fn open(sealed: &[u8], id: &Identity) -> Result<Self> {
        if sealed.len() < 32 {
            return Err(Error::TrustStoreTampered);
        }
        let (mac, json) = sealed.split_at(32);
        let key = id.local_mac_key(MAC_CONTEXT);
        let mac: [u8; 32] = mac.try_into().map_err(|_| Error::TrustStoreTampered)?;
        if blake3::keyed_hash(&key, json) != blake3::Hash::from(mac) {
            return Err(Error::TrustStoreTampered);
        }
        Ok(serde_json::from_slice(json)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(seed: u8, expires_at: Option<u64>) -> TrustedPeer {
        TrustedPeer {
            device: Identity::from_seed([seed; 32]).device_id(),
            label: format!("peer {seed}"),
            profile: Profile::Support,
            added_at: 100,
            expires_at,
        }
    }

    #[test]
    fn seal_open_round_trip() {
        let id = Identity::from_seed([1; 32]);
        let mut s = TrustStore::default();
        s.upsert(peer(2, None));
        let sealed = s.seal(&id).expect("seal");
        assert_eq!(TrustStore::open(&sealed, &id).expect("open"), s);
    }

    #[test]
    fn tamper_and_wrong_key_rejected() {
        let id = Identity::from_seed([1; 32]);
        let mut s = TrustStore::default();
        s.upsert(peer(2, None));
        let mut sealed = s.seal(&id).expect("seal");
        let last = sealed.len() - 2;
        sealed[last] ^= 1;
        assert!(matches!(
            TrustStore::open(&sealed, &id),
            Err(Error::TrustStoreTampered)
        ));
        let good = s.seal(&id).expect("seal");
        let other = Identity::from_seed([5; 32]);
        assert!(TrustStore::open(&good, &other).is_err());
    }

    #[test]
    fn expiry_and_revoke() {
        let mut s = TrustStore::default();
        let p = peer(3, Some(200));
        let d = p.device;
        s.upsert(p);
        assert!(s.lookup(&d, 150).is_some());
        assert!(s.lookup(&d, 200).is_none());
        assert!(s.revoke(&d));
        assert!(!s.revoke(&d));
    }
}
