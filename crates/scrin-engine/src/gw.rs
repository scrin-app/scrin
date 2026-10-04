//! Host side of browser sessions bridged by the scrin-server gateway
//! (ALPN [`GW_ALPN`]). The contract is `docs/protocol/gateway-session.md`
//! (inner layer) and `crates/scrin-server/GATEWAY.md` (outer layer); this
//! module implements its host half.
//!
//! The iroh peer of a `scrin-gw/1` connection is the **gateway**, an
//! untrusted pipe; its endpoint id is ignored. The controller (a browser) is
//! identified by the Ed25519 key in its `Identify`, bound by SPAKE2 and
//! proven by its `Attest` signature. Gateway sessions are always anonymous
//! (60-minute cap, no unattended setup; ADR-0009).
//!
//! Streams start with a 3-byte plaintext header `kind u8 ‖ ordinal u16 BE`;
//! lane = `opener_is_host << 24 | kind << 16 | ordinal`. Handshake on the
//! Control stream (`00 00 00`), frames `u32 BE len ‖ tag ‖ payload`:
//!
//! ```text
//! C→H Hello(1,1,0)       H→C Hello(1,1,0)
//! C→H Identify(C id)     H→C Identify(H id)
//! C→H PairStart(mC)      H→C PairStart(mH)     ← the code is consumed here
//! C→H PairConfirm(tagC)  H→C PairConfirm(tagH) (or Result(reason))
//! C→H Attest(sigC)       H→C Attest(sigH)      (or Result(reason))
//! C→H Result(0)
//! ```
//!
//! Afterwards every Control/Input frame body is `seal(lane, envelope)` and
//! every datagram `seal(0xFFFFFFFF, shard)` with [`scrin_crypto::channel`]
//! keyed by `Paired::export("scrin gateway channel v1")`.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use iroh::endpoint::{Connection, RecvStream, SendStream, VarInt};
use scrin_crypto::PROTOCOL;
use scrin_crypto::channel::{self, Opener, Ordering, Sealer, Side};
use scrin_crypto::code::OneTimeCode;
use scrin_crypto::identity::{DeviceId, Identity};
use scrin_crypto::pake::{Pairing, Role};
use scrin_crypto::sas::Sas;
use scrin_net::NetError;
use scrin_net::framing::{StreamKind, UNKNOWN_KIND_CODE, read_frame_capped, write_frame};
use scrin_net::handshake::{PROTOCOL_VERSION_MAX, PROTOCOL_VERSION_MIN};
use zeroize::Zeroizing;

/// ALPN the gateway dials hosts with.
pub const GW_ALPN: &[u8] = b"scrin-gw/1";
/// `Paired::export` context of the channel secret.
pub const CHANNEL_CONTEXT: &str = "scrin gateway channel v1";
/// Lane of every datagram (both directions, unordered).
pub const DATAGRAM_LANE: u32 = 0xFFFF_FFFF;
/// Lane of the Control stream (controller-opened, kind 0, ordinal 0).
pub const CONTROL_LANE: u32 = 0;
/// Domain separator of the `Attest` signatures.
pub const ATTEST_LABEL: &[u8] = b"/gateway attest v1";

/// Application close codes of gateway sessions (contract §6; < 0x100, so the
/// gateway passes them through).
pub mod close {
    pub const NORMAL: u32 = 0x00;
    pub const PROTOCOL: u32 = 0x01;
    pub const PAIRING_FAILED: u32 = 0x02;
    pub const SAS_MISMATCH: u32 = 0x03;
    pub const TIMEOUT: u32 = 0x04;
    /// Not in the contract table yet: the host is in another session.
    pub const BUSY: u32 = 0x10;
}

pub const TAG_HELLO: u8 = 1;
pub const TAG_PAIR_START: u8 = 2;
pub const TAG_PAIR_CONFIRM: u8 = 3;
pub const TAG_RESULT: u8 = 4;
pub const TAG_IDENTIFY: u8 = 7;
pub const TAG_ATTEST: u8 = 8;

/// `RejectReason` bytes (same values as `scrin_net::handshake`).
pub mod reject {
    pub const WRONG_CODE: u8 = 1;
    pub const CODE_UNAVAILABLE: u8 = 2;
    pub const VERSION_MISMATCH: u8 = 3;
    pub const BAD_SIGNATURE: u8 = 5;
    pub const WRONG_MODE: u8 = 7;
}

const INTENT_PAIR: u8 = 0;
const MAX_MSG: usize = 4096;
const TIMEOUT: Duration = Duration::from_secs(30);
const HEADER_TIMEOUT: Duration = Duration::from_secs(10);

/// Lane of a stream: `opener_is_host << 24 | kind << 16 | ordinal`.
#[must_use]
pub const fn stream_lane(host_opened: bool, kind: StreamKind, ordinal: u16) -> u32 {
    ((host_opened as u32) << 24) | ((kind.as_byte() as u32) << 16) | ordinal as u32
}

/// Bytes each side signs in its `Attest`:
/// `PROTOCOL ‖ label ‖ signer ('C' | 'H') ‖ H id ‖ C id ‖ tagC ‖ tagH`.
#[must_use]
pub fn attest_message(
    signer_is_host: bool,
    host: &DeviceId,
    controller: &DeviceId,
    controller_tag: &[u8; 32],
    host_tag: &[u8; 32],
) -> Vec<u8> {
    let mut m = Vec::with_capacity(PROTOCOL.len() + ATTEST_LABEL.len() + 129);
    m.extend_from_slice(PROTOCOL.as_bytes());
    m.extend_from_slice(ATTEST_LABEL);
    m.push(if signer_is_host { b'H' } else { b'C' });
    m.extend_from_slice(&host.0);
    m.extend_from_slice(&controller.0);
    m.extend_from_slice(controller_tag);
    m.extend_from_slice(host_tag);
    m
}

/// The inner end-to-end channel of one gateway session (host side).
pub struct Channel {
    secret: Zeroizing<[u8; 32]>,
    side: Side,
    sealers: Mutex<HashMap<u32, Sealer>>,
    openers: Mutex<HashMap<u32, Opener>>,
    /// Closed with [`close::PROTOCOL`] when a stream frame fails to open.
    conn: Option<Connection>,
}

impl std::fmt::Debug for Channel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Channel")
            .field("side", &self.side)
            .finish_non_exhaustive()
    }
}

const fn ordering(lane: u32) -> Ordering {
    if lane == DATAGRAM_LANE {
        Ordering::Unordered
    } else {
        Ordering::Ordered
    }
}

impl Channel {
    #[must_use]
    pub fn new(secret: Zeroizing<[u8; 32]>, side: Side, conn: Option<Connection>) -> Self {
        Self {
            secret,
            side,
            sealers: Mutex::default(),
            openers: Mutex::default(),
            conn,
        }
    }

    pub fn seal(&self, lane: u32, plaintext: &[u8]) -> scrin_crypto::Result<Vec<u8>> {
        let mut map = self.sealers.lock().unwrap_or_else(PoisonError::into_inner);
        map.entry(lane)
            .or_insert_with(|| channel::lane(&self.secret, self.side, lane, ordering(lane)).0)
            .seal(plaintext)
    }

    pub fn open(&self, lane: u32, sealed: &[u8]) -> scrin_crypto::Result<Vec<u8>> {
        let mut map = self.openers.lock().unwrap_or_else(PoisonError::into_inner);
        map.entry(lane)
            .or_insert_with(|| channel::lane(&self.secret, self.side, lane, ordering(lane)).1)
            .open(sealed)
    }

    /// Opens a stream frame; on failure closes the session with `PROTOCOL`
    /// (contract §4: a frame that fails to open is fatal).
    pub(crate) fn open_stream_frame(&self, lane: u32, sealed: &[u8]) -> Option<Vec<u8>> {
        let opened = self.open(lane, sealed).ok();
        if opened.is_none()
            && let Some(c) = &self.conn
        {
            c.close(VarInt::from_u32(close::PROTOCOL), b"bad frame");
        }
        opened
    }
}

/// Optional sealing: `None` on the native path (QUIC is end to end there).
pub(crate) type Seal = Option<Arc<Channel>>;

/// The one-time code as the gateway path consumes it. Handshakes are
/// serialised by the engine (one at a time) and either path consuming it
/// rotates both, so a code still allows exactly one guess.
#[derive(Debug)]
pub(crate) struct GwCode(Mutex<Option<OneTimeCode>>);

impl GwCode {
    pub(crate) fn new(code: Option<OneTimeCode>) -> Self {
        Self(Mutex::new(code))
    }

    pub(crate) fn is_consumed(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_none()
    }

    fn take(&self) -> Result<OneTimeCode, NetError> {
        let code = self
            .0
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

/// A paired gateway controller.
#[derive(Debug)]
pub(crate) struct GwOutcome {
    /// The controller key, proven by `Attest`.
    pub peer: DeviceId,
    pub sas: Sas,
    pub send: SendStream,
    pub recv: RecvStream,
    pub channel: Arc<Channel>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Msg {
    Hello { min: u16, max: u16, intent: u8 },
    PairStart(Vec<u8>),
    PairConfirm([u8; 32]),
    Result(u8),
    Identify([u8; 32]),
    Attest([u8; 64]),
}

impl Msg {
    fn encode(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(72);
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
            Self::Result(r) => v.extend_from_slice(&[TAG_RESULT, *r]),
            Self::Identify(k) => {
                v.push(TAG_IDENTIFY);
                v.extend_from_slice(k);
            }
            Self::Attest(s) => {
                v.push(TAG_ATTEST);
                v.extend_from_slice(s);
            }
        }
        v
    }

    fn decode(buf: &[u8]) -> Result<Self, NetError> {
        let bad = || NetError::Protocol("malformed gateway handshake message");
        let (&tag, body) = buf.split_first().ok_or_else(bad)?;
        Ok(match tag {
            TAG_HELLO => match body {
                [a, b, c, d, intent] => Self::Hello {
                    min: u16::from_be_bytes([*a, *b]),
                    max: u16::from_be_bytes([*c, *d]),
                    intent: *intent,
                },
                _ => return Err(bad()),
            },
            TAG_PAIR_START if !body.is_empty() => Self::PairStart(body.to_vec()),
            TAG_PAIR_CONFIRM => Self::PairConfirm(body.try_into().map_err(|_| bad())?),
            TAG_RESULT => match body {
                [r] => Self::Result(*r),
                _ => return Err(bad()),
            },
            TAG_IDENTIFY => Self::Identify(body.try_into().map_err(|_| bad())?),
            TAG_ATTEST => Self::Attest(body.try_into().map_err(|_| bad())?),
            _ => return Err(bad()),
        })
    }
}

/// Reads the 3-byte stream header of an accepted bi-stream.
async fn read_header(recv: &mut RecvStream) -> Option<(u8, u16)> {
    let mut h = [0u8; 3];
    match tokio::time::timeout(HEADER_TIMEOUT, recv.read_exact(&mut h)).await {
        Ok(Ok(())) => Some((h[0], u16::from_be_bytes([h[1], h[2]]))),
        _ => None,
    }
}

/// Accepts the next controller-opened bi-stream of a known kind with a lane
/// not used before; others are reset with `0x5c01` and skipped. Errors only
/// when the connection ends.
pub(crate) async fn accept_stream(
    conn: &Connection,
    used: &mut HashSet<u32>,
) -> Result<(StreamKind, u32, SendStream, RecvStream), NetError> {
    loop {
        let (mut send, mut recv) = conn
            .accept_bi()
            .await
            .map_err(|e| NetError::Connection(e.to_string()))?;
        let header = read_header(&mut recv).await;
        let known = header.and_then(|(k, ord)| Some((StreamKind::from_byte(k)?, ord)));
        if let Some((kind, ord)) = known {
            let lane = stream_lane(false, kind, ord);
            if used.insert(lane) {
                return Ok((kind, lane, send, recv));
            }
        }
        let _ = recv.stop(UNKNOWN_KIND_CODE);
        let _ = send.reset(UNKNOWN_KIND_CODE);
    }
}

struct Ctl {
    send: SendStream,
    recv: RecvStream,
}

impl Ctl {
    async fn send(&mut self, m: &Msg) -> Result<(), NetError> {
        write_frame(&mut self.send, &m.encode()).await
    }

    async fn recv(&mut self) -> Result<Msg, NetError> {
        let buf = read_frame_capped(&mut self.recv, MAX_MSG)
            .await?
            .ok_or(NetError::StreamClosed)?;
        Msg::decode(&buf)
    }

    async fn reject(&mut self, r: u8) {
        let _ = self.send(&Msg::Result(r)).await;
    }
}

fn negotiate(min: u16, max: u16) -> Option<u16> {
    let hi = max.min(PROTOCOL_VERSION_MAX);
    let lo = min.max(PROTOCOL_VERSION_MIN);
    (lo <= hi).then_some(hi)
}

/// The close code that matches a handshake failure.
#[must_use]
pub(crate) fn close_code(e: &NetError) -> u32 {
    match e {
        NetError::PairingFailed | NetError::BadSignature => close::PAIRING_FAILED,
        NetError::Timeout => close::TIMEOUT,
        _ => close::PROTOCOL,
    }
}

/// Host side of the gateway handshake. On error, the controller key from
/// `Identify` (if one arrived) for the failure report.
pub(crate) async fn host_pair(
    conn: &Connection,
    me: &Identity,
    code: &GwCode,
) -> Result<GwOutcome, (Option<DeviceId>, NetError)> {
    let mut peer = None;
    match tokio::time::timeout(TIMEOUT, run_host(conn, me, code, &mut peer)).await {
        Ok(Ok(o)) => Ok(o),
        Ok(Err(e)) => Err((peer, e)),
        Err(_) => Err((peer, NetError::Timeout)),
    }
}

async fn run_host(
    conn: &Connection,
    me: &Identity,
    code: &GwCode,
    peer_out: &mut Option<DeviceId>,
) -> Result<GwOutcome, NetError> {
    let (mut send, mut recv) = conn
        .accept_bi()
        .await
        .map_err(|e| NetError::Connection(e.to_string()))?;
    if read_header(&mut recv).await != Some((StreamKind::Control.as_byte(), 0)) {
        let _ = recv.stop(UNKNOWN_KIND_CODE);
        let _ = send.reset(UNKNOWN_KIND_CODE);
        return Err(NetError::Protocol("first stream must be Control 00 00 00"));
    }
    let mut ctl = Ctl { send, recv };
    let host = me.device_id();

    let Msg::Hello { min, max, intent } = ctl.recv().await? else {
        return Err(NetError::Protocol("expected Hello"));
    };
    ctl.send(&Msg::Hello {
        min: PROTOCOL_VERSION_MIN,
        max: PROTOCOL_VERSION_MAX,
        intent: INTENT_PAIR,
    })
    .await?;
    if negotiate(min, max).is_none() {
        ctl.reject(reject::VERSION_MISMATCH).await;
        return Err(NetError::VersionMismatch);
    }
    if intent != INTENT_PAIR {
        // Trusted access is not offered over the gateway (contract §3).
        ctl.reject(reject::WRONG_MODE).await;
        return Err(NetError::Rejected(
            scrin_net::handshake::RejectReason::WrongMode,
        ));
    }

    let Msg::Identify(key) = ctl.recv().await? else {
        return Err(NetError::Protocol("expected Identify"));
    };
    if iroh::PublicKey::from_bytes(&key).is_err() {
        return Err(NetError::Protocol("controller key is not an Ed25519 key"));
    }
    let peer = DeviceId(key);
    *peer_out = Some(peer);
    ctl.send(&Msg::Identify(host.0)).await?;

    let Msg::PairStart(peer_msg) = ctl.recv().await? else {
        return Err(NetError::Protocol("expected PairStart"));
    };
    let code = match code.take() {
        Ok(c) => c,
        Err(e) => {
            ctl.reject(reject::CODE_UNAVAILABLE).await;
            return Err(e);
        }
    };
    let (pairing, my_msg) = Pairing::start(code.as_str(), Role::Host, host, peer);
    drop(code);
    ctl.send(&Msg::PairStart(my_msg)).await?;
    let Ok(paired) = pairing.finish(&peer_msg) else {
        ctl.reject(reject::WRONG_CODE).await;
        return Err(NetError::PairingFailed);
    };

    let Msg::PairConfirm(tag_c) = ctl.recv().await? else {
        return Err(NetError::Protocol("expected PairConfirm"));
    };
    if paired.verify_peer(&tag_c).is_err() {
        ctl.reject(reject::WRONG_CODE).await;
        return Err(NetError::PairingFailed);
    }
    let tag_h = paired.confirmation();
    ctl.send(&Msg::PairConfirm(tag_h)).await?;

    let Msg::Attest(sig_c) = ctl.recv().await? else {
        return Err(NetError::Protocol("expected Attest"));
    };
    if peer
        .verify(&attest_message(false, &host, &peer, &tag_c, &tag_h), &sig_c)
        .is_err()
    {
        ctl.reject(reject::BAD_SIGNATURE).await;
        return Err(NetError::BadSignature);
    }
    let sig_h = me.sign(&attest_message(true, &host, &peer, &tag_c, &tag_h));
    ctl.send(&Msg::Attest(sig_h)).await?;
    match ctl.recv().await? {
        Msg::Result(0) => {}
        Msg::Result(_) => return Err(NetError::PairingFailed),
        _ => return Err(NetError::Protocol("expected Result")),
    }
    let channel = Arc::new(Channel::new(
        paired.export(CHANNEL_CONTEXT),
        Side::Host,
        Some(conn.clone()),
    ));
    Ok(GwOutcome {
        peer,
        sas: paired.sas(),
        send: ctl.send,
        recv: ctl.recv,
        channel,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_round_trip() {
        for m in [
            Msg::Hello {
                min: 1,
                max: 2,
                intent: 0,
            },
            Msg::PairStart(vec![1, 2, 3]),
            Msg::PairConfirm([4; 32]),
            Msg::Result(0),
            Msg::Result(reject::WRONG_CODE),
            Msg::Identify([5; 32]),
            Msg::Attest([6; 64]),
        ] {
            assert_eq!(Msg::decode(&m.encode()).expect("decode"), m);
        }
        assert_eq!(
            Msg::Hello {
                min: 1,
                max: 1,
                intent: 0
            }
            .encode(),
            [1, 0, 1, 0, 1, 0]
        );
        assert!(Msg::decode(&[TAG_IDENTIFY, 1, 2]).is_err());
        assert!(Msg::decode(&[]).is_err());
        assert!(Msg::decode(&[9]).is_err());
    }

    #[test]
    fn lanes_follow_the_contract() {
        assert_eq!(stream_lane(false, StreamKind::Control, 0), CONTROL_LANE);
        assert_eq!(stream_lane(false, StreamKind::Input, 0), 0x0001_0000);
        assert_eq!(stream_lane(true, StreamKind::File, 2), 0x0103_0002);
    }

    #[test]
    fn attest_binds_signer_and_both_tags() {
        let h = DeviceId([1; 32]);
        let c = DeviceId([2; 32]);
        let m = attest_message(false, &h, &c, &[3; 32], &[4; 32]);
        assert_eq!(m.len(), PROTOCOL.len() + ATTEST_LABEL.len() + 129);
        assert_ne!(m, attest_message(true, &h, &c, &[3; 32], &[4; 32]));
        assert_ne!(m, attest_message(false, &h, &c, &[4; 32], &[3; 32]));
    }

    #[test]
    fn channel_sides_interoperate_per_lane() {
        let host = Channel::new(Zeroizing::new([5; 32]), Side::Host, None);
        let ctl = Channel::new(Zeroizing::new([5; 32]), Side::Controller, None);
        let s = host.seal(CONTROL_LANE, b"accept").expect("seal");
        assert_eq!(ctl.open(CONTROL_LANE, &s).expect("open"), b"accept");
        let x = host.seal(CONTROL_LANE, b"x").expect("seal");
        assert!(ctl.open(0x0001_0000, &x).is_err(), "lane bound");
        let a = host.seal(DATAGRAM_LANE, b"a").expect("seal");
        let b = host.seal(DATAGRAM_LANE, b"b").expect("seal");
        assert_eq!(ctl.open(DATAGRAM_LANE, &b).expect("b"), b"b");
        assert_eq!(ctl.open(DATAGRAM_LANE, &a).expect("a"), b"a");
    }

    #[test]
    fn gw_code_is_single_use() {
        let c = GwCode::new(Some(OneTimeCode::generate().expect("rng")));
        assert!(!c.is_consumed());
        assert!(c.take().is_ok());
        assert!(matches!(c.take(), Err(NetError::CodeConsumed)));
    }
}
