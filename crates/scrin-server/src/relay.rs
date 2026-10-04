//! Embedded iroh relay (`/relay` on the TCP listener) with access control.
//!
//! The relay handshake proves the client holds the secret key of its
//! `EndpointId`, which is the scrin device key. [`RelayAccess`] admits a
//! connection only if that key is registered here and not blocked, so a
//! public instance cannot be used as a free relay by strangers.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use axum::Router;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use iroh_relay::KeyCache;
use iroh_relay::server::{Access, AccessControl, ClientRequest, ConnectionId};
use iroh_relay::server::{ClientRateLimit, Handlers, Metrics as RelayMetrics, RelayService};
use scrin_crypto::identity::DeviceId;

use crate::metrics::{Metrics, inc};
use crate::store::Store;

/// Path the iroh relay client connects to.
pub const RELAY_PATH: &str = iroh_relay::http::RELAY_PATH;
const KEY_CACHE: usize = 64 * 1024;

/// Admission policy for relay clients.
#[derive(Debug)]
pub struct RelayAccess {
    store: Arc<dyn Store>,
    metrics: Arc<Metrics>,
    /// Admit any authenticated key (dev / private deployments).
    open: bool,
    /// Keys admitted without registration (this server's own gateway endpoint).
    extra: HashSet<[u8; 32]>,
}

impl RelayAccess {
    #[must_use]
    pub fn new(
        store: Arc<dyn Store>,
        metrics: Arc<Metrics>,
        open: bool,
        extra: HashSet<[u8; 32]>,
    ) -> Self {
        Self {
            store,
            metrics,
            open,
            extra,
        }
    }

    /// The decision, separated from the iroh types for tests.
    #[must_use]
    pub fn decide(&self, key: &[u8; 32]) -> Access {
        let device = DeviceId(*key);
        match self.store.is_blocked(&device) {
            Ok(true) => return deny("blocked"),
            Ok(false) => {}
            Err(e) => {
                tracing::error!(error = %e, "relay access: store error");
                return deny("unavailable");
            }
        }
        if self.open || self.extra.contains(key) {
            return Access::Allow;
        }
        match self.store.id_for_key(&device) {
            Ok(Some(_)) => Access::Allow,
            Ok(None) => deny("device not registered with this server"),
            Err(e) => {
                tracing::error!(error = %e, "relay access: store error");
                deny("unavailable")
            }
        }
    }
}

fn deny(reason: &str) -> Access {
    Access::Deny {
        reason: Some(reason.to_owned()),
    }
}

impl AccessControl for RelayAccess {
    fn on_connect(&self, request: &ClientRequest) -> impl Future<Output = Access> + Send {
        let access = self.decide(request.endpoint_id().as_bytes());
        if access == Access::Allow {
            self.metrics.relay_active.fetch_add(1, Ordering::Relaxed);
        } else {
            inc(&self.metrics.relay_denied);
            tracing::debug!(endpoint = %request.endpoint_id().fmt_short(), "relay: denied");
        }
        std::future::ready(access)
    }

    fn on_disconnect(&self, _endpoint_id: iroh::EndpointId, _connection_id: ConnectionId) {
        self.metrics.relay_active.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Builds the relay service that handles `GET /relay` WebSocket upgrades.
#[must_use]
pub fn service(access: RelayAccess, bytes_per_sec: u32) -> RelayService {
    let limit = std::num::NonZeroU32::new(bytes_per_sec).map(ClientRateLimit::new);
    RelayService::new(
        Handlers::default(),
        axum::http::HeaderMap::new(),
        limit,
        KeyCache::new(KEY_CACHE),
        Arc::new(access),
        Arc::new(RelayMetrics::default()),
    )
}

/// `/ping` (latency probe, HTTPS) and `/generate_204` (captive portal, HTTP),
/// as served by the stock iroh relay.
pub fn probe_router() -> Router {
    Router::new()
        .route(iroh_relay::http::RELAY_PROBE_PATH, get(ping))
        .route("/generate_204", get(generate_204))
}

async fn ping() -> Response {
    (StatusCode::OK, [(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")]).into_response()
}

async fn generate_204(headers: HeaderMap) -> Response {
    let mut res = StatusCode::NO_CONTENT.into_response();
    if let Some(ch) = headers
        .get("x-iroh-challenge")
        .and_then(|v| v.to_str().ok())
        && !ch.is_empty()
        && ch.len() < 64
        && ch
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        && let Ok(v) = HeaderValue::from_str(&format!("response {ch}"))
    {
        res.headers_mut().insert("x-iroh-response", v);
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::MemoryStore;

    #[test]
    fn only_registered_unblocked_keys_are_admitted() {
        let store: Arc<dyn Store> = Arc::new(MemoryStore::new());
        let metrics = Arc::new(Metrics::default());
        let registered = DeviceId([1; 32]);
        let blocked = DeviceId([2; 32]);
        store.register(&registered, 0).expect("reg");
        store.register(&blocked, 0).expect("reg");
        store.block(&blocked, "t", 0).expect("block");
        let gw = [9u8; 32];
        let acc = RelayAccess::new(store.clone(), metrics.clone(), false, HashSet::from([gw]));
        assert_eq!(acc.decide(&registered.0), Access::Allow);
        assert!(matches!(acc.decide(&blocked.0), Access::Deny { .. }));
        assert!(matches!(acc.decide(&[3; 32]), Access::Deny { .. }));
        assert_eq!(acc.decide(&gw), Access::Allow);

        let open = RelayAccess::new(store, metrics, true, HashSet::new());
        assert_eq!(open.decide(&[3; 32]), Access::Allow);
        assert!(matches!(open.decide(&blocked.0), Access::Deny { .. }));
    }
}
