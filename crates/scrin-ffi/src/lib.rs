//! `UniFFI` bindings of the scrin core for Android (library mode, proc-macros only).
//!
//! Coarse surface: one [`ScrinCore`] object per app process owning the device
//! identity, the trust store, the iroh endpoint and a tokio runtime; sessions
//! report through a Kotlin-implemented [`SessionListener`]. Protocol, crypto
//! and session policy stay in Rust (ADR-0006); Kotlin owns capture, codecs,
//! accessibility input and UI.
//!
//! Listener callbacks run on core worker threads. They must not call a
//! blocking core method ([`ScrinCore::host_info`]) synchronously; such calls
//! return [`ScrinError::State`] instead of deadlocking.

mod media;
mod rendezvous;
mod session;
mod ticket;
mod types;
mod wire;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use iroh::EndpointAddr;
use scrin_crypto::code::{self, OneTimeCode};
use scrin_crypto::identity::{DeviceId, Identity};
use scrin_crypto::trust::{Profile, TrustStore, TrustedPeer};
use scrin_net::handshake::HostCode;
use scrin_net::{NetConfig, NetEndpoint, RelayConfig, RelayUrl};
use tokio::runtime::Runtime;
use tokio::task::JoinHandle;

pub use types::*;

uniffi::setup_scaffolding!();

const TRUST_FILE: &str = "trust.bin";
/// How long `host_info` waits for the endpoint to learn a direct address.
const ADDR_WAIT: Duration = Duration::from_secs(3);
/// Presence refresh bounds (the server's TTL decides within them).
const PRESENCE_MIN: Duration = Duration::from_secs(10);
const PRESENCE_MAX: Duration = Duration::from_secs(300);
const PRESENCE_RETRY: Duration = Duration::from_secs(15);

/// What a controller dials.
#[derive(Debug, Clone)]
pub(crate) enum Target {
    Addr(EndpointAddr),
    /// 9-digit scrin ID, resolved through the rendezvous server.
    ScrinId(String),
    /// Five dictated words (D24): locator looked up on the server; the
    /// secret words are the pairing password (`code` is ignored).
    Phrase {
        locator: u32,
        password: zeroize::Zeroizing<String>,
    },
}

/// State shared between the exported object and its background tasks.
pub(crate) struct Shared {
    identity: Identity,
    data_dir: PathBuf,
    config: CoreConfig,
    server: Option<rendezvous::Client>,
    endpoint: tokio::sync::OnceCell<NetEndpoint>,
    trust: Mutex<TrustStore>,
    code: Mutex<Option<Arc<HostCode>>>,
    session: Mutex<Option<Arc<session::Session>>>,
    host_loop: Mutex<Option<JoinHandle<()>>>,
    presence_loop: Mutex<Option<JoinHandle<()>>>,
    scrin_id: Mutex<Option<String>>,
}

impl Shared {
    pub(crate) async fn endpoint(&self) -> Result<&NetEndpoint, ScrinError> {
        self.endpoint
            .get_or_try_init(|| async {
                let cfg = net_config(&self.config)?;
                Ok::<_, ScrinError>(NetEndpoint::bind(*self.identity.seed(), cfg).await?)
            })
            .await
    }

    pub(crate) fn device_id(&self) -> DeviceId {
        self.identity.device_id()
    }

    /// The endpoint's dialable address; waits (bounded) for a direct address
    /// when the endpoint has just been bound.
    pub(crate) async fn dial_addr(&self) -> Result<EndpointAddr, ScrinError> {
        let ep = self.endpoint().await?;
        let deadline = tokio::time::Instant::now() + ADDR_WAIT;
        while !self.config.loopback_only
            && ep.addr().ip_addrs().next().is_none()
            && tokio::time::Instant::now() < deadline
        {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Ok(ep.addr())
    }

    pub(crate) fn host_code(&self) -> Option<Arc<HostCode>> {
        lock(&self.code).clone()
    }

    pub(crate) fn add_trust(&self, device: DeviceId, label: String, profile: Profile) {
        let mut t = lock(&self.trust);
        t.upsert(TrustedPeer {
            device,
            label,
            profile,
            added_at: unix_now(),
            expires_at: None,
        });
        // Best effort: an unwritable data dir keeps the in-memory entry for this run.
        let _ = self.save_trust(&t);
    }

    fn save_trust(&self, t: &TrustStore) -> Result<(), ScrinError> {
        let sealed = t.seal(&self.identity)?;
        std::fs::create_dir_all(&self.data_dir).map_err(|e| ScrinError::storage(&e))?;
        std::fs::write(self.data_dir.join(TRUST_FILE), sealed).map_err(|e| ScrinError::storage(&e))
    }

    pub(crate) fn current_session(&self) -> Option<Arc<session::Session>> {
        lock(&self.session).clone()
    }

    /// Installs `s` unless another session is running.
    pub(crate) fn set_session(&self, s: &Arc<session::Session>) -> bool {
        let mut cur = lock(&self.session);
        if cur.is_some() {
            return false;
        }
        *cur = Some(Arc::clone(s));
        true
    }

    pub(crate) fn clear_session(&self, id: u64) {
        let mut s = lock(&self.session);
        if s.as_ref().is_some_and(|x| x.id() == id) {
            *s = None;
        }
    }
}

/// Host: register with the rendezvous server, then keep the presence fresh
/// with the current addresses until aborted.
async fn presence_loop(shared: Arc<Shared>, listener: Arc<dyn SessionListener>) {
    let Some(server) = shared.server.clone() else {
        return;
    };
    let mut registered = false;
    loop {
        let wait = match shared.dial_addr().await {
            Err(e) => {
                listener.on_error(e.to_string());
                return;
            }
            Ok(addr) => {
                let ep_bound = match shared.endpoint().await {
                    Ok(ep) => ep.bound_sockets(),
                    Err(_) => Vec::new(),
                };
                let hint = rendezvous::AddrHint::from_addr(&addr, &ep_bound);
                let res = if registered {
                    match server.presence(&shared.identity, &hint).await {
                        Err(rendezvous::ServerError::NotRegistered) => {
                            server.register(&shared.identity, &hint).await
                        }
                        other => other,
                    }
                } else {
                    server.register(&shared.identity, &hint).await
                };
                match res {
                    Ok((id, ttl)) => {
                        registered = true;
                        let changed =
                            lock(&shared.scrin_id).replace(id.clone()) != Some(id.clone());
                        if changed {
                            listener.on_registered(id);
                        }
                        Duration::from_secs(ttl / 2).clamp(PRESENCE_MIN, PRESENCE_MAX)
                    }
                    Err(e) => {
                        listener.on_error(ScrinError::from(e).to_string());
                        PRESENCE_RETRY
                    }
                }
            }
        };
        tokio::time::sleep(wait).await;
    }
}

/// The scrin core: identity, trust list, network endpoint, sessions.
#[derive(uniffi::Object)]
pub struct ScrinCore {
    rt: Option<Runtime>,
    shared: Arc<Shared>,
}

impl std::fmt::Debug for ScrinCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrinCore")
            .field("device", &self.shared.device_id())
            .finish_non_exhaustive()
    }
}

impl Drop for ScrinCore {
    fn drop(&mut self) {
        if let Some(rt) = self.rt.take() {
            rt.shutdown_background();
        }
    }
}

impl ScrinCore {
    fn rt(&self) -> Result<&Runtime, ScrinError> {
        self.rt
            .as_ref()
            .ok_or_else(|| ScrinError::state("core shut down"))
    }

    fn block_on<T>(&self, f: impl Future<Output = Result<T, ScrinError>>) -> Result<T, ScrinError> {
        if tokio::runtime::Handle::try_current().is_ok() {
            return Err(ScrinError::state(
                "blocking core call from a core callback thread",
            ));
        }
        self.rt()?.block_on(f)
    }

    fn with_session(
        &self,
        f: impl FnOnce(&session::Session) -> Result<(), ScrinError>,
    ) -> Result<(), ScrinError> {
        let s = self
            .shared
            .current_session()
            .ok_or_else(|| ScrinError::state("no session"))?;
        f(&s)
    }
}

// UniFFI passes owned values across the boundary; taking them by value is the ABI.
#[allow(clippy::needless_pass_by_value)]
#[uniffi::export]
impl ScrinCore {
    /// `data_dir`: app-private directory for the sealed trust store.
    /// `seed`: the 32-byte identity seed unsealed from the Android Keystore,
    /// or `None` on first run (then persist [`Self::identity_seed`]).
    #[uniffi::constructor]
    pub fn new(
        data_dir: String,
        seed: Option<Vec<u8>>,
        config: CoreConfig,
    ) -> Result<Arc<Self>, ScrinError> {
        let identity = match seed {
            Some(s) => {
                let arr: [u8; 32] = s
                    .as_slice()
                    .try_into()
                    .map_err(|_| ScrinError::input("seed must be 32 bytes"))?;
                Identity::from_seed(arr)
            }
            None => Identity::generate()?,
        };
        net_config(&config)?;
        let server = config
            .server_url
            .as_deref()
            .map(str::trim)
            .filter(|u| !u.is_empty())
            .map(rendezvous::Client::new)
            .transpose()?;
        let data_dir = PathBuf::from(data_dir);
        // A missing file is a fresh install; a bad MAC fails closed to an empty store.
        let trust = std::fs::read(data_dir.join(TRUST_FILE))
            .ok()
            .and_then(|b| TrustStore::open(&b, &identity).ok())
            .unwrap_or_default();
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("scrin-core")
            .enable_all()
            .build()
            .map_err(|e| ScrinError::state(e.to_string()))?;
        Ok(Arc::new(Self {
            rt: Some(rt),
            shared: Arc::new(Shared {
                identity,
                data_dir,
                config,
                server,
                endpoint: tokio::sync::OnceCell::new(),
                trust: Mutex::new(trust),
                code: Mutex::new(None),
                session: Mutex::new(None),
                host_loop: Mutex::new(None),
                presence_loop: Mutex::new(None),
                scrin_id: Mutex::new(None),
            }),
        }))
    }

    /// The seed to seal into the Android Keystore. Never log it.
    pub fn identity_seed(&self) -> Vec<u8> {
        self.shared.identity.seed().to_vec()
    }

    /// 64-hex device id (= iroh endpoint id).
    pub fn device_id(&self) -> String {
        self.shared.device_id().to_hex()
    }

    /// Short `xxxx-xxxx-xxxx-xxxx` fingerprint for settings and dialogs.
    pub fn fingerprint(&self) -> String {
        self.shared.device_id().fingerprint()
    }

    /// Replaces the host's one-time code (single use, 10 minutes).
    pub fn new_one_time_code(&self) -> Result<CodeInfo, ScrinError> {
        let code = OneTimeCode::generate()?;
        let info = CodeInfo {
            display: code.display(),
            expires_in_s: u32::try_from(code.remaining().as_secs()).unwrap_or(u32::MAX),
        };
        *lock(&self.shared.code) = Some(Arc::new(HostCode::new(code)));
        Ok(info)
    }

    /// Whether the current code can still be used (exists and not consumed).
    pub fn code_valid(&self) -> bool {
        lock(&self.shared.code)
            .as_ref()
            .is_some_and(|c| !c.is_consumed())
    }

    /// Arms a five-word passphrase on the current one-time code slot (D24):
    /// asks the server for a locator, draws three secret words locally.
    /// Call after [`Self::new_one_time_code`] (a new code drops the words)
    /// and when `expires_in_s` runs out. Needs a server and a registered
    /// host (`on_registered`). `lang`: UI locale such as `ro-RO`. Blocking.
    pub fn new_passphrase(&self, lang: String) -> Result<PassphraseInfo, ScrinError> {
        let slot = self
            .shared
            .host_code()
            .ok_or_else(|| ScrinError::state("generate a one-time code first"))?;
        let shared = Arc::clone(&self.shared);
        let (locator, ttl) = self.block_on(async move {
            let server = shared
                .server
                .as_ref()
                .ok_or_else(|| ScrinError::input("a passphrase needs a server"))?;
            Ok(server.allocate_locator(&shared.identity).await?)
        })?;
        let phrase = scrin_crypto::phrase::Passphrase::generate(locator)?;
        slot.set_phrase(phrase.pake_password());
        Ok(PassphraseInfo {
            words: phrase.display(scrin_crypto::phrase::Lang::from_locale(&lang)),
            expires_in_s: u32::try_from(ttl).unwrap_or(u32::MAX),
        })
    }

    /// Binds the endpoint if needed and returns the connect ticket. Blocking.
    pub fn host_info(&self) -> Result<HostInfo, ScrinError> {
        let shared = Arc::clone(&self.shared);
        self.block_on(async move {
            let addr = shared.dial_addr().await?;
            let ep = shared.endpoint().await?;
            Ok(HostInfo {
                ticket: ticket::encode(&addr, &ep.bound_sockets()),
                device_id: shared.device_id().to_hex(),
                fingerprint: shared.device_id().fingerprint(),
                scrin_id: lock(&shared.scrin_id).clone(),
            })
        })
    }

    /// Starts accepting quick-connect sessions with the current one-time code
    /// and, with a server configured, registers this device and keeps its
    /// presence fresh (`on_registered` reports the scrin ID).
    pub fn start_host(&self, listener: Arc<dyn SessionListener>) -> Result<(), ScrinError> {
        if lock(&self.shared.code).is_none() {
            return Err(ScrinError::state("generate a one-time code first"));
        }
        let rt = self.rt()?;
        if self.shared.server.is_some() {
            let mut p = lock(&self.shared.presence_loop);
            if p.as_ref().is_none_or(JoinHandle::is_finished) {
                *p = Some(rt.spawn(presence_loop(
                    Arc::clone(&self.shared),
                    Arc::clone(&listener),
                )));
            }
        }
        let task = rt.spawn(session::host_loop(Arc::clone(&self.shared), listener));
        if let Some(old) = lock(&self.shared.host_loop).replace(task) {
            old.abort();
        }
        Ok(())
    }

    /// Stops accepting new sessions and refreshing presence (a running session continues).
    pub fn stop_host(&self) {
        if let Some(t) = lock(&self.shared.host_loop).take() {
            t.abort();
        }
        if let Some(t) = lock(&self.shared.presence_loop).take() {
            t.abort();
        }
    }

    /// Dials `target` (ticket, 64-hex id, or a 9-digit scrin ID when a server is
    /// configured) and pairs with `code`. Returns at once; progress arrives on `listener`.
    pub fn connect(
        &self,
        target: String,
        code: String,
        listener: Arc<dyn SessionListener>,
    ) -> Result<(), ScrinError> {
        let phrase = scrin_crypto::phrase::parse(&target).ok();
        let target = match (rendezvous::normalize_scrin_id(&target), phrase) {
            (Some(id), _) if self.shared.server.is_some() => Target::ScrinId(id),
            (_, Some(p)) if self.shared.server.is_some() => Target::Phrase {
                locator: p.locator,
                password: p.password,
            },
            (Some(_), _) | (_, Some(_)) => {
                return Err(ScrinError::input("a scrin ID or passphrase needs a server"));
            }
            (None, None) => Target::Addr(ticket::decode(&target)?),
        };
        // Validate locally so a typo never reaches the host (and never burns its code).
        if !matches!(target, Target::Phrase { .. }) {
            code::normalize(&code)?;
        }
        let s = session::Session::controller(listener);
        if !self.shared.set_session(&s) {
            return Err(ScrinError::state("a session is already running"));
        }
        let task = self.rt()?.spawn(session::controller_flow(
            Arc::clone(&self.shared),
            Arc::clone(&s),
            target,
            code,
        ));
        s.track(task);
        Ok(())
    }

    /// Host: the person tapped Accept with this selection.
    pub fn host_accept(&self, permissions: Vec<SessionPermission>) -> Result<(), ScrinError> {
        self.with_session(|s| s.host_accept(perms_from_ffi(&permissions)))
    }

    pub fn host_reject(&self) -> Result<(), ScrinError> {
        self.with_session(session::Session::host_reject)
    }

    pub fn grant_permission(&self, permission: SessionPermission) -> Result<(), ScrinError> {
        self.with_session(|s| s.host_grant(permission.to_core()))
    }

    pub fn revoke_permission(&self, permission: SessionPermission) -> Result<(), ScrinError> {
        self.with_session(|s| s.host_revoke(permission.to_core()))
    }

    /// Host: trust this controller for unattended access (refused for anonymous sessions).
    pub fn trust_current_peer(&self) -> Result<(), ScrinError> {
        self.with_session(session::Session::host_add_trust)
    }

    /// Controller: ask the host for one more permission.
    pub fn request_permission(&self, permission: SessionPermission) -> Result<(), ScrinError> {
        self.with_session(|s| s.controller_request(permission.to_core()))
    }

    /// Controller: send input (dropped unless the host granted Input).
    pub fn send_input(&self, event: RemoteInput) -> Result<(), ScrinError> {
        self.with_session(|s| s.send_input(event))
    }

    /// Host: announce the encoder configuration (before the first frame and on change).
    pub fn send_video_config(&self, config: VideoConfigInfo) -> Result<(), ScrinError> {
        self.with_session(|s| s.send_video_config(config))
    }

    /// Host: one encoded access unit (Annex B). FEC-sharded into datagrams.
    pub fn send_video_frame(&self, data: Vec<u8>, keyframe: bool) -> Result<(), ScrinError> {
        self.with_session(|s| s.send_video_frame(&data, keyframe))
    }

    /// Controller: the decoder lost sync; ask the host for a keyframe.
    pub fn request_keyframe(&self) -> Result<(), ScrinError> {
        self.with_session(session::Session::request_keyframe)
    }

    /// Ends the session (host: Stop; controller: disconnect). No-op when idle.
    pub fn end_session(&self) {
        if let Some(s) = self.shared.current_session() {
            s.end_by_user(false);
        }
    }

    /// Host: end the session and report the controller (ADR-0009).
    pub fn stop_and_report(&self) {
        if let Some(s) = self.shared.current_session() {
            s.end_by_user(true);
        }
    }

    pub fn list_trusted(&self) -> Vec<TrustedDevice> {
        let now = unix_now();
        lock(&self.shared.trust)
            .peers()
            .iter()
            .filter(|p| p.expires_at.is_none_or(|e| now < e))
            .map(|p| TrustedDevice {
                device_id: p.device.to_hex(),
                fingerprint: p.device.fingerprint(),
                label: p.label.clone(),
                profile: profile_to_ffi(p.profile),
                added_at: p.added_at,
                expires_at: p.expires_at,
            })
            .collect()
    }

    pub fn add_trusted(
        &self,
        device_id: String,
        label: String,
        profile: TrustProfile,
        expires_at: Option<u64>,
    ) -> Result<(), ScrinError> {
        let device = parse_device_id(&device_id)?;
        let mut t = lock(&self.shared.trust);
        t.upsert(TrustedPeer {
            device,
            label,
            profile: profile_from_ffi(profile),
            added_at: unix_now(),
            expires_at,
        });
        self.shared.save_trust(&t)
    }

    /// Returns whether an entry was removed.
    pub fn remove_trusted(&self, device_id: String) -> Result<bool, ScrinError> {
        let device = parse_device_id(&device_id)?;
        let mut t = lock(&self.shared.trust);
        let removed = t.revoke(&device);
        if removed {
            self.shared.save_trust(&t)?;
        }
        Ok(removed)
    }
}

/// Parses a connect ticket for display before dialling.
#[uniffi::export]
pub fn parse_ticket(ticket: &str) -> Result<TicketInfo, ScrinError> {
    let addr = ticket::decode(ticket)?;
    let id = DeviceId(*addr.id.as_bytes());
    Ok(TicketInfo {
        device_id: id.to_hex(),
        fingerprint: id.fingerprint(),
        direct_addresses: u32::try_from(addr.ip_addrs().count()).unwrap_or(u32::MAX),
        relay_url: addr.relay_urls().next().map(ToString::to_string),
    })
}

/// Whether `input` is a well-formed one-time code (8 symbols, dashes/spaces ignored).
#[uniffi::export]
pub fn is_valid_code(input: &str) -> bool {
    code::normalize(input).is_ok()
}

/// Version of the native core.
#[uniffi::export]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn net_config(c: &CoreConfig) -> Result<NetConfig, ScrinError> {
    if c.loopback_only {
        return Ok(NetConfig::loopback());
    }
    if c.relay_urls.is_empty() {
        return Ok(NetConfig::default());
    }
    let urls = c
        .relay_urls
        .iter()
        .map(|u| u.parse::<RelayUrl>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ScrinError::input("relay url"))?;
    Ok(NetConfig {
        relay: RelayConfig::Custom(urls),
        bind_addr: None,
    })
}

fn parse_device_id(hex: &str) -> Result<DeviceId, ScrinError> {
    let bytes = data_encoding::HEXLOWER_PERMISSIVE
        .decode(hex.trim().as_bytes())
        .map_err(|_| ScrinError::input("device id"))?;
    let arr: [u8; 32] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| ScrinError::input("device id length"))?;
    Ok(DeviceId(arr))
}

const fn profile_to_ffi(p: Profile) -> TrustProfile {
    match p {
        Profile::ViewOnly => TrustProfile::ViewOnly,
        Profile::Support => TrustProfile::Support,
        Profile::Full => TrustProfile::Full,
    }
}

const fn profile_from_ffi(p: TrustProfile) -> Profile {
    match p {
        TrustProfile::ViewOnly => Profile::ViewOnly,
        TrustProfile::Support => Profile::Support,
        TrustProfile::Full => Profile::Full,
    }
}
