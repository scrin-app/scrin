//! Two real iroh endpoints on loopback, relays and address lookup off.
//!
//! No internet, no n0 infrastructure: each endpoint binds 127.0.0.1:0 and the
//! dialler gets the other's bound socket directly.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use scrin_crypto::code::OneTimeCode;
use scrin_crypto::identity::Identity;
use scrin_crypto::trust::{Profile, TrustStore, TrustedPeer};
use scrin_net::datagram::{max_datagram_size, recv_datagram, send_datagram};
use scrin_net::framing::{accept_stream, open_stream, read_frame, write_frame};
use scrin_net::handshake::{
    HostCode, controller_auth_trusted, controller_pair, host_auth_trusted, host_pair,
};
use scrin_net::{
    ALPN, Connection, EndpointAddr, NetConfig, NetEndpoint, NetError, StreamKind, TransportAddr,
};

const T: Duration = Duration::from_secs(8);

async fn bind(seed: u8) -> (Identity, NetEndpoint) {
    let id = Identity::from_seed([seed; 32]);
    let ep = NetEndpoint::bind(*id.seed(), NetConfig::loopback())
        .await
        .expect("bind loopback");
    (id, ep)
}

fn dial_addr(ep: &NetEndpoint) -> EndpointAddr {
    let id = ep.inner().id();
    EndpointAddr::from_parts(id, ep.bound_sockets().into_iter().map(TransportAddr::Ip))
}

/// Connects `ctl` to `host`; returns (controller side, host side).
async fn connect(ctl: &NetEndpoint, host: &NetEndpoint) -> (Connection, Connection) {
    let accept = async {
        host.accept_connection()
            .await
            .expect("endpoint open")
            .expect("accepted")
    };
    let (c, h) = tokio::time::timeout(T, async {
        tokio::join!(ctl.connect(dial_addr(host)), accept)
    })
    .await
    .expect("connect in time");
    (c.expect("dialled"), h)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_secs()
}

#[tokio::test(flavor = "multi_thread")]
async fn device_id_is_the_iroh_endpoint_id_and_alpn_matches() {
    let (id, ep) = bind(1).await;
    assert_eq!(ep.device_id(), id.device_id());
    assert_eq!(ep.inner().id().as_bytes(), &id.device_id().0);
    assert_eq!(ALPN, scrin_crypto::PROTOCOL.as_bytes());
}

#[tokio::test(flavor = "multi_thread")]
async fn correct_code_pairs_and_both_sides_see_the_same_sas() {
    let (host_id, host) = bind(10).await;
    let (ctl_id, ctl) = bind(11).await;
    let otc = OneTimeCode::generate().expect("rng");
    let typed = otc.display().to_lowercase();
    let slot = HostCode::new(otc);

    let (c, h) = connect(&ctl, &host).await;
    let (hr, cr) = tokio::time::timeout(T, async {
        tokio::join!(
            host_pair(&h, host.device_id(), &slot),
            controller_pair(&c, ctl.device_id(), &typed)
        )
    })
    .await
    .expect("pair in time");
    let hr = hr.expect("host pairs");
    let cr = cr.expect("controller pairs");

    assert_eq!(hr.sas, cr.sas);
    assert_eq!(hr.peer, ctl_id.device_id());
    assert_eq!(cr.peer, host_id.device_id());
    assert_eq!(*hr.export("inner"), *cr.export("inner"));
    assert_eq!(hr.control.version, 1);
    assert!(slot.is_consumed());
}

#[tokio::test(flavor = "multi_thread")]
async fn wrong_code_fails_both_sides_and_consumes_the_code() {
    let (_, host) = bind(20).await;
    let (_, ctl) = bind(21).await;
    let otc = OneTimeCode::generate().expect("rng");
    let right = otc.as_str().to_owned();
    // A different valid code: change the first symbol.
    let first = if right.starts_with('A') { 'B' } else { 'A' };
    let wrong: String = std::iter::once(first)
        .chain(right.chars().skip(1))
        .collect();
    let slot = HostCode::new(otc);

    let (c, h) = connect(&ctl, &host).await;
    let (hr, cr) = tokio::time::timeout(T, async {
        tokio::join!(
            host_pair(&h, host.device_id(), &slot),
            controller_pair(&c, ctl.device_id(), &wrong)
        )
    })
    .await
    .expect("fail in time");
    assert!(matches!(hr, Err(NetError::PairingFailed)), "{hr:?}");
    assert!(matches!(cr, Err(NetError::PairingFailed)), "{cr:?}");
    assert!(slot.is_consumed());

    // The right code on a second attempt is refused: the code is gone.
    let (c2, h2) = connect(&ctl, &host).await;
    let (hr, cr) = tokio::time::timeout(T, async {
        tokio::join!(
            host_pair(&h2, host.device_id(), &slot),
            controller_pair(&c2, ctl.device_id(), &right)
        )
    })
    .await
    .expect("refuse in time");
    assert!(matches!(hr, Err(NetError::CodeConsumed)), "{hr:?}");
    assert!(matches!(cr, Err(NetError::CodeConsumed)), "{cr:?}");
}

fn trust_of(peer: &Identity, expires_at: Option<u64>) -> TrustStore {
    let mut t = TrustStore::default();
    t.upsert(TrustedPeer {
        device: peer.device_id(),
        label: "laptop".into(),
        profile: Profile::Full,
        added_at: 0,
        expires_at,
    });
    t
}

#[tokio::test(flavor = "multi_thread")]
async fn trusted_auth_admits_a_trusted_peer() {
    let (_, host) = bind(30).await;
    let (ctl_id, ctl) = bind(31).await;
    let trust = trust_of(&ctl_id, None);

    let (c, h) = connect(&ctl, &host).await;
    let (hr, cr) = tokio::time::timeout(T, async {
        tokio::join!(
            host_auth_trusted(&h, host.device_id(), &trust, now()),
            controller_auth_trusted(&c, &ctl_id)
        )
    })
    .await
    .expect("auth in time");
    let hr = hr.expect("host admits");
    cr.expect("controller admitted");
    assert_eq!(hr.peer, ctl_id.device_id());
    assert_eq!(hr.profile, Profile::Full);
}

#[tokio::test(flavor = "multi_thread")]
async fn trusted_auth_refuses_untrusted_and_expired_peers() {
    let (_, host) = bind(40).await;
    let (ctl_id, ctl) = bind(41).await;
    let stranger = Identity::from_seed([42; 32]);

    for trust in [trust_of(&stranger, None), trust_of(&ctl_id, Some(1))] {
        let (c, h) = connect(&ctl, &host).await;
        let (hr, cr) = tokio::time::timeout(T, async {
            tokio::join!(
                host_auth_trusted(&h, host.device_id(), &trust, now()),
                controller_auth_trusted(&c, &ctl_id)
            )
        })
        .await
        .expect("refuse in time");
        assert!(matches!(hr, Err(NetError::Untrusted)), "{hr:?}");
        assert!(matches!(cr, Err(NetError::Rejected(_))), "{cr:?}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn trusted_auth_rejects_a_signature_from_another_key() {
    let (_, host) = bind(50).await;
    let (ctl_id, ctl) = bind(51).await;
    // The connection is authenticated as ctl, but the proof is signed by
    // another identity: the host must check it against the QUIC peer id.
    let impostor = Identity::from_seed([52; 32]);
    let trust = trust_of(&ctl_id, None);

    let (c, h) = connect(&ctl, &host).await;
    let (hr, cr) = tokio::time::timeout(T, async {
        tokio::join!(
            host_auth_trusted(&h, host.device_id(), &trust, now()),
            controller_auth_trusted(&c, &impostor)
        )
    })
    .await
    .expect("refuse in time");
    assert!(matches!(hr, Err(NetError::BadSignature)), "{hr:?}");
    assert!(matches!(cr, Err(NetError::Rejected(_))), "{cr:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn framed_stream_echo_carries_its_kind() {
    let (_, host) = bind(60).await;
    let (_, ctl) = bind(61).await;
    let (c, h) = connect(&ctl, &host).await;

    let server = tokio::spawn(async move {
        let (kind, mut send, mut recv) = accept_stream(&h).await.expect("stream");
        while let Some(frame) = read_frame(&mut recv).await.expect("read") {
            write_frame(&mut send, &frame).await.expect("echo");
        }
        send.finish().expect("finish");
        // Keep the connection alive until the peer has read everything.
        let _ = h.closed().await;
        kind
    });

    tokio::time::timeout(T, async {
        let (mut send, mut recv) = open_stream(&c, StreamKind::Clipboard).await.expect("open");
        let big = vec![0xabu8; 200_000];
        for payload in [&b"hello"[..], &b""[..], &big[..]] {
            write_frame(&mut send, payload).await.expect("write");
            let echoed = read_frame(&mut recv).await.expect("read").expect("frame");
            assert_eq!(echoed, payload);
        }
        send.finish().expect("finish");
        assert_eq!(read_frame(&mut recv).await.expect("eof"), None);
    })
    .await
    .expect("echo in time");
    c.close(0u32.into(), b"done");
    assert_eq!(server.await.expect("join"), StreamKind::Clipboard);
}

#[tokio::test(flavor = "multi_thread")]
async fn unknown_stream_kind_is_skipped() {
    let (_, host) = bind(70).await;
    let (_, ctl) = bind(71).await;
    let (c, h) = connect(&ctl, &host).await;

    tokio::time::timeout(T, async {
        // A newer peer opens kind 42 first, then a Chat stream.
        let (mut s1, _r1) = c.open_bi().await.expect("open");
        s1.write_all(&[42]).await.expect("kind");
        let (mut s2, _r2) = open_stream(&c, StreamKind::Chat).await.expect("open");
        write_frame(&mut s2, b"hi").await.expect("write");

        let (kind, _s, mut r) = accept_stream(&h).await.expect("accept");
        assert_eq!(kind, StreamKind::Chat);
        assert_eq!(
            read_frame(&mut r).await.expect("read"),
            Some(b"hi".to_vec())
        );
        // The unknown stream was stopped, so the opener sees it.
        assert!(s1.stopped().await.is_ok());
    })
    .await
    .expect("in time");
}

#[tokio::test(flavor = "multi_thread")]
async fn datagram_round_trip_and_size_check() {
    let (_, host) = bind(80).await;
    let (_, ctl) = bind(81).await;
    let (c, h) = connect(&ctl, &host).await;

    let max = max_datagram_size(&c).expect("datagrams supported");
    assert!(max >= 1000, "max datagram {max}");
    // PMTU discovery may raise the limit between calls, so exceed any UDP payload.
    let err = send_datagram(&c, Bytes::from(vec![0u8; 65_536])).unwrap_err();
    assert!(matches!(err, NetError::DatagramTooLarge { .. }), "{err:?}");

    tokio::time::timeout(T, async {
        // Datagrams are unreliable: resend until one lands.
        let payload = Bytes::from_static(b"frame-0001");
        loop {
            send_datagram(&c, payload.clone()).expect("send");
            if let Ok(got) =
                tokio::time::timeout(Duration::from_millis(200), recv_datagram(&h)).await
            {
                assert_eq!(got.expect("recv"), payload);
                break;
            }
        }
    })
    .await
    .expect("datagram in time");
}
