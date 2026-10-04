//! Turning what the user typed into something iroh can dial.
//!
//! A connect target is one of:
//! - a **scrin ID** (9 digits, spaces/dashes ignored) → looked up through a
//!   [`Resolver`] (`GET {server}/v1/resolve/{id}` in production);
//! - a **ticket** `scrin:<64 hex endpoint id>[?a=<ip:port>][&r=<relay url>]…`
//!   (what [`encode_ticket`] prints; works with no server at all);
//! - a bare **64-hex endpoint id** (dialable only through relays/address lookup).
//! - a dictated **passphrase** of five words ([`scrin_crypto::phrase`]): the
//!   first two are a server locator, the rest the pairing secret.

use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use iroh::{EndpointAddr, EndpointId, RelayUrl, TransportAddr};
use scrin_crypto::phrase;
use serde::Deserialize;
use zeroize::Zeroizing;

use crate::{EngineError, Result};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Looks up the current address of a scrin ID.
pub trait Resolver: Send + Sync + std::fmt::Debug + 'static {
    fn resolve<'a>(&'a self, scrin_id: &'a str) -> BoxFuture<'a, Result<EndpointAddr>>;

    /// Looks up a passphrase locator (`GET /v1/locator/{n}`). Resolvers
    /// without a rendezvous server cannot.
    fn resolve_locator(&self, locator: u32) -> BoxFuture<'_, Result<EndpointAddr>> {
        Box::pin(async move {
            Err(EngineError::Resolve(format!(
                "passphrase {locator}: no server configured"
            )))
        })
    }
}

/// In-memory map; tests and LAN demos.
#[derive(Debug, Clone, Default)]
pub struct StaticResolver {
    map: Arc<Mutex<HashMap<String, EndpointAddr>>>,
}

impl StaticResolver {
    pub fn insert(&self, scrin_id: &str, addr: EndpointAddr) {
        self.map
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(normalize_scrin_id(scrin_id).unwrap_or_default(), addr);
    }
}

impl Resolver for StaticResolver {
    fn resolve<'a>(&'a self, scrin_id: &'a str) -> BoxFuture<'a, Result<EndpointAddr>> {
        let found = self
            .map
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(scrin_id)
            .cloned();
        Box::pin(async move { found.ok_or_else(|| EngineError::Resolve(scrin_id.to_owned())) })
    }
}

/// `GET {base}/v1/resolve/{id}` on the rendezvous server.
///
/// The response contract is owned by `crates/scrin-server/GATEWAY.md`. Until
/// that is published this accepts, leniently:
/// `{ "endpoint_id": "<64 hex | iroh base32>", "relay_url": "https://…"?,
///    "direct_addrs": ["ip:port", …]?, "ticket": "scrin:…"? }`
/// (`ticket` wins when present). 404 means "unknown or offline".
#[derive(Debug, Clone)]
pub struct HttpResolver {
    base: String,
    client: reqwest::Client,
}

#[derive(Debug, Deserialize)]
struct ResolveResponse {
    #[serde(alias = "endpointId")]
    endpoint_id: Option<String>,
    #[serde(alias = "relayUrl", default)]
    relay_url: Option<String>,
    #[serde(alias = "directAddrs", default)]
    direct_addrs: Vec<String>,
    #[serde(default)]
    ticket: Option<String>,
}

/// Responses beyond this size are refused (a resolve answer is < 1 KiB).
const MAX_RESOLVE_BODY: usize = 16 * 1024;

impl HttpResolver {
    pub fn new(base: &str) -> Result<Self> {
        // reqwest is built without a default crypto provider; use ring, the
        // provider iroh already links. Already installed is fine.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            .https_only(
                !base.starts_with("http://127.0.0.1") && !base.starts_with("http://localhost"),
            )
            .build()
            .map_err(|e| EngineError::Resolve(format!("http client: {e}")))?;
        Ok(Self {
            base: base.trim_end_matches('/').to_owned(),
            client,
        })
    }

    async fn fetch(&self, id: &str) -> Result<EndpointAddr> {
        let url = format!("{}/v1/resolve/{id}", self.base);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| EngineError::Resolve(format!("{id}: {e}")))?;
        if !resp.status().is_success() {
            return Err(EngineError::Resolve(format!(
                "{id}: HTTP {}",
                resp.status()
            )));
        }
        let body = resp
            .bytes()
            .await
            .map_err(|e| EngineError::Resolve(format!("{id}: {e}")))?;
        if body.len() > MAX_RESOLVE_BODY {
            return Err(EngineError::Resolve(format!("{id}: oversized answer")));
        }
        let r: ResolveResponse = serde_json::from_slice(&body)
            .map_err(|_| EngineError::Resolve(format!("{id}: malformed answer")))?;
        if let Some(t) = r.ticket {
            return parse_ticket(&t);
        }
        let key = r
            .endpoint_id
            .ok_or_else(|| EngineError::Resolve(format!("{id}: no endpoint id")))?;
        let mut addrs = Vec::new();
        for a in &r.direct_addrs {
            if let Ok(s) = a.parse::<SocketAddr>() {
                addrs.push(TransportAddr::Ip(s));
            }
        }
        if let Some(u) = r
            .relay_url
            .as_deref()
            .and_then(|u| u.parse::<RelayUrl>().ok())
        {
            addrs.push(TransportAddr::Relay(u));
        }
        Ok(EndpointAddr::from_parts(parse_endpoint_id(&key)?, addrs))
    }
}

impl Resolver for HttpResolver {
    fn resolve<'a>(&'a self, scrin_id: &'a str) -> BoxFuture<'a, Result<EndpointAddr>> {
        Box::pin(self.fetch(scrin_id))
    }
}

/// What a connect string refers to.
#[derive(Clone, PartialEq, Eq)]
pub enum ConnectTarget {
    ScrinId(String),
    Addr(EndpointAddr),
    /// Five dictated words: where to dial and the pairing secret.
    Phrase {
        locator: u32,
        password: Zeroizing<String>,
    },
}

impl std::fmt::Debug for ConnectTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ScrinId(id) => f.debug_tuple("ScrinId").field(id).finish(),
            Self::Addr(a) => f.debug_tuple("Addr").field(a).finish(),
            Self::Phrase { locator, .. } => f
                .debug_struct("Phrase")
                .field("locator", locator)
                .finish_non_exhaustive(),
        }
    }
}

/// The 9 digits of a scrin ID, or `None`.
#[must_use]
pub fn normalize_scrin_id(input: &str) -> Option<String> {
    let digits: String = input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect();
    (digits.len() == 9 && digits.bytes().all(|b| b.is_ascii_digit())).then_some(digits)
}

pub fn parse_target(input: &str) -> Result<ConnectTarget> {
    let t = input.trim();
    if let Some(id) = normalize_scrin_id(t) {
        return Ok(ConnectTarget::ScrinId(id));
    }
    if t.starts_with("scrin:") {
        return parse_ticket(t).map(ConnectTarget::Addr);
    }
    if t.len() == 64 {
        let id = parse_endpoint_id(t)?;
        return Ok(ConnectTarget::Addr(EndpointAddr::from_parts(id, [])));
    }
    if let Ok(p) = phrase::parse(t) {
        return Ok(ConnectTarget::Phrase {
            locator: p.locator,
            password: p.password,
        });
    }
    Err(EngineError::InvalidTarget(
        "expected a scrin ID, ticket or five-word passphrase",
    ))
}

fn parse_endpoint_id(s: &str) -> Result<EndpointId> {
    if s.len() == 64
        && let Ok(bytes) = data_encoding::HEXLOWER_PERMISSIVE.decode(s.as_bytes())
        && let Ok(arr) = <[u8; 32]>::try_from(bytes.as_slice())
    {
        return EndpointId::from_bytes(&arr)
            .map_err(|_| EngineError::InvalidTarget("endpoint id is not a valid key"));
    }
    s.parse::<EndpointId>()
        .map_err(|_| EngineError::InvalidTarget("endpoint id"))
}

/// `scrin:<hex id>?a=<ip:port>&r=<relay>` — query keys may repeat.
pub fn parse_ticket(ticket: &str) -> Result<EndpointAddr> {
    let rest = ticket
        .strip_prefix("scrin:")
        .ok_or(EngineError::InvalidTarget("ticket prefix"))?;
    if rest.len() > 4096 {
        return Err(EngineError::InvalidTarget("ticket too long"));
    }
    let (id, query) = rest.split_once('?').unwrap_or((rest, ""));
    let id = parse_endpoint_id(id)?;
    let mut addrs = Vec::new();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair
            .split_once('=')
            .ok_or(EngineError::InvalidTarget("ticket query"))?;
        match k {
            "a" => addrs.push(TransportAddr::Ip(
                v.parse()
                    .map_err(|_| EngineError::InvalidTarget("ticket address"))?,
            )),
            "r" => addrs.push(TransportAddr::Relay(
                v.parse()
                    .map_err(|_| EngineError::InvalidTarget("ticket relay"))?,
            )),
            _ => {}
        }
    }
    Ok(EndpointAddr::from_parts(id, addrs))
}

#[must_use]
pub fn encode_ticket(addr: &EndpointAddr) -> String {
    let mut s = format!(
        "scrin:{}",
        data_encoding::HEXLOWER.encode(addr.id.as_bytes())
    );
    let mut sep = '?';
    for a in &addr.addrs {
        let part = match a {
            TransportAddr::Ip(ip) => format!("a={ip}"),
            TransportAddr::Relay(u) => format!("r={u}"),
            _ => continue,
        };
        s.push(sep);
        s.push_str(&part);
        sep = '&';
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(b: u8) -> EndpointId {
        let id = scrin_crypto::identity::Identity::from_seed([b; 32]).device_id();
        EndpointId::from_bytes(&id.0).expect("valid key")
    }

    #[test]
    fn ticket_round_trip() {
        let addr = EndpointAddr::from_parts(
            key(3),
            [
                TransportAddr::Ip("127.0.0.1:4433".parse().expect("addr")),
                TransportAddr::Relay("https://relay.example.org./".parse().expect("url")),
            ],
        );
        let t = encode_ticket(&addr);
        assert!(t.starts_with("scrin:"));
        assert_eq!(parse_ticket(&t).expect("parse"), addr);
        assert_eq!(parse_target(&t).expect("target"), ConnectTarget::Addr(addr));
    }

    #[test]
    fn scrin_ids_and_garbage() {
        assert_eq!(
            parse_target("123 456-789").expect("id"),
            ConnectTarget::ScrinId("123456789".into())
        );
        assert!(parse_target("12345").is_err());
        assert!(parse_target("scrin:zz").is_err());
        assert!(parse_ticket("scrin:00?a=nope").is_err());
    }

    #[tokio::test]
    async fn static_resolver_finds_and_misses() {
        let r = StaticResolver::default();
        let addr = EndpointAddr::from_parts(key(4), []);
        r.insert("111-222-333", addr.clone());
        assert_eq!(r.resolve("111222333").await.expect("hit"), addr);
        assert!(r.resolve("999999999").await.is_err());
    }
}
