//! Signed client of the scrin rendezvous server (`crates/scrin-server`):
//! `register` + `presence` for a host, signed `resolve` for a controller.
//!
//! Every call is signed with the device key over the canonical byte string of
//! `crates/scrin-server/src/auth.rs`:
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
//!
//! The hint's canonical form is the relay URL (or empty) followed by one
//! direct address per line, joined with `\n`, in the order sent. This mirrors
//! `scrin_engine::rendezvous` (the desktop client) byte for byte.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use iroh::{EndpointAddr, EndpointId, RelayUrl, TransportAddr};
use scrin_crypto::identity::Identity;
use serde::{Deserialize, Serialize};

use crate::types::ScrinError;

pub(crate) const LABEL_REGISTER: &str = "scrin rendezvous register v1";
pub(crate) const LABEL_PRESENCE: &str = "scrin rendezvous presence v1";
pub(crate) const LABEL_RESOLVE: &str = "scrin rendezvous resolve v1";
pub(crate) const LABEL_LOCATOR: &str = "scrin rendezvous locator v1";

/// Answers larger than this are refused (every answer is < 1 KiB).
const MAX_BODY: usize = 16 * 1024;
/// The server rejects hints with more direct addresses.
const MAX_DIRECT: usize = 16;

/// The bytes signed for `label` (identical to `scrin_server::auth::canonical`).
pub(crate) fn canonical(label: &str, device: &[u8; 32], timestamp: u64, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(label.len() + 45 + body.len());
    out.extend_from_slice(label.as_bytes());
    out.push(0);
    out.extend_from_slice(device);
    out.extend_from_slice(&timestamp.to_be_bytes());
    let len = u32::try_from(body.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(body);
    out
}

/// Where a device can be reached (the server's `AddrHint`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AddrHint {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay_url: Option<String>,
    #[serde(default)]
    pub direct_addrs: Vec<String>,
}

impl AddrHint {
    /// The endpoint's relay plus its direct addresses (and any extra bound
    /// sockets that are not unspecified).
    pub(crate) fn from_addr(addr: &EndpointAddr, extra: &[SocketAddr]) -> Self {
        let mut ips: Vec<SocketAddr> = addr
            .ip_addrs()
            .copied()
            .chain(extra.iter().copied())
            .filter(|s| !s.ip().is_unspecified())
            .collect();
        ips.sort_unstable();
        ips.dedup();
        ips.truncate(MAX_DIRECT);
        Self {
            relay_url: addr.relay_urls().next().map(ToString::to_string),
            direct_addrs: ips.iter().map(ToString::to_string).collect(),
        }
    }

    pub(crate) fn canonical(&self) -> Vec<u8> {
        let mut parts: Vec<&str> = vec![self.relay_url.as_deref().unwrap_or("")];
        parts.extend(self.direct_addrs.iter().map(String::as_str));
        parts.join("\n").into_bytes()
    }

    /// The dialable address of `key` according to this hint.
    pub(crate) fn endpoint_addr(&self, key: EndpointId) -> EndpointAddr {
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

#[derive(Debug, Serialize)]
struct SignedHint<'a> {
    device_pub: String,
    addr_hint: &'a AddrHint,
    timestamp: u64,
    signature: String,
}

#[derive(Debug, Deserialize)]
struct Registered {
    id: String,
    #[serde(default)]
    presence_ttl: u64,
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

#[derive(Debug, Deserialize)]
struct ErrorBody {
    #[serde(default)]
    error: String,
}

/// Why a server call failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ServerError {
    NotRegistered,
    Offline,
    Other(String),
}

impl From<ServerError> for ScrinError {
    fn from(e: ServerError) -> Self {
        Self::Network {
            msg: match e {
                ServerError::NotRegistered => "device is not registered".into(),
                ServerError::Offline => "that scrin ID is offline or unknown".into(),
                ServerError::Other(m) => m,
            },
        }
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

/// The 9 digits of a scrin ID (spaces and dashes ignored), or `None`.
pub(crate) fn normalize_scrin_id(input: &str) -> Option<String> {
    let digits: String = input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect();
    (digits.len() == 9 && digits.bytes().all(|b| b.is_ascii_digit())).then_some(digits)
}

/// Signed client for one server.
#[derive(Clone)]
pub(crate) struct Client {
    base: String,
    http: reqwest::Client,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("base", &self.base)
            .finish_non_exhaustive()
    }
}

impl Client {
    /// `base`: `https://scrin.example.org`, or `http://…` on a private network.
    pub(crate) fn new(base: &str) -> Result<Self, ScrinError> {
        let base = base.trim().trim_end_matches('/').to_owned();
        let scheme_ok = base
            .split_once("://")
            .is_some_and(|(s, rest)| (s == "https" || s == "http") && !rest.is_empty());
        if !scheme_ok {
            return Err(ScrinError::input("server url must be http(s)://"));
        }
        // rustls + ring + webpki roots, preconfigured: reqwest's platform
        // verifier is never used (it needs JNI setup on Android).
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let roots = rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        let tls = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| ScrinError::state(format!("tls: {e}")))?
            .with_root_certificates(roots)
            .with_no_client_auth();
        let http = reqwest::Client::builder()
            .tls_backend_preconfigured(tls)
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            .build()
            .map_err(|e| ScrinError::state(format!("http client: {e}")))?;
        Ok(Self { base, http })
    }

    fn sign(id: &Identity, label: &str, ts: u64, body: &[u8]) -> String {
        hex(&id.sign(&canonical(label, &id.device_id().0, ts, body)))
    }

    async fn send<T: for<'de> Deserialize<'de>>(
        &self,
        req: reqwest::RequestBuilder,
    ) -> Result<T, ServerError> {
        let resp = req
            .send()
            .await
            .map_err(|e| ServerError::Other(e.without_url().to_string()))?;
        let status = resp.status().as_u16();
        let body = resp
            .bytes()
            .await
            .map_err(|e| ServerError::Other(e.without_url().to_string()))?;
        if body.len() > MAX_BODY {
            return Err(ServerError::Other("oversized answer".into()));
        }
        if (200..300).contains(&status) {
            return serde_json::from_slice(&body)
                .map_err(|_| ServerError::Other("malformed answer".into()));
        }
        let code = serde_json::from_slice::<ErrorBody>(&body)
            .map(|b| b.error)
            .unwrap_or_default();
        Err(match (status, code.as_str()) {
            (404, "not_registered") => ServerError::NotRegistered,
            (404, "unknown_locator") => ServerError::Other("unknown or expired passphrase".into()),
            (404, _) => ServerError::Offline,
            _ => ServerError::Other(format!("server answered {status} {code}")),
        })
    }

    async fn post_hint<T: for<'de> Deserialize<'de>>(
        &self,
        id: &Identity,
        path: &str,
        label: &str,
        hint: &AddrHint,
    ) -> Result<T, ServerError> {
        let ts = unix_s();
        let body = SignedHint {
            device_pub: id.device_id().to_hex(),
            addr_hint: hint,
            timestamp: ts,
            signature: Self::sign(id, label, ts, &hint.canonical()),
        };
        self.send(self.http.post(format!("{}{path}", self.base)).json(&body))
            .await
    }

    /// `POST /v1/register`: (scrin ID, presence TTL in seconds).
    pub(crate) async fn register(
        &self,
        id: &Identity,
        hint: &AddrHint,
    ) -> Result<(String, u64), ServerError> {
        let r: Registered = self
            .post_hint(id, "/v1/register", LABEL_REGISTER, hint)
            .await?;
        Ok((r.id, r.presence_ttl))
    }

    /// `POST /v1/locator` (D24): (locator, seconds until it expires). The
    /// host must be registered; a new locator replaces the old one.
    pub(crate) async fn allocate_locator(&self, id: &Identity) -> Result<(u32, u64), ServerError> {
        #[derive(Serialize)]
        struct Req {
            device_pub: String,
            timestamp: u64,
            signature: String,
        }
        #[derive(Deserialize)]
        struct Resp {
            locator: u32,
            expires_in: u64,
        }
        let ts = unix_s();
        let req = Req {
            device_pub: id.device_id().to_hex(),
            timestamp: ts,
            signature: Self::sign(id, LABEL_LOCATOR, ts, &[]),
        };
        let r: Resp = self
            .send(
                self.http
                    .post(format!("{}/v1/locator", self.base))
                    .json(&req),
            )
            .await?;
        Ok((r.locator, r.expires_in))
    }

    /// `POST /v1/presence`: (scrin ID, seconds until the presence expires).
    pub(crate) async fn presence(
        &self,
        id: &Identity,
        hint: &AddrHint,
    ) -> Result<(String, u64), ServerError> {
        let a: PresenceAnswer = self
            .post_hint(id, "/v1/presence", LABEL_PRESENCE, hint)
            .await?;
        Ok((a.id, a.expires_in))
    }

    /// `POST /v1/resolve`, signed with the controller's key.
    pub(crate) async fn resolve(
        &self,
        me: &Identity,
        scrin_id: &str,
    ) -> Result<EndpointAddr, ServerError> {
        let ts = unix_s();
        let req = ResolveReq {
            id: scrin_id,
            controller_pub: me.device_id().to_hex(),
            timestamp: ts,
            signature: Self::sign(me, LABEL_RESOLVE, ts, scrin_id.as_bytes()),
        };
        let a: ResolveAnswer = self
            .send(
                self.http
                    .post(format!("{}/v1/resolve", self.base))
                    .json(&req),
            )
            .await?;
        answer_addr(&a)
    }

    /// `GET /v1/locator/{n}` (anonymous): where a passphrase host is (D24).
    pub(crate) async fn lookup_locator(&self, locator: u32) -> Result<EndpointAddr, ServerError> {
        let a: ResolveAnswer = self
            .send(self.http.get(format!("{}/v1/locator/{locator}", self.base)))
            .await?;
        answer_addr(&a)
    }
}

fn answer_addr(a: &ResolveAnswer) -> Result<EndpointAddr, ServerError> {
    let malformed = || ServerError::Other("malformed answer".into());
    let raw = data_encoding::HEXLOWER_PERMISSIVE
        .decode(a.device_pub.as_bytes())
        .map_err(|_| malformed())?;
    let key: [u8; 32] = raw.try_into().map_err(|_| malformed())?;
    let key = EndpointId::from_bytes(&key).map_err(|_| malformed())?;
    Ok(a.addr_hint.endpoint_addr(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_layout_matches_the_server() {
        let id = Identity::from_seed([7; 32]).device_id();
        let c = canonical("lbl", &id.0, 0x0102, b"xyz");
        assert_eq!(&c[..4], b"lbl\0");
        assert_eq!(&c[4..36], &id.0);
        assert_eq!(&c[36..44], &0x0102u64.to_be_bytes());
        assert_eq!(&c[44..48], &3u32.to_be_bytes());
        assert_eq!(&c[48..], b"xyz");
    }

    #[test]
    fn hint_round_trips_and_skips_unspecified() {
        let key = EndpointId::from_bytes(&Identity::from_seed([3; 32]).device_id().0).expect("key");
        let addr = EndpointAddr::from_parts(
            key,
            [
                TransportAddr::Ip("10.0.0.2:5000".parse().expect("ip")),
                TransportAddr::Relay("http://127.0.0.1:4433".parse().expect("url")),
            ],
        );
        let h = AddrHint::from_addr(&addr, &["0.0.0.0:9".parse().expect("sock")]);
        assert_eq!(h.direct_addrs, vec!["10.0.0.2:5000".to_owned()]);
        let relay = h.relay_url.clone().expect("relay");
        assert_eq!(
            h.canonical(),
            format!("{relay}\n10.0.0.2:5000").into_bytes()
        );
        assert_eq!(h.endpoint_addr(key), addr);
    }

    #[test]
    fn scrin_ids_are_nine_digits() {
        assert_eq!(
            normalize_scrin_id("123 456-789").as_deref(),
            Some("123456789")
        );
        assert_eq!(normalize_scrin_id("12345678"), None);
        assert_eq!(normalize_scrin_id("12345678a"), None);
    }

    #[test]
    fn client_rejects_non_http_urls() {
        assert!(Client::new("ftp://x").is_err());
        assert!(Client::new("https://").is_err());
        assert!(Client::new("http://127.0.0.1:1").is_ok());
    }
}
