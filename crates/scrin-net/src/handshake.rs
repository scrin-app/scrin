//! Session handshake on the Control stream: version negotiation, then either
//! quick-connect pairing (SPAKE2 over the one-time code) or trusted
//! (unattended) authentication by device signature.
//!
//! Messages are frames of `tag (1 byte) || payload`:
//!
//! | tag | message         | payload                                     |
//! |-----|-----------------|---------------------------------------------|
//! | 1   | `Hello`         | `min u16 BE`, `max u16 BE`, `intent u8`     |
//! | 2   | `PairStart`     | SPAKE2 message                              |
//! | 3   | `PairConfirm`   | 32-byte confirmation tag                    |
//! | 4   | `Result`        | `0` ok, else a [`RejectReason`] byte        |
//! | 5   | `AuthChallenge` | 32-byte host nonce                          |
//! | 6   | `AuthProof`     | `timestamp u64 BE`, 64-byte Ed25519 sig     |
//!
//! Pairing: C→H `Hello`, H→C `Hello`, C→H `PairStart`, H→C `PairStart`,
//! C→H `PairConfirm`, H→C `PairConfirm` (or `Result` fail), C→H `Result`.
//! The host consumes the code when the first `PairStart` arrives, before it
//! learns whether the guess was right: one online guess per code.
//!
//! Trusted: C→H `Hello`, H→C `Hello`, H→C `AuthChallenge`, C→H `AuthProof`,
//! H→C `Result`.
//! The nonce is fresh per attempt, so a captured proof cannot be replayed.
//!
//! The peer's [`DeviceId`] always comes from the authenticated QUIC
//! connection ([`remote_device_id`]), never from a message.

use std::sync::{Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use iroh::endpoint::{Connection, RecvStream, SendStream};
use scrin_crypto::PROTOCOL;
use scrin_crypto::code::{OneTimeCode, normalize};
use scrin_crypto::identity::{DeviceId, Identity};
use scrin_crypto::pake::{Paired, Pairing, Role};
use scrin_crypto::sas::Sas;
use scrin_crypto::trust::{Profile, TrustStore};

use crate::endpoint::remote_device_id;
use crate::framing::{StreamKind, accept_stream, open_stream, read_frame_capped, write_frame};
use crate::{NetError, Result};

/// Wire protocol versions this build speaks.
pub const PROTOCOL_VERSION_MIN: u16 = 1;
pub const PROTOCOL_VERSION_MAX: u16 = 1;

/// Whole-handshake deadline, so a silent peer cannot pin a task.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

/// Allowed clock difference for trusted-auth timestamps.
pub const MAX_CLOCK_SKEW_SECS: u64 = 300;

/// Domain separator of the trusted-auth signature.
const TRUSTED_AUTH_LABEL: &[u8] = b"/trusted-auth v1";

/// No handshake message comes close to this.
const MAX_HANDSHAKE_MSG: usize = 4096;

const TAG_HELLO: u8 = 1;
const TAG_PAIR_START: u8 = 2;
const TAG_PAIR_CONFIRM: u8 = 3;
const TAG_RESULT: u8 = 4;
const TAG_AUTH_CHALLENGE: u8 = 5;
const TAG_AUTH_PROOF: u8 = 6;

const INTENT_PAIR: u8 = 0;
const INTENT_TRUSTED: u8 = 1;

/// Why a peer refused. Travels as one byte; unknown values are preserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    WrongCode,
    CodeUnavailable,
    VersionMismatch,
    Untrusted,
    BadSignature,
    StaleTimestamp,
    WrongMode,
    Other(u8),
}

impl RejectReason {
    const fn to_byte(self) -> u8 {
        match self {
            Self::WrongCode => 1,
            Self::CodeUnavailable => 2,
            Self::VersionMismatch => 3,
            Self::Untrusted => 4,
            Self::BadSignature => 5,
            Self::StaleTimestamp => 6,
            Self::WrongMode => 7,
            Self::Other(b) => b,
        }
    }

    const fn from_byte(b: u8) -> Self {
        match b {
            1 => Self::WrongCode,
            2 => Self::CodeUnavailable,
            3 => Self::VersionMismatch,
            4 => Self::Untrusted,
            5 => Self::BadSignature,
            6 => Self::StaleTimestamp,
            7 => Self::WrongMode,
            b => Self::Other(b),
        }
    }
}

impl std::fmt::Display for RejectReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongCode => f.write_str("wrong code"),
            Self::CodeUnavailable => f.write_str("code used or expired"),
            Self::VersionMismatch => f.write_str("no common protocol version"),
            Self::Untrusted => f.write_str("device not trusted"),
            Self::BadSignature => f.write_str("bad signature"),
            Self::StaleTimestamp => f.write_str("clock skew too large"),
            Self::WrongMode => f.write_str("unexpected handshake mode"),
            Self::Other(b) => write!(f, "reason {b}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Msg {
    Hello { min: u16, max: u16, intent: u8 },
    PairStart(Vec<u8>),
    PairConfirm([u8; 32]),
    Result(Option<RejectReason>),
    AuthChallenge([u8; 32]),
    AuthProof { timestamp: u64, sig: [u8; 64] },
}

impl Msg {
    fn encode(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(80);
        match self {
            Self::Hello { min, max, intent } => {
                v.push(TAG_HELLO);
                v.extend_from_slice(&min.to_be_bytes());
                v.extend_from_slice(&max.to_be_bytes());
                v.push(*intent);
            }
            Self::PairStart(m) => {
                v.push(TAG_PAIR_START);
                v.extend_from_slice(m);
            }
            Self::PairConfirm(t) => {
                v.push(TAG_PAIR_CONFIRM);
                v.extend_from_slice(t);
            }
            Self::Result(r) => {
                v.push(TAG_RESULT);
                v.push(r.map_or(0, RejectReason::to_byte));
            }
            Self::AuthChallenge(n) => {
                v.push(TAG_AUTH_CHALLENGE);
                v.extend_from_slice(n);
            }
            Self::AuthProof { timestamp, sig } => {
                v.push(TAG_AUTH_PROOF);
                v.extend_from_slice(&timestamp.to_be_bytes());
                v.extend_from_slice(sig);
            }
        }
        v
    }

    fn decode(buf: &[u8]) -> Result<Self> {
        let (&tag, body) = buf
            .split_first()
            .ok_or(NetError::Protocol("empty message"))?;
        Ok(match tag {
            TAG_HELLO => {
                let b: [u8; 5] = body.try_into().map_err(|_| bad("hello"))?;
                Self::Hello {
                    min: u16::from_be_bytes([b[0], b[1]]),
                    max: u16::from_be_bytes([b[2], b[3]]),
                    intent: b[4],
                }
            }
            TAG_PAIR_START if !body.is_empty() => Self::PairStart(body.to_vec()),
            TAG_PAIR_CONFIRM => Self::PairConfirm(body.try_into().map_err(|_| bad("confirm"))?),
            TAG_RESULT => match body {
                [0] => Self::Result(None),
                [r] => Self::Result(Some(RejectReason::from_byte(*r))),
                _ => return Err(bad("result")),
            },
            TAG_AUTH_CHALLENGE => {
                Self::AuthChallenge(body.try_into().map_err(|_| bad("challenge"))?)
            }
            TAG_AUTH_PROOF => {
                if body.len() != 72 {
                    return Err(bad("proof"));
                }
                let (ts, sig) = body.split_at(8);
                Self::AuthProof {
                    timestamp: u64::from_be_bytes(ts.try_into().map_err(|_| bad("proof"))?),
                    sig: sig.try_into().map_err(|_| bad("proof"))?,
                }
            }
            _ => return Err(NetError::Protocol("unknown handshake message")),
        })
    }
}

fn bad(_what: &'static str) -> NetError {
    NetError::Protocol("malformed handshake message")
}

/// The Control stream, handed back for the rest of the session.
#[derive(Debug)]
pub struct ControlStream {
    pub send: SendStream,
    pub recv: RecvStream,
    /// Negotiated wire protocol version.
    pub version: u16,
}

impl ControlStream {
    async fn send(&mut self, msg: &Msg) -> Result<()> {
        write_frame(&mut self.send, &msg.encode()).await
    }

    async fn recv(&mut self) -> Result<Msg> {
        let buf = read_frame_capped(&mut self.recv, MAX_HANDSHAKE_MSG)
            .await?
            .ok_or(NetError::StreamClosed)?;
        Msg::decode(&buf)
    }

    async fn reject(&mut self, reason: RejectReason) {
        // Best effort: the error we return matters more than the peer's copy.
        let _ = self.send(&Msg::Result(Some(reason))).await;
    }
}

/// Successful quick-connect pairing.
#[derive(Debug)]
pub struct PairOutcome {
    /// Authenticated by QUIC, confirmed by the PAKE.
    pub peer: DeviceId,
    /// Emoji the users compare before the host shares anything.
    pub sas: Sas,
    pub paired: Paired,
    pub control: ControlStream,
}

impl PairOutcome {
    /// Domain-separated key material for an inner channel.
    #[must_use]
    pub fn export(&self, context: &str) -> zeroize::Zeroizing<[u8; 32]> {
        self.paired.export(context)
    }
}

/// Successful trusted (unattended) authentication, host side.
#[derive(Debug)]
pub struct TrustedOutcome {
    pub peer: DeviceId,
    pub profile: Profile,
    pub control: ControlStream,
}

/// Host-side holder of the one-time code. The first attempt takes it.
#[derive(Debug)]
pub struct HostCode {
    slot: Mutex<Option<OneTimeCode>>,
}

impl HostCode {
    #[must_use]
    pub fn new(code: OneTimeCode) -> Self {
        Self {
            slot: Mutex::new(Some(code)),
        }
    }

    #[must_use]
    pub fn is_consumed(&self) -> bool {
        self.slot
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_none()
    }

    /// Takes the code. Afterwards every attempt fails with [`NetError::CodeConsumed`].
    fn take(&self) -> Result<OneTimeCode> {
        let code = self
            .slot
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .ok_or(NetError::CodeConsumed)?;
        if code.is_expired() {
            return Err(NetError::CodeExpired);
        }
        Ok(code)
    }
}

/// Host side of quick connect. `me` is this endpoint's device id.
pub async fn host_pair(conn: &Connection, me: DeviceId, code: &HostCode) -> Result<PairOutcome> {
    timed(async {
        let mut ctl = host_hello(conn, INTENT_PAIR).await?;
        let peer = remote_device_id(conn);

        let Msg::PairStart(peer_msg) = ctl.recv().await? else {
            return Err(NetError::Protocol("expected PairStart"));
        };
        // Consumed here, before the outcome is known: one guess per code.
        let code = match code.take() {
            Ok(c) => c,
            Err(e) => {
                ctl.reject(RejectReason::CodeUnavailable).await;
                return Err(e);
            }
        };
        let (pairing, my_msg) = Pairing::start(code.as_str(), Role::Host, me, peer);
        drop(code);
        ctl.send(&Msg::PairStart(my_msg)).await?;
        let Ok(paired) = pairing.finish(&peer_msg) else {
            ctl.reject(RejectReason::WrongCode).await;
            return Err(NetError::PairingFailed);
        };

        let Msg::PairConfirm(tag) = ctl.recv().await? else {
            return Err(NetError::Protocol("expected PairConfirm"));
        };
        if paired.verify_peer(&tag).is_err() {
            ctl.reject(RejectReason::WrongCode).await;
            return Err(NetError::PairingFailed);
        }
        ctl.send(&Msg::PairConfirm(paired.confirmation())).await?;
        match ctl.recv().await? {
            Msg::Result(None) => Ok(PairOutcome {
                peer,
                sas: paired.sas(),
                paired,
                control: ctl,
            }),
            Msg::Result(Some(_)) => Err(NetError::PairingFailed),
            _ => Err(NetError::Protocol("expected Result")),
        }
    })
    .await
}

/// Controller side of quick connect. `me` is this endpoint's device id;
/// `typed_code` is what the user typed (any case, dashes and spaces allowed).
pub async fn controller_pair(
    conn: &Connection,
    me: DeviceId,
    typed_code: &str,
) -> Result<PairOutcome> {
    let code = normalize(typed_code)?;
    timed(async {
        let mut ctl = controller_hello(conn, INTENT_PAIR).await?;
        let peer = remote_device_id(conn);

        let (pairing, my_msg) = Pairing::start(code.as_str(), Role::Controller, me, peer);
        ctl.send(&Msg::PairStart(my_msg)).await?;
        let peer_msg = match ctl.recv().await? {
            Msg::PairStart(m) => m,
            Msg::Result(Some(r)) => return Err(pair_rejection(r)),
            _ => return Err(NetError::Protocol("expected PairStart")),
        };
        let paired = pairing
            .finish(&peer_msg)
            .map_err(|_| NetError::PairingFailed)?;
        ctl.send(&Msg::PairConfirm(paired.confirmation())).await?;
        let tag = match ctl.recv().await? {
            Msg::PairConfirm(t) => t,
            Msg::Result(Some(r)) => return Err(pair_rejection(r)),
            _ => return Err(NetError::Protocol("expected PairConfirm")),
        };
        if paired.verify_peer(&tag).is_err() {
            ctl.reject(RejectReason::WrongCode).await;
            return Err(NetError::PairingFailed);
        }
        ctl.send(&Msg::Result(None)).await?;
        Ok(PairOutcome {
            peer,
            sas: paired.sas(),
            paired,
            control: ctl,
        })
    })
    .await
}

fn pair_rejection(r: RejectReason) -> NetError {
    match r {
        RejectReason::WrongCode => NetError::PairingFailed,
        RejectReason::CodeUnavailable => NetError::CodeConsumed,
        RejectReason::VersionMismatch => NetError::VersionMismatch,
        other => NetError::Rejected(other),
    }
}

/// Controller side of unattended access: proves possession of `identity`,
/// which must be the key this controller's endpoint is bound with.
pub async fn controller_auth_trusted(
    conn: &Connection,
    identity: &Identity,
) -> Result<ControlStream> {
    timed(async {
        let mut ctl = controller_hello(conn, INTENT_TRUSTED).await?;
        let host = remote_device_id(conn);
        let nonce = match ctl.recv().await? {
            Msg::AuthChallenge(n) => n,
            Msg::Result(Some(r)) => return Err(NetError::Rejected(r)),
            _ => return Err(NetError::Protocol("expected AuthChallenge")),
        };
        let timestamp = unix_now();
        let msg = trusted_auth_message(host, identity.device_id(), timestamp, &nonce);
        ctl.send(&Msg::AuthProof {
            timestamp,
            sig: identity.sign(&msg),
        })
        .await?;
        match ctl.recv().await? {
            Msg::Result(None) => Ok(ctl),
            Msg::Result(Some(r)) => Err(NetError::Rejected(r)),
            _ => Err(NetError::Protocol("expected Result")),
        }
    })
    .await
}

/// Host side of unattended access. `now` is Unix seconds (trust expiry and
/// timestamp skew are judged against it).
pub async fn host_auth_trusted(
    conn: &Connection,
    me: DeviceId,
    trust: &TrustStore,
    now: u64,
) -> Result<TrustedOutcome> {
    timed(async {
        let mut ctl = host_hello(conn, INTENT_TRUSTED).await?;
        let peer = remote_device_id(conn);
        let nonce = random_nonce()?;
        ctl.send(&Msg::AuthChallenge(nonce)).await?;
        let Msg::AuthProof { timestamp, sig } = ctl.recv().await? else {
            return Err(NetError::Protocol("expected AuthProof"));
        };

        let Some(entry) = trust.lookup(&peer, now) else {
            ctl.reject(RejectReason::Untrusted).await;
            return Err(NetError::Untrusted);
        };
        let profile = entry.profile;
        let msg = trusted_auth_message(me, peer, timestamp, &nonce);
        if peer.verify(&msg, &sig).is_err() {
            ctl.reject(RejectReason::BadSignature).await;
            return Err(NetError::BadSignature);
        }
        if timestamp.abs_diff(now) > MAX_CLOCK_SKEW_SECS {
            ctl.reject(RejectReason::StaleTimestamp).await;
            return Err(NetError::StaleTimestamp);
        }
        ctl.send(&Msg::Result(None)).await?;
        Ok(TrustedOutcome {
            peer,
            profile,
            control: ctl,
        })
    })
    .await
}

/// `PROTOCOL || label || host id || controller id || timestamp BE || nonce`.
fn trusted_auth_message(
    host: DeviceId,
    controller: DeviceId,
    ts: u64,
    nonce: &[u8; 32],
) -> Vec<u8> {
    let mut m = Vec::with_capacity(PROTOCOL.len() + TRUSTED_AUTH_LABEL.len() + 32 * 3 + 8);
    m.extend_from_slice(PROTOCOL.as_bytes());
    m.extend_from_slice(TRUSTED_AUTH_LABEL);
    m.extend_from_slice(&host.0);
    m.extend_from_slice(&controller.0);
    m.extend_from_slice(&ts.to_be_bytes());
    m.extend_from_slice(nonce);
    m
}

async fn controller_hello(conn: &Connection, intent: u8) -> Result<ControlStream> {
    let (send, recv) = open_stream(conn, StreamKind::Control).await?;
    let mut ctl = ControlStream {
        send,
        recv,
        version: 0,
    };
    ctl.send(&our_hello(intent)).await?;
    match ctl.recv().await? {
        Msg::Hello { min, max, .. } => {
            ctl.version = negotiate(min, max).ok_or(NetError::VersionMismatch)?;
            Ok(ctl)
        }
        Msg::Result(Some(r)) => Err(pair_rejection(r)),
        _ => Err(NetError::Protocol("expected Hello")),
    }
}

async fn host_hello(conn: &Connection, expected_intent: u8) -> Result<ControlStream> {
    let (kind, send, recv) = accept_stream(conn).await?;
    if kind != StreamKind::Control {
        return Err(NetError::Protocol("first stream must be Control"));
    }
    let mut ctl = ControlStream {
        send,
        recv,
        version: 0,
    };
    let Msg::Hello { min, max, intent } = ctl.recv().await? else {
        return Err(NetError::Protocol("expected Hello"));
    };
    ctl.send(&our_hello(expected_intent)).await?;
    let Some(version) = negotiate(min, max) else {
        ctl.reject(RejectReason::VersionMismatch).await;
        return Err(NetError::VersionMismatch);
    };
    if intent != expected_intent {
        ctl.reject(RejectReason::WrongMode).await;
        return Err(NetError::Rejected(RejectReason::WrongMode));
    }
    ctl.version = version;
    Ok(ctl)
}

fn our_hello(intent: u8) -> Msg {
    Msg::Hello {
        min: PROTOCOL_VERSION_MIN,
        max: PROTOCOL_VERSION_MAX,
        intent,
    }
}

/// Highest version both ranges contain.
fn negotiate(peer_min: u16, peer_max: u16) -> Option<u16> {
    let hi = peer_max.min(PROTOCOL_VERSION_MAX);
    let lo = peer_min.max(PROTOCOL_VERSION_MIN);
    (lo <= hi).then_some(hi)
}

async fn timed<T>(fut: impl Future<Output = Result<T>>) -> Result<T> {
    tokio::time::timeout(HANDSHAKE_TIMEOUT, fut)
        .await
        .map_err(|_| NetError::Timeout)?
}

fn random_nonce() -> Result<[u8; 32]> {
    let mut n = [0u8; 32];
    getrandom::fill(&mut n).map_err(|e| NetError::Random(e.to_string()))?;
    Ok(n)
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_round_trip() {
        let msgs = [
            Msg::Hello {
                min: 1,
                max: 3,
                intent: INTENT_TRUSTED,
            },
            Msg::PairStart(vec![1, 2, 3]),
            Msg::PairConfirm([9; 32]),
            Msg::Result(None),
            Msg::Result(Some(RejectReason::Untrusted)),
            Msg::Result(Some(RejectReason::Other(200))),
            Msg::AuthChallenge([4; 32]),
            Msg::AuthProof {
                timestamp: 1_800_000_000,
                sig: [5; 64],
            },
        ];
        for m in msgs {
            assert_eq!(Msg::decode(&m.encode()).expect("decodes"), m);
        }
    }

    #[test]
    fn malformed_messages_are_rejected() {
        assert!(Msg::decode(&[]).is_err());
        assert!(Msg::decode(&[TAG_HELLO, 0, 1]).is_err());
        assert!(Msg::decode(&[TAG_PAIR_START]).is_err());
        assert!(Msg::decode(&[TAG_PAIR_CONFIRM, 1, 2]).is_err());
        assert!(Msg::decode(&[TAG_RESULT]).is_err());
        assert!(Msg::decode(&[TAG_AUTH_PROOF, 0]).is_err());
        assert!(Msg::decode(&[99, 0]).is_err());
    }

    #[test]
    fn version_negotiation() {
        assert_eq!(negotiate(1, 1), Some(1));
        assert_eq!(negotiate(1, 9), Some(PROTOCOL_VERSION_MAX));
        assert_eq!(negotiate(2, 9), None);
        assert_eq!(negotiate(0, 0), None);
    }

    #[test]
    fn signed_message_binds_every_field() {
        let h = DeviceId([1; 32]);
        let c = DeviceId([2; 32]);
        let base = trusted_auth_message(h, c, 10, &[3; 32]);
        assert!(base.starts_with(PROTOCOL.as_bytes()));
        assert_ne!(base, trusted_auth_message(c, h, 10, &[3; 32]));
        assert_ne!(base, trusted_auth_message(h, c, 11, &[3; 32]));
        assert_ne!(base, trusted_auth_message(h, c, 10, &[4; 32]));
    }

    #[test]
    fn host_code_is_single_use() {
        let slot = HostCode::new(OneTimeCode::generate().expect("rng"));
        assert!(!slot.is_consumed());
        assert!(slot.take().is_ok());
        assert!(slot.is_consumed());
        assert!(matches!(slot.take(), Err(NetError::CodeConsumed)));
    }

    #[test]
    fn expired_code_is_consumed_and_refused() {
        let slot = HostCode::new(OneTimeCode::generate_with_ttl(Duration::ZERO).expect("rng"));
        assert!(matches!(slot.take(), Err(NetError::CodeExpired)));
        assert!(slot.is_consumed());
    }
}
