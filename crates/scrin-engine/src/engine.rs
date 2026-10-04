//! The engine actor: one tokio task owns all mutable state (identity, trust
//! store, one-time code, the host session machine, controller sessions) and
//! is driven by commands from the UI, messages from per-connection tasks and
//! a 100 ms tick. Nothing outside this task touches that state, so there are
//! no locks around session logic.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use iroh::{EndpointAddr, TransportAddr};
use scrin_crypto::code::{DEFAULT_TTL, OneTimeCode};
use scrin_crypto::identity::{DeviceId, Identity};
use scrin_crypto::phrase::{Lang, Passphrase};
use scrin_crypto::sas::Sas;
use scrin_crypto::trust::{Profile, TrustStore, TrustedPeer};
use scrin_media::adapt::Mode;
use scrin_media::clock::{ClockSync, Exchange};
use scrin_net::framing::{StreamKind, accept_stream, open_stream, write_frame};
use scrin_net::handshake::{
    ControlStream, HostCode, RejectReason as NetReject, controller_auth_trusted, controller_pair,
    controller_pair_phrase, host_auth_trusted, host_pair,
};
use scrin_net::reconnect::Backoff;
use scrin_net::{
    ALPN, Connection, NetConfig, NetEndpoint, NetError, RelayConfig, remote_device_id,
};
use scrin_proto::v1::{self, envelope::Payload};
use scrin_session::{
    ControllerAction, ControllerEnd, ControllerEvent, ControllerSession, ControllerState,
    ControllerStatus, HostAction, HostEvent, HostSession, HostState, PeerId, Permissions, Policy,
    SessionKind,
};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};
use zeroize::Zeroizing;

use crate::api::{
    Command, Event, Quality, Reply, Role, SessionId, SessionState, StatsInfo, Status, TrustedInfo,
    parse_permission, parse_permissions, permission_names,
};
use crate::backend::{DecoderConfig, InputEvent, MediaBackend, default_backend};
use crate::gw::{self, GW_ALPN, GwCode, Seal};
use crate::media::{
    HostStream, HostStreamConfig, MediaCtl, Receiver, ReceiverConfig, ReceiverStats,
    start_host_stream, start_receiver,
};
use crate::rendezvous::{AddrHint, Locator, RendezvousClient, ServerError, relay_urls};
use crate::resolve::{ConnectTarget, Resolver, StaticResolver, encode_ticket, parse_target};
use crate::secret::{SecretStore, platform_store};
use crate::wire::{
    Outgoing, end_from_wire, end_reason_name, end_to_wire, env, input_to_payload, perms_from_wire,
    perms_to_wire, read_input_stream, reader_task, reject_from_wire, reject_reason_name,
    reject_to_wire, writer_task,
};
use crate::{EngineError, Result};

const SEED_NAME: &str = "identity-seed";
const TRUST_FILE: &str = "trust.sealed";
const ID_FILE: &str = "scrin-id.json";
const TICK: Duration = Duration::from_millis(100);
const DIAL_TIMEOUT: Duration = Duration::from_secs(20);
/// Presence heartbeat (halved server TTL, at most this).
const PRESENCE_INTERVAL: Duration = Duration::from_secs(30);
/// Re-dial attempts of a dropped trusted session (backoff 0.5 s → 8 s, ~40 s).
const RECONNECT_ATTEMPTS: u32 = 8;
/// Failed code guesses within [`FAILURE_WINDOW_MS`] that trigger a lockout.
const MAX_FAILURES: usize = 5;
const FAILURE_WINDOW_MS: u64 = 10 * 60 * 1000;
const LOCKOUT_MS: u64 = 60 * 1000;
/// QUIC application close code for "host busy".
const BUSY_CODE: u32 = 0x5c10;
/// Ask for a new passphrase locator this long before the old one expires.
const PHRASE_RENEW_MS: u64 = 15_000;

/// How the engine is built. [`EngineConfig::new`] gives production defaults.
pub struct EngineConfig {
    /// Where the sealed trust store lives.
    pub data_dir: PathBuf,
    /// Where the identity seed lives (DPAPI on Windows).
    pub secrets: Box<dyn SecretStore>,
    pub net: NetConfig,
    pub backend: Arc<dyn MediaBackend>,
    pub resolver: Arc<dyn Resolver>,
    pub policy: Policy,
    /// Shown to the other side in the request interstitial.
    pub device_name: String,
    pub code_ttl: Duration,
    /// Rendezvous server base URL (`https://…`, or `http://…` on a private
    /// network). Enables registration (the real 9-digit scrin ID), presence
    /// every 30 s, signed resolve of scrin IDs (overrides `resolver`),
    /// failure reports, and the server's relays (when `net.relay` is
    /// `Default`).
    pub server: Option<String>,
    /// Put direct addresses in the presence hint. `false` advertises the
    /// relay only (every dial goes through the relay first).
    pub advertise_direct: bool,
    /// Accept browser sessions bridged by the server's gateway (`scrin-gw/1`).
    pub accept_gateway: bool,
}

impl std::fmt::Debug for EngineConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EngineConfig")
            .field("data_dir", &self.data_dir)
            .field("backend", &self.backend.name())
            .field("device_name", &self.device_name)
            .field("server", &self.server)
            .finish_non_exhaustive()
    }
}

impl EngineConfig {
    #[must_use]
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        let data_dir = data_dir.into();
        Self {
            secrets: platform_store(data_dir.join("secrets")),
            data_dir,
            net: NetConfig::default(),
            backend: default_backend(),
            resolver: Arc::new(StaticResolver::default()),
            policy: Policy::default(),
            device_name: std::env::var("COMPUTERNAME").unwrap_or_else(|_| "scrin".into()),
            code_ttl: DEFAULT_TTL,
            server: None,
            advertise_direct: true,
            accept_gateway: true,
        }
    }
}

/// Cheap, cloneable handle to a running engine.
#[derive(Debug, Clone)]
pub struct EngineHandle {
    tx: mpsc::UnboundedSender<ActorMsg>,
}

impl EngineHandle {
    /// Runs one command and waits for its reply.
    pub async fn call(&self, cmd: Command) -> Result<Reply> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(ActorMsg::Call(cmd, tx))
            .map_err(|_| EngineError::Stopped)?;
        rx.await.map_err(|_| EngineError::Stopped)?
    }

    /// `call` with a deadline, for UI bridges that must never hang.
    pub async fn call_timeout(&self, cmd: Command, limit: Duration) -> Result<Reply> {
        tokio::time::timeout(limit, self.call(cmd))
            .await
            .map_err(|_| EngineError::Timeout)?
    }

    pub async fn status(&self) -> Result<Status> {
        match self.call(Command::GetStatus).await? {
            Reply::Status(s) => Ok(s),
            _ => Err(EngineError::Protocol("unexpected reply")),
        }
    }

    /// Ends every session, closes the endpoint and stops the actor.
    pub async fn shutdown(&self) {
        let (tx, rx) = oneshot::channel();
        if self.tx.send(ActorMsg::Shutdown(tx)).is_ok() {
            let _ = rx.await;
        }
    }
}

/// Starts the engine: loads or creates the identity, binds the endpoint and
/// spawns the actor. Must be called inside a tokio runtime.
pub async fn start(
    mut cfg: EngineConfig,
) -> Result<(EngineHandle, mpsc::UnboundedReceiver<Event>)> {
    let identity = Arc::new(load_identity(&*cfg.secrets)?);
    let rendezvous = match cfg.server.as_deref().map(str::trim) {
        Some(s) if !s.is_empty() => Some(RendezvousClient::new(s, identity.clone())?),
        _ => None,
    };
    if let Some(rv) = &rendezvous
        && matches!(cfg.net.relay, RelayConfig::Default)
    {
        // The server's own relays; if /v1/info is unreachable now, the server
        // URL itself (scrin-server serves /relay on its API listener).
        let info = tokio::time::timeout(Duration::from_secs(3), rv.info())
            .await
            .ok()
            .and_then(std::result::Result::ok);
        let urls = relay_urls(rv.base(), info.as_ref());
        if !urls.is_empty() {
            info!(relays = ?urls, "using the server's relays");
            cfg.net.relay = RelayConfig::Custom(urls);
        }
    }
    let endpoint = NetEndpoint::bind(*identity.seed(), cfg.net.clone()).await?;
    if cfg.accept_gateway {
        endpoint
            .inner()
            .set_alpns(vec![ALPN.to_vec(), GW_ALPN.to_vec()]);
    }
    let (events, events_rx) = mpsc::unbounded_channel();
    let (trust, tampered) = load_trust(&cfg.data_dir, &identity);
    if tampered {
        let _ = events.send(Event::Error {
            session: None,
            code: "trust-tampered".into(),
            message: "trusted-device list failed its integrity check and was reset".into(),
        });
    }
    let (tx, rx) = mpsc::unbounded_channel();
    spawn_accept_loop(endpoint.clone(), tx.clone());
    let clock = Clock::new();
    let code = CodeState::new(cfg.code_ttl, &clock)?;
    let registered_id = rendezvous
        .as_ref()
        .and_then(|rv| load_registered(&cfg.data_dir, rv.base(), &identity.device_id()));
    let presence_task = rendezvous.as_ref().map(|rv| {
        spawn_presence(
            rv.clone(),
            endpoint.clone(),
            cfg.advertise_direct,
            tx.clone(),
        )
    });
    let resolver: Arc<dyn Resolver> = match &rendezvous {
        Some(rv) => Arc::new(rv.clone()),
        None => cfg.resolver,
    };
    info!(
        device = %identity.device_id().fingerprint(),
        backend = cfg.backend.name(),
        server = cfg.server.as_deref().unwrap_or("-"),
        "engine started"
    );
    let actor = Actor {
        identity,
        endpoint,
        backend: cfg.backend,
        resolver,
        device_name: cfg.device_name,
        code_ttl: cfg.code_ttl,
        data_dir: cfg.data_dir,
        trust,
        code,
        events,
        self_tx: tx.clone(),
        host: HostSession::new(cfg.policy),
        host_slot: None,
        host_handshakes: 0,
        controllers: HashMap::new(),
        next_id: 0,
        clock,
        failures: VecDeque::new(),
        lockout_until: None,
        last_second: Instant::now(),
        accept_gateway: cfg.accept_gateway,
        server_online: rendezvous.as_ref().map(|_| false),
        rendezvous,
        registered_id,
        presence_task,
        phrase: None,
        phrase_generation: 0,
    };
    tokio::spawn(actor.run(rx));
    Ok((EngineHandle { tx }, events_rx))
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SavedId {
    server: String,
    device: String,
    id: String,
}

/// The scrin ID this device got from `server` earlier, if any.
fn load_registered(dir: &std::path::Path, server: &str, device: &DeviceId) -> Option<String> {
    let bytes = std::fs::read(dir.join(ID_FILE)).ok()?;
    let s: SavedId = serde_json::from_slice(&bytes).ok()?;
    (s.server == server && s.device == device.to_hex() && s.id.len() == 9).then_some(s.id)
}

fn save_registered(dir: &std::path::Path, server: &str, device: &DeviceId, id: &str) -> Result<()> {
    let json = serde_json::to_vec(&SavedId {
        server: server.to_owned(),
        device: device.to_hex(),
        id: id.to_owned(),
    })
    .map_err(|_| EngineError::Invalid("scrin id"))?;
    std::fs::create_dir_all(dir)?;
    let path = dir.join(ID_FILE);
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

/// Registers once, then refreshes presence every ~30 s (half the server's
/// TTL), with exponential backoff (1 s → 60 s) while the server is
/// unreachable. Reports every outcome to the actor.
fn spawn_presence(
    rv: RendezvousClient,
    ep: NetEndpoint,
    direct: bool,
    tx: mpsc::UnboundedSender<ActorMsg>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        // Give the endpoint a moment to find its home relay so the first
        // hint already carries it.
        let _ = tokio::time::timeout(Duration::from_secs(5), ep.inner().online()).await;
        let mut backoff = Backoff::new(Duration::from_secs(1), Duration::from_secs(60), 0.2);
        let mut registered = false;
        let mut interval = PRESENCE_INTERVAL;
        loop {
            let hint = AddrHint::from_addr(&dial_addr(&ep), direct);
            let result = if registered {
                rv.presence(&hint).await
            } else {
                rv.register(&hint).await.map(|r| (r.id, r.presence_ttl))
            };
            let delay = match result {
                Ok((id, ttl)) => {
                    registered = true;
                    backoff.reset();
                    if ttl > 0 {
                        interval = Duration::from_secs((ttl / 2).clamp(1, 30));
                    }
                    let _ = tx.send(ActorMsg::Presence {
                        id: Some(id),
                        online: true,
                    });
                    interval
                }
                Err(e) => {
                    if e == ServerError::NotRegistered {
                        registered = false;
                    }
                    debug!(error = %e, "presence failed");
                    let _ = tx.send(ActorMsg::Presence {
                        id: None,
                        online: false,
                    });
                    backoff.next_delay_random()
                }
            };
            if tx.is_closed() {
                return;
            }
            tokio::time::sleep(delay).await;
        }
    })
}

/// This endpoint's dialable address: iroh's view plus concrete bound sockets.
fn dial_addr(ep: &NetEndpoint) -> EndpointAddr {
    let mut addr = ep.addr();
    for s in ep.bound_sockets() {
        if !s.ip().is_unspecified() {
            addr.addrs.insert(TransportAddr::Ip(s));
        }
    }
    addr
}

fn load_identity(store: &dyn SecretStore) -> Result<Identity> {
    if let Some(seed) = store.load(SEED_NAME)? {
        let seed: [u8; 32] = seed
            .as_slice()
            .try_into()
            .map_err(|_| EngineError::Secret("stored identity seed is not 32 bytes".into()))?;
        let seed = Zeroizing::new(seed);
        return Ok(Identity::from_seed(*seed));
    }
    let id = Identity::generate()?;
    store.store(SEED_NAME, id.seed())?;
    Ok(id)
}

/// The trust store, and whether a tampered file was discarded (fail closed).
fn load_trust(dir: &std::path::Path, id: &Identity) -> (TrustStore, bool) {
    match std::fs::read(dir.join(TRUST_FILE)) {
        Ok(bytes) => match TrustStore::open(&bytes, id) {
            Ok(t) => (t, false),
            Err(_) => (TrustStore::default(), true),
        },
        Err(_) => (TrustStore::default(), false),
    }
}

fn spawn_accept_loop(ep: NetEndpoint, tx: mpsc::UnboundedSender<ActorMsg>) {
    tokio::spawn(async move {
        while let Some(incoming) = ep.accept().await {
            let tx = tx.clone();
            tokio::spawn(async move {
                let Ok(accepting) = incoming.accept() else {
                    return;
                };
                match tokio::time::timeout(Duration::from_secs(10), accepting).await {
                    Ok(Ok(conn)) => {
                        let _ = tx.send(ActorMsg::Incoming(conn));
                    }
                    Ok(Err(e)) => debug!(error = %e, "incoming QUIC handshake failed"),
                    Err(_) => debug!("incoming QUIC handshake timed out"),
                }
            });
        }
    });
}

/// Epoch-aligned monotonic milliseconds.
#[derive(Debug, Clone, Copy)]
struct Clock {
    epoch_ms: u64,
    start: Instant,
}

impl Clock {
    fn new() -> Self {
        Self {
            epoch_ms: unix_ms(),
            start: Instant::now(),
        }
    }

    fn now_ms(&self) -> u64 {
        self.epoch_ms
            .saturating_add(u64::try_from(self.start.elapsed().as_millis()).unwrap_or(u64::MAX))
    }

    fn now_us(&self) -> u64 {
        u64::try_from(self.start.elapsed().as_micros()).unwrap_or(u64::MAX)
    }
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

fn unix_s() -> u64 {
    unix_ms() / 1000
}

/// The code on screen and the slot the handshake consumes it from.
struct CodeState {
    code: Option<OneTimeCode>,
    slot: Arc<HostCode>,
    /// The same code for the gateway path. Handshakes never overlap and any
    /// consumption rotates both, so the code still allows one guess.
    gw: Arc<GwCode>,
    issued_at: u64,
    expires_at: u64,
}

impl CodeState {
    fn new(ttl: Duration, clock: &Clock) -> Result<Self> {
        let code = OneTimeCode::generate_with_ttl(ttl)?;
        let now = clock.now_ms();
        Ok(Self {
            slot: Arc::new(HostCode::new(code.clone())),
            gw: Arc::new(GwCode::new(Some(code.clone()))),
            expires_at: now.saturating_add(u64::try_from(ttl.as_millis()).unwrap_or(u64::MAX)),
            issued_at: now,
            code: Some(code),
        })
    }

    /// No usable code: every attempt fails until the next rotation.
    fn locked(clock: &Clock, until: u64) -> Result<Self> {
        let dead = OneTimeCode::generate_with_ttl(Duration::ZERO)?;
        Ok(Self {
            slot: Arc::new(HostCode::new(dead)),
            gw: Arc::new(GwCode::new(None)),
            code: None,
            issued_at: clock.now_ms(),
            expires_at: until,
        })
    }

    fn is_consumed(&self) -> bool {
        self.slot.is_consumed() || self.gw.is_consumed()
    }
}

/// The passphrase on screen (D24). The locator comes from the server and
/// lives until `expires_at`; the secret words are re-drawn with every code
/// rotation and after each use, so one phrase allows one guess too.
struct PhraseState {
    lang: Lang,
    /// Bumped by every enable/disable, so a late server answer for an older
    /// request is ignored.
    generation: u64,
    /// `None` while the locator request is in flight.
    current: Option<Passphrase>,
    /// Epoch ms.
    expires_at: u64,
}

enum ActorMsg {
    Call(Command, oneshot::Sender<Result<Reply>>),
    Shutdown(oneshot::Sender<()>),
    Incoming(Connection),
    HostHandshake {
        conn: Connection,
        peer: DeviceId,
        result: std::result::Result<HostPaired, NetError>,
    },
    CtlHandshake {
        session: SessionId,
        result: std::result::Result<CtlPaired, CtlFailure>,
    },
    Control {
        session: SessionId,
        env: Option<v1::Envelope>,
    },
    Input {
        session: SessionId,
        event: InputEvent,
    },
    MediaError {
        session: SessionId,
        message: String,
    },
    /// Outcome of a register/presence call.
    Presence {
        id: Option<String>,
        online: bool,
    },
    Locator {
        generation: u64,
        result: std::result::Result<Locator, ServerError>,
    },
}

struct HostPaired {
    control: ControlStream,
    kind: SessionKind,
    sas: Option<Sas>,
    /// Gateway path: the inner channel, and the controller key from `Hello`.
    seal: Seal,
    claimed_peer: Option<DeviceId>,
}

struct CtlPaired {
    conn: Connection,
    control: ControlStream,
    sas: Option<Sas>,
    peer: DeviceId,
    trusted: bool,
}

#[derive(Debug)]
struct CtlFailure {
    code: &'static str,
    message: String,
    /// The failure happened during pairing (vs. resolve/dial).
    pairing: bool,
}

struct HostSlot {
    id: SessionId,
    conn: Connection,
    peer: DeviceId,
    out: mpsc::UnboundedSender<Outgoing>,
    stream: Option<HostStream>,
    sas: Option<[u8; 5]>,
    kind: SessionKind,
    trust_label: Option<String>,
    mode: Mode,
    seal: Seal,
    tasks: Vec<JoinHandle<()>>,
}

struct CtlSlot {
    machine: ControllerSession,
    code: Zeroizing<String>,
    /// What the user dialled, for re-dialling a trusted session.
    target: ConnectTarget,
    requested: Permissions,
    /// A trusted session is being re-established after a drop.
    reconnecting: bool,
    handshake: Option<JoinHandle<()>>,
    conn: Option<Connection>,
    out: Option<mpsc::UnboundedSender<Outgoing>>,
    input: Option<mpsc::UnboundedSender<v1::Envelope>>,
    receiver: Option<Receiver>,
    pending_video: Option<DecoderConfig>,
    stats: Arc<ReceiverStats>,
    cap: Arc<AtomicU32>,
    sas: Option<[u8; 5]>,
    peer: Option<DeviceId>,
    trusted: bool,
    clock: ClockSync,
    ping_seq: u64,
    last: StatsSnapshot,
    tasks: Vec<JoinHandle<()>>,
}

#[derive(Debug, Clone, Copy, Default)]
struct StatsSnapshot {
    at: Option<Instant>,
    decoded: u64,
    bytes: u64,
    completed: u64,
    lost: u64,
    decode_us: u64,
}

struct Actor {
    identity: Arc<Identity>,
    endpoint: NetEndpoint,
    backend: Arc<dyn MediaBackend>,
    resolver: Arc<dyn Resolver>,
    device_name: String,
    code_ttl: Duration,
    data_dir: PathBuf,
    trust: TrustStore,
    code: CodeState,
    events: mpsc::UnboundedSender<Event>,
    self_tx: mpsc::UnboundedSender<ActorMsg>,
    host: HostSession,
    host_slot: Option<HostSlot>,
    host_handshakes: usize,
    controllers: HashMap<SessionId, CtlSlot>,
    next_id: u64,
    clock: Clock,
    failures: VecDeque<u64>,
    lockout_until: Option<u64>,
    last_second: Instant,
    accept_gateway: bool,
    /// `None` without a server; else whether the last presence call worked.
    server_online: Option<bool>,
    rendezvous: Option<RendezvousClient>,
    registered_id: Option<String>,
    presence_task: Option<JoinHandle<()>>,
    phrase: Option<PhraseState>,
    phrase_generation: u64,
}

impl Actor {
    async fn run(mut self, mut rx: mpsc::UnboundedReceiver<ActorMsg>) {
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                msg = rx.recv() => match msg {
                    Some(ActorMsg::Shutdown(done)) => {
                        self.shutdown().await;
                        let _ = done.send(());
                        return;
                    }
                    Some(msg) => self.on_msg(msg),
                    None => {
                        self.shutdown().await;
                        return;
                    }
                },
                _ = tick.tick() => self.on_tick(),
            }
        }
    }

    fn emit(&self, e: Event) {
        let _ = self.events.send(e);
    }

    fn error(&self, session: Option<&str>, code: &str, message: impl Into<String>) {
        self.emit(Event::Error {
            session: session.map(str::to_owned),
            code: code.into(),
            message: message.into(),
        });
    }

    fn next_session(&mut self, prefix: char) -> SessionId {
        self.next_id += 1;
        format!("{prefix}{}", self.next_id)
    }

    fn on_msg(&mut self, msg: ActorMsg) {
        match msg {
            ActorMsg::Call(cmd, reply) => {
                let r = self.on_command(cmd);
                let _ = reply.send(r);
            }
            ActorMsg::Shutdown(_) => {}
            ActorMsg::Incoming(conn) => self.on_incoming(conn),
            ActorMsg::HostHandshake { conn, peer, result } => {
                self.on_host_handshake(conn, peer, result);
            }
            ActorMsg::CtlHandshake { session, result } => self.on_ctl_handshake(&session, result),
            ActorMsg::Control { session, env } => {
                if self.host_slot.as_ref().is_some_and(|h| h.id == session) {
                    self.on_host_control(env);
                } else {
                    self.on_ctl_control(&session, env);
                }
            }
            ActorMsg::Input { session, event } => self.on_host_input(&session, &event),
            ActorMsg::MediaError { session, message } => {
                self.error(Some(&session), "media", message);
            }
            ActorMsg::Presence { id, online } => self.on_presence(id, online),
            ActorMsg::Locator { generation, result } => self.on_locator(generation, result),
        }
    }

    // ---- status, code, trust ------------------------------------------------

    /// Asks the server for a new locator (new first two words). The secret
    /// words are drawn when the answer arrives.
    fn request_locator(&mut self, lang: Lang) -> Result<()> {
        let rv = self.rendezvous.clone().ok_or(EngineError::Invalid(
            "a passphrase needs a rendezvous server",
        ))?;
        self.phrase_generation += 1;
        let generation = self.phrase_generation;
        self.phrase = Some(PhraseState {
            lang,
            generation,
            current: None,
            expires_at: 0,
        });
        self.code.slot.clear_phrase();
        let tx = self.self_tx.clone();
        tokio::spawn(async move {
            let result = rv.allocate_locator().await;
            let _ = tx.send(ActorMsg::Locator { generation, result });
        });
        Ok(())
    }

    fn disable_phrase(&mut self) {
        self.phrase_generation += 1;
        self.code.slot.clear_phrase();
        if self.phrase.take().is_some()
            && let Some(rv) = self.rendezvous.clone()
        {
            tokio::spawn(async move {
                if let Err(e) = rv.release_locator().await {
                    debug!(error = %e, "could not release the passphrase locator");
                }
            });
        }
    }

    fn on_locator(&mut self, generation: u64, result: std::result::Result<Locator, ServerError>) {
        let now = self.clock.now_ms();
        let Some(state) = self.phrase.as_mut().filter(|p| p.generation == generation) else {
            return;
        };
        let drawn = result.map_err(|e| e.to_string()).and_then(|l| {
            Passphrase::generate(l.locator)
                .map(|p| (p, l.expires_in))
                .map_err(|e| e.to_string())
        });
        match drawn {
            Ok((p, ttl_s)) => {
                state.expires_at = now.saturating_add(ttl_s.saturating_mul(1000));
                self.code.slot.set_phrase(p.pake_password());
                state.current = Some(p);
            }
            Err(e) => {
                self.phrase = None;
                self.error(None, "phrase-unavailable", e);
            }
        }
        self.emit(Event::Status(self.status()));
    }

    /// After a code rotation or a used phrase: same locator, new secret words.
    fn redraw_phrase_secret(&mut self) {
        let Some(state) = self.phrase.as_mut() else {
            return;
        };
        let Some(locator) = state.current.as_ref().map(Passphrase::locator) else {
            return;
        };
        match Passphrase::generate(locator) {
            Ok(p) => {
                self.code.slot.set_phrase(p.pake_password());
                state.current = Some(p);
            }
            Err(e) => warn!(error = %e, "could not draw passphrase words"),
        }
    }

    fn on_presence(&mut self, id: Option<String>, online: bool) {
        let mut changed = self.server_online != Some(online);
        self.server_online = Some(online);
        if let Some(id) = id
            && self.registered_id.as_deref() != Some(id.as_str())
        {
            if let Some(rv) = &self.rendezvous
                && let Err(e) =
                    save_registered(&self.data_dir, rv.base(), &self.identity.device_id(), &id)
            {
                warn!(error = %e, "could not persist the scrin id");
            }
            info!(scrin_id = %id, "registered with the server");
            self.registered_id = Some(id);
            changed = true;
        }
        if changed {
            self.emit(Event::Status(self.status()));
        }
    }

    fn status(&self) -> Status {
        let id = self.identity.device_id();
        let addr = dial_addr(&self.endpoint);
        let (phrase, phrase_expires_at) = self
            .phrase
            .as_ref()
            .and_then(|s| {
                s.current
                    .as_ref()
                    .map(|p| (p.display(s.lang), s.expires_at))
            })
            .unwrap_or_default();
        Status {
            device_id: id.to_hex(),
            fingerprint: id.fingerprint(),
            scrin_id: self
                .registered_id
                .clone()
                .unwrap_or_else(|| provisional_scrin_id(&id)),
            online: self.server_online.unwrap_or(!addr.addrs.is_empty()),
            ticket: encode_ticket(&addr),
            code: self
                .code
                .code
                .as_ref()
                .map(OneTimeCode::display)
                .unwrap_or_default(),
            code_issued_at: self.code.issued_at,
            code_expires_at: self.code.expires_at,
            phrase,
            phrase_expires_at,
            backend: self.backend.name().into(),
        }
    }

    fn rotate_code(&mut self) -> Result<()> {
        let now = self.clock.now_ms();
        let lock = self.lockout_until.filter(|until| now < *until);
        self.code = if let Some(until) = lock {
            CodeState::locked(&self.clock, until)?
        } else {
            self.lockout_until = None;
            CodeState::new(self.code_ttl, &self.clock)?
        };
        // The new slot starts without a phrase; arm fresh secret words unless
        // locked out (a lockout disables every short secret).
        if lock.is_none() {
            self.redraw_phrase_secret();
        }
        self.emit(Event::Status(self.status()));
        Ok(())
    }

    fn record_failure(&mut self) {
        let now = self.clock.now_ms();
        self.failures.push_back(now);
        while self
            .failures
            .front()
            .is_some_and(|t| now.saturating_sub(*t) > FAILURE_WINDOW_MS)
        {
            self.failures.pop_front();
        }
        if self.failures.len() >= MAX_FAILURES {
            warn!(
                failures = self.failures.len(),
                "too many wrong codes; locking out"
            );
            self.failures.clear();
            self.lockout_until = Some(now + LOCKOUT_MS);
        }
    }

    fn save_trust(&self) -> Result<()> {
        let sealed = self.trust.seal(&self.identity)?;
        std::fs::create_dir_all(&self.data_dir)?;
        let path = self.data_dir.join(TRUST_FILE);
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, sealed)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }

    fn trusted_list(&self) -> Vec<TrustedInfo> {
        self.trust
            .peers()
            .iter()
            .map(|p| TrustedInfo {
                device: p.device.to_hex(),
                fingerprint: p.device.fingerprint(),
                label: p.label.clone(),
                profile: match p.profile {
                    Profile::ViewOnly => "view_only",
                    Profile::Support => "support",
                    Profile::Full => "full",
                }
                .into(),
                added_at: p.added_at,
                expires_at: p.expires_at,
            })
            .collect()
    }

    // ---- commands -----------------------------------------------------------

    #[expect(clippy::too_many_lines)] // a flat dispatch table; one arm per command
    fn on_command(&mut self, cmd: Command) -> Result<Reply> {
        match cmd {
            Command::GetStatus => Ok(Reply::Status(self.status())),
            Command::RegenerateCode => {
                self.rotate_code()?;
                Ok(Reply::Status(self.status()))
            }
            Command::EnablePhrase { lang } => {
                self.request_locator(Lang::from_locale(&lang))?;
                Ok(Reply::Status(self.status()))
            }
            Command::DisablePhrase => {
                self.disable_phrase();
                Ok(Reply::Status(self.status()))
            }
            Command::Connect {
                target,
                code,
                requested,
            } => {
                let requested = match requested {
                    Some(names) => parse_permissions(&names)?,
                    None => Permissions::support(),
                };
                self.connect(&target, code, requested).map(Reply::Session)
            }
            Command::ConfirmSas { session, matches } => {
                self.ctl_slot(&session)?;
                if !matches {
                    self.error(
                        Some(&session),
                        "sas-mismatch",
                        "emoji did not match; connection closed",
                    );
                    self.ctl_event(&session, ControllerEvent::Cancel);
                }
                Ok(Reply::Ok)
            }
            Command::Accept {
                session,
                permissions,
            } => {
                self.host_check(&session)?;
                let perms = parse_permissions(&permissions)?;
                self.host_event(HostEvent::UserAccept(perms));
                if matches!(self.host.state(), HostState::IncomingRequest { .. }) {
                    self.error(
                        Some(&session),
                        "accept-too-early",
                        "Accept is enabled after a short delay",
                    );
                }
                Ok(Reply::Ok)
            }
            Command::Reject { session } => {
                self.host_check(&session)?;
                self.host_event(HostEvent::UserReject);
                Ok(Reply::Ok)
            }
            Command::Revoke {
                session,
                permission,
            } => {
                self.host_check(&session)?;
                self.host_event(HostEvent::UserRevoke(parse_permission(&permission)?));
                Ok(Reply::Ok)
            }
            Command::Grant {
                session,
                permission,
            } => {
                self.host_check(&session)?;
                self.host_event(HostEvent::UserGrant(parse_permission(&permission)?));
                Ok(Reply::Ok)
            }
            Command::TrustPeer { session, label } => {
                self.host_check(&session)?;
                if let Some(h) = self.host_slot.as_mut() {
                    h.trust_label = Some(label.chars().take(64).collect());
                }
                self.host_event(HostEvent::UserAddTrust);
                Ok(Reply::Ok)
            }
            Command::EndSession { session } => {
                if self.host_slot.as_ref().is_some_and(|h| h.id == session) {
                    self.host_event(HostEvent::UserStop);
                } else {
                    self.ctl_slot(&session)?;
                    self.ctl_event(&session, ControllerEvent::Cancel);
                }
                Ok(Reply::Ok)
            }
            Command::SendInput { session, event } => {
                self.send_input(&session, &event)?;
                Ok(Reply::Ok)
            }
            Command::SetQuality { session, quality } => {
                self.set_quality(&session, quality)?;
                Ok(Reply::Ok)
            }
            Command::ListTrusted => Ok(Reply::Trusted(self.trusted_list())),
            Command::RemoveTrusted { device } => {
                let bytes = data_encoding::HEXLOWER_PERMISSIVE
                    .decode(device.as_bytes())
                    .ok()
                    .and_then(|b| <[u8; 32]>::try_from(b.as_slice()).ok())
                    .ok_or(EngineError::Invalid("device id"))?;
                if self.trust.revoke(&DeviceId(bytes)) {
                    self.save_trust()?;
                }
                Ok(Reply::Trusted(self.trusted_list()))
            }
        }
    }

    fn host_check(&self, session: &str) -> Result<()> {
        if self.host_slot.as_ref().is_some_and(|h| h.id == session) {
            Ok(())
        } else {
            Err(EngineError::UnknownSession(session.to_owned()))
        }
    }

    fn ctl_slot(&mut self, session: &str) -> Result<&mut CtlSlot> {
        self.controllers
            .get_mut(session)
            .ok_or_else(|| EngineError::UnknownSession(session.to_owned()))
    }

    fn send_input(&mut self, session: &str, event: &InputEvent) -> Result<()> {
        let slot = self.ctl_slot(session)?;
        if !slot
            .machine
            .granted()
            .contains(scrin_session::Permission::Input)
        {
            return Err(EngineError::Invalid("input permission not granted"));
        }
        let tx = slot
            .input
            .as_ref()
            .ok_or(EngineError::Invalid("input stream not open"))?;
        tx.send(env(input_to_payload(event)))
            .map_err(|_| EngineError::Invalid("input stream closed"))
    }

    fn set_quality(&mut self, session: &str, quality: Quality) -> Result<()> {
        if let Some(h) = self.host_slot.as_mut().filter(|h| h.id == session) {
            h.mode = if quality == Quality::Speed {
                Mode::Latency
            } else {
                Mode::Quality
            };
            if let Some(s) = &h.stream {
                s.control(MediaCtl::Mode(h.mode));
            }
            return Ok(());
        }
        let slot = self.ctl_slot(session)?;
        slot.cap
            .store(quality.cap_bps().unwrap_or(0), Ordering::Relaxed);
        Ok(())
    }

    // ---- host side ----------------------------------------------------------

    fn host_busy(&self) -> bool {
        self.host_slot.is_some()
            || self.host_handshakes > 0
            || matches!(
                self.host.state(),
                HostState::IncomingRequest { .. } | HostState::Active { .. }
            )
    }

    fn on_incoming(&mut self, conn: Connection) {
        let gateway = conn.alpn() == GW_ALPN;
        if gateway && !self.accept_gateway {
            conn.close(0u32.into(), b"gateway sessions disabled");
            return;
        }
        let peer = remote_device_id(&conn);
        // A trusted controller re-dialling after a network drop replaces its
        // own session (the old connection may not have timed out yet).
        if !gateway
            && self.host_handshakes == 0
            && self.trust.lookup(&peer, unix_s()).is_some()
            && let Some(old) = self
                .host_slot
                .as_ref()
                .filter(|h| h.peer == peer && h.seal.is_none())
                .map(|h| h.id.clone())
        {
            info!(session = %old, "trusted controller reconnected; replacing its session");
            self.host_event(HostEvent::PeerDisconnected);
            self.close_host(&old, "reconnected");
        }
        if self.host_busy() {
            debug!(peer = ?peer, "busy; refusing connection");
            let code = if gateway { gw::close::BUSY } else { BUSY_CODE };
            conn.close(code.into(), b"busy");
            return;
        }
        self.host_handshakes += 1;
        let me = self.endpoint.device_id();
        let tx = self.self_tx.clone();
        if gateway {
            // The iroh peer is the gateway; never look it up in the trust store.
            let code = self.code.gw.clone();
            let identity = self.identity.clone();
            tokio::spawn(async move {
                let (peer, result) = match gw::host_pair(&conn, &identity, &code).await {
                    Ok(o) => (
                        o.peer,
                        Ok(HostPaired {
                            control: ControlStream {
                                send: o.send,
                                recv: o.recv,
                                version: 1,
                            },
                            kind: SessionKind::Anonymous,
                            sas: Some(o.sas),
                            seal: Some(o.channel),
                            claimed_peer: Some(o.peer),
                        }),
                    ),
                    Err((claimed, e)) => (claimed.unwrap_or(peer), Err(e)),
                };
                let _ = tx.send(ActorMsg::HostHandshake { conn, peer, result });
            });
            return;
        }
        let trusted = self.trust.lookup(&peer, unix_s()).is_some();
        let slot = self.code.slot.clone();
        let trust = self.trust.clone();
        tokio::spawn(async move {
            let result = if trusted {
                host_auth_trusted(&conn, me, &trust, unix_s())
                    .await
                    .map(|o| HostPaired {
                        control: o.control,
                        kind: SessionKind::Trusted,
                        sas: None,
                        seal: None,
                        claimed_peer: None,
                    })
            } else {
                host_pair(&conn, me, &slot).await.map(|o| HostPaired {
                    control: o.control,
                    kind: SessionKind::Anonymous,
                    sas: Some(o.sas),
                    seal: None,
                    claimed_peer: None,
                })
            };
            let _ = tx.send(ActorMsg::HostHandshake { conn, peer, result });
        });
    }

    fn on_host_handshake(
        &mut self,
        conn: Connection,
        peer: DeviceId,
        result: std::result::Result<HostPaired, NetError>,
    ) {
        self.host_handshakes = self.host_handshakes.saturating_sub(1);
        if self.code.is_consumed()
            && let Err(e) = self.rotate_code()
        {
            warn!(error = %e, "could not rotate the one-time code");
        }
        let paired = match result {
            Ok(p) => p,
            Err(e) => {
                if matches!(e, NetError::PairingFailed | NetError::BadSignature) {
                    self.record_failure();
                    if self.lockout_until.is_some() {
                        let _ = self.rotate_code();
                    }
                    self.report_failure(peer);
                }
                info!(peer = %peer.fingerprint(), error = %e, "incoming handshake failed");
                let close = if conn.alpn() == GW_ALPN {
                    gw::close_code(&e)
                } else {
                    0
                };
                conn.close(close.into(), b"handshake failed");
                return;
            }
        };
        let peer = paired.claimed_peer.unwrap_or(peer);
        let id = self.next_session('h');
        let (out, out_rx) = mpsc::unbounded_channel();
        let ControlStream { send, recv, .. } = paired.control;
        let mut tasks = vec![tokio::spawn(writer_task(
            conn.clone(),
            send,
            out_rx,
            paired.seal.clone(),
        ))];
        let tx = self.self_tx.clone();
        let sid = id.clone();
        tasks.push(tokio::spawn(reader_task(
            recv,
            paired.seal.clone(),
            move |env| {
                let _ = tx.send(ActorMsg::Control {
                    session: sid.clone(),
                    env,
                });
            },
        )));
        tasks.push(tokio::spawn(accept_input_streams(
            conn.clone(),
            id.clone(),
            paired.seal.clone(),
            self.self_tx.clone(),
        )));
        info!(
            session = %id,
            peer = %peer.fingerprint(),
            trusted = paired.kind == SessionKind::Trusted,
            gateway = paired.seal.is_some(),
            "controller paired"
        );
        self.host_slot = Some(HostSlot {
            id,
            conn,
            peer,
            out,
            stream: None,
            sas: paired.sas.map(|s| s.0),
            kind: paired.kind,
            trust_label: None,
            mode: Mode::Quality,
            seal: paired.seal,
            tasks,
        });
    }

    /// Tells the server a pairing failed here, so it can lock the ID out
    /// across hosts and controllers (best effort, off the actor).
    fn report_failure(&self, controller: DeviceId) {
        let Some(rv) = self.rendezvous.clone() else {
            return;
        };
        tokio::spawn(async move {
            match rv.report_failure(Some(controller)).await {
                Ok(a) if a.locked => warn!("server locked this scrin ID after failed pairings"),
                Ok(_) => {}
                Err(e) => debug!(error = %e, "could not report the failed pairing"),
            }
        });
    }

    fn host_event(&mut self, event: HostEvent) {
        let actions = self.host.handle(self.clock.now_ms(), event);
        self.host_actions(actions);
    }

    fn host_send(&self, p: Payload) {
        if let Some(h) = &self.host_slot {
            let _ = h.out.send(Outgoing::Env(env(p)));
        }
    }

    #[expect(clippy::too_many_lines)] // one arm per HostAction; splitting hides the mapping
    fn host_actions(&mut self, actions: Vec<HostAction>) {
        // EndSession takes the slot; later actions (Notify) still need the id.
        let Some(id) = self.host_slot.as_ref().map(|h| h.id.clone()) else {
            return;
        };
        for action in actions {
            let id = id.clone();
            match action {
                HostAction::ShowRequestDialog {
                    kind,
                    requested,
                    allowed,
                    accept_enabled_at,
                    expires_at,
                    ..
                } => {
                    let Some(h) = &self.host_slot else { continue };
                    self.emit(Event::IncomingRequest {
                        session: id.clone(),
                        peer: h.peer.to_hex(),
                        fingerprint: h.peer.fingerprint(),
                        kind: kind_name(&kind).into(),
                        sas: h.sas,
                        requested: permission_names(requested),
                        allowed: permission_names(allowed),
                        accept_enabled_at,
                        expires_at,
                    });
                    self.emit(Event::StateChanged {
                        session: id,
                        role: Role::Host,
                        state: SessionState::AwaitingAccept,
                        peer: Some(h.peer.to_hex()),
                        reason: None,
                    });
                }
                HostAction::HideRequestDialog | HostAction::ShowIndicator { .. } => {}
                HostAction::SendAccept(granted) => {
                    let displays = self
                        .backend
                        .displays()
                        .into_iter()
                        .map(|d| v1::DisplayInfo {
                            id: d.id,
                            name: d.name,
                            width: d.width,
                            height: d.height,
                            scale: 1.0,
                            primary: d.primary,
                            ..Default::default()
                        })
                        .collect();
                    let max = self
                        .host
                        .kind()
                        .and_then(|k| self.host.policy().max_duration_ms(k))
                        .map_or(0, |ms| u32::try_from(ms / 1000).unwrap_or(u32::MAX));
                    self.host_send(Payload::SessionAccept(v1::SessionAccept {
                        granted: perms_to_wire(granted),
                        displays,
                        max_duration_s: max,
                    }));
                    let peer = self.host_slot.as_ref().map(|h| h.peer.to_hex());
                    self.emit(Event::StateChanged {
                        session: id.clone(),
                        role: Role::Host,
                        state: SessionState::Active,
                        peer,
                        reason: None,
                    });
                    self.apply_host_permissions(&id, granted);
                }
                HostAction::SendReject { reason, .. } => {
                    self.host_send(Payload::SessionReject(v1::SessionReject {
                        reason: reject_to_wire(reason).into(),
                        message: String::new(),
                    }));
                }
                HostAction::SendPermissions(granted) => {
                    self.host_send(Payload::PermissionsUpdate(v1::PermissionsUpdate {
                        granted: perms_to_wire(granted),
                    }));
                    self.apply_host_permissions(&id, granted);
                }
                HostAction::AskGrant(p) => self.emit(Event::PermissionRequested {
                    session: id,
                    permission: p.name().into(),
                }),
                HostAction::PolicyDenied(p) => self.error(
                    Some(&id),
                    "policy",
                    format!("policy does not allow '{}' for this session", p.name()),
                ),
                HostAction::AddToTrustList(peer) => self.add_trust(&id, peer),
                HostAction::TrustDenied => self.error(
                    Some(&id),
                    "policy",
                    "a quick-connect session cannot set up unattended access",
                ),
                HostAction::EndSession(reason) => {
                    if !matches!(
                        reason,
                        scrin_session::EndReason::PeerEnded
                            | scrin_session::EndReason::PeerDisconnected
                            | scrin_session::EndReason::Rejected(_)
                    ) {
                        self.host_send(Payload::SessionEnd(v1::SessionEnd {
                            reason: end_to_wire(reason).into(),
                            message: String::new(),
                        }));
                    }
                    self.close_host(&id, end_reason_name(reason));
                }
                HostAction::ReportAbuse { peer } => {
                    warn!(session = %id, peer = ?peer, "controller reported (no report endpoint yet)");
                }
                HostAction::Notify(log) => info!(
                    session = %id,
                    duration_ms = log.duration_ms(),
                    used = ?log.permissions_used,
                    "session log"
                ),
            }
        }
    }

    fn apply_host_permissions(&mut self, id: &str, granted: Permissions) {
        self.emit(Event::PermissionsChanged {
            session: id.to_owned(),
            granted: permission_names(granted),
        });
        let wants_video = granted.contains(scrin_session::Permission::View);
        let backend = self.backend.clone();
        let tx = self.self_tx.clone();
        let Some(h) = self.host_slot.as_mut() else {
            return;
        };
        if wants_video && h.stream.is_none() {
            let display = backend.displays().first().map_or(1, |d| d.id);
            let sid = h.id.clone();
            match start_host_stream(HostStreamConfig {
                conn: h.conn.clone(),
                backend,
                control: h.out.clone(),
                display,
                mode: h.mode,
                seal: h.seal.clone(),
                on_error: Box::new(move |message| {
                    let _ = tx.send(ActorMsg::MediaError {
                        session: sid.clone(),
                        message,
                    });
                }),
            }) {
                Ok(s) => h.stream = Some(s),
                Err(e) => warn!(error = %e, "could not start the video thread"),
            }
        } else if !wants_video && let Some(s) = h.stream.take() {
            drop_blocking(s);
        }
    }

    fn add_trust(&mut self, id: &str, peer: PeerId) {
        let label = self
            .host_slot
            .as_ref()
            .and_then(|h| h.trust_label.clone())
            .unwrap_or_default();
        self.trust.upsert(TrustedPeer {
            device: DeviceId(peer.0),
            label,
            profile: Profile::Support,
            added_at: unix_s(),
            expires_at: None,
        });
        if let Err(e) = self.save_trust() {
            self.error(Some(id), "internal", format!("could not save trust: {e}"));
        }
    }

    fn close_host(&mut self, id: &str, reason: &str) {
        let Some(h) = self.host_slot.take() else {
            return;
        };
        let _ = h.out.send(Outgoing::Close);
        if let Some(s) = h.stream {
            drop_blocking(s);
        }
        // The writer task closes the connection after flushing; the other
        // tasks end with it. Abort only the readers.
        for t in h.tasks.iter().skip(1) {
            t.abort();
        }
        self.emit(Event::StateChanged {
            session: id.to_owned(),
            role: Role::Host,
            state: SessionState::Ended,
            peer: Some(h.peer.to_hex()),
            reason: Some(reason.to_owned()),
        });
        info!(session = %id, reason, "host session ended");
    }

    fn on_host_control(&mut self, env: Option<v1::Envelope>) {
        let Some(payload) = env.and_then(|e| e.payload) else {
            self.host_event(HostEvent::PeerDisconnected);
            // A pairing that never sent its request has no state-machine
            // session; close it here.
            if let Some(id) = self.host_slot.as_ref().map(|h| h.id.clone()) {
                self.close_host(&id, "disconnected");
            }
            return;
        };
        match payload {
            Payload::SessionRequest(req) => {
                let requested = perms_from_wire(&req.requested);
                if matches!(self.host.state(), HostState::Active { .. }) {
                    for p in requested.iter() {
                        self.host_event(HostEvent::PeerRequestPermission(p));
                    }
                    return;
                }
                let Some(h) = &self.host_slot else { return };
                let event = HostEvent::Request {
                    peer: PeerId(h.peer.0),
                    kind: h.kind.clone(),
                    requested,
                };
                self.host_event(event);
            }
            Payload::BitrateFeedback(fb) => {
                if let Some(s) = self.host_slot.as_ref().and_then(|h| h.stream.as_ref()) {
                    s.control(MediaCtl::Feedback(fb));
                }
            }
            Payload::KeyframeRequest(_) => {
                if let Some(s) = self.host_slot.as_ref().and_then(|h| h.stream.as_ref()) {
                    s.control(MediaCtl::Keyframe);
                }
            }
            Payload::Ping(p) => {
                let t = self.clock.now_us();
                self.host_send(Payload::Pong(v1::Pong {
                    seq: p.seq,
                    t1_us: p.t1_us,
                    t2_us: t,
                    t3_us: t,
                }));
            }
            Payload::SessionEnd(_) => self.host_event(HostEvent::PeerEnded),
            other => debug!(payload = ?std::mem::discriminant(&other), "ignored on host"),
        }
    }

    fn on_host_input(&mut self, session: &str, event: &InputEvent) {
        if self.host_check(session).is_err()
            || !self
                .host
                .granted()
                .contains(scrin_session::Permission::Input)
        {
            return;
        }
        if let Err(e) = self.backend.inject(event) {
            debug!(error = %e, "input injection failed");
        }
    }

    // ---- controller side ----------------------------------------------------

    fn connect(&mut self, target: &str, code: String, requested: Permissions) -> Result<SessionId> {
        let parsed = parse_target(target)?;
        let id = self.next_session('c');
        let mut slot = CtlSlot {
            machine: ControllerSession::default(),
            code: Zeroizing::new(code),
            target: parsed.clone(),
            requested,
            reconnecting: false,
            handshake: None,
            conn: None,
            out: None,
            input: None,
            receiver: None,
            pending_video: None,
            stats: Arc::default(),
            cap: Arc::default(),
            sas: None,
            peer: None,
            trusted: false,
            clock: ClockSync::new(8),
            ping_seq: 0,
            last: StatsSnapshot::default(),
            tasks: Vec::new(),
        };
        let actions = slot
            .machine
            .handle(self.clock.now_ms(), ControllerEvent::Connect { requested });
        let endpoint = self.endpoint.clone();
        let identity = self.identity.clone();
        let resolver = self.resolver.clone();
        let code = slot.code.clone();
        let tx = self.self_tx.clone();
        let sid = id.clone();
        slot.handshake = Some(tokio::spawn(async move {
            let result =
                controller_connect(&endpoint, &identity, &*resolver, parsed, code.as_str()).await;
            let _ = tx.send(ActorMsg::CtlHandshake {
                session: sid,
                result,
            });
        }));
        self.controllers.insert(id.clone(), slot);
        self.ctl_actions(&id, actions);
        Ok(id)
    }

    fn ctl_event(&mut self, id: &str, event: ControllerEvent) {
        let now = self.clock.now_ms();
        let Some(slot) = self.controllers.get_mut(id) else {
            return;
        };
        let actions = slot.machine.handle(now, event);
        self.ctl_actions(id, actions);
    }

    fn on_ctl_handshake(&mut self, id: &str, result: std::result::Result<CtlPaired, CtlFailure>) {
        let Some(slot) = self.controllers.get_mut(id) else {
            return;
        };
        slot.handshake = None;
        if slot.reconnecting {
            self.on_reconnected(id, result);
            return;
        }
        match result {
            Ok(p) => {
                slot.sas = p.sas.map(|s| s.0);
                slot.trusted = p.trusted;
                let sas_text = p.sas.map(|s| s.emoji()).unwrap_or_default();
                self.attach_ctl_conn(id, p.conn, p.control, p.peer);
                self.ctl_event(id, ControllerEvent::Connected);
                self.ctl_event(id, ControllerEvent::Paired { sas: sas_text });
            }
            Err(f) => {
                self.error(Some(id), f.code, f.message);
                if f.pairing {
                    self.ctl_event(id, ControllerEvent::Connected);
                    self.ctl_event(id, ControllerEvent::PairingFailed);
                } else {
                    self.ctl_event(id, ControllerEvent::Disconnected);
                }
            }
        }
    }

    /// Starts the control reader/writer of a (re)established connection.
    fn attach_ctl_conn(
        &mut self,
        id: &str,
        conn: Connection,
        control: ControlStream,
        peer: DeviceId,
    ) {
        let tx = self.self_tx.clone();
        let Some(slot) = self.controllers.get_mut(id) else {
            conn.close(0u32.into(), b"gone");
            return;
        };
        let (out, out_rx) = mpsc::unbounded_channel();
        let ControlStream { send, recv, .. } = control;
        slot.tasks
            .push(tokio::spawn(writer_task(conn.clone(), send, out_rx, None)));
        let sid = id.to_owned();
        slot.tasks
            .push(tokio::spawn(reader_task(recv, None, move |env| {
                let _ = tx.send(ActorMsg::Control {
                    session: sid.clone(),
                    env,
                });
            })));
        slot.out = Some(out);
        slot.conn = Some(conn);
        slot.peer = Some(peer);
    }

    /// A trusted, active session lost its connection: drop the dead pieces
    /// and re-dial with backoff, re-authenticating by signature (no code).
    fn start_reconnect(&mut self, id: &str) {
        let endpoint = self.endpoint.clone();
        let identity = self.identity.clone();
        let resolver = self.resolver.clone();
        let tx = self.self_tx.clone();
        let Some(slot) = self.controllers.get_mut(id) else {
            return;
        };
        for t in slot.tasks.drain(..) {
            t.abort();
        }
        slot.receiver = None;
        slot.input = None;
        slot.out = None;
        if let Some(c) = slot.conn.take() {
            c.close(0u32.into(), b"reconnecting");
        }
        slot.reconnecting = true;
        let target = slot.target.clone();
        let sid = id.to_owned();
        slot.handshake = Some(tokio::spawn(async move {
            let mut backoff = Backoff::new(Duration::from_millis(500), Duration::from_secs(8), 0.2);
            let mut last = fail(
                "connection-lost",
                "the connection to the host was lost",
                false,
            );
            for attempt in 1..=RECONNECT_ATTEMPTS {
                tokio::time::sleep(backoff.next_delay_random()).await;
                match controller_connect(&endpoint, &identity, &*resolver, target.clone(), "").await
                {
                    Ok(p) => {
                        let _ = tx.send(ActorMsg::CtlHandshake {
                            session: sid,
                            result: Ok(p),
                        });
                        return;
                    }
                    Err(f) => {
                        debug!(attempt, code = f.code, "reconnect attempt failed");
                        let final_answer = f.code == "untrusted";
                        last = f;
                        if final_answer {
                            break;
                        }
                    }
                }
            }
            let _ = tx.send(ActorMsg::CtlHandshake {
                session: sid,
                result: Err(last),
            });
        }));
        let peer = slot.peer.map(|p| p.to_hex());
        info!(session = %id, "connection lost; reconnecting");
        self.emit(Event::StateChanged {
            session: id.to_owned(),
            role: Role::Controller,
            state: SessionState::Connecting,
            peer,
            reason: Some("reconnecting".into()),
        });
    }

    fn on_reconnected(&mut self, id: &str, result: std::result::Result<CtlPaired, CtlFailure>) {
        let expected = self.controllers.get(id).and_then(|s| s.peer);
        match result {
            Ok(p) if Some(p.peer) == expected && p.trusted => {
                self.attach_ctl_conn(id, p.conn, p.control, p.peer);
                let requested = self
                    .controllers
                    .get(id)
                    .map_or(Permissions::empty(), |s| match s.machine.granted() {
                        g if g == Permissions::empty() => s.requested,
                        g => g,
                    });
                // Stay `reconnecting` until the host answers.
                self.ctl_send(
                    id,
                    Payload::SessionRequest(v1::SessionRequest {
                        requested: perms_to_wire(requested),
                        controller_name: self.device_name.clone(),
                        unattended: true,
                    }),
                );
            }
            Ok(p) => {
                p.conn.close(0u32.into(), b"wrong host");
                self.reconnect_failed(id, "reconnected to a different device");
            }
            Err(f) => self.reconnect_failed(id, &f.message),
        }
    }

    fn reconnect_failed(&mut self, id: &str, why: &str) {
        if let Some(slot) = self.controllers.get_mut(id) {
            slot.reconnecting = false;
        }
        self.error(
            Some(id),
            "connection-lost",
            format!("the connection to the host was lost ({why})"),
        );
        self.ctl_event(id, ControllerEvent::Disconnected);
    }

    /// The host re-accepted a reconnected trusted session.
    fn on_reaccepted(&mut self, id: &str, granted: Permissions) {
        if let Some(slot) = self.controllers.get_mut(id) {
            slot.reconnecting = false;
        }
        self.start_controller_media(id);
        let peer = self
            .controllers
            .get(id)
            .and_then(|s| s.peer)
            .map(|p| p.to_hex());
        self.emit(Event::StateChanged {
            session: id.to_owned(),
            role: Role::Controller,
            state: SessionState::Active,
            peer,
            reason: Some("reconnected".into()),
        });
        self.ctl_event(id, ControllerEvent::PermissionsChanged(granted));
        info!(session = %id, "reconnected");
    }

    fn ctl_send(&self, id: &str, p: Payload) {
        if let Some(out) = self.controllers.get(id).and_then(|s| s.out.as_ref()) {
            let _ = out.send(Outgoing::Env(env(p)));
        }
    }

    fn ctl_actions(&mut self, id: &str, actions: Vec<ControllerAction>) {
        for action in actions {
            match action {
                ControllerAction::Dial
                | ControllerAction::StartPairing
                | ControllerAction::PermissionPending(_) => {}
                ControllerAction::ShowSas(_) => {
                    if let Some(emoji) = self.controllers.get(id).and_then(|s| s.sas) {
                        self.emit(Event::Sas {
                            session: id.to_owned(),
                            emoji,
                        });
                    }
                }
                ControllerAction::SendRequest(requested) => {
                    let unattended = self.controllers.get(id).is_some_and(|s| s.trusted);
                    self.ctl_send(
                        id,
                        Payload::SessionRequest(v1::SessionRequest {
                            requested: perms_to_wire(requested),
                            controller_name: self.device_name.clone(),
                            unattended,
                        }),
                    );
                }
                ControllerAction::SendPermissionRequest(p) => self.ctl_send(
                    id,
                    Payload::SessionRequest(v1::SessionRequest {
                        requested: perms_to_wire(Permissions::only(p)),
                        controller_name: self.device_name.clone(),
                        unattended: false,
                    }),
                ),
                ControllerAction::ShowStatus(status) => {
                    let state = match status {
                        ControllerStatus::Connecting => SessionState::Connecting,
                        ControllerStatus::Pairing => SessionState::Pairing,
                        ControllerStatus::AwaitingAccept => SessionState::AwaitingAccept,
                        ControllerStatus::Active => SessionState::Active,
                    };
                    if state == SessionState::Active {
                        self.start_controller_media(id);
                    }
                    let peer = self
                        .controllers
                        .get(id)
                        .and_then(|s| s.peer)
                        .map(|p| p.to_hex());
                    self.emit(Event::StateChanged {
                        session: id.to_owned(),
                        role: Role::Controller,
                        state,
                        peer,
                        reason: None,
                    });
                }
                ControllerAction::ShowPermissions(p) => self.emit(Event::PermissionsChanged {
                    session: id.to_owned(),
                    granted: permission_names(p),
                }),
                ControllerAction::Disconnect => {
                    let active_or_waiting = self.controllers.get(id).is_some_and(|s| {
                        s.out.is_some()
                            && !matches!(
                                s.machine.state(),
                                scrin_session::ControllerState::Ended(
                                    ControllerEnd::Ended(_) | ControllerEnd::Rejected(_)
                                )
                            )
                    });
                    if active_or_waiting {
                        self.ctl_send(
                            id,
                            Payload::SessionEnd(v1::SessionEnd {
                                reason: v1::SessionEndReason::ClosedByController.into(),
                                message: String::new(),
                            }),
                        );
                    }
                    if let Some(s) = self.controllers.get(id) {
                        if let Some(out) = &s.out {
                            let _ = out.send(Outgoing::Close);
                        }
                        if let Some(h) = &s.handshake {
                            h.abort();
                        }
                    }
                }
                ControllerAction::ShowEnded(end) => self.close_controller(id, end),
            }
        }
    }

    fn start_controller_media(&mut self, id: &str) {
        let backend = self.backend.clone();
        let events = self.events.clone();
        let Some(slot) = self.controllers.get_mut(id) else {
            return;
        };
        let (Some(conn), Some(out)) = (slot.conn.clone(), slot.out.clone()) else {
            return;
        };
        if slot.receiver.is_none() {
            let sid = id.to_owned();
            match start_receiver(ReceiverConfig {
                conn: conn.clone(),
                backend,
                control: out,
                stats: slot.stats.clone(),
                cap_bps: slot.cap.clone(),
                on_frame: Box::new(move |frame| {
                    let _ = events.send(Event::VideoFrame {
                        session: sid.clone(),
                        frame: Arc::new(frame),
                    });
                }),
            }) {
                Ok(r) => {
                    if let Some(cfg) = slot.pending_video.take() {
                        r.configure(cfg);
                    }
                    slot.receiver = Some(r);
                }
                Err(e) => warn!(error = %e, "could not start the decoder thread"),
            }
        }
        if slot.input.is_none() {
            let (tx, rx) = mpsc::unbounded_channel();
            slot.tasks.push(tokio::spawn(input_writer(conn, rx)));
            slot.input = Some(tx);
        }
        slot.last = StatsSnapshot {
            at: Some(Instant::now()),
            ..StatsSnapshot::default()
        };
    }

    fn close_controller(&mut self, id: &str, end: ControllerEnd) {
        let Some(slot) = self.controllers.remove(id) else {
            return;
        };
        if let Some(h) = slot.handshake {
            h.abort();
        }
        if let Some(out) = &slot.out {
            let _ = out.send(Outgoing::Close);
        } else if let Some(c) = &slot.conn {
            c.close(0u32.into(), b"bye");
        }
        // Writer (index 0) flushes and closes; stop everything else now.
        for t in slot.tasks.iter().skip(1) {
            t.abort();
        }
        drop(slot.receiver);
        let reason = match end {
            ControllerEnd::Cancelled => "cancelled",
            ControllerEnd::Rejected(r) => reject_reason_name(r),
            ControllerEnd::Ended(r) => end_reason_name(r),
            ControllerEnd::ConnectFailed => "connect-failed",
            ControllerEnd::PairingFailed => "pairing-failed",
            ControllerEnd::Timeout => "timeout",
        };
        if let ControllerEnd::Rejected(r) = end {
            let code = match r {
                scrin_session::RejectReason::Busy => "busy",
                scrin_session::RejectReason::Timeout => "timeout",
                _ => "rejected",
            };
            self.error(Some(id), code, "the host declined the session");
        }
        self.emit(Event::StateChanged {
            session: id.to_owned(),
            role: Role::Controller,
            state: SessionState::Ended,
            peer: slot.peer.map(|p| p.to_hex()),
            reason: Some(reason.into()),
        });
        info!(session = %id, reason, "controller session ended");
    }

    fn on_ctl_control(&mut self, id: &str, env: Option<v1::Envelope>) {
        let (active, trusted, reconnecting) =
            self.controllers.get(id).map_or((false, false, false), |s| {
                (
                    matches!(s.machine.state(), ControllerState::Active { .. }),
                    s.trusted,
                    s.reconnecting,
                )
            });
        let Some(payload) = env.and_then(|e| e.payload) else {
            if reconnecting {
                return;
            }
            if active && trusted {
                self.start_reconnect(id);
                return;
            }
            if active {
                // A code session cannot resume without a new code (the old
                // one is spent); the UI offers to reconnect.
                self.error(
                    Some(id),
                    "connection-lost",
                    "the connection to the host was lost; ask for a new code to reconnect",
                );
            }
            self.ctl_event(id, ControllerEvent::Disconnected);
            return;
        };
        match payload {
            Payload::SessionAccept(a) if reconnecting => {
                self.on_reaccepted(id, perms_from_wire(&a.granted));
            }
            Payload::SessionReject(_) if reconnecting => {
                self.reconnect_failed(id, "the host declined");
            }
            Payload::SessionAccept(a) => {
                self.ctl_event(id, ControllerEvent::Accepted(perms_from_wire(&a.granted)));
            }
            Payload::SessionReject(r) => {
                self.ctl_event(id, ControllerEvent::Rejected(reject_from_wire(r.reason)));
            }
            Payload::PermissionsUpdate(u) => self.ctl_event(
                id,
                ControllerEvent::PermissionsChanged(perms_from_wire(&u.granted)),
            ),
            Payload::SessionEnd(e) => {
                self.ctl_event(id, ControllerEvent::Ended(end_from_wire(e.reason)));
            }
            Payload::VideoConfig(vc) => {
                let cfg = DecoderConfig {
                    codec: v1::Codec::try_from(vc.codec).unwrap_or(v1::Codec::Unspecified),
                    width: vc.width,
                    height: vc.height,
                    codec_config: vc.codec_config,
                };
                if let Some(slot) = self.controllers.get_mut(id) {
                    match &slot.receiver {
                        Some(r) => r.configure(cfg),
                        None => slot.pending_video = Some(cfg),
                    }
                }
            }
            Payload::Pong(p) => {
                let now = i64::try_from(self.clock.now_us()).unwrap_or(i64::MAX);
                if let Some(slot) = self.controllers.get_mut(id) {
                    let as_i64 = |v: u64| i64::try_from(v).unwrap_or(i64::MAX);
                    slot.clock.add(Exchange {
                        t0: as_i64(p.t1_us),
                        t1: as_i64(p.t2_us),
                        t2: as_i64(p.t3_us),
                        t3: now,
                    });
                }
            }
            other => debug!(payload = ?std::mem::discriminant(&other), "ignored on controller"),
        }
    }

    // ---- periodic -----------------------------------------------------------

    fn on_tick(&mut self) {
        let now = self.clock.now_ms();
        let actions = self.host.on_tick(now);
        self.host_actions(actions);
        let ids: Vec<SessionId> = self.controllers.keys().cloned().collect();
        for id in &ids {
            if let Some(slot) = self.controllers.get_mut(id) {
                let a = slot.machine.on_tick(now);
                self.ctl_actions(id, a);
            }
        }
        let code_due = match &self.code.code {
            Some(c) => c.is_expired(),
            None => self.lockout_until.is_none_or(|u| now >= u),
        };
        if code_due && let Err(e) = self.rotate_code() {
            warn!(error = %e, "could not rotate the one-time code");
        }
        // New locator (new words) shortly before the server forgets the old one.
        if let Some(lang) = self
            .phrase
            .as_ref()
            .filter(|p| p.current.is_some() && now.saturating_add(PHRASE_RENEW_MS) >= p.expires_at)
            .map(|p| p.lang)
            && let Err(e) = self.request_locator(lang)
        {
            warn!(error = %e, "could not renew the passphrase");
        }
        if self.last_second.elapsed() >= Duration::from_secs(1) {
            self.last_second = Instant::now();
            for id in &ids {
                self.second_tick(id);
            }
        }
    }

    fn second_tick(&mut self, id: &str) {
        let now_us = self.clock.now_us();
        let Some(slot) = self.controllers.get_mut(id) else {
            return;
        };
        if slot.receiver.is_none() {
            return;
        }
        slot.ping_seq += 1;
        if let Some(out) = &slot.out {
            let _ = out.send(Outgoing::Env(env(Payload::Ping(v1::Ping {
                seq: slot.ping_seq,
                t1_us: now_us,
            }))));
        }
        let s = &slot.stats;
        let cur = StatsSnapshot {
            at: Some(Instant::now()),
            decoded: s.decoded.load(Ordering::Relaxed),
            bytes: s.bytes.load(Ordering::Relaxed),
            completed: s.completed.load(Ordering::Relaxed),
            lost: s.lost.load(Ordering::Relaxed),
            decode_us: s.decode_us.load(Ordering::Relaxed),
        };
        let prev = std::mem::replace(&mut slot.last, cur);
        let dt = prev
            .at
            .zip(cur.at)
            .map_or(1.0, |(a, b)| b.duration_since(a).as_secs_f64().max(0.001));
        #[expect(clippy::cast_precision_loss)] // per-second counters
        let stats = {
            let decoded = cur.decoded.saturating_sub(prev.decoded);
            let completed = cur.completed.saturating_sub(prev.completed);
            let lost = cur.lost.saturating_sub(prev.lost);
            let bytes = cur.bytes.saturating_sub(prev.bytes);
            let decode_us = cur.decode_us.saturating_sub(prev.decode_us);
            StatsInfo {
                rtt_ms: slot
                    .clock
                    .estimate()
                    .map_or(0.0, |c| c.rtt_us as f64 / 1000.0),
                fps: decoded as f64 / dt,
                #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                bitrate_bps: (bytes as f64 * 8.0 / dt) as u64,
                loss: if completed + lost == 0 {
                    0.0
                } else {
                    lost as f64 / (completed + lost) as f64
                },
                decode_ms: if decoded == 0 {
                    0.0
                } else {
                    decode_us as f64 / decoded as f64 / 1000.0
                },
                width: s.width.load(Ordering::Relaxed),
                height: s.height.load(Ordering::Relaxed),
                frames_total: cur.decoded,
            }
        };
        self.emit(Event::Stats {
            session: id.to_owned(),
            stats,
        });
    }

    async fn shutdown(&mut self) {
        if let Some(id) = self.host_slot.as_ref().map(|h| h.id.clone()) {
            self.host_event(HostEvent::UserStop);
            self.close_host(&id, "shutdown");
        }
        let ids: Vec<SessionId> = self.controllers.keys().cloned().collect();
        for id in ids {
            self.ctl_event(&id, ControllerEvent::Cancel);
            self.close_controller(&id, ControllerEnd::Cancelled);
        }
        // Give writers a moment to flush SessionEnd before the endpoint goes.
        tokio::time::sleep(Duration::from_millis(50)).await;
        if let Some(t) = self.presence_task.take() {
            t.abort();
        }
        self.endpoint.close().await;
        info!("engine stopped");
    }
}

/// Media threads join on drop; never block the actor on that.
fn drop_blocking(s: HostStream) {
    tokio::task::spawn_blocking(move || drop(s));
}

const fn kind_name(k: &SessionKind) -> &'static str {
    match k {
        SessionKind::Anonymous => "anonymous",
        SessionKind::Verified { .. } => "verified",
        SessionKind::Trusted => "trusted",
        SessionKind::OrgPolicy => "org",
    }
}

/// A stable 9-digit number derived from the device key, used until the
/// rendezvous server assigns the registered scrin ID.
#[must_use]
pub fn provisional_scrin_id(id: &DeviceId) -> String {
    let h = blake3::derive_key("scrin provisional id v1", &id.0);
    let mut n = [0u8; 8];
    n.copy_from_slice(&h[..8]);
    (100_000_000 + u64::from_le_bytes(n) % 900_000_000).to_string()
}

async fn accept_input_streams(
    conn: Connection,
    session: SessionId,
    seal: Seal,
    tx: mpsc::UnboundedSender<ActorMsg>,
) {
    // Gateway path: 3-byte stream headers and per-stream lanes; the Control
    // lane is taken by the handshake stream.
    let mut used = std::collections::HashSet::from([gw::CONTROL_LANE]);
    loop {
        let next = if seal.is_some() {
            gw::accept_stream(&conn, &mut used)
                .await
                .map(|(k, lane, _s, r)| (k, lane, r))
        } else {
            accept_stream(&conn).await.map(|(k, _s, r)| (k, 0, r))
        };
        let Ok((kind, lane, recv)) = next else { break };
        if kind != StreamKind::Input {
            continue;
        }
        let tx = tx.clone();
        let sid = session.clone();
        tokio::spawn(read_input_stream(recv, seal.clone(), lane, move |event| {
            let _ = tx.send(ActorMsg::Input {
                session: sid.clone(),
                event,
            });
        }));
    }
}

async fn input_writer(conn: Connection, mut rx: mpsc::UnboundedReceiver<v1::Envelope>) {
    let Ok((mut send, _recv)) = open_stream(&conn, StreamKind::Input).await else {
        return;
    };
    while let Some(e) = rx.recv().await {
        if write_frame(&mut send, &scrin_proto::encode_envelope(&e))
            .await
            .is_err()
        {
            break;
        }
    }
    let _ = send.finish();
}

fn fail(code: &'static str, message: impl Into<String>, pairing: bool) -> CtlFailure {
    CtlFailure {
        code,
        message: message.into(),
        pairing,
    }
}

fn net_failure(e: &NetError, conn: &Connection, trusted: bool) -> CtlFailure {
    let busy = conn
        .close_reason()
        .is_some_and(|r| r.to_string().contains("busy"));
    if busy {
        return fail("busy", "the host is in another session", false);
    }
    let code = match e {
        NetError::PairingFailed => "wrong-code",
        NetError::CodeConsumed | NetError::CodeExpired => "code-unavailable",
        NetError::Untrusted | NetError::Rejected(NetReject::Untrusted | NetReject::WrongMode)
            if trusted =>
        {
            "untrusted"
        }
        NetError::Timeout => "timeout",
        NetError::Rejected(NetReject::VersionMismatch) | NetError::VersionMismatch => "version",
        _ => "offline",
    };
    fail(code, e.to_string(), true)
}

async fn dial(
    endpoint: &NetEndpoint,
    addr: EndpointAddr,
) -> std::result::Result<Connection, CtlFailure> {
    match tokio::time::timeout(DIAL_TIMEOUT, endpoint.connect(addr)).await {
        Ok(Ok(c)) => Ok(c),
        Ok(Err(e)) => Err(fail("offline", e.to_string(), false)),
        Err(_) => Err(fail("timeout", "the host did not answer", false)),
    }
}

async fn controller_connect(
    endpoint: &NetEndpoint,
    identity: &Identity,
    resolver: &dyn Resolver,
    target: ConnectTarget,
    code: &str,
) -> std::result::Result<CtlPaired, CtlFailure> {
    let addr = match target {
        ConnectTarget::Addr(a) => a,
        ConnectTarget::ScrinId(id) => resolver
            .resolve(&id)
            .await
            .map_err(|e| fail("offline", e.to_string(), false))?,
        ConnectTarget::Phrase { locator, password } => {
            let addr = resolver
                .resolve_locator(locator)
                .await
                .map_err(|e| fail("offline", e.to_string(), false))?;
            let conn = dial(endpoint, addr).await?;
            return match controller_pair_phrase(&conn, endpoint.device_id(), &password).await {
                Ok(o) => Ok(CtlPaired {
                    peer: o.peer,
                    sas: Some(o.sas),
                    control: o.control,
                    conn,
                    trusted: false,
                }),
                Err(e) => Err(net_failure(&e, &conn, false)),
            };
        }
    };
    let me = endpoint.device_id();
    if !code.trim().is_empty() {
        let conn = dial(endpoint, addr.clone()).await?;
        match controller_pair(&conn, me, code).await {
            Ok(o) => {
                return Ok(CtlPaired {
                    peer: o.peer,
                    sas: Some(o.sas),
                    control: o.control,
                    conn,
                    trusted: false,
                });
            }
            // The host already trusts us and asked for a signature instead:
            // retry as unattended on a fresh connection (no code was used).
            Err(NetError::Rejected(NetReject::WrongMode)) => {
                conn.close(0u32.into(), b"retry trusted");
            }
            Err(NetError::Crypto(_)) => {
                return Err(fail("wrong-code", "that is not a valid code", true));
            }
            Err(e) => return Err(net_failure(&e, &conn, false)),
        }
    }
    let conn = dial(endpoint, addr).await?;
    match controller_auth_trusted(&conn, identity).await {
        Ok(control) => Ok(CtlPaired {
            peer: remote_device_id(&conn),
            sas: None,
            control,
            conn,
            trusted: true,
        }),
        Err(e) => Err(net_failure(&e, &conn, true)),
    }
}
