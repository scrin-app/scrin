//! Rendezvous HTTP API (axum). Request/response schemas live here; the
//! signed-byte layout of each request is in [`crate::auth`].

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Instant;

use axum::extract::{FromRequestParts, Path, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use scrin_crypto::identity::DeviceId;
use serde::{Deserialize, Serialize};

use crate::auth::{self, AuthError};
use crate::ids;
use crate::limits::{Lockout, Rate, RateLimiter};
use crate::metrics::{Metrics, inc};
use crate::store::{Presence, Store, StoreError};

/// Largest accepted request body.
pub const MAX_BODY: usize = 16 * 1024;
const MAX_DIRECT_ADDRS: usize = 16;
const MAX_URL_LEN: usize = 256;
const MAX_REASON_LEN: usize = 500;
/// Bound on distinct keys a limiter tracks before it fails closed.
const LIMITER_KEYS: usize = 100_000;

/// Where a device can be reached: its home relay and its direct sockets.
/// Together with the device key this is an iroh `EndpointAddr`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddrHint {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay_url: Option<String>,
    #[serde(default)]
    pub direct_addrs: Vec<String>,
}

impl AddrHint {
    /// Signed form: relay URL (or empty) then one direct address per line,
    /// joined by `\n`, exactly as sent.
    #[must_use]
    pub fn canonical(&self) -> Vec<u8> {
        let mut parts: Vec<&str> = vec![self.relay_url.as_deref().unwrap_or("")];
        parts.extend(self.direct_addrs.iter().map(String::as_str));
        parts.join("\n").into_bytes()
    }

    pub fn validate(&self) -> Result<(), ApiError> {
        if let Some(u) = &self.relay_url {
            let ok = u.len() <= MAX_URL_LEN
                && url::Url::parse(u).is_ok_and(|p| matches!(p.scheme(), "http" | "https"));
            if !ok {
                return Err(ApiError::BadRequest("addr_hint.relay_url"));
            }
        }
        if self.direct_addrs.len() > MAX_DIRECT_ADDRS
            || self
                .direct_addrs
                .iter()
                .any(|a| a.parse::<SocketAddr>().is_err())
        {
            return Err(ApiError::BadRequest("addr_hint.direct_addrs"));
        }
        Ok(())
    }

    pub fn socket_addrs(&self) -> impl Iterator<Item = SocketAddr> + '_ {
        self.direct_addrs.iter().filter_map(|a| a.parse().ok())
    }
}

/// `POST /v1/register` — signed body: [`AddrHint::canonical`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterReq {
    pub device_pub: String,
    #[serde(default)]
    pub addr_hint: AddrHint,
    pub timestamp: u64,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterResp {
    pub id: String,
    pub created: bool,
    pub presence_ttl: u64,
}

/// `POST /v1/presence` — same shape and signed body as register.
pub type PresenceReq = RegisterReq;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresenceResp {
    pub id: String,
    pub expires_in: u64,
}

/// `POST /v1/resolve` — signed body: the 9-digit `id` string.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveReq {
    pub id: String,
    pub controller_pub: String,
    pub timestamp: u64,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveResp {
    pub id: String,
    pub device_pub: String,
    pub addr_hint: AddrHint,
    pub expires_in: u64,
}

/// `POST /v1/report-failure` — sent by the HOST after a failed pairing.
/// Signed body: `controller_pub` as sent (or empty).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportFailureReq {
    pub device_pub: String,
    #[serde(default)]
    pub controller_pub: Option<String>,
    pub timestamp: u64,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportFailureResp {
    pub locked: bool,
}

/// `POST /v1/abuse` — signed body: `subject_pub \n subject_id \n reason`
/// (absent fields as empty strings).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbuseReq {
    pub device_pub: String,
    #[serde(default)]
    pub subject_pub: Option<String>,
    #[serde(default)]
    pub subject_id: Option<String>,
    pub reason: String,
    pub timestamp: u64,
    pub signature: String,
}

impl AbuseReq {
    #[must_use]
    pub fn canonical(&self) -> Vec<u8> {
        format!(
            "{}\n{}\n{}",
            self.subject_pub.as_deref().unwrap_or(""),
            self.subject_id.as_deref().unwrap_or(""),
            self.reason
        )
        .into_bytes()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbuseResp {
    pub reports: u64,
    pub blocked: bool,
}

/// `GET /v1/info` — what a client needs to use this server.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Info {
    pub relay_urls: Vec<String>,
    pub presence_ttl: u64,
    /// SHA-256 of the WebTransport certificate (hex), when self-signed: pass
    /// it to `new WebTransport(url, { serverCertificateHashes })`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wt_cert_sha256: Option<String>,
    pub gateway: bool,
    pub version: String,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: &'static str,
    message: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("bad request: {0}")]
    BadRequest(&'static str),
    #[error("{0}")]
    Auth(#[from] AuthError),
    #[error("device is not registered")]
    NotRegistered,
    #[error("device is offline or unknown")]
    Offline,
    #[error("rate limited")]
    RateLimited,
    #[error("too many failed pairings for this id; try again later")]
    LockedOut,
    #[error("blocked")]
    Blocked,
    #[error("host unreachable")]
    Unreachable,
    #[error("internal error")]
    Internal(#[from] StoreError),
}

impl ApiError {
    #[must_use]
    pub fn status(&self) -> StatusCode {
        match self {
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Auth(_) => StatusCode::UNAUTHORIZED,
            Self::NotRegistered | Self::Offline => StatusCode::NOT_FOUND,
            Self::RateLimited | Self::LockedOut => StatusCode::TOO_MANY_REQUESTS,
            Self::Blocked => StatusCode::FORBIDDEN,
            Self::Unreachable => StatusCode::BAD_GATEWAY,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::BadRequest(_) => "bad_request",
            Self::Auth(AuthError::Expired) => "expired",
            Self::Auth(_) => "bad_signature",
            Self::NotRegistered => "not_registered",
            Self::Offline => "offline",
            Self::RateLimited => "rate_limited",
            Self::LockedOut => "locked_out",
            Self::Blocked => "blocked",
            Self::Unreachable => "unreachable",
            Self::Internal(_) => "internal",
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if let Self::Internal(e) = &self {
            tracing::error!(error = %e, "store error");
        }
        let mut res = (
            self.status(),
            Json(ErrorBody {
                error: self.code(),
                message: self.to_string(),
            }),
        )
            .into_response();
        if matches!(self, Self::RateLimited | Self::LockedOut) {
            res.headers_mut()
                .insert(header::RETRY_AFTER, header::HeaderValue::from_static("60"));
        }
        res
    }
}

/// Rate limiters for the API. Separate buckets so anonymous lookups can't
/// starve signed clients, and resolves can't starve heartbeats.
#[derive(Debug)]
pub struct Limiters {
    pub write_ip: RateLimiter<IpAddr>,
    pub resolve_get_ip: RateLimiter<IpAddr>,
    pub resolve_post_ip: RateLimiter<IpAddr>,
    pub resolve_target: RateLimiter<u64>,
    pub resolve_signed_target: RateLimiter<(DeviceId, u64)>,
    pub gateway_ip: RateLimiter<IpAddr>,
}

impl Default for Limiters {
    fn default() -> Self {
        Self {
            // Heartbeats every ~30 s from many devices behind one NAT.
            write_ip: RateLimiter::new(Rate::new(60.0, 2.0), LIMITER_KEYS),
            // Browsers / anonymous: 10 burst, 12 per minute.
            resolve_get_ip: RateLimiter::new(Rate::new(10.0, 0.2), LIMITER_KEYS),
            resolve_post_ip: RateLimiter::new(Rate::new(30.0, 1.0), LIMITER_KEYS),
            resolve_target: RateLimiter::new(Rate::new(10.0, 0.5), LIMITER_KEYS),
            resolve_signed_target: RateLimiter::new(Rate::new(10.0, 0.5), LIMITER_KEYS),
            gateway_ip: RateLimiter::new(Rate::new(10.0, 0.2), LIMITER_KEYS),
        }
    }
}

/// Shared state of the rendezvous API (also used by relay access control and
/// the gateway).
#[derive(Debug)]
pub struct AppState {
    pub store: Arc<dyn Store>,
    pub metrics: Arc<Metrics>,
    pub limiters: Limiters,
    pub lockout: Lockout,
    pub presence_ttl: u64,
    pub abuse_block_threshold: u64,
    pub trust_forwarded: bool,
    pub info: Info,
}

impl AppState {
    #[must_use]
    pub fn new(store: Arc<dyn Store>, metrics: Arc<Metrics>) -> Self {
        Self {
            store,
            metrics,
            limiters: Limiters::default(),
            lockout: Lockout::standard(),
            presence_ttl: 60,
            abuse_block_threshold: 3,
            trust_forwarded: false,
            info: Info::default(),
        }
    }

    /// Anonymous lookup of `id` (GET resolve and the gateway).
    pub fn resolve_anonymous(
        &self,
        ip_limiter: &RateLimiter<IpAddr>,
        ip: IpAddr,
        id: u64,
    ) -> Result<Presence, ApiError> {
        let now = Instant::now();
        if !ip_limiter.check(&ip, now) {
            inc(&self.metrics.rate_limited);
            return Err(ApiError::RateLimited);
        }
        if self.lockout.is_locked(id, now) {
            inc(&self.metrics.locked_out);
            return Err(ApiError::LockedOut);
        }
        if !self.limiters.resolve_target.check(&id, now) {
            inc(&self.metrics.rate_limited);
            return Err(ApiError::RateLimited);
        }
        self.presence_of(id)
    }

    /// Signed lookup by `controller` (already authenticated).
    pub fn resolve_signed(
        &self,
        ip: IpAddr,
        controller: &DeviceId,
        id: u64,
    ) -> Result<Presence, ApiError> {
        let now = Instant::now();
        if self.store.is_blocked(controller)? {
            inc(&self.metrics.blocked_rejections);
            return Err(ApiError::Blocked);
        }
        if !self.limiters.resolve_post_ip.check(&ip, now) {
            inc(&self.metrics.rate_limited);
            return Err(ApiError::RateLimited);
        }
        // A locked target stays reachable only for controllers registered here:
        // they have an identity that can be reported and blocked.
        if self.lockout.is_locked(id, now) && self.store.id_for_key(controller)?.is_none() {
            inc(&self.metrics.locked_out);
            return Err(ApiError::LockedOut);
        }
        if !self
            .limiters
            .resolve_signed_target
            .check(&(*controller, id), now)
        {
            inc(&self.metrics.rate_limited);
            return Err(ApiError::RateLimited);
        }
        self.presence_of(id)
    }

    fn presence_of(&self, id: u64) -> Result<Presence, ApiError> {
        if let Some(p) = self.store.presence(id, auth::now_secs())? {
            inc(&self.metrics.resolves);
            Ok(p)
        } else {
            inc(&self.metrics.resolves_offline);
            Err(ApiError::Offline)
        }
    }

    fn check_sig(
        &self,
        label: &str,
        key_hex: &str,
        ts: u64,
        body: &[u8],
        sig_hex: &str,
    ) -> Result<DeviceId, ApiError> {
        let res = (|| {
            let key = auth::parse_key(key_hex)?;
            let sig = auth::parse_sig(sig_hex)?;
            auth::verify(label, &key, ts, body, &sig, auth::now_secs())?;
            Ok::<_, AuthError>(key)
        })();
        res.map_err(|e| {
            inc(&self.metrics.bad_signatures);
            ApiError::Auth(e)
        })
    }

    fn check_write_rate(&self, ip: IpAddr) -> Result<(), ApiError> {
        if self.limiters.write_ip.check(&ip, Instant::now()) {
            Ok(())
        } else {
            inc(&self.metrics.rate_limited);
            Err(ApiError::RateLimited)
        }
    }
}

/// The TCP peer, inserted as a request extension by the connection loop.
#[derive(Debug, Clone, Copy)]
pub struct Peer(pub SocketAddr);

/// Client IP: the TCP peer, or the first `X-Forwarded-For` hop when
/// `--trust-forwarded` is set.
#[derive(Debug, Clone, Copy)]
pub struct ClientIp(pub IpAddr);

impl FromRequestParts<Arc<AppState>> for ClientIp {
    type Rejection = std::convert::Infallible;

    fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> + Send {
        std::future::ready(Ok(Self(client_ip(
            &parts.headers,
            parts.extensions.get::<Peer>(),
            state.trust_forwarded,
        ))))
    }
}

#[must_use]
pub fn client_ip(headers: &HeaderMap, peer: Option<&Peer>, trust_forwarded: bool) -> IpAddr {
    if trust_forwarded
        && let Some(ip) = headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(',').next())
            .and_then(|v| v.trim().parse().ok())
    {
        return ip;
    }
    peer.map_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED), |p| p.0.ip())
}

/// Routes of the rendezvous role.
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/v1/register", post(register))
        .route("/v1/presence", post(presence))
        .route("/v1/resolve/{id}", get(resolve_get))
        .route("/v1/resolve", post(resolve_post))
        .route("/v1/report-failure", post(report_failure))
        .route("/v1/abuse", post(abuse))
        .with_state(state)
}

/// `/health`, `/ready`, `/metrics`, `/v1/info` — mounted for every role.
pub fn ops_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/ready", get(ready))
        .route("/metrics", get(metrics))
        .route("/v1/info", get(info))
        .with_state(state)
}

fn parse_id(s: &str) -> Result<u64, ApiError> {
    ids::parse_id(s).ok_or(ApiError::BadRequest("id"))
}

fn upsert_presence(state: &AppState, id: u64, hint: &AddrHint) -> Result<u64, ApiError> {
    let json = serde_json::to_string(hint).map_err(|_| ApiError::BadRequest("addr_hint"))?;
    let expires_at = auth::now_secs() + state.presence_ttl;
    state.store.set_presence(id, &json, expires_at)?;
    Ok(state.presence_ttl)
}

async fn register(
    State(state): State<Arc<AppState>>,
    ClientIp(ip): ClientIp,
    Json(req): Json<RegisterReq>,
) -> Result<Json<RegisterResp>, ApiError> {
    state.check_write_rate(ip)?;
    let key = state.check_sig(
        auth::LABEL_REGISTER,
        &req.device_pub,
        req.timestamp,
        &req.addr_hint.canonical(),
        &req.signature,
    )?;
    req.addr_hint.validate()?;
    if state.store.is_blocked(&key)? {
        inc(&state.metrics.blocked_rejections);
        return Err(ApiError::Blocked);
    }
    let reg = state.store.register(&key, auth::now_secs())?;
    if reg.created {
        inc(&state.metrics.registrations);
        tracing::info!(id = reg.id, key = %key.fingerprint(), "registered");
    }
    let ttl = upsert_presence(&state, reg.id, &req.addr_hint)?;
    Ok(Json(RegisterResp {
        id: ids::format_id(reg.id),
        created: reg.created,
        presence_ttl: ttl,
    }))
}

async fn presence(
    State(state): State<Arc<AppState>>,
    ClientIp(ip): ClientIp,
    Json(req): Json<PresenceReq>,
) -> Result<Json<PresenceResp>, ApiError> {
    state.check_write_rate(ip)?;
    let key = state.check_sig(
        auth::LABEL_PRESENCE,
        &req.device_pub,
        req.timestamp,
        &req.addr_hint.canonical(),
        &req.signature,
    )?;
    req.addr_hint.validate()?;
    if state.store.is_blocked(&key)? {
        inc(&state.metrics.blocked_rejections);
        return Err(ApiError::Blocked);
    }
    let id = state
        .store
        .id_for_key(&key)?
        .ok_or(ApiError::NotRegistered)?;
    let ttl = upsert_presence(&state, id, &req.addr_hint)?;
    inc(&state.metrics.presence_updates);
    Ok(Json(PresenceResp {
        id: ids::format_id(id),
        expires_in: ttl,
    }))
}

fn resolve_resp(state: &AppState, id: u64, p: &Presence) -> Result<ResolveResp, ApiError> {
    let addr_hint: AddrHint = serde_json::from_str(&p.addr_hint)
        .map_err(|_| ApiError::Internal(StoreError::Corrupt("addr_hint")))?;
    let _ = state;
    Ok(ResolveResp {
        id: ids::format_id(id),
        device_pub: p.key.to_hex(),
        addr_hint,
        expires_in: p.expires_at.saturating_sub(auth::now_secs()),
    })
}

async fn resolve_get(
    State(state): State<Arc<AppState>>,
    ClientIp(ip): ClientIp,
    Path(id): Path<String>,
) -> Result<Json<ResolveResp>, ApiError> {
    let id = parse_id(&id)?;
    let p = state.resolve_anonymous(&state.limiters.resolve_get_ip, ip, id)?;
    Ok(Json(resolve_resp(&state, id, &p)?))
}

async fn resolve_post(
    State(state): State<Arc<AppState>>,
    ClientIp(ip): ClientIp,
    Json(req): Json<ResolveReq>,
) -> Result<Json<ResolveResp>, ApiError> {
    let controller = state.check_sig(
        auth::LABEL_RESOLVE,
        &req.controller_pub,
        req.timestamp,
        req.id.as_bytes(),
        &req.signature,
    )?;
    let id = parse_id(&req.id)?;
    let p = state.resolve_signed(ip, &controller, id)?;
    Ok(Json(resolve_resp(&state, id, &p)?))
}

async fn report_failure(
    State(state): State<Arc<AppState>>,
    ClientIp(ip): ClientIp,
    Json(req): Json<ReportFailureReq>,
) -> Result<Json<ReportFailureResp>, ApiError> {
    state.check_write_rate(ip)?;
    let body = req
        .controller_pub
        .as_deref()
        .unwrap_or("")
        .as_bytes()
        .to_vec();
    let host = state.check_sig(
        auth::LABEL_REPORT_FAILURE,
        &req.device_pub,
        req.timestamp,
        &body,
        &req.signature,
    )?;
    let id = state
        .store
        .id_for_key(&host)?
        .ok_or(ApiError::NotRegistered)?;
    inc(&state.metrics.failure_reports);
    let locked = state.lockout.record_failure(id, Instant::now());
    if locked {
        tracing::warn!(id, "pairing lockout engaged");
    }
    Ok(Json(ReportFailureResp { locked }))
}

async fn abuse(
    State(state): State<Arc<AppState>>,
    ClientIp(ip): ClientIp,
    Json(req): Json<AbuseReq>,
) -> Result<Json<AbuseResp>, ApiError> {
    state.check_write_rate(ip)?;
    let reporter = state.check_sig(
        auth::LABEL_ABUSE,
        &req.device_pub,
        req.timestamp,
        &req.canonical(),
        &req.signature,
    )?;
    if req.reason.is_empty() || req.reason.chars().count() > MAX_REASON_LEN {
        return Err(ApiError::BadRequest("reason"));
    }
    // Only registered devices may report: each report is tied to an identity.
    if state.store.id_for_key(&reporter)?.is_none() {
        return Err(ApiError::NotRegistered);
    }
    let subject = match (&req.subject_pub, &req.subject_id) {
        (Some(k), _) => auth::parse_key(k).map_err(|_| ApiError::BadRequest("subject_pub"))?,
        (None, Some(id)) => state
            .store
            .key_for_id(parse_id(id)?)?
            .ok_or(ApiError::NotRegistered)?,
        (None, None) => return Err(ApiError::BadRequest("subject")),
    };
    if subject == reporter {
        return Err(ApiError::BadRequest("subject"));
    }
    let now = auth::now_secs();
    let reports = state
        .store
        .add_abuse_report(&reporter, &subject, &req.reason, now)?;
    inc(&state.metrics.abuse_reports);
    let blocked = if reports >= state.abuse_block_threshold {
        state.store.block(&subject, "abuse reports", now)?;
        tracing::warn!(subject = %subject.fingerprint(), reports, "device key blocked");
        true
    } else {
        state.store.is_blocked(&subject)?
    };
    Ok(Json(AbuseResp { reports, blocked }))
}

async fn ready(State(state): State<Arc<AppState>>) -> Response {
    match state.store.ping() {
        Ok(()) => (StatusCode::OK, "ready").into_response(),
        Err(e) => {
            tracing::error!(error = %e, "store not ready");
            (StatusCode::SERVICE_UNAVAILABLE, "store unavailable").into_response()
        }
    }
}

async fn metrics(State(state): State<Arc<AppState>>) -> Response {
    let devices = state.store.device_count().unwrap_or(0);
    (
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        state.metrics.render(devices),
    )
        .into_response()
}

async fn info(State(state): State<Arc<AppState>>) -> Json<Info> {
    Json(state.info.clone())
}
