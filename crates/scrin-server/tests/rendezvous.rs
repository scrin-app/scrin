//! Rendezvous API end to end over a real TCP listener on 127.0.0.1:0.

mod common;

use std::time::Duration;

use common::{Client, abuse_body, failure_body, register, register_body, resolve_body, start};
use scrin_crypto::identity::Identity;
use scrin_server::api::AddrHint;
use scrin_server::{Role, auth};
use serde_json::json;

fn hint() -> AddrHint {
    AddrHint {
        relay_url: Some("https://relay.example.org/".into()),
        direct_addrs: vec!["192.0.2.10:41641".into()],
    }
}

#[tokio::test]
async fn register_resolve_presence_expiry_then_404() {
    let server = start(&[Role::Rendezvous]).await;
    let c = Client::new(server.tcp_addr);
    let host = Identity::generate().expect("rng");

    let (s, v) = c.get("/health").await;
    assert_eq!((s, v.as_str()), (200, Some("ok")));
    assert_eq!(c.get("/ready").await.0, 200);

    let id = register(&c, &host, &hint()).await;
    assert_eq!(id.len(), 9);
    // Idempotent for the same key.
    let ts = auth::now_secs();
    let (s, v) = c
        .post(
            "/v1/register",
            &register_body(&host, &hint(), ts, auth::LABEL_REGISTER),
        )
        .await;
    assert_eq!(s, 200);
    assert_eq!(v["id"], id);
    assert_eq!(v["created"], false);

    let (s, v) = c.get(&format!("/v1/resolve/{id}")).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["device_pub"], host.device_id().to_hex());
    assert_eq!(v["addr_hint"]["direct_addrs"][0], "192.0.2.10:41641");

    // Presence TTL is 2 s in tests: after 3 s the host is offline.
    tokio::time::sleep(Duration::from_secs(3)).await;
    let (s, v) = c.get(&format!("/v1/resolve/{id}")).await;
    assert_eq!(s, 404, "{v}");
    assert_eq!(v["error"], "offline");

    // A heartbeat brings it back.
    let (s, v) = c
        .post(
            "/v1/presence",
            &register_body(&host, &hint(), auth::now_secs(), auth::LABEL_PRESENCE),
        )
        .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(c.get(&format!("/v1/resolve/{id}")).await.0, 200);

    // Unknown id.
    assert_eq!(c.get("/v1/resolve/100000000").await.0, 404);
    assert_eq!(c.get("/v1/resolve/abc").await.0, 400);

    let (s, m) = c.get("/metrics").await;
    assert_eq!(s, 200);
    let m = m.as_str().expect("text").to_owned();
    assert!(m.contains("scrin_registrations_total 1"), "{m}");
    assert!(m.contains("scrin_registered_devices 1"), "{m}");
    server.shutdown().await;
}

#[tokio::test]
async fn bad_and_expired_signatures_are_rejected() {
    let server = start(&[Role::Rendezvous]).await;
    let c = Client::new(server.tcp_addr);
    let host = Identity::generate().expect("rng");
    let other = Identity::generate().expect("rng");

    // Signed by a different key.
    let mut body = register_body(&other, &hint(), auth::now_secs(), auth::LABEL_REGISTER);
    body["device_pub"] = json!(host.device_id().to_hex());
    let (s, v) = c.post("/v1/register", &body).await;
    assert_eq!((s, v["error"].as_str()), (401, Some("bad_signature")));

    // Wrong label (a presence signature replayed as register).
    let body = register_body(&host, &hint(), auth::now_secs(), auth::LABEL_PRESENCE);
    assert_eq!(c.post("/v1/register", &body).await.0, 401);

    // Expired.
    let body = register_body(&host, &hint(), auth::now_secs() - 400, auth::LABEL_REGISTER);
    let (s, v) = c.post("/v1/register", &body).await;
    assert_eq!((s, v["error"].as_str()), (401, Some("expired")));

    // Tampered hint after signing.
    let mut body = register_body(&host, &hint(), auth::now_secs(), auth::LABEL_REGISTER);
    body["addr_hint"]["direct_addrs"][0] = json!("198.51.100.1:1");
    assert_eq!(c.post("/v1/register", &body).await.0, 401);

    // Presence for an unregistered key.
    let body = register_body(&host, &hint(), auth::now_secs(), auth::LABEL_PRESENCE);
    assert_eq!(c.post("/v1/presence", &body).await.0, 404);
    server.shutdown().await;
}

#[tokio::test]
async fn five_failure_reports_lock_anonymous_resolve() {
    let server = start(&[Role::Rendezvous]).await;
    let c = Client::new(server.tcp_addr);
    let host = Identity::generate().expect("rng");
    let id = register(&c, &host, &hint()).await;

    for i in 0..5 {
        let (s, v) = c.post("/v1/report-failure", &failure_body(&host)).await;
        assert_eq!(s, 200, "{v}");
        assert_eq!(v["locked"], i == 4);
    }
    let (s, v) = c.get(&format!("/v1/resolve/{id}")).await;
    assert_eq!((s, v["error"].as_str()), (429, Some("locked_out")));

    // An unregistered signed controller is locked out too…
    let stranger = Identity::generate().expect("rng");
    assert_eq!(
        c.post("/v1/resolve", &resolve_body(&stranger, &id)).await.0,
        429
    );
    // …a registered one (reportable identity) still resolves.
    let known = Identity::generate().expect("rng");
    register(&c, &known, &AddrHint::default()).await;
    let (s, v) = c.post("/v1/resolve", &resolve_body(&known, &id)).await;
    assert_eq!(s, 200, "{v}");

    // Only the target's own key may report failures against it.
    let (s, _) = c.post("/v1/report-failure", &failure_body(&stranger)).await;
    assert_eq!(s, 404);
    server.shutdown().await;
}

#[tokio::test]
async fn abuse_reports_block_a_controller_key() {
    let server = start(&[Role::Rendezvous]).await;
    let c = Client::new(server.tcp_addr);
    let host = Identity::generate().expect("rng");
    let id = register(&c, &host, &hint()).await;
    let scammer = Identity::generate().expect("rng");
    let scammer_hex = scammer.device_id().to_hex();

    // Before reports the scammer can resolve.
    assert_eq!(
        c.post("/v1/resolve", &resolve_body(&scammer, &id)).await.0,
        200
    );

    // Unregistered reporters are refused (each report is tied to an identity).
    let anon = Identity::generate().expect("rng");
    assert_eq!(
        c.post("/v1/abuse", &abuse_body(&anon, &scammer_hex, "x"))
            .await
            .0,
        404
    );

    // Three distinct registered reporters block the key (threshold 3).
    let mut last = serde_json::Value::Null;
    for _ in 0..3 {
        let reporter = Identity::generate().expect("rng");
        register(&c, &reporter, &AddrHint::default()).await;
        let (s, v) = c
            .post(
                "/v1/abuse",
                &abuse_body(&reporter, &scammer_hex, "tech support scam"),
            )
            .await;
        assert_eq!(s, 200, "{v}");
        last = v;
    }
    assert_eq!(last["reports"], 3);
    assert_eq!(last["blocked"], true);

    let (s, v) = c.post("/v1/resolve", &resolve_body(&scammer, &id)).await;
    assert_eq!((s, v["error"].as_str()), (403, Some("blocked")));
    // A blocked key can no longer register either.
    let body = register_body(&scammer, &hint(), auth::now_secs(), auth::LABEL_REGISTER);
    assert_eq!(c.post("/v1/register", &body).await.0, 403);
    server.shutdown().await;
}

#[tokio::test]
async fn anonymous_resolve_is_rate_limited_per_ip() {
    let server = start(&[Role::Rendezvous]).await;
    let c = Client::new(server.tcp_addr);
    let mut codes = Vec::new();
    for _ in 0..15 {
        codes.push(c.get("/v1/resolve/123456789").await.0);
    }
    // Burst of 10 (all 404: unknown id), then 429.
    assert!(codes[..10].iter().all(|s| *s == 404), "{codes:?}");
    assert!(codes[10..].iter().all(|s| *s == 429), "{codes:?}");
    server.shutdown().await;
}

#[tokio::test]
async fn sqlite_store_survives_restart() {
    let dir = tempfile::tempdir().expect("tmp");
    let db = dir.path().join("scrin.db");
    let host = Identity::generate().expect("rng");
    let mut cfg = scrin_server::Config::for_tests();
    cfg.roles = vec![Role::Rendezvous];
    cfg.tls = Some(scrin_server::TlsMode::None);
    cfg.db = Some(db.clone());

    let s1 = scrin_server::Server::start(cfg.clone())
        .await
        .expect("start");
    let id = register(&Client::new(s1.tcp_addr), &host, &hint()).await;
    s1.shutdown().await;

    let s2 = scrin_server::Server::start(cfg).await.expect("restart");
    let c = Client::new(s2.tcp_addr);
    let (s, v) = c
        .post(
            "/v1/register",
            &register_body(&host, &hint(), auth::now_secs(), auth::LABEL_REGISTER),
        )
        .await;
    assert_eq!(s, 200);
    assert_eq!(v["id"], id);
    assert_eq!(v["created"], false);
    s2.shutdown().await;
}
