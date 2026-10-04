//! Passphrase locators (D24) end to end: `POST /v1/locator`,
//! `POST /v1/locator/release`, `GET /v1/locator/{n}`.

mod common;

use std::time::Duration;

use common::{Client, abuse_body, failure_body, locator_body, register, register_body};
use scrin_crypto::identity::Identity;
use scrin_server::api::AddrHint;
use scrin_server::{Config, Role, Server, TlsMode, auth};
use serde_json::{Value, json};

fn hint() -> AddrHint {
    AddrHint {
        relay_url: Some("https://relay.example.org/".into()),
        direct_addrs: vec!["192.0.2.10:41641".into()],
    }
}

async fn start_with(f: impl FnOnce(&mut Config)) -> Server {
    let mut cfg = Config::for_tests();
    cfg.roles = vec![Role::Rendezvous];
    cfg.tls = Some(TlsMode::None);
    cfg.presence_ttl = 2;
    f(&mut cfg);
    Server::start(cfg).await.expect("server starts")
}

async fn allocate(c: &Client, host: &Identity) -> (u16, Value) {
    c.post("/v1/locator", &locator_body(host, auth::LABEL_LOCATOR))
        .await
}

async fn heartbeat(c: &Client, host: &Identity) {
    let body = register_body(host, &hint(), auth::now_secs(), auth::LABEL_PRESENCE);
    assert_eq!(c.post("/v1/presence", &body).await.0, 200);
}

fn locator_of(v: &Value) -> u64 {
    v["locator"].as_u64().expect("locator")
}

#[tokio::test]
async fn allocate_lookup_rotate_release() {
    let server = start_with(|_| {}).await;
    let c = Client::new(server.tcp_addr);
    let host = Identity::generate().expect("rng");
    let id = register(&c, &host, &hint()).await;

    let (s, v) = allocate(&c, &host).await;
    assert_eq!(s, 200, "{v}");
    let first = locator_of(&v);
    assert!(first <= 1_048_575);
    assert_eq!(v["expires_in"], 600);

    // Same answer as resolving the id.
    let (s, by_loc) = c.get(&format!("/v1/locator/{first}")).await;
    assert_eq!(s, 200, "{by_loc}");
    let (s, by_id) = c.get(&format!("/v1/resolve/{id}")).await;
    assert_eq!(s, 200, "{by_id}");
    assert_eq!(by_loc["id"], by_id["id"]);
    assert_eq!(by_loc["device_pub"], host.device_id().to_hex());
    assert_eq!(by_loc["addr_hint"], by_id["addr_hint"]);

    // Rotation replaces the old locator immediately.
    let (s, v) = allocate(&c, &host).await;
    assert_eq!(s, 200, "{v}");
    let second = locator_of(&v);
    if second != first {
        let (s, v) = c.get(&format!("/v1/locator/{first}")).await;
        assert_eq!((s, v["error"].as_str()), (404, Some("unknown_locator")));
    }
    assert_eq!(c.get(&format!("/v1/locator/{second}")).await.0, 200);

    // Release is idempotent and frees the locator.
    let rel = locator_body(&host, auth::LABEL_LOCATOR_RELEASE);
    let (s, v) = c.post("/v1/locator/release", &rel).await;
    assert_eq!((s, v["released"].as_bool()), (200, Some(true)), "{v}");
    let rel = locator_body(&host, auth::LABEL_LOCATOR_RELEASE);
    let (s, v) = c.post("/v1/locator/release", &rel).await;
    assert_eq!((s, v["released"].as_bool()), (200, Some(false)), "{v}");
    let (s, v) = c.get(&format!("/v1/locator/{second}")).await;
    assert_eq!((s, v["error"].as_str()), (404, Some("unknown_locator")));

    // Malformed locators.
    assert_eq!(c.get("/v1/locator/abc").await.0, 400);
    assert_eq!(c.get("/v1/locator/1048576").await.0, 400);

    let (_, m) = c.get("/metrics").await;
    let m = m.as_str().expect("text").to_owned();
    assert!(m.contains("scrin_locator_allocations_total 2"), "{m}");
    assert!(
        m.contains("scrin_locator_misses_total 2") || second == first,
        "{m}"
    );
    server.shutdown().await;
}

#[tokio::test]
async fn allocate_requires_registered_host_and_its_signature() {
    let server = start_with(|_| {}).await;
    let c = Client::new(server.tcp_addr);
    let host = Identity::generate().expect("rng");

    let (s, v) = allocate(&c, &host).await;
    assert_eq!((s, v["error"].as_str()), (404, Some("not_registered")));

    register(&c, &host, &hint()).await;
    // A release signature is not an allocate signature (and vice versa).
    let body = locator_body(&host, auth::LABEL_LOCATOR_RELEASE);
    let (s, v) = c.post("/v1/locator", &body).await;
    assert_eq!((s, v["error"].as_str()), (401, Some("bad_signature")));
    let body = locator_body(&host, auth::LABEL_LOCATOR);
    assert_eq!(c.post("/v1/locator/release", &body).await.0, 401);

    // Signed by another key.
    let other = Identity::generate().expect("rng");
    let mut body = locator_body(&other, auth::LABEL_LOCATOR);
    body["device_pub"] = json!(host.device_id().to_hex());
    assert_eq!(c.post("/v1/locator", &body).await.0, 401);

    // Expired timestamp.
    let ts = auth::now_secs() - 400;
    let body = json!({
        "device_pub": host.device_id().to_hex(),
        "timestamp": ts,
        "signature": common::signed(&host, auth::LABEL_LOCATOR, ts, b""),
    });
    let (s, v) = c.post("/v1/locator", &body).await;
    assert_eq!((s, v["error"].as_str()), (401, Some("expired")));
    server.shutdown().await;
}

#[tokio::test]
async fn offline_host_and_expired_locator_are_404() {
    let server = start_with(|cfg| cfg.locator_ttl = 4).await;
    let c = Client::new(server.tcp_addr);
    let host = Identity::generate().expect("rng");
    register(&c, &host, &hint()).await;
    let (s, v) = allocate(&c, &host).await;
    assert_eq!((s, v["expires_in"].as_u64()), (200, Some(4)), "{v}");
    let l = locator_of(&v);

    // Presence TTL is 2 s: after 3 s the locator is live but the host is offline.
    tokio::time::sleep(Duration::from_secs(3)).await;
    let (s, v) = c.get(&format!("/v1/locator/{l}")).await;
    assert_eq!((s, v["error"].as_str()), (404, Some("offline")));
    heartbeat(&c, &host).await;
    assert_eq!(c.get(&format!("/v1/locator/{l}")).await.0, 200);

    // After the 4 s locator TTL it is unknown even with fresh presence.
    tokio::time::sleep(Duration::from_secs(2)).await;
    heartbeat(&c, &host).await;
    let (s, v) = c.get(&format!("/v1/locator/{l}")).await;
    assert_eq!((s, v["error"].as_str()), (404, Some("unknown_locator")));
    server.shutdown().await;
}

#[tokio::test]
async fn lockout_of_the_id_applies_to_its_locator() {
    let server = start_with(|_| {}).await;
    let c = Client::new(server.tcp_addr);
    let host = Identity::generate().expect("rng");
    register(&c, &host, &hint()).await;
    let (_, v) = allocate(&c, &host).await;
    let l = locator_of(&v);
    assert_eq!(c.get(&format!("/v1/locator/{l}")).await.0, 200);

    for _ in 0..5 {
        assert_eq!(
            c.post("/v1/report-failure", &failure_body(&host)).await.0,
            200
        );
    }
    let (s, v) = c.get(&format!("/v1/locator/{l}")).await;
    assert_eq!((s, v["error"].as_str()), (429, Some("locked_out")));
    server.shutdown().await;
}

#[tokio::test]
async fn blocked_host_cannot_allocate() {
    let server = start_with(|_| {}).await;
    let c = Client::new(server.tcp_addr);
    let host = Identity::generate().expect("rng");
    register(&c, &host, &hint()).await;
    let host_hex = host.device_id().to_hex();
    for _ in 0..3 {
        let reporter = Identity::generate().expect("rng");
        register(&c, &reporter, &AddrHint::default()).await;
        let (s, v) = c
            .post("/v1/abuse", &abuse_body(&reporter, &host_hex, "scam"))
            .await;
        assert_eq!(s, 200, "{v}");
    }
    let (s, v) = allocate(&c, &host).await;
    assert_eq!((s, v["error"].as_str()), (403, Some("blocked")));
    let rel = locator_body(&host, auth::LABEL_LOCATOR_RELEASE);
    assert_eq!(c.post("/v1/locator/release", &rel).await.0, 403);
    server.shutdown().await;
}

#[tokio::test]
async fn anonymous_lookup_is_rate_limited_per_ip_with_its_own_bucket() {
    let server = start_with(|_| {}).await;
    let c = Client::new(server.tcp_addr);
    let mut codes = Vec::new();
    for n in 0..15 {
        codes.push(c.get(&format!("/v1/locator/{n}")).await.0);
    }
    // Burst of 10 (all 404: unknown locator), then 429.
    assert!(codes[..10].iter().all(|s| *s == 404), "{codes:?}");
    assert!(codes[10..].iter().all(|s| *s == 429), "{codes:?}");
    // The resolve bucket is separate and still full.
    assert_eq!(c.get("/v1/resolve/123456789").await.0, 404);
    server.shutdown().await;
}

#[tokio::test]
async fn one_locator_is_rate_limited_across_ips() {
    let server = start_with(|cfg| cfg.trust_forwarded = true).await;
    let c = Client::new(server.tcp_addr);
    let mut codes = Vec::new();
    for i in 0..12 {
        let r = c
            .http
            .get(format!("{}/v1/locator/424242", c.base))
            .header("x-forwarded-for", format!("198.51.100.{}", i + 1))
            .send()
            .await
            .expect("request");
        codes.push(r.status().as_u16());
    }
    // Fresh IP each time, same locator: the per-locator bucket (10) trips.
    assert!(codes[..10].iter().all(|s| *s == 404), "{codes:?}");
    assert!(codes[10..].iter().all(|s| *s == 429), "{codes:?}");
    server.shutdown().await;
}
