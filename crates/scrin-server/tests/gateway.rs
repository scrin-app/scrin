//! Gateway end to end on loopback: a fake host (iroh endpoint accepting
//! `scrin-gw/1`, echoing streams and datagrams) registers with the server;
//! a WebSocket client and a WebTransport client reach it through the gateway.

mod common;

use std::time::Duration;

use bytes::{Bytes, BytesMut};
use common::{Client, register, start};
use futures_util::{SinkExt as _, StreamExt as _};
use iroh::endpoint::{Connection, presets};
use iroh::{Endpoint, RelayMode, SecretKey};
use scrin_crypto::identity::Identity;
use scrin_server::Role;
use scrin_server::api::AddrHint;
use scrin_server::gateway::GW_ALPN;
use scrin_server::gateway::frame::Frame;
use tokio_tungstenite::tungstenite::Message;

const TIMEOUT: Duration = Duration::from_secs(10);

/// Binds a loopback iroh host and spawns an echo handler. Returns the
/// identity, its endpoint (keep alive) and its address hint.
async fn echo_host() -> (Identity, Endpoint, AddrHint) {
    let id = Identity::generate().expect("rng");
    let ep = Endpoint::builder(presets::Minimal)
        .relay_mode(RelayMode::Disabled)
        .clear_address_lookup()
        .secret_key(SecretKey::from_bytes(id.seed()))
        .alpns(vec![GW_ALPN.to_vec()])
        .clear_ip_transports()
        .bind_addr("127.0.0.1:0")
        .expect("bind addr")
        .bind()
        .await
        .expect("bind");
    let hint = AddrHint {
        relay_url: None,
        direct_addrs: ep.bound_sockets().iter().map(ToString::to_string).collect(),
    };
    let accept = ep.clone();
    tokio::spawn(async move {
        while let Some(inc) = accept.accept().await {
            let Ok(accepting) = inc.accept() else {
                continue;
            };
            let Ok(conn) = accepting.await else { continue };
            tokio::spawn(echo(conn));
        }
    });
    (id, ep, hint)
}

async fn echo(conn: Connection) {
    loop {
        tokio::select! {
            r = conn.accept_bi() => {
                let Ok((mut s, mut r)) = r else { return };
                tokio::spawn(async move {
                    let _ = tokio::io::copy(&mut r, &mut s).await;
                    let _ = s.finish();
                    let _ = s.stopped().await;
                });
            }
            d = conn.read_datagram() => {
                let Ok(d) = d else { return };
                let _ = conn.send_datagram(d);
            }
        }
    }
}

#[tokio::test]
async fn websocket_fallback_echoes_streams_and_datagrams() {
    let server = start(&[Role::Rendezvous, Role::Gateway]).await;
    let c = Client::new(server.tcp_addr);
    let (host, _ep, hint) = echo_host().await;
    let id = register(&c, &host, &hint).await;

    let url = format!("ws://{}/v1/ws?id={id}", server.tcp_addr);
    let (mut ws, _) = tokio::time::timeout(TIMEOUT, tokio_tungstenite::connect_async(&url))
        .await
        .expect("connect in time")
        .expect("ws connect");

    // Stream 0 (browser bidi): data + FIN, expect echo + FIN.
    let data = Frame::Data {
        id: 0,
        payload: Bytes::from_static(b"hello host"),
    };
    ws.send(Message::Binary(data.encode())).await.expect("send");
    ws.send(Message::Binary(Frame::Fin { id: 0 }.encode()))
        .await
        .expect("fin");
    // Datagram.
    ws.send(Message::Binary(
        Frame::Dgram(Bytes::from_static(b"dg")).encode(),
    ))
    .await
    .expect("dgram");

    let mut echoed = BytesMut::new();
    let (mut got_fin, mut got_dgram) = (false, false);
    while !(got_fin && got_dgram) {
        let msg = tokio::time::timeout(TIMEOUT, ws.next())
            .await
            .expect("frame in time")
            .expect("open")
            .expect("ok");
        let Message::Binary(b) = msg else { continue };
        match Frame::decode(&b).expect("frame") {
            Frame::Data { id: 0, payload } => echoed.extend_from_slice(&payload),
            Frame::Fin { id: 0 } => got_fin = true,
            Frame::Dgram(p) => {
                assert_eq!(&p[..], b"dg");
                got_dgram = true;
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(&echoed[..], b"hello host");

    let (_, m) = c.get("/metrics").await;
    let m = m.as_str().expect("text").to_owned();
    assert!(m.contains("scrin_gateway_active_sessions 1"), "{m}");
    ws.close(None).await.expect("close");
    server.shutdown().await;
}

#[tokio::test]
async fn websocket_to_offline_id_is_404() {
    let server = start(&[Role::Rendezvous, Role::Gateway]).await;
    let url = format!("ws://{}/v1/ws?id=123456789", server.tcp_addr);
    let err = tokio_tungstenite::connect_async(&url)
        .await
        .expect_err("rejected");
    let tokio_tungstenite::tungstenite::Error::Http(res) = err else {
        panic!("expected http error, got {err:?}");
    };
    assert_eq!(res.status().as_u16(), 404);
    server.shutdown().await;
}

#[tokio::test]
async fn webtransport_echoes_streams_and_datagrams() {
    let server = start(&[Role::Rendezvous, Role::Gateway]).await;
    let c = Client::new(server.tcp_addr);
    let (host, _ep, hint) = echo_host().await;
    let id = register(&c, &host, &hint).await;

    let hash = server.cert_sha256.expect("self-signed cert hash");
    let cfg = wtransport::ClientConfig::builder()
        .with_bind_default()
        .with_server_certificate_hashes([wtransport::tls::Sha256Digest::new(hash)])
        .build();
    let client = wtransport::Endpoint::client(cfg).expect("client");
    let wt = server.wt_addr.expect("wt addr");
    let url = format!("https://127.0.0.1:{}/v1/gw?id={id}", wt.port());
    let conn = tokio::time::timeout(TIMEOUT, client.connect(url))
        .await
        .expect("connect in time")
        .expect("wt connect");

    let (mut s, mut r) = conn.open_bi().await.expect("open").await.expect("opened");
    s.write_all(b"through the gateway").await.expect("write");
    s.finish().await.expect("finish");
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        match tokio::time::timeout(TIMEOUT, r.read(&mut chunk))
            .await
            .expect("read in time")
        {
            Ok(Some(n)) => buf.extend_from_slice(&chunk[..n]),
            Ok(None) => break,
            Err(e) => panic!("read: {e}"),
        }
    }
    assert_eq!(buf, b"through the gateway");

    conn.send_datagram(b"ping").expect("dgram");
    let d = tokio::time::timeout(TIMEOUT, conn.receive_datagram())
        .await
        .expect("dgram in time")
        .expect("dgram");
    assert_eq!(&d.payload()[..], b"ping");
    conn.close(wtransport::VarInt::from_u32(0), b"");
    server.shutdown().await;
}
