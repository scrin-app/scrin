//! Browser gateway: WebTransport (`/v1/gw`, UDP) and WebSocket (`/v1/ws`,
//! TCP) sessions bridged opaquely to a host's iroh endpoint over ALPN
//! [`GW_ALPN`]. The exact contract is in `crates/scrin-server/GATEWAY.md`.
//!
//! The gateway never parses what it forwards: the browser and the host run
//! the scrin handshake (SPAKE2 / trusted auth) and the inner E2E channel
//! (`scrin_crypto::channel`) through it.

pub mod frame;
mod pipe;
pub mod quota;
mod ws;
mod wt;

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use iroh::endpoint::{Connection, ConnectionError, VarInt, presets};
use iroh::{Endpoint, EndpointAddr, RelayMode, RelayUrl, SecretKey, TransportAddr};

pub use self::ws::router as ws_router;
pub use self::wt::serve as serve_webtransport;
use crate::api::{AddrHint, ApiError, AppState};
use crate::ids;
use crate::metrics::inc;

/// ALPN the gateway uses when dialling a host.
pub const GW_ALPN: &[u8] = b"scrin-gw/1";

const DIAL_TIMEOUT: Duration = Duration::from_secs(10);

/// Session close codes (WebTransport session / iroh connection application
/// codes). Codes below `0x100` belong to the browser and host and pass
/// through the gateway unchanged; codes from `0x100` are the gateway's own.
pub mod code {
    pub const NORMAL: u32 = 0;
    pub const HOST_UNREACHABLE: u32 = 0x101;
    pub const SESSION_LIMIT: u32 = 0x102;
    pub const IDLE: u32 = 0x103;
    pub const QUOTA: u32 = 0x104;
    pub const PROTOCOL: u32 = 0x105;
    pub const SHUTDOWN: u32 = 0x106;
    pub const BROWSER_GONE: u32 = 0x107;
    pub const HOST_GONE: u32 = 0x108;

    /// WebSocket close code for a session code: `4000 + code` (capped at 4999).
    #[must_use]
    pub fn ws(code: u32) -> u16 {
        u16::try_from(4000 + code.min(999)).unwrap_or(4999)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct GatewayConfig {
    pub limits: quota::Limits,
    pub max_sessions: i64,
}

/// The gateway's iroh endpoint plus shared rendezvous state.
#[derive(Debug)]
pub struct Gateway {
    endpoint: Endpoint,
    state: Arc<AppState>,
    cfg: GatewayConfig,
}

impl Gateway {
    /// Binds a fresh iroh endpoint (new random key per process) for dialling hosts.
    pub async fn bind(
        state: Arc<AppState>,
        cfg: GatewayConfig,
        relays: Vec<RelayUrl>,
        bind: Option<SocketAddr>,
    ) -> anyhow::Result<Arc<Self>> {
        let mut seed = [0u8; 32];
        getrandom::fill(&mut seed).map_err(|e| anyhow::anyhow!("rng: {e}"))?;
        let secret = SecretKey::from_bytes(&seed);
        let builder = if relays.is_empty() {
            Endpoint::builder(presets::Minimal)
                .relay_mode(RelayMode::Disabled)
                .clear_address_lookup()
        } else {
            Endpoint::builder(presets::Minimal).relay_mode(RelayMode::custom(relays))
        };
        let mut builder = builder.secret_key(secret);
        if let Some(addr) = bind {
            builder = builder.clear_ip_transports().bind_addr(addr)?;
        }
        let endpoint = builder.bind().await?;
        Ok(Arc::new(Self {
            endpoint,
            state,
            cfg,
        }))
    }

    #[must_use]
    pub fn endpoint_id(&self) -> [u8; 32] {
        *self.endpoint.id().as_bytes()
    }

    pub async fn close(&self) {
        self.endpoint.close().await;
    }

    /// Resolves `id` (anonymous lookup, gateway rate limits) and dials the
    /// host. Errors map to HTTP statuses before the browser session is accepted.
    pub async fn open(&self, ip: IpAddr, id: &str) -> Result<Connection, ApiError> {
        let id = ids::parse_id(id).ok_or(ApiError::BadRequest("id"))?;
        if self.state.metrics.gateway_active.load(Ordering::Relaxed) >= self.cfg.max_sessions {
            inc(&self.state.metrics.rate_limited);
            return Err(ApiError::RateLimited);
        }
        let presence = self
            .state
            .resolve_anonymous(&self.state.limiters.gateway_ip, ip, id)?;
        let hint: AddrHint =
            serde_json::from_str(&presence.addr_hint).map_err(|_| ApiError::Offline)?;
        let key = iroh::PublicKey::from_bytes(&presence.key.0).map_err(|_| ApiError::Offline)?;
        let mut addrs: Vec<TransportAddr> = hint.socket_addrs().map(TransportAddr::Ip).collect();
        if let Some(url) = hint
            .relay_url
            .as_deref()
            .and_then(|u| u.parse::<RelayUrl>().ok())
        {
            addrs.push(TransportAddr::Relay(url));
        }
        let addr = EndpointAddr::from_parts(key, addrs);
        match tokio::time::timeout(DIAL_TIMEOUT, self.endpoint.connect(addr, GW_ALPN)).await {
            Ok(Ok(conn)) => Ok(conn),
            Ok(Err(e)) => {
                tracing::debug!(id, error = %e, "gateway: dial failed");
                Err(ApiError::Unreachable)
            }
            Err(_) => {
                tracing::debug!(id, "gateway: dial timed out");
                Err(ApiError::Unreachable)
            }
        }
    }

    fn limits(&self) -> quota::Limits {
        self.cfg.limits
    }

    fn session(&self) -> SessionGuard {
        let m = &self.state.metrics;
        m.gateway_active.fetch_add(1, Ordering::Relaxed);
        inc(&m.gateway_sessions);
        SessionGuard(self.state.clone())
    }

    fn count_bytes(&self, n: usize) {
        self.state
            .metrics
            .gateway_bytes
            .fetch_add(n as u64, Ordering::Relaxed);
    }
}

/// Decrements the active-session gauge on drop.
#[derive(Debug)]
struct SessionGuard(Arc<AppState>);

impl Drop for SessionGuard {
    fn drop(&mut self) {
        self.0
            .metrics
            .gateway_active
            .fetch_sub(1, Ordering::Relaxed);
    }
}

fn iroh_varint(v: u64) -> VarInt {
    VarInt::from_u64(v).unwrap_or(VarInt::from_u32(0))
}

/// The application code the host closed with, or [`code::HOST_GONE`].
fn host_close_code(e: &ConnectionError) -> u32 {
    match e {
        ConnectionError::ApplicationClosed(c) => {
            u32::try_from(c.error_code.into_inner()).unwrap_or(code::HOST_GONE)
        }
        _ => code::HOST_GONE,
    }
}

fn close_host(host: &Connection, code: u32) {
    host.close(VarInt::from_u32(code), b"");
}
