//! Shared helpers for the integration tests: start a server on 127.0.0.1:0
//! and a tiny signed-request client.

#![allow(dead_code)]

use std::net::SocketAddr;

use scrin_crypto::identity::Identity;
use scrin_server::api::AddrHint;
use scrin_server::auth;
use scrin_server::{Config, Role, Server, TlsMode};
use serde_json::{Value, json};

pub async fn start(roles: &[Role]) -> Server {
    let mut cfg = Config::for_tests();
    cfg.roles = roles.to_vec();
    cfg.tls = Some(TlsMode::None);
    cfg.presence_ttl = 2;
    Server::start(cfg).await.expect("server starts")
}

pub struct Client {
    pub http: reqwest::Client,
    pub base: String,
}

impl Client {
    pub fn new(addr: SocketAddr) -> Self {
        Self {
            http: reqwest::Client::new(),
            base: format!("http://{addr}"),
        }
    }

    pub async fn post(&self, path: &str, body: &Value) -> (u16, Value) {
        let r = self
            .http
            .post(format!("{}{path}", self.base))
            .json(body)
            .send()
            .await
            .expect("request");
        let status = r.status().as_u16();
        (status, r.json().await.unwrap_or(Value::Null))
    }

    pub async fn get(&self, path: &str) -> (u16, Value) {
        let r = self
            .http
            .get(format!("{}{path}", self.base))
            .send()
            .await
            .expect("request");
        let status = r.status().as_u16();
        let text = r.text().await.unwrap_or_default();
        (
            status,
            serde_json::from_str(&text).unwrap_or(Value::String(text)),
        )
    }
}

pub fn hex(b: &[u8]) -> String {
    data_encoding::HEXLOWER.encode(b)
}

pub fn signed(id: &Identity, label: &str, ts: u64, body: &[u8]) -> String {
    hex(&id.sign(&auth::canonical(label, &id.device_id(), ts, body)))
}

pub fn register_body(id: &Identity, hint: &AddrHint, ts: u64, label: &str) -> Value {
    json!({
        "device_pub": id.device_id().to_hex(),
        "addr_hint": hint,
        "timestamp": ts,
        "signature": signed(id, label, ts, &hint.canonical()),
    })
}

pub async fn register(c: &Client, id: &Identity, hint: &AddrHint) -> String {
    let (s, v) = c
        .post(
            "/v1/register",
            &register_body(id, hint, auth::now_secs(), auth::LABEL_REGISTER),
        )
        .await;
    assert_eq!(s, 200, "{v}");
    v["id"].as_str().expect("id").to_owned()
}

pub fn resolve_body(controller: &Identity, target: &str) -> Value {
    let ts = auth::now_secs();
    json!({
        "id": target,
        "controller_pub": controller.device_id().to_hex(),
        "timestamp": ts,
        "signature": signed(controller, auth::LABEL_RESOLVE, ts, target.as_bytes()),
    })
}

pub fn failure_body(host: &Identity) -> Value {
    let ts = auth::now_secs();
    json!({
        "device_pub": host.device_id().to_hex(),
        "timestamp": ts,
        "signature": signed(host, auth::LABEL_REPORT_FAILURE, ts, b""),
    })
}

pub fn abuse_body(reporter: &Identity, subject_pub: &str, reason: &str) -> Value {
    let ts = auth::now_secs();
    let body = format!("{subject_pub}\n\n{reason}");
    json!({
        "device_pub": reporter.device_id().to_hex(),
        "subject_pub": subject_pub,
        "reason": reason,
        "timestamp": ts,
        "signature": signed(reporter, auth::LABEL_ABUSE, ts, body.as_bytes()),
    })
}
