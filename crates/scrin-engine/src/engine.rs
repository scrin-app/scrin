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
use scrin_crypto::sas::Sas;
use scrin_crypto::trust::{Profile, TrustStore, TrustedPeer};
use scrin_media::adapt::Mode;
use scrin_media::clock::{ClockSync, Exchange};
use scrin_net::framing::{StreamKind, accept_stream, open_stream, write_frame};
use scrin_net::handshake::{
    ControlStream, HostCode, RejectReason as NetReject, controller_auth_trusted, controller_pair,
    host_auth_trusted, host_pair,
};
use scrin_net::{Connection, NetConfig, NetEndpoint, NetError, remote_device_id};
use scrin_proto::v1::{self, envelope::Payload};
use scrin_session::{
    ControllerAction, ControllerEnd, ControllerEvent, ControllerSession, ControllerStatus,
    HostAction, HostEvent, HostSession, HostState, PeerId, Permissions, Policy, SessionKind,
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
use crate::media::{
    HostStream, HostStreamConfig, MediaCtl, Receiver, ReceiverConfig, ReceiverStats,
    start_host_stream, start_receiver,
};
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
const TICK: Duration = Duration::from_millis(100);
const DIAL_TIMEOUT: Duration = Duration::from_secs(20);
/// Failed code guesses within [`FAILURE_WINDOW_MS`] that trigger a lockout.
const MAX_FAILURES: usize = 5;
const FAILURE_WINDOW_MS: u64 = 10 * 60 * 1000;
const LOCKOUT_MS: u64 = 60 * 1000;
/// QUIC application close code for "host busy".
const BUSY_CODE: u32 = 0x5c10;

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
}

impl std::fmt::Debug for EngineConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EngineConfig")
            .field("data_dir", &self.data_dir)
            .field("backend", &self.backend.name())
            .field("device_name", &self.device_name)
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
pub async fn start(cfg: EngineConfig) -> Result<(EngineHandle, mpsc::UnboundedReceiver<Event>)> {
    let identity = Arc::new(load_identity(&*cfg.secrets)?);
    let endpoint = NetEndpoint::bind(*identity.seed(), cfg.net.clone()).await?;
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
    info!(
        device = %identity.device_id().fingerprint(),
        backend = cfg.backend.name(),
        "engine started"
    );
    let actor = Actor {
        identity,
        endpoint,
        backend: cfg.backend,
        resolver: cfg.resolver,
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
    };
    tokio::spawn(actor.run(rx));
    Ok((EngineHandle { tx }, events_rx))
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
    issued_at: u64,
    expires_at: u64,
}

impl CodeState {
    fn new(ttl: Duration, clock: &Clock) -> Result<Self> {
        let code = OneTimeCode::generate_with_ttl(ttl)?;
        let now = clock.now_ms();
        Ok(Self {
            slot: Arc::new(HostCode::new(code.clone())),
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
            code: None,
            issued_at: clock.now_ms(),
            expires_at: until,
        })
    }
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
}

struct HostPaired {
    control: ControlStream,
    kind: SessionKind,
    sas: Option<Sas>,
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
    tasks: Vec<JoinHandle<()>>,
}

struct CtlSlot {
    machine: ControllerSession,
    code: Zeroizing<String>,
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
        }
    }

    // ---- status, code, trust ------------------------------------------------

    fn dial_addr(&self) -> EndpointAddr {
        let mut addr = self.endpoint.addr();
        for s in self.endpoint.bound_sockets() {
            if !s.ip().is_unspecified() {
                addr.addrs.insert(TransportAddr::Ip(s));
            }
        }
        addr
    }

    fn status(&self) -> Status {
        let id = self.identity.device_id();
        let addr = self.dial_addr();
        Status {
            device_id: id.to_hex(),
            fingerprint: id.fingerprint(),
            scrin_id: provisional_scrin_id(&id),
            online: !addr.addrs.is_empty(),
            ticket: encode_ticket(&addr),
            code: self
                .code
                .code
                .as_ref()
                .map(OneTimeCode::display)
                .unwrap_or_default(),
            code_issued_at: self.code.issued_at,
            code_expires_at: self.code.expires_at,
            backend: self.backend.name().into(),
        }
    }

    fn rotate_code(&mut self) -> Result<()> {
        let now = self.clock.now_ms();
        self.code = match self.lockout_until {
            Some(until) if now < until => CodeState::locked(&self.clock, until)?,
            _ => {
                self.lockout_until = None;
                CodeState::new(self.code_ttl, &self.clock)?
            }
        };
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
        let peer = remote_device_id(&conn);
        if self.host_busy() {
            debug!(peer = ?peer, "busy; refusing connection");
            conn.close(BUSY_CODE.into(), b"busy");
            return;
        }
        self.host_handshakes += 1;
        let trusted = self.trust.lookup(&peer, unix_s()).is_some();
        let me = self.endpoint.device_id();
        let slot = self.code.slot.clone();
        let trust = self.trust.clone();
        let tx = self.self_tx.clone();
        tokio::spawn(async move {
            let result = if trusted {
                host_auth_trusted(&conn, me, &trust, unix_s())
                    .await
                    .map(|o| HostPaired {
                        control: o.control,
                        kind: SessionKind::Trusted,
                        sas: None,
                    })
            } else {
                host_pair(&conn, me, &slot).await.map(|o| HostPaired {
                    control: o.control,
                    kind: SessionKind::Anonymous,
                    sas: Some(o.sas),
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
        if self.code.slot.is_consumed()
            && let Err(e) = self.rotate_code()
        {
            warn!(error = %e, "could not rotate the one-time code");
        }
        let paired = match result {
            Ok(p) => p,
            Err(e) => {
                if matches!(e, NetError::PairingFailed) {
                    self.record_failure();
                    if self.lockout_until.is_some() {
                        let _ = self.rotate_code();
                    }
                }
                info!(peer = ?peer, error = %e, "incoming handshake failed");
                conn.close(0u32.into(), b"handshake failed");
                return;
            }
        };
        let id = self.next_session('h');
        let (out, out_rx) = mpsc::unbounded_channel();
        let ControlStream { send, recv, .. } = paired.control;
        let mut tasks = vec![tokio::spawn(writer_task(conn.clone(), send, out_rx))];
        let tx = self.self_tx.clone();
        let sid = id.clone();
        tasks.push(tokio::spawn(reader_task(recv, move |env| {
            let _ = tx.send(ActorMsg::Control {
                session: sid.clone(),
                env,
            });
        })));
        tasks.push(tokio::spawn(accept_input_streams(
            conn.clone(),
            id.clone(),
            self.self_tx.clone(),
        )));
        info!(session = %id, peer = ?peer, trusted = paired.kind == SessionKind::Trusted, "controller paired");
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
            tasks,
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
        match result {
            Ok(p) => {
                let (out, out_rx) = mpsc::unbounded_channel();
                let ControlStream { send, recv, .. } = p.control;
                slot.tasks
                    .push(tokio::spawn(writer_task(p.conn.clone(), send, out_rx)));
                let tx = self.self_tx.clone();
                let sid = id.to_owned();
                slot.tasks.push(tokio::spawn(reader_task(recv, move |env| {
                    let _ = tx.send(ActorMsg::Control {
                        session: sid.clone(),
                        env,
                    });
                })));
                slot.out = Some(out);
                slot.conn = Some(p.conn);
                slot.sas = p.sas.map(|s| s.0);
                slot.peer = Some(p.peer);
                slot.trusted = p.trusted;
                let sas_text = p.sas.map(|s| s.emoji()).unwrap_or_default();
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
        let Some(payload) = env.and_then(|e| e.payload) else {
            self.ctl_event(id, ControllerEvent::Disconnected);
            return;
        };
        match payload {
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
    tx: mpsc::UnboundedSender<ActorMsg>,
) {
    while let Ok((kind, _send, recv)) = accept_stream(&conn).await {
        if kind != StreamKind::Input {
            continue;
        }
        let tx = tx.clone();
        let sid = session.clone();
        tokio::spawn(read_input_stream(recv, move |event| {
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
