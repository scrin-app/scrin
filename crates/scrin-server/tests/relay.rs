//! Two scrin-net endpoints connect through the embedded relay of a local
//! server. The dial address carries only the relay URL (no direct
//! addresses), so the connection must be established through the relay.
//! Access control: only keys registered with the rendezvous may use it.

mod common;

use std::net::SocketAddr;
use std::time::Duration;

use common::{Client, register, start};
use scrin_crypto::identity::Identity;
use scrin_net::{EndpointAddr, NetConfig, NetEndpoint, RelayConfig, RelayUrl, TransportAddr};
use scrin_server::Role;
use scrin_server::api::AddrHint;

const TIMEOUT: Duration = Duration::from_secs(20);

async fn bind(id: &Identity, relay: &RelayUrl) -> NetEndpoint {
    NetEndpoint::bind(
        *id.seed(),
        NetConfig {
            relay: RelayConfig::Custom(vec![relay.clone()]),
            bind_addr: Some(SocketAddr::from(([127, 0, 0, 1], 0))),
        },
    )
    .await
    .expect("bind")
}

#[tokio::test]
async fn registered_endpoints_connect_through_the_relay() {
    let server = start(&[Role::Rendezvous, Role::Relay]).await;
    let c = Client::new(server.tcp_addr);
    let relay: RelayUrl = format!("http://{}", server.tcp_addr).parse().expect("url");

    let host_id = Identity::generate().expect("rng");
    let ctl_id = Identity::generate().expect("rng");
    register(&c, &host_id, &AddrHint::default()).await;
    register(&c, &ctl_id, &AddrHint::default()).await;

    let host = bind(&host_id, &relay).await;
    let ctl = bind(&ctl_id, &relay).await;
    tokio::time::timeout(TIMEOUT, host.inner().online())
        .await
        .expect("host reaches its home relay");

    let accept = tokio::spawn({
        let host = host.clone();
        async move {
            host.accept_connection()
                .await
                .expect("incoming")
                .expect("handshake")
        }
    });
    let id = iroh::PublicKey::from_bytes(&host.device_id().0).expect("key");
    let addr = EndpointAddr::from_parts(id, [TransportAddr::Relay(relay.clone())]);
    let conn = tokio::time::timeout(TIMEOUT, ctl.connect(addr))
        .await
        .expect("connect in time")
        .expect("connect");
    let host_conn = tokio::time::timeout(TIMEOUT, accept)
        .await
        .expect("accept in time")
        .expect("task");
    assert_eq!(scrin_net::remote_device_id(&host_conn), ctl.device_id());
    assert_eq!(scrin_net::remote_device_id(&conn), host.device_id());

    // A byte round trip proves the path carries data.
    let (mut s, mut r) = conn.open_bi().await.expect("open");
    s.write_all(b"via relay").await.expect("write");
    s.finish().expect("finish");
    let (mut hs, mut hr) = host_conn.accept_bi().await.expect("accept bi");
    let got = hr.read_to_end(64).await.expect("read");
    assert_eq!(got, b"via relay");
    hs.write_all(b"ok").await.expect("write back");
    hs.finish().expect("finish");
    assert_eq!(r.read_to_end(64).await.expect("read back"), b"ok");

    let (_, m) = c.get("/metrics").await;
    let m = m.as_str().expect("text").to_owned();
    assert!(m.contains("scrin_relay_active_connections 2"), "{m}");

    ctl.close().await;
    host.close().await;
    server.shutdown().await;
}

#[tokio::test]
async fn unregistered_endpoint_is_refused_by_the_relay() {
    let server = start(&[Role::Rendezvous, Role::Relay]).await;
    let c = Client::new(server.tcp_addr);
    let relay: RelayUrl = format!("http://{}", server.tcp_addr).parse().expect("url");

    let stranger = Identity::generate().expect("rng");
    let ep = bind(&stranger, &relay).await;
    // The relay refuses the key: the denial counter moves and the endpoint
    // never gets a home relay (online() does not complete).
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    let mut m = String::new();
    while tokio::time::Instant::now() < deadline {
        let (_, v) = c.get("/metrics").await;
        m = v.as_str().expect("text").to_owned();
        if !m.contains("scrin_relay_denied_total 0") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert!(!m.contains("scrin_relay_denied_total 0"), "{m}");
    assert!(m.contains("scrin_relay_active_connections 0"), "{m}");
    let online = tokio::time::timeout(Duration::from_secs(2), ep.inner().online()).await;
    assert!(
        online.is_err(),
        "unregistered key must not get a home relay"
    );
    ep.close().await;
    server.shutdown().await;
}
