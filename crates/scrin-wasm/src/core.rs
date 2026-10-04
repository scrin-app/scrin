//! Pure-Rust core of the browser bindings: no `wasm_bindgen` types, so it is
//! unit-testable on the host target and shared with the test-vector generator.
//!
//! The contract is `docs/protocol/gateway-session.md`.

use std::collections::HashMap;

use scrin_crypto::channel::{self, Opener, Ordering, Sealer, Side};
use scrin_crypto::identity::{DeviceId, Identity};
use scrin_crypto::pake::{Paired, Pairing, Role};
use scrin_media::fec::{CompletedFrame, FrameReassembler, MediaKind, ReassemblyStats};
use zeroize::Zeroizing;

/// `Paired::export` context of the inner channel secret.
pub const CHANNEL_CONTEXT: &str = "scrin gateway channel v1";

/// Lane of every datagram (both directions, unordered).
pub const DATAGRAM_LANE: u32 = 0xFFFF_FFFF;

/// Lane of the Control stream (controller-opened, kind 0, ordinal 0).
pub const CONTROL_LANE: u32 = 0;

/// Domain separator of the controller's `Attest` signature.
pub const ATTEST_LABEL: &[u8] = b"/gateway attest v1";

/// Bytes each side signs in its `Attest`:
/// `PROTOCOL || label || signer ('C' | 'H') || host id || controller id ||
/// controller tag || host tag`.
pub fn attest_message(
    signer_is_host: bool,
    host: &[u8],
    controller: &[u8],
    controller_tag: &[u8],
    host_tag: &[u8],
) -> CoreResult<Vec<u8>> {
    let parts: [(&[u8], &'static str); 4] = [
        (host, "host id"),
        (controller, "device id"),
        (controller_tag, "tag"),
        (host_tag, "tag"),
    ];
    let mut m = Vec::with_capacity(scrin_crypto::PROTOCOL.len() + ATTEST_LABEL.len() + 129);
    m.extend_from_slice(scrin_crypto::PROTOCOL.as_bytes());
    m.extend_from_slice(ATTEST_LABEL);
    m.push(if signer_is_host { b'H' } else { b'C' });
    for (p, what) in parts {
        m.extend_from_slice(&array32(p, what)?);
    }
    Ok(m)
}

/// Public key of a seed-based identity (browsers without `WebCrypto` Ed25519).
pub fn seed_public_key(seed: &[u8]) -> CoreResult<[u8; 32]> {
    let seed = Zeroizing::new(array32(seed, "seed")?);
    Ok(Identity::from_seed(*seed).device_id().0)
}

/// Ed25519 signature with a seed-based identity.
pub fn seed_sign(seed: &[u8], msg: &[u8]) -> CoreResult<[u8; 64]> {
    let seed = Zeroizing::new(array32(seed, "seed")?);
    Ok(Identity::from_seed(*seed).sign(msg))
}

/// Ed25519 verification (strict length checks; false on any malformed input).
#[must_use]
pub fn verify_signature(public_key: &[u8], msg: &[u8], sig: &[u8]) -> bool {
    let (Ok(pk), Ok(sig)) = (array32(public_key, "key"), <[u8; 64]>::try_from(sig)) else {
        return false;
    };
    DeviceId(pk).verify(msg, &sig).is_ok()
}

/// Errors surfaced to JS as messages; never carry secret material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreError {
    BadCode,
    BadLength(&'static str),
    PairingFailed,
    ChannelOpen,
    ChannelSeal,
    Spent,
}

impl std::fmt::Display for CoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadCode => f.write_str("malformed one-time code"),
            Self::BadLength(what) => write!(f, "{what} has the wrong length"),
            Self::PairingFailed => f.write_str("pairing failed"),
            Self::ChannelOpen => f.write_str("channel frame rejected"),
            Self::ChannelSeal => f.write_str("channel seal failed"),
            Self::Spent => f.write_str("object already consumed"),
        }
    }
}

impl std::error::Error for CoreError {}

pub type CoreResult<T> = Result<T, CoreError>;

fn array32(v: &[u8], what: &'static str) -> CoreResult<[u8; 32]> {
    v.try_into().map_err(|_| CoreError::BadLength(what))
}

/// Builds a stream lane id: `opener << 24 | kind << 16 | ordinal`.
/// `opener`: 0 controller, 1 host.
#[must_use]
pub const fn stream_lane(host_opened: bool, kind: u8, ordinal: u16) -> u32 {
    ((host_opened as u32) << 24) | ((kind as u32) << 16) | ordinal as u32
}

/// Controller half of SPAKE2 over the gateway, before the peer's message.
#[derive(Debug)]
pub struct ControllerPairing {
    inner: Option<Pairing>,
    msg: Vec<u8>,
}

impl ControllerPairing {
    /// `code`: as typed (normalised here). `me`/`host`: 32-byte device ids.
    /// `entropy`: 32 fresh random bytes from the platform CSPRNG.
    pub fn start(code: &str, me: &[u8], host: &[u8], entropy: &[u8]) -> CoreResult<Self> {
        let code = scrin_crypto::code::normalize(code).map_err(|_| CoreError::BadCode)?;
        let me = DeviceId(array32(me, "device id")?);
        let host = DeviceId(array32(host, "host id")?);
        let entropy = Zeroizing::new(array32(entropy, "entropy")?);
        let (inner, msg) =
            Pairing::start_with_entropy(code.as_str(), Role::Controller, me, host, &entropy);
        Ok(Self {
            inner: Some(inner),
            msg,
        })
    }

    /// The SPAKE2 message for `PairStart`.
    #[must_use]
    pub fn message(&self) -> &[u8] {
        &self.msg
    }

    /// Consumes the pairing with the host's `PairStart` message.
    pub fn finish(&mut self, peer_msg: &[u8]) -> CoreResult<ControllerPaired> {
        let inner = self.inner.take().ok_or(CoreError::Spent)?;
        let paired = inner
            .finish(peer_msg)
            .map_err(|_| CoreError::PairingFailed)?;
        Ok(ControllerPaired { paired })
    }
}

/// Unconfirmed pairing result.
#[derive(Debug)]
pub struct ControllerPaired {
    paired: Paired,
}

impl ControllerPaired {
    #[must_use]
    pub fn confirmation(&self) -> [u8; 32] {
        self.paired.confirmation()
    }

    /// Constant-time check of the host's confirmation tag.
    #[must_use]
    pub fn verify_peer(&self, tag: &[u8]) -> bool {
        array32(tag, "tag").is_ok_and(|t| self.paired.verify_peer(&t).is_ok())
    }

    #[must_use]
    pub fn sas(&self) -> [u8; 5] {
        self.paired.sas().0
    }

    /// The inner channel, controller side.
    #[must_use]
    pub fn channel(&self) -> Lanes {
        Lanes::new(&self.paired.export(CHANNEL_CONTEXT), Side::Controller)
    }
}

/// All lanes of one inner channel, created on first use.
pub struct Lanes {
    secret: Zeroizing<[u8; 32]>,
    side: Side,
    open: HashMap<u32, (Sealer, Opener)>,
}

impl std::fmt::Debug for Lanes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lanes")
            .field("side", &self.side)
            .field("lanes", &self.open.len())
            .finish_non_exhaustive()
    }
}

impl Lanes {
    #[must_use]
    pub fn new(secret: &[u8; 32], side: Side) -> Self {
        Self {
            secret: Zeroizing::new(*secret),
            side,
            open: HashMap::new(),
        }
    }

    fn lane(&mut self, lane: u32) -> &mut (Sealer, Opener) {
        let (secret, side) = (&self.secret, self.side);
        self.open.entry(lane).or_insert_with(|| {
            let ordering = if lane == DATAGRAM_LANE {
                Ordering::Unordered
            } else {
                Ordering::Ordered
            };
            channel::lane(secret, side, lane, ordering)
        })
    }

    pub fn seal(&mut self, lane: u32, plaintext: &[u8]) -> CoreResult<Vec<u8>> {
        self.lane(lane)
            .0
            .seal(plaintext)
            .map_err(|_| CoreError::ChannelSeal)
    }

    pub fn open(&mut self, lane: u32, sealed: &[u8]) -> CoreResult<Vec<u8>> {
        self.lane(lane)
            .1
            .open(sealed)
            .map_err(|_| CoreError::ChannelOpen)
    }
}

/// Video-only view of the FEC reassembler.
#[derive(Debug, Default)]
pub struct VideoReassembler {
    inner: FrameReassembler,
}

impl VideoReassembler {
    /// Feeds one (already opened) shard datagram. Audio and malformed shards
    /// yield `None`; malformed ones count in `stats().invalid_shards`.
    pub fn push(&mut self, datagram: &[u8]) -> Option<CompletedFrame> {
        match self.inner.push(datagram) {
            Ok(Some(f)) if f.kind == MediaKind::Video => Some(f),
            _ => None,
        }
    }

    #[must_use]
    pub fn stats(&self) -> ReassemblyStats {
        self.inner.stats()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scrin_crypto::identity::Identity;
    use scrin_media::fec::FrameEncoder;

    #[test]
    fn controller_pairs_with_a_native_host() {
        let host = Identity::from_seed([1; 32]).device_id();
        let me = Identity::from_seed([2; 32]).device_id();
        let mut c = ControllerPairing::start("abcd-efgh", &me.0, &host.0, &[3; 32]).expect("start");
        let (ph, mh) = Pairing::start("ABCDEFGH", Role::Host, host, me);
        let kh = ph.finish(c.message()).expect("host");
        let kc = c.finish(&mh).expect("controller");
        assert!(kc.verify_peer(&kh.confirmation()));
        kh.verify_peer(&kc.confirmation()).expect("tag");
        assert_eq!(kc.sas(), kh.sas().0);
        assert_eq!(c.finish(&mh).err(), Some(CoreError::Spent));

        let mut cl = kc.channel();
        let mut hl = Lanes::new(&kh.export(CHANNEL_CONTEXT), Side::Host);
        let sealed = hl.seal(DATAGRAM_LANE, b"shard").expect("seal");
        assert_eq!(cl.open(DATAGRAM_LANE, &sealed).expect("open"), b"shard");
        let up = cl.seal(CONTROL_LANE, b"env").expect("seal");
        assert_eq!(hl.open(CONTROL_LANE, &up).expect("open"), b"env");
        assert_eq!(
            cl.open(CONTROL_LANE, &sealed),
            Err(CoreError::ChannelOpen),
            "lanes are separate"
        );
    }

    #[test]
    fn bad_inputs_are_rejected() {
        assert_eq!(
            ControllerPairing::start("short", &[0; 32], &[0; 32], &[0; 32]).err(),
            Some(CoreError::BadCode)
        );
        assert_eq!(
            ControllerPairing::start("ABCDEFGH", &[0; 31], &[0; 32], &[0; 32]).err(),
            Some(CoreError::BadLength("device id"))
        );
    }

    #[test]
    fn lane_ids_follow_the_contract() {
        assert_eq!(stream_lane(false, 0, 0), CONTROL_LANE);
        assert_eq!(stream_lane(false, 1, 0), 0x0001_0000);
        assert_eq!(stream_lane(true, 3, 2), 0x0103_0002);
    }

    #[test]
    fn seed_identity_signs_the_attest_message() {
        let seed = [5u8; 32];
        let id = DeviceId(seed_public_key(&seed).expect("pk"));
        let m = attest_message(false, &[1; 32], &id.0, &[2; 32], &[3; 32]).expect("msg");
        assert_eq!(
            m.len(),
            scrin_crypto::PROTOCOL.len() + ATTEST_LABEL.len() + 129
        );
        assert_ne!(
            m,
            attest_message(true, &[1; 32], &id.0, &[2; 32], &[3; 32]).expect("msg")
        );
        id.verify(&m, &seed_sign(&seed, &m).expect("sig"))
            .expect("verifies");
        assert!(attest_message(false, &[1; 31], &id.0, &[2; 32], &[3; 32]).is_err());
    }

    #[test]
    fn reassembler_yields_video_only() {
        let mut r = VideoReassembler::default();
        let enc = FrameEncoder::new(MediaKind::Video, 0.0).expect("enc");
        let audio = FrameEncoder::new(MediaKind::Audio, 0.0).expect("enc");
        for s in audio.encode(9, false, b"opus").expect("shard") {
            assert!(r.push(&s.to_bytes()).is_none());
        }
        let frame = vec![7u8; 3000];
        let mut out = None;
        for s in enc.encode(1, true, &frame).expect("shard") {
            out = out.or(r.push(&s.to_bytes()));
        }
        let f = out.expect("frame");
        assert!(f.keyframe);
        assert_eq!(f.data, frame);
        assert!(r.push(&[1, 2, 3]).is_none());
        assert_eq!(r.stats().invalid_shards, 1);
    }
}
