//! SPAKE2 pairing on a short code.
//!
//! Runs *inside* the QUIC channel (already encrypted and authenticated to each
//! endpoint's key). The SPAKE2 identity binds the protocol label and both
//! endpoint ids, so the shared key only matches when both sides typed the same
//! code AND talk to the endpoints they think they do — a rendezvous server
//! swapping keys yields a mismatch, not a session.
//!
//! Flow (symmetric):
//! 1. both call [`Pairing::start`], send `msg` to the peer;
//! 2. both call [`Pairing::finish`] with the peer's msg → [`Paired`];
//! 3. both send [`Paired::confirmation`] for their role and check the peer's
//!    with [`Paired::verify_peer`]. Only then is the peer trusted.

use spake2::{Ed25519Group, Identity as SpakeIdentity, Password, Spake2};
use zeroize::Zeroizing;

use crate::identity::DeviceId;
use crate::sas::Sas;
use crate::{Error, PROTOCOL, Result};

/// Which side of the session this device is. Used to separate confirmation tags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Host,
    Controller,
}

impl Role {
    fn label(self) -> &'static [u8] {
        match self {
            Role::Host => b"host",
            Role::Controller => b"controller",
        }
    }

    fn other(self) -> Role {
        match self {
            Role::Host => Role::Controller,
            Role::Controller => Role::Host,
        }
    }
}

#[derive(Debug)]
pub struct Pairing {
    state: Spake2<Ed25519Group>,
    transcript: Vec<u8>,
    role: Role,
    my_msg: Vec<u8>,
}

impl Pairing {
    /// `code` must already be normalised (see [`crate::code::normalize`]).
    #[must_use]
    pub fn start(code: &str, role: Role, me: DeviceId, peer: DeviceId) -> (Self, Vec<u8>) {
        let transcript = binding(me, peer);
        let (state, msg) = Spake2::<Ed25519Group>::start_symmetric(
            &Password::new(code.as_bytes()),
            &SpakeIdentity::new(&transcript),
        );
        Self::started(state, msg, transcript, role)
    }

    /// Like [`Pairing::start`], but the SPAKE2 scalar is drawn from `entropy`
    /// (expanded with BLAKE3) instead of the OS RNG. For runtimes that bring
    /// their own CSPRNG (the browser's `crypto.getRandomValues`) and for
    /// deterministic test vectors. `entropy` MUST be fresh and secret per
    /// attempt; reusing it across codes leaks the password to an observer.
    #[must_use]
    pub fn start_with_entropy(
        code: &str,
        role: Role,
        me: DeviceId,
        peer: DeviceId,
        entropy: &[u8; 32],
    ) -> (Self, Vec<u8>) {
        let transcript = binding(me, peer);
        let (state, msg) = Spake2::<Ed25519Group>::start_symmetric_with_rng(
            &Password::new(code.as_bytes()),
            &SpakeIdentity::new(&transcript),
            EntropyRng::new(entropy),
        );
        Self::started(state, msg, transcript, role)
    }

    fn started(
        state: Spake2<Ed25519Group>,
        msg: Vec<u8>,
        transcript: Vec<u8>,
        role: Role,
    ) -> (Self, Vec<u8>) {
        (
            Self {
                state,
                transcript,
                role,
                my_msg: msg.clone(),
            },
            msg,
        )
    }

    pub fn finish(self, peer_msg: &[u8]) -> Result<Paired> {
        let key = self
            .state
            .finish(peer_msg)
            .map_err(|_| Error::PairingFailed)?;
        // Messages are ordered so both sides hash the same bytes.
        let (a, b) = if self.my_msg.as_slice() <= peer_msg {
            (self.my_msg.as_slice(), peer_msg)
        } else {
            (peer_msg, self.my_msg.as_slice())
        };
        let mut h = blake3::Hasher::new_derive_key("scrin pake session v1");
        h.update(&key);
        h.update(&self.transcript);
        h.update(a);
        h.update(b);
        Ok(Paired {
            key: Zeroizing::new(*h.finalize().as_bytes()),
            role: self.role,
        })
    }
}

/// Result of a SPAKE2 exchange that is not yet confirmed.
pub struct Paired {
    key: Zeroizing<[u8; 32]>,
    role: Role,
}

impl Paired {
    /// Tag this device sends to prove it derived the same key.
    #[must_use]
    pub fn confirmation(&self) -> [u8; 32] {
        tag(&self.key, self.role)
    }

    /// Constant-time check of the peer's confirmation tag.
    pub fn verify_peer(&self, tag_from_peer: &[u8; 32]) -> Result<()> {
        let expected = blake3::Hash::from(tag(&self.key, self.role.other()));
        if expected == blake3::Hash::from(*tag_from_peer) {
            Ok(())
        } else {
            Err(Error::PairingFailed)
        }
    }

    /// Emoji both users compare out loud.
    #[must_use]
    pub fn sas(&self) -> Sas {
        Sas::derive(&self.key)
    }

    /// Key material for an inner channel (browser gateway path). Domain-separated.
    #[must_use]
    pub fn export(&self, context: &str) -> Zeroizing<[u8; 32]> {
        let mut h = blake3::Hasher::new_derive_key("scrin pake export v1");
        h.update(context.as_bytes());
        h.update(self.key.as_slice());
        Zeroizing::new(*h.finalize().as_bytes())
    }
}

impl std::fmt::Debug for Paired {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Paired")
            .field("role", &self.role)
            .finish_non_exhaustive()
    }
}

fn tag(key: &[u8; 32], role: Role) -> [u8; 32] {
    let mut h = blake3::Hasher::new_keyed(key);
    h.update(b"scrin confirm v1/");
    h.update(role.label());
    *h.finalize().as_bytes()
}

fn binding(me: DeviceId, peer: DeviceId) -> Vec<u8> {
    let (lo, hi) = if me <= peer { (me, peer) } else { (peer, me) };
    let mut v = Vec::with_capacity(PROTOCOL.len() + 64);
    v.extend_from_slice(PROTOCOL.as_bytes());
    v.extend_from_slice(&lo.0);
    v.extend_from_slice(&hi.0);
    v
}

/// A CSPRNG (BLAKE3 XOF) over caller-provided entropy, for `start_with_entropy`.
struct EntropyRng(blake3::OutputReader);

impl EntropyRng {
    fn new(entropy: &[u8; 32]) -> Self {
        let mut h = blake3::Hasher::new_derive_key("scrin pake entropy v1");
        h.update(entropy);
        Self(h.finalize_xof())
    }
}

impl rand_core_06::RngCore for EntropyRng {
    fn next_u32(&mut self) -> u32 {
        let mut b = [0u8; 4];
        self.0.fill(&mut b);
        u32::from_le_bytes(b)
    }

    fn next_u64(&mut self) -> u64 {
        let mut b = [0u8; 8];
        self.0.fill(&mut b);
        u64::from_le_bytes(b)
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        self.0.fill(dest);
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> std::result::Result<(), rand_core_06::Error> {
        self.0.fill(dest);
        Ok(())
    }
}

impl rand_core_06::CryptoRng for EntropyRng {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::Identity;

    fn ids() -> (DeviceId, DeviceId) {
        (
            Identity::from_seed([1; 32]).device_id(),
            Identity::from_seed([2; 32]).device_id(),
        )
    }

    fn run(code_h: &str, code_c: &str, host_sees: DeviceId) -> Result<(Paired, Paired)> {
        let (h, c) = ids();
        let (ph, mh) = Pairing::start(code_h, Role::Host, h, c);
        let (pc, mc) = Pairing::start(code_c, Role::Controller, c, host_sees);
        let kh = ph.finish(&mc)?;
        let kc = pc.finish(&mh)?;
        kh.verify_peer(&kc.confirmation())?;
        kc.verify_peer(&kh.confirmation())?;
        Ok((kh, kc))
    }

    #[test]
    fn same_code_pairs_and_sas_matches() {
        let (host, _) = ids();
        let (kh, kc) = run("ABCDEFGH", "ABCDEFGH", host).expect("pairs");
        assert_eq!(kh.sas(), kc.sas());
        assert_eq!(*kh.export("x"), *kc.export("x"));
    }

    #[test]
    fn wrong_code_fails_confirmation() {
        let (host, _) = ids();
        assert!(run("ABCDEFGH", "ABCDEFGJ", host).is_err());
    }

    #[test]
    fn substituted_endpoint_fails() {
        // Controller believes it talks to a different host key (MITM by server).
        let evil = Identity::from_seed([9; 32]).device_id();
        assert!(run("ABCDEFGH", "ABCDEFGH", evil).is_err());
    }

    #[test]
    fn reflected_confirmation_is_rejected() {
        let (h, c) = ids();
        let (ph, mh) = Pairing::start("ABCDEFGH", Role::Host, h, c);
        let (pc, mc) = Pairing::start("ABCDEFGH", Role::Controller, c, h);
        let kh = ph.finish(&mc).expect("finish");
        let _kc = pc.finish(&mh).expect("finish");
        // Host's own tag echoed back must not verify as the controller's.
        assert!(kh.verify_peer(&kh.confirmation()).is_err());
    }

    #[test]
    fn entropy_start_is_deterministic_and_interoperates() {
        let (h, c) = ids();
        let (_, m1) = Pairing::start_with_entropy("ABCDEFGH", Role::Controller, c, h, &[7; 32]);
        let (pc, m2) = Pairing::start_with_entropy("ABCDEFGH", Role::Controller, c, h, &[7; 32]);
        assert_eq!(m1, m2);
        let (_, m3) = Pairing::start_with_entropy("ABCDEFGH", Role::Controller, c, h, &[8; 32]);
        assert_ne!(m1, m3);
        // An OS-RNG host pairs with an entropy-seeded controller.
        let (ph, mh) = Pairing::start("ABCDEFGH", Role::Host, h, c);
        let kh = ph.finish(&m2).expect("host finish");
        let kc = pc.finish(&mh).expect("controller finish");
        kh.verify_peer(&kc.confirmation()).expect("controller tag");
        kc.verify_peer(&kh.confirmation()).expect("host tag");
    }
}
