//! End to end against a REAL in-process scrin-server (rendezvous + relay +
//! gateway on 127.0.0.1:0):
//!
//! 1. host registers and heartbeats → controller resolves the 9-digit ID with
//!    a signed request → pairs with the code **through the server's relay**
//!    (direct addresses are not advertised) → frames flow → end;
//! 2. a wrong code makes the host report the failure → the server's
//!    `scrin_failure_reports_total` moves;
//! 3. a Rust "browser" pairs with the host through the WebSocket gateway
//!    with the browser core (`scrin_wasm::core`) per
//!    `docs/protocol/gateway-session.md` (Identify, SPAKE2, Attest, sealed
//!    lanes): sealed `SessionRequest` → `SessionAccept`, `VideoConfig`,
//!    sealed video datagrams, and sealed input on the Input stream.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use bytes::{BufMut, Bytes, BytesMut};
use futures_util::{SinkExt as _, StreamExt as _};
use scrin_crypto::identity::DeviceId;
use scrin_engine::gw::{self, DATAGRAM_LANE};
use scrin_engine::secret::FileStore;
use scrin_engine::{
    Command, EngineConfig, EngineHandle, Event, NetConfig, RelayConfig, Reply, SessionState,
    SyntheticBackend,
};
use scrin_media::fec::{FrameReassembler, ShardHeader};
use scrin_proto::v1::{self, envelope::Payload};
use scrin_server::{Config, Role as SrvRole, Server, TlsMode};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio_tungstenite::tungstenite::Message;

const T: Duration = Duration::from_secs(20);

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let mut n = [0u8; 8];
    getrandom::fill(&mut n).expect("rng");
    std::env::temp_dir().join(format!("scrin-engine-srv-{tag}-{}", u64::from_le_bytes(n)))
}

async fn server() -> Server {
    let mut cfg = Config::for_tests();
    cfg.roles = vec![SrvRole::Rendezvous, SrvRole::Relay, SrvRole::Gateway];
    cfg.tls = Some(TlsMode::None);
    cfg.presence_ttl = 60;
    Server::start(cfg).await.expect("server starts")
}

/// An engine using `srv` for rendezvous and as its only relay. Direct
/// addresses are bound on loopback but never advertised, so every dial
/// through the server goes via the relay.
async fn engine(
    tag: &str,
    srv: &Server,
    backend: SyntheticBackend,
    advertise_direct: bool,
) -> (EngineHandle, UnboundedReceiver<Event>) {
    let dir = temp_dir(tag);
    let base = format!("http://{}", srv.tcp_addr);
    let mut cfg = EngineConfig::new(&dir);
    cfg.secrets = Box::new(FileStore::new(dir.join("secrets")));
    cfg.net = NetConfig {
        relay: RelayConfig::Custom(vec![base.parse().expect("relay url")]),
        bind_addr: Some(SocketAddr::from(([127, 0, 0, 1], 0))),
    };
    cfg.server = Some(base);
    cfg.advertise_direct = advertise_direct;
    cfg.backend = Arc::new(backend);
    cfg.device_name = tag.into();
    cfg.policy.anonymous_accept_delay_ms = 0;
    scrin_engine::start(cfg).await.expect("engine starts")
}

async fn wait_for<R>(
    rx: &mut UnboundedReceiver<Event>,
    mut f: impl FnMut(&Event) -> Option<R>,
) -> R {
    tokio::time::timeout(T, async {
        loop {
            let e = rx.recv().await.expect("engine alive");
            if let Some(v) = f(&e) {
                return v;
            }
        }
    })
    .await
    .expect("event in time")
}

/// Waits until the engine reports itself registered and online.
async fn registered(h: &EngineHandle, rx: &mut UnboundedReceiver<Event>) -> String {
    let s = h.status().await.expect("status");
    if s.online {
        return s.scrin_id;
    }
    wait_for(rx, |e| match e {
        Event::Status(s) if s.online => Some(s.scrin_id.clone()),
        _ => None,
    })
    .await
}

async fn metric(srv: &Server, name: &str) -> u64 {
    let body = scrin_engine::rendezvous::http_client()
        .expect("client")
        .get(format!("http://{}/metrics", srv.tcp_addr))
        .send()
        .await
        .expect("metrics")
        .text()
        .await
        .expect("text");
    body.lines()
        .find_map(|l| l.strip_prefix(&format!("{name} ")))
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn register_resolve_pair_over_relay_and_stream() {
    let srv = server().await;
    let (host, mut host_ev) = engine("host", &srv, SyntheticBackend::new(320, 180), false).await;
    let (ctl, mut ctl_ev) = engine("ctl", &srv, SyntheticBackend::new(64, 64), false).await;

    let host_id = registered(&host, &mut host_ev).await;
    registered(&ctl, &mut ctl_ev).await;
    let status = host.status().await.expect("status");
    assert_eq!(status.scrin_id, host_id);
    assert_ne!(
        host_id,
        scrin_engine::provisional_scrin_id(&DeviceId(hex32(&status.device_id))),
        "the server-assigned id replaces the provisional one"
    );
    assert!(metric(&srv, "scrin_registrations_total").await >= 2);

    let sess = match ctl
        .call(Command::Connect {
            target: host_id.clone(),
            code: status.code.clone(),
            requested: None,
        })
        .await
        .expect("connect")
    {
        Reply::Session(s) => s,
        other => panic!("{other:?}"),
    };
    let h_sess = wait_for(&mut host_ev, |e| match e {
        Event::IncomingRequest { session, kind, .. } => {
            assert_eq!(kind, "anonymous");
            Some(session.clone())
        }
        _ => None,
    })
    .await;
    assert!(
        metric(&srv, "scrin_resolves_total").await >= 1,
        "signed resolve hit the server"
    );
    ctl.call(Command::ConfirmSas {
        session: sess.clone(),
        matches: true,
    })
    .await
    .expect("sas");
    host.call(Command::Accept {
        session: h_sess,
        permissions: vec!["view".into(), "input".into()],
    })
    .await
    .expect("accept");

    let mut frames = 0;
    tokio::time::timeout(T, async {
        while frames < 20 {
            if let Some(Event::VideoFrame { frame, .. }) = ctl_ev.recv().await {
                assert_eq!((frame.width, frame.height), (320, 180));
                frames += 1;
            }
        }
    })
    .await
    .expect("frames over the relay");
    assert!(
        metric(&srv, "scrin_relay_active_connections").await >= 2,
        "both engines sit on the server's relay"
    );

    ctl.call(Command::EndSession { session: sess })
        .await
        .expect("end");
    let reason = wait_for(&mut host_ev, |e| match e {
        Event::StateChanged {
            state: SessionState::Ended,
            reason,
            ..
        } => Some(reason.clone()),
        _ => None,
    })
    .await;
    assert_eq!(reason.as_deref(), Some("peer-ended"));
    ctl.shutdown().await;
    host.shutdown().await;
    srv.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn wrong_code_is_reported_to_the_server() {
    let srv = server().await;
    let (host, mut host_ev) = engine("host-w", &srv, SyntheticBackend::default(), false).await;
    let (ctl, mut ctl_ev) = engine("ctl-w", &srv, SyntheticBackend::default(), false).await;
    let host_id = registered(&host, &mut host_ev).await;
    registered(&ctl, &mut ctl_ev).await;
    let before = metric(&srv, "scrin_failure_reports_total").await;

    let code = host.status().await.expect("status").code;
    let raw: String = code.chars().filter(|c| *c != '-').collect();
    let first = if raw.starts_with('A') { 'B' } else { 'A' };
    let wrong: String = std::iter::once(first).chain(raw.chars().skip(1)).collect();
    ctl.call(Command::Connect {
        target: host_id,
        code: wrong,
        requested: None,
    })
    .await
    .expect("connect");
    let err = wait_for(&mut ctl_ev, |e| match e {
        Event::Error { code, .. } => Some(code.clone()),
        _ => None,
    })
    .await;
    assert_eq!(err, "wrong-code");

    let deadline = tokio::time::Instant::now() + T;
    let mut after = before;
    while after == before && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(100)).await;
        after = metric(&srv, "scrin_failure_reports_total").await;
    }
    assert_eq!(
        after,
        before + 1,
        "host reported exactly one failed pairing"
    );
    ctl.shutdown().await;
    host.shutdown().await;
    srv.shutdown().await;
}

fn hex32(s: &str) -> [u8; 32] {
    data_encoding::HEXLOWER_PERMISSIVE
        .decode(s.as_bytes())
        .expect("hex")
        .try_into()
        .expect("32")
}

// ---- a minimal browser over the WebSocket gateway framing ------------------

const TAG_DATA: u8 = 0x00;
const TAG_DGRAM: u8 = 0x01;

fn put_varint(out: &mut BytesMut, v: u64) {
    if v < 1 << 6 {
        out.put_u8(u8::try_from(v).expect("small"));
    } else {
        out.put_u16(u16::try_from(v).expect("fits") | 0x4000);
    }
}

fn get_varint(b: &[u8]) -> (u64, usize) {
    let len = 1usize << (b[0] >> 6);
    let mut v = u64::from(b[0] & 0x3f);
    for x in &b[1..len] {
        v = (v << 8) | u64::from(*x);
    }
    (v, len)
}

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Stream 0 (browser bidi #0) is Control. Reassembles length-prefixed frames.
struct Browser {
    ws: Ws,
    rx: HashMap<u64, Vec<u8>>,
    dgrams: Vec<Bytes>,
}

impl Browser {
    async fn send_stream(&mut self, id: u64, bytes: &[u8]) {
        let mut m = BytesMut::new();
        m.put_u8(TAG_DATA);
        put_varint(&mut m, id);
        m.extend_from_slice(bytes);
        self.ws
            .send(Message::Binary(m.freeze()))
            .await
            .expect("ws send");
    }

    async fn send_frame(&mut self, id: u64, payload: &[u8]) {
        let mut b = u32::try_from(payload.len())
            .expect("len")
            .to_be_bytes()
            .to_vec();
        b.extend_from_slice(payload);
        self.send_stream(id, &b).await;
    }

    /// Next complete length-prefixed frame on stream `id`.
    async fn recv_frame(&mut self, id: u64) -> Vec<u8> {
        tokio::time::timeout(T, async {
            loop {
                if let Some(buf) = self.rx.get_mut(&id)
                    && buf.len() >= 4
                {
                    let n = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
                    if buf.len() >= 4 + n {
                        let frame = buf[4..4 + n].to_vec();
                        buf.drain(..4 + n);
                        return frame;
                    }
                }
                self.pump().await;
            }
        })
        .await
        .expect("frame in time")
    }

    async fn pump(&mut self) {
        let msg = self.ws.next().await.expect("open").expect("ok");
        let Message::Binary(b) = msg else { return };
        match b[0] {
            TAG_DATA => {
                let (id, n) = get_varint(&b[1..]);
                self.rx
                    .entry(id)
                    .or_default()
                    .extend_from_slice(&b[1 + n..]);
            }
            TAG_DGRAM => self.dgrams.push(b.slice(1..)),
            _ => {}
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn browser_pairs_through_the_websocket_gateway() {
    use scrin_wasm::core::{ControllerPairing, attest_message, seed_public_key, seed_sign};

    let srv = server().await;
    // The test server's gateway endpoint has no relay map (no public relay
    // URL is known before it binds), so the host advertises its loopback
    // sockets for the gateway to dial.
    let backend = SyntheticBackend::new(160, 90);
    let (host, mut host_ev) = engine("host-gw", &srv, backend.clone(), true).await;
    let host_id = registered(&host, &mut host_ev).await;
    let status = host.status().await.expect("status");
    let host_key = hex32(&status.device_id);

    let url = format!("ws://{}/v1/ws?id={host_id}", srv.tcp_addr);
    let (ws, _) = tokio::time::timeout(T, tokio_tungstenite::connect_async(&url))
        .await
        .expect("ws in time")
        .expect("ws connect");
    let mut b = Browser {
        ws,
        rx: HashMap::new(),
        dgrams: Vec::new(),
    };
    let seed = [0x42u8; 32];
    let me = seed_public_key(&seed).expect("pk");

    // Control stream 0: header 00 00 00, then the handshake (contract §3).
    b.send_stream(0, &[0, 0, 0]).await;
    b.send_frame(0, &[gw::TAG_HELLO, 0, 1, 0, 1, 0]).await;
    assert_eq!(b.recv_frame(0).await, [gw::TAG_HELLO, 0, 1, 0, 1, 0]);
    let mut ident = vec![gw::TAG_IDENTIFY];
    ident.extend_from_slice(&me);
    b.send_frame(0, &ident).await;
    let their = b.recv_frame(0).await;
    assert_eq!(their[0], gw::TAG_IDENTIFY);
    assert_eq!(
        &their[1..],
        &host_key,
        "host identifies with its device key"
    );

    let mut pairing =
        ControllerPairing::start(&status.code, &me, &host_key, &[7; 32]).expect("pake start");
    let mut ps = vec![gw::TAG_PAIR_START];
    ps.extend_from_slice(pairing.message());
    b.send_frame(0, &ps).await;
    let start = b.recv_frame(0).await;
    assert_eq!(start[0], gw::TAG_PAIR_START);
    let paired = pairing.finish(&start[1..]).expect("pake finish");
    let tag_c = paired.confirmation();
    let mut pc = vec![gw::TAG_PAIR_CONFIRM];
    pc.extend_from_slice(&tag_c);
    b.send_frame(0, &pc).await;
    let confirm = b.recv_frame(0).await;
    assert_eq!(confirm[0], gw::TAG_PAIR_CONFIRM);
    assert!(paired.verify_peer(&confirm[1..]), "host proves the code");
    let tag_h: [u8; 32] = confirm[1..].try_into().expect("32");

    let msg = attest_message(false, &host_key, &me, &tag_c, &tag_h).expect("attest msg");
    let mut at = vec![gw::TAG_ATTEST];
    at.extend_from_slice(&seed_sign(&seed, &msg).expect("sign"));
    b.send_frame(0, &at).await;
    let host_at = b.recv_frame(0).await;
    assert_eq!(host_at[0], gw::TAG_ATTEST);
    let host_msg = attest_message(true, &host_key, &me, &tag_c, &tag_h).expect("attest msg");
    let sig: [u8; 64] = host_at[1..].try_into().expect("64");
    DeviceId(host_key)
        .verify(&host_msg, &sig)
        .expect("host attests with its device key");
    b.send_frame(0, &[gw::TAG_RESULT, 0]).await;
    let mut chan = paired.channel();

    // Sealed SessionRequest → the host shows its dialog with the same SAS.
    let req = scrin_proto::encode_envelope(&scrin_proto::envelope(Payload::SessionRequest(
        v1::SessionRequest {
            requested: vec![
                i32::from(v1::Permission::View),
                i32::from(v1::Permission::Input),
            ],
            controller_name: "browser".into(),
            unattended: false,
        },
    )));
    b.send_frame(0, &chan.seal(gw::CONTROL_LANE, &req).expect("seal"))
        .await;
    let (h_sess, sas, peer) = wait_for(&mut host_ev, |e| match e {
        Event::IncomingRequest {
            session,
            sas,
            kind,
            peer,
            ..
        } => {
            assert_eq!(kind, "anonymous", "gateway sessions are always anonymous");
            Some((session.clone(), *sas, peer.clone()))
        }
        _ => None,
    })
    .await;
    assert_eq!(sas, Some(paired.sas()), "SAS matches across the gateway");
    assert_eq!(hex32(&peer), me, "the host shows the attested browser key");
    host.call(Command::Accept {
        session: h_sess,
        permissions: vec!["view".into(), "input".into()],
    })
    .await
    .expect("accept");

    // Sealed SessionAccept then VideoConfig on Control.
    let (mut got_accept, mut got_config) = (false, false);
    while !(got_accept && got_config) {
        let sealed = b.recv_frame(0).await;
        let plain = chan
            .open(gw::CONTROL_LANE, &sealed)
            .expect("host seals control frames");
        match scrin_proto::decode_envelope(&plain).expect("env").payload {
            Some(Payload::SessionAccept(a)) => {
                assert!(a.granted.contains(&i32::from(v1::Permission::View)));
                got_accept = true;
            }
            Some(Payload::VideoConfig(vc)) => {
                assert_eq!((vc.width, vc.height), (160, 90));
                got_config = true;
            }
            _ => {}
        }
    }

    // Sealed video datagrams reassemble into frames.
    let mut reasm = FrameReassembler::default();
    let mut frames = 0;
    tokio::time::timeout(T, async {
        while frames < 5 {
            for d in std::mem::take(&mut b.dgrams) {
                let plain = chan.open(DATAGRAM_LANE, &d).expect("sealed datagram");
                ShardHeader::decode(&plain).expect("shard header");
                if reasm.push(&plain).expect("shard").is_some() {
                    frames += 1;
                }
            }
            if frames < 5 {
                b.pump().await;
            }
        }
    })
    .await
    .expect("video through the gateway");

    // Input stream (browser bidi #1 = ws id 4): header 01 00 00, sealed on
    // lane 0x00010000.
    let key = v1::KeyEvent {
        hid_usage: 0x04,
        down: true,
        modifiers: 0,
        text: None,
        repeat: false,
    };
    let input =
        scrin_proto::encode_envelope(&scrin_proto::envelope(Payload::KeyEvent(key.clone())));
    b.send_stream(4, &[1, 0, 0]).await;
    b.send_frame(4, &chan.seal(0x0001_0000, &input).expect("seal"))
        .await;
    tokio::time::timeout(T, async {
        while !backend
            .injected()
            .contains(&scrin_engine::InputEvent::Key(key.clone()))
        {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("sealed input injected on the host");
    assert!(metric(&srv, "scrin_gateway_bytes_total").await > 0);

    let _ = b.ws.close(None).await;
    host.shutdown().await;
    srv.shutdown().await;
}
