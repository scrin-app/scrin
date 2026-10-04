//! Client of the scrin rendezvous server (`crates/scrin-server`).
//!
//! Every mutating call is signed with the device key over the canonical byte
//! string of `crates/scrin-server/src/auth.rs`:
//!
//! ```text
//! label || 0x00 || device_pub (32) || timestamp_secs (u64 BE) || body_len (u32 BE) || body
//! ```
//!
//! | call | label | signed body |
//! |---|---|---|
//! | `POST /v1/register` | `scrin rendezvous register v1` | address hint, canonical form |
//! | `POST /v1/presence` | `scrin rendezvous presence v1` | address hint, canonical form |
//! | `POST /v1/resolve` | `scrin rendezvous resolve v1` | the 9-digit id |
//! | `POST /v1/report-failure` | `scrin rendezvous report-failure v1` | controller key hex (or empty) |
//!
//! The address hint's canonical form is the relay URL (or empty) followed by
//! one direct address per line, joined with `\n`, in the order sent.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use iroh::{EndpointAddr, EndpointId, RelayUrl, TransportAddr};
use scrin_crypto::identity::{DeviceId, Identity};
use serde::{Deserialize, Serialize};

use crate::resolve::{BoxFuture, Resolver};
use crate::{EngineError, Result};

pub const LABEL_REGISTER: &str = "scrin rendezvous register v1";
pub const LABEL_PRESENCE: &str = "scrin rendezvous presence v1";
pub const LABEL_RESOLVE: &str = "scrin rendezvous resolve v1";
pub const LABEL_REPORT_FAILURE: &str = "scrin rendezvous report-failure v1";

/// Answers larger than this are refused (every answer is < 1 KiB).
const MAX_BODY: usize = 16 * 1024;

/// The bytes signed for `label` (identical to `scrin_server::auth::canonical`).
#[must_use]
pub fn canonical(label: &str, device: &DeviceId, timestamp: u64, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(label.len() + 45 + body.len());
    out.extend_from_slice(label.as_bytes());
    out.push(0);
    out.extend_from_slice(&device.0);
    out.extend_from_slice(&timestamp.to_be_bytes());
    let len = u32::try_from(body.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(body);
    out
}

/// Where a device can be reached (the server's `AddrHint`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddrHint {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay_url: Option<String>,
    #[serde(default)]
    pub direct_addrs: Vec<String>,
}

impl AddrHint {
    /// Builds a hint from an endpoint address. `direct = false` advertises the
    /// relay only (forces relayed dials; tests and privacy).
    #[must_use]
    pub fn from_addr(addr: &EndpointAddr, direct: bool) -> Self {
        let mut hint = Self::default();
        for a in &addr.addrs {
            match a {
                TransportAddr::Relay(u) if hint.relay_url.is_none() => {
                    hint.relay_url = Some(u.to_string());
                }
                TransportAddr::Ip(ip) if direct && hint.direct_addrs.len() < 16 => {
                    hint.direct_addrs.push(ip.to_string());
                }
                _ => {}
            }
        }
        hint
    }

    #[must_use]
    pub fn canonical(&self) -> Vec<u8> {
        let mut parts: Vec<&str> = vec![self.relay_url.as_deref().unwrap_or("")];
        parts.extend(self.direct_addrs.iter().map(String::as_str));
        parts.join("\n").into_bytes()
    }

    /// The dialable address of `key` according to this hint.
    #[must_use]
    pub fn endpoint_addr(&self, key: EndpointId) -> EndpointAddr {
        let mut addrs: Vec<TransportAddr> = self
            .direct_addrs
            .iter()
            .filter_map(|a| a.parse::<SocketAddr>().ok())
            .map(TransportAddr::Ip)
            .collect();
        if let Some(u) = self
            .relay_url
            .as_deref()
            .and_then(|u| u.parse::<RelayUrl>().ok())
        {
            addrs.push(TransportAddr::Relay(u));
        }
        EndpointAddr::from_parts(key, addrs)
    }
}

/// `GET /v1/info`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ServerInfo {
    #[serde(default)]
    pub relay_urls: Vec<String>,
    #[serde(default)]
    pub presence_ttl: u64,
    #[serde(default)]
    pub gateway: bool,
    #[serde(default)]
    pub version: String,
}

#[derive(Debug, Serialize)]
struct SignedHint<'a> {
    device_pub: String,
    addr_hint: &'a AddrHint,
    timestamp: u64,
    signature: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Registered {
    pub id: String,
    #[serde(default)]
    pub created: bool,
    #[serde(default)]
    pub presence_ttl: u64,
}

#[derive(Debug, Deserialize)]
struct PresenceAnswer {
    id: String,
    #[serde(default)]
    expires_in: u64,
}

#[derive(Debug, Serialize)]
struct ResolveReq<'a> {
    id: &'a str,
    controller_pub: String,
    timestamp: u64,
    signature: String,
}

#[derive(Debug, Deserialize)]
struct ResolveAnswer {
    device_pub: String,
    #[serde(default)]
    addr_hint: AddrHint,
}

#[derive(Debug, Serialize)]
struct FailureReq {
    device_pub: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    controller_pub: Option<String>,
    timestamp: u64,
    signature: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FailureAnswer {
    pub locked: bool,
}

#[derive(Debug, Deserialize)]
struct ErrorBody {
    #[serde(default)]
    error: String,
}

/// Why a server call failed, coarse enough for retry decisions.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ServerError {
    /// `404 not_registered`: register first.
    #[error("device is not registered")]
    NotRegistered,
    /// `404 offline`: the target has no fresh presence.
    #[error("offline or unknown id")]
    Offline,
    #[error("rate limited")]
    RateLimited,
    #[error("locked out after failed pairings")]
    LockedOut,
    #[error("blocked")]
    Blocked,
    #[error("server error {status}: {code}")]
    Http { status: u16, code: String },
    #[error("network: {0}")]
    Network(String),
    #[error("malformed answer")]
    Malformed,
}

impl From<ServerError> for EngineError {
    fn from(e: ServerError) -> Self {
        Self::Resolve(e.to_string())
    }
}

fn unix_s() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn hex(b: &[u8]) -> String {
    data_encoding::HEXLOWER.encode(b)
}

/// An HTTPS client with rustls (ring) and the webpki root set. No platform
/// verifier: identical behaviour on Windows, Android and in tests.
pub fn http_client() -> Result<reqwest::Client> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let tls = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| EngineError::Resolve(format!("tls: {e}")))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    reqwest::Client::builder()
        .tls_backend_preconfigured(tls)
        .timeout(Duration::from_secs(10))
        .connect_timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| EngineError::Resolve(format!("http client: {e}")))
}

/// Signed client for one server, acting as one device.
#[derive(Clone)]
pub struct RendezvousClient {
    base: String,
    http: reqwest::Client,
    identity: Arc<Identity>,
}

impl std::fmt::Debug for RendezvousClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RendezvousClient")
            .field("base", &self.base)
            .finish_non_exhaustive()
    }
}

impl RendezvousClient {
    /// `base` is the server URL, e.g. `https://scrin.example.org` or
    /// `http://100.64.0.1:8443` (plain HTTP only for private networks).
    pub fn new(base: &str, identity: Arc<Identity>) -> Result<Self> {
        let base = base.trim().trim_end_matches('/').to_owned();
        let ok = url_scheme(&base).is_some_and(|s| s == "https" || s == "http");
        if !ok {
            return Err(EngineError::Invalid("server url must be http(s)://"));
        }
        Ok(Self {
            base,
            http: http_client()?,
            identity,
        })
    }

    #[must_use]
    pub fn base(&self) -> &str {
        &self.base
    }

    fn sign(&self, label: &str, ts: u64, body: &[u8]) -> String {
        hex(&self
            .identity
            .sign(&canonical(label, &self.identity.device_id(), ts, body)))
    }

    async fn send<T: for<'de> Deserialize<'de>>(
        &self,
        req: reqwest::RequestBuilder,
    ) -> std::result::Result<T, ServerError> {
        let resp = req
            .send()
            .await
            .map_err(|e| ServerError::Network(e.without_url().to_string()))?;
        let status = resp.status().as_u16();
        let body = resp
            .bytes()
            .await
            .map_err(|e| ServerError::Network(e.without_url().to_string()))?;
        if body.len() > MAX_BODY {
            return Err(ServerError::Malformed);
        }
        if (200..300).contains(&status) {
            return serde_json::from_slice(&body).map_err(|_| ServerError::Malformed);
        }
        let code = serde_json::from_slice::<ErrorBody>(&body)
            .map(|b| b.error)
            .unwrap_or_default();
        Err(match (status, code.as_str()) {
            (404, "not_registered") => ServerError::NotRegistered,
            (404, _) => ServerError::Offline,
            (429, "locked_out") => ServerError::LockedOut,
            (429, _) => ServerError::RateLimited,
            (403, _) => ServerError::Blocked,
            _ => ServerError::Http { status, code },
        })
    }

    pub async fn info(&self) -> std::result::Result<ServerInfo, ServerError> {
        self.send(self.http.get(format!("{}/v1/info", self.base)))
            .await
    }

    async fn post_hint<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        label: &str,
        hint: &AddrHint,
    ) -> std::result::Result<T, ServerError> {
        let ts = unix_s();
        let body = SignedHint {
            device_pub: self.identity.device_id().to_hex(),
            addr_hint: hint,
            timestamp: ts,
            signature: self.sign(label, ts, &hint.canonical()),
        };
        self.send(self.http.post(format!("{}{path}", self.base)).json(&body))
            .await
    }

    /// `POST /v1/register`: the scrin ID of this device (stable per key).
    pub async fn register(&self, hint: &AddrHint) -> std::result::Result<Registered, ServerError> {
        self.post_hint("/v1/register", LABEL_REGISTER, hint).await
    }

    /// `POST /v1/presence`: refreshes the address hint; returns the id and TTL.
    pub async fn presence(
        &self,
        hint: &AddrHint,
    ) -> std::result::Result<(String, u64), ServerError> {
        let a: PresenceAnswer = self.post_hint("/v1/presence", LABEL_PRESENCE, hint).await?;
        Ok((a.id, a.expires_in))
    }

    /// `POST /v1/resolve`, signed with this device's key (the controller).
    pub async fn resolve(&self, id: &str) -> std::result::Result<EndpointAddr, ServerError> {
        let ts = unix_s();
        let req = ResolveReq {
            id,
            controller_pub: self.identity.device_id().to_hex(),
            timestamp: ts,
            signature: self.sign(LABEL_RESOLVE, ts, id.as_bytes()),
        };
        let a: ResolveAnswer = self
            .send(
                self.http
                    .post(format!("{}/v1/resolve", self.base))
                    .json(&req),
            )
            .await?;
        let raw = data_encoding::HEXLOWER_PERMISSIVE
            .decode(a.device_pub.as_bytes())
            .map_err(|_| ServerError::Malformed)?;
        let key: [u8; 32] = raw.try_into().map_err(|_| ServerError::Malformed)?;
        let key = EndpointId::from_bytes(&key).map_err(|_| ServerError::Malformed)?;
        Ok(a.addr_hint.endpoint_addr(key))
    }

    /// `POST /v1/report-failure`, sent by the host after a wrong code.
    pub async fn report_failure(
        &self,
        controller: Option<DeviceId>,
    ) -> std::result::Result<FailureAnswer, ServerError> {
        let ts = unix_s();
        let controller_pub = controller.map(|c| c.to_hex());
        let body = controller_pub.as_deref().unwrap_or("").as_bytes().to_vec();
        let req = FailureReq {
            device_pub: self.identity.device_id().to_hex(),
            controller_pub,
            timestamp: ts,
            signature: self.sign(LABEL_REPORT_FAILURE, ts, &body),
        };
        self.send(
            self.http
                .post(format!("{}/v1/report-failure", self.base))
                .json(&req),
        )
        .await
    }
}

impl Resolver for RendezvousClient {
    fn resolve<'a>(&'a self, scrin_id: &'a str) -> BoxFuture<'a, Result<EndpointAddr>> {
        Box::pin(async move {
            self.resolve(scrin_id)
                .await
                .map_err(|e| EngineError::Resolve(format!("{scrin_id}: {e}")))
        })
    }
}

fn url_scheme(u: &str) -> Option<&str> {
    let (scheme, rest) = u.split_once("://")?;
    (!rest.is_empty()).then_some(scheme)
}

/// The relay URLs to use with `server`: what `/v1/info` advertises, or the
/// server itself (scrin-server serves `/relay` on its API listener).
#[must_use]
pub fn relay_urls(server: &str, info: Option<&ServerInfo>) -> Vec<RelayUrl> {
    let advertised: Vec<RelayUrl> = info
        .map(|i| i.relay_urls.iter().filter_map(|u| u.parse().ok()).collect())
        .unwrap_or_default();
    if advertised.is_empty() {
        server.parse().ok().into_iter().collect()
    } else {
        advertised
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_layout() {
        let id = Identity::from_seed([7; 32]).device_id();
        let c = canonical("lbl", &id, 0x0102, b"xyz");
        assert_eq!(&c[..4], b"lbl\0");
        assert_eq!(&c[4..36], &id.0);
        assert_eq!(&c[36..44], &0x0102u64.to_be_bytes());
        assert_eq!(&c[44..48], &3u32.to_be_bytes());
        assert_eq!(&c[48..], b"xyz");
    }

    #[test]
    fn hint_canonical_and_addr() {
        let key = EndpointId::from_bytes(&Identity::from_seed([3; 32]).device_id().0).expect("key");
        let addr = EndpointAddr::from_parts(
            key,
            [
                TransportAddr::Ip("10.0.0.2:5000".parse().expect("ip")),
                TransportAddr::Relay("http://127.0.0.1:4433".parse().expect("url")),
            ],
        );
        let h = AddrHint::from_addr(&addr, true);
        assert_eq!(h.direct_addrs, vec!["10.0.0.2:5000".to_owned()]);
        let relay = h.relay_url.clone().expect("relay");
        assert_eq!(
            h.canonical(),
            format!("{relay}\n10.0.0.2:5000").into_bytes()
        );
        assert_eq!(h.endpoint_addr(key), addr);
        let relay_only = AddrHint::from_addr(&addr, false);
        assert_eq!(relay_only.direct_addrs.len(), 0);
        assert_eq!(relay_only.canonical(), relay.into_bytes());
    }

    #[test]
    fn relay_fallback_is_the_server() {
        let urls = relay_urls("http://127.0.0.1:9", None);
        assert_eq!(urls.len(), 1);
        let info = ServerInfo {
            relay_urls: vec!["https://relay.example.org".into()],
            ..ServerInfo::default()
        };
        assert_eq!(
            relay_urls("http://127.0.0.1:9", Some(&info))[0].to_string(),
            "https://relay.example.org/"
        );
    }

    #[test]
    fn bad_server_urls_are_refused() {
        let id = Arc::new(Identity::from_seed([1; 32]));
        assert!(RendezvousClient::new("ftp://x", id.clone()).is_err());
        assert!(RendezvousClient::new("nonsense", id.clone()).is_err());
        assert!(RendezvousClient::new("http://127.0.0.1:1/", id).is_ok());
    }
}
