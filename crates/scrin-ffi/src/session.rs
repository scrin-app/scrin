//! Session tasks: one tokio task per session drives the pure state machine from
//! `scrin-session` with events from the peer, FFI commands and a tick.
//!
//! The wire matches the desktop engine (`crates/scrin-engine`) so Android and
//! Windows interoperate in both roles:
//!
//! - **Control stream** (opened by the handshake): length-prefixed
//!   `scrin.v1.Envelope`s — `SessionRequest` (also used mid-session to ask
//!   for one more permission), `SessionAccept`/`Reject`, `PermissionsUpdate`,
//!   `SessionEnd`, `VideoConfig`, `KeyframeRequest`, `BitrateFeedback`,
//!   `Ping`/`Pong`.
//! - **Input stream** (`StreamKind::Input`, opened by the controller):
//!   input envelopes only, so a burst of moves never delays control traffic.
//! - **Datagrams**: one `scrin_media::fec` shard each (16-byte `ShardHeader`
//!   + payload); the controller reassembles complete access units.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bytes::Bytes;
use iroh::EndpointAddr;
use scrin_crypto::identity::DeviceId;
use scrin_crypto::sas::{EMOJI, Sas};
use scrin_crypto::trust::Profile;
use scrin_media::fec::{FrameEncoder, FrameReassembler, MediaKind, ShardHeader};
use scrin_net::datagram::{recv_datagram, send_datagram};
use scrin_net::handshake::{
    ControlStream, HostCode, controller_auth_trusted, controller_pair, host_pair,
};
use scrin_net::{Connection, NetError, SendStream};
use scrin_proto::v1::{self, envelope::Payload};
use scrin_session::{
    ControllerAction, ControllerEnd, ControllerEvent, ControllerSession, ControllerStatus,
    EndReason, HostAction, HostEvent, HostSession, HostState, PeerId, Permission, Permissions,
    Policy, SessionKind,
};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;

use crate::media::{ArrivalLog, FEEDBACK_INTERVAL, Meter, has_sps, micros_since};
use crate::types::{
    EndInfo, EndKind, IncomingRequest, Notice, RemoteInput, SasInfo, ScrinError, SessionListener,
    SessionPermission, SessionState, SessionStats, VideoConfigInfo, perms_to_ffi,
};
use crate::{Shared, Target, lock, wire};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const TICK: Duration = Duration::from_millis(250);
const STATS_EVERY_TICKS: u32 = 4;
const FEC_PARITY: f32 = 0.1;
const KEYFRAME_REQUEST_GAP: Duration = Duration::from_millis(500);
/// How long a goodbye may take to reach the peer before the connection closes.
const GOODBYE_FLUSH: Duration = Duration::from_millis(500);

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Host,
    Controller,
}

#[derive(Debug)]
pub(crate) enum Cmd {
    Accept(Permissions),
    Reject,
    Grant(Permission),
    Revoke(Permission),
    AddTrust,
    RequestPermission(Permission),
    Input(RemoteInput),
    VideoConfig(VideoConfigInfo),
    RequestKeyframe,
    Feedback(v1::BitrateFeedback),
    End { report: bool },
}

struct MediaTx {
    conn: Connection,
    encoder: FrameEncoder,
    next_frame: u32,
}

/// One host or controller session, shared by its task and the FFI object.
pub(crate) struct Session {
    id: u64,
    role: Role,
    listener: Arc<dyn SessionListener>,
    cmd: UnboundedSender<Cmd>,
    cmd_rx: Mutex<Option<UnboundedReceiver<Cmd>>>,
    granted: AtomicU32,
    media: Mutex<Option<MediaTx>>,
    /// Host: SPS/PPS from the encoder, prepended to keyframes that lack them
    /// (Android encoders emit them once, as a `CODEC_CONFIG` buffer).
    codec_config: Mutex<Vec<u8>>,
    stats: Mutex<SessionStats>,
    meter: Mutex<Meter>,
    tasks: Mutex<Vec<JoinHandle<()>>>,
    ended: AtomicBool,
}

impl Session {
    fn new(role: Role, listener: Arc<dyn SessionListener>) -> Arc<Self> {
        let (cmd, rx) = mpsc::unbounded_channel();
        Arc::new(Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            role,
            listener,
            cmd,
            cmd_rx: Mutex::new(Some(rx)),
            granted: AtomicU32::new(0),
            media: Mutex::new(None),
            codec_config: Mutex::new(Vec::new()),
            stats: Mutex::new(SessionStats::default()),
            meter: Mutex::new(Meter::default()),
            tasks: Mutex::new(Vec::new()),
            ended: AtomicBool::new(false),
        })
    }

    pub(crate) fn controller(listener: Arc<dyn SessionListener>) -> Arc<Self> {
        Self::new(Role::Controller, listener)
    }

    pub(crate) const fn id(&self) -> u64 {
        self.id
    }

    pub(crate) fn track(&self, task: JoinHandle<()>) {
        lock(&self.tasks).push(task);
    }

    fn take_cmds(&self) -> Option<UnboundedReceiver<Cmd>> {
        lock(&self.cmd_rx).take()
    }

    fn granted(&self) -> Permissions {
        Permissions::from_bits_truncate(self.granted.load(Ordering::Acquire))
    }

    fn set_granted(&self, p: Permissions) {
        self.granted.store(p.bits(), Ordering::Release);
        self.listener.on_permissions(perms_to_ffi(p));
    }

    fn send_cmd(&self, role: Role, c: Cmd) -> Result<(), ScrinError> {
        if self.role != role {
            return Err(ScrinError::state(match role {
                Role::Host => "only the host can do this",
                Role::Controller => "only the controller can do this",
            }));
        }
        self.cmd
            .send(c)
            .map_err(|_| ScrinError::state("session is over"))
    }

    pub(crate) fn host_accept(&self, p: Permissions) -> Result<(), ScrinError> {
        self.send_cmd(Role::Host, Cmd::Accept(p))
    }

    pub(crate) fn host_reject(&self) -> Result<(), ScrinError> {
        self.send_cmd(Role::Host, Cmd::Reject)
    }

    pub(crate) fn host_grant(&self, p: Permission) -> Result<(), ScrinError> {
        self.send_cmd(Role::Host, Cmd::Grant(p))
    }

    pub(crate) fn host_revoke(&self, p: Permission) -> Result<(), ScrinError> {
        self.send_cmd(Role::Host, Cmd::Revoke(p))
    }

    pub(crate) fn host_add_trust(&self) -> Result<(), ScrinError> {
        self.send_cmd(Role::Host, Cmd::AddTrust)
    }

    pub(crate) fn controller_request(&self, p: Permission) -> Result<(), ScrinError> {
        self.send_cmd(Role::Controller, Cmd::RequestPermission(p))
    }

    /// Dropped silently unless Input is granted: a touch stream must not raise per-event errors.
    pub(crate) fn send_input(&self, e: RemoteInput) -> Result<(), ScrinError> {
        if self.role == Role::Controller && !self.granted().contains(Permission::Input) {
            return Ok(());
        }
        self.send_cmd(Role::Controller, Cmd::Input(e))
    }

    pub(crate) fn send_video_config(&self, c: VideoConfigInfo) -> Result<(), ScrinError> {
        if self.role == Role::Host {
            lock(&self.codec_config).clone_from(&c.codec_config);
        }
        self.send_cmd(Role::Host, Cmd::VideoConfig(c))
    }

    pub(crate) fn request_keyframe(&self) -> Result<(), ScrinError> {
        self.send_cmd(Role::Controller, Cmd::RequestKeyframe)
    }

    /// Shards one encoded access unit into datagrams. Frames are dropped (not
    /// queued) while View is not granted or the path rejects a datagram.
    pub(crate) fn send_video_frame(&self, data: &[u8], keyframe: bool) -> Result<(), ScrinError> {
        if self.role != Role::Host {
            return Err(ScrinError::state("only the host sends video"));
        }
        if !self.granted().contains(Permission::View) {
            return Ok(());
        }
        // A decoder joining at this keyframe (Windows openh264, a MediaCodec
        // reconfigured after a resolution change) needs SPS/PPS in band.
        let with_config;
        let data = {
            let cfg = lock(&self.codec_config);
            if keyframe && !cfg.is_empty() && !has_sps(data) {
                with_config = [cfg.as_slice(), data].concat();
                with_config.as_slice()
            } else {
                data
            }
        };
        let mut media = lock(&self.media);
        let Some(m) = media.as_mut() else {
            return Ok(());
        };
        let id = m.next_frame;
        m.next_frame = m.next_frame.wrapping_add(1);
        let shards = m
            .encoder
            .encode(id, keyframe, data)
            .map_err(|e| ScrinError::input(e.to_string()))?;
        let mut sent = 0usize;
        for shard in shards {
            let bytes = shard.to_bytes();
            let len = bytes.len();
            match send_datagram(&m.conn, Bytes::from(bytes)) {
                Ok(()) => sent += len,
                Err(NetError::DatagramTooLarge { .. } | NetError::DatagramsUnsupported) => {}
                Err(e) => return Err(e.into()),
            }
        }
        drop(media);
        let mut meter = lock(&self.meter);
        meter.add_frame();
        meter.add_bytes(sent);
        drop(meter);
        lock(&self.stats).frames += 1;
        Ok(())
    }

    pub(crate) fn end_by_user(&self, report: bool) {
        // Host and controller both end through the command channel.
        let _ = self.cmd.send(Cmd::End { report });
    }

    /// Emits the end exactly once and releases the session slot.
    fn finish(&self, shared: &Shared, end: EndInfo, conn: Option<&Connection>) {
        if self.ended.swap(true, Ordering::AcqRel) {
            return;
        }
        *lock(&self.media) = None;
        for t in lock(&self.tasks).drain(..) {
            t.abort();
        }
        if let Some(c) = conn {
            c.close(0u32.into(), b"session ended");
        }
        shared.clear_session(self.id);
        self.listener.on_state(SessionState::Ended);
        self.listener.on_ended(end);
    }

    fn emit_stats(&self, conn: &Connection) {
        let (fps, bps) = lock(&self.meter).rates(Instant::now());
        let mut st = lock(&self.stats).clone();
        let qs = conn.stats();
        st.bytes_in = qs.udp_rx.bytes;
        st.bytes_out = qs.udp_tx.bytes;
        st.fps = fps;
        st.bitrate_bps = bps;
        let paths = conn.paths();
        if let Some(p) = paths.iter().find(iroh::endpoint::Path::is_selected) {
            st.rtt_ms = u32::try_from(p.rtt().as_millis()).unwrap_or(u32::MAX);
            st.direct = p.is_ip();
        }
        self.listener.on_stats(st);
    }
}

fn ended(kind: EndKind, detail: impl Into<String>) -> EndInfo {
    EndInfo {
        kind,
        detail: detail.into(),
        duration_ms: None,
    }
}

/// Sends a final `SessionEnd`, finishes the stream and waits (bounded) until the
/// peer has read it, so closing the connection does not discard it.
async fn goodbye(send: &mut SendStream, reason: v1::SessionEndReason) {
    let msg = v1::SessionEnd {
        reason: reason.into(),
        message: String::new(),
    };
    if wire::send(send, Payload::SessionEnd(msg)).await.is_ok() && send.finish().is_ok() {
        let _ = tokio::time::timeout(GOODBYE_FLUSH, send.stopped()).await;
    }
}

fn ms_since(t0: Instant) -> u64 {
    u64::try_from(t0.elapsed().as_millis()).unwrap_or(u64::MAX)
}

pub(crate) fn sas_info(sas: Sas) -> SasInfo {
    let (emoji, names) = sas
        .0
        .iter()
        .map(|&i| {
            let (e, n) = EMOJI[usize::from(i) % EMOJI.len()];
            (e.to_owned(), n.to_owned())
        })
        .unzip();
    SasInfo { emoji, names }
}

/// Runs `fut` until it completes or the user ends the session (`None`).
async fn until_cancel<T>(
    fut: impl Future<Output = T>,
    cmds: &mut UnboundedReceiver<Cmd>,
) -> Option<T> {
    tokio::pin!(fut);
    loop {
        tokio::select! {
            v = &mut fut => return Some(v),
            c = cmds.recv() => match c {
                Some(Cmd::End { .. }) | None => return None,
                Some(_) => {}
            },
        }
    }
}

const fn end_kind(r: EndReason) -> EndKind {
    match r {
        EndReason::HostStopped => EndKind::HostStopped,
        EndReason::Reported => EndKind::Reported,
        EndReason::PeerEnded => EndKind::PeerEnded,
        EndReason::PeerDisconnected => EndKind::Disconnected,
        EndReason::TimeLimit => EndKind::TimeLimit,
        EndReason::Rejected(_) => EndKind::Rejected,
    }
}

fn controller_end(e: ControllerEnd) -> EndInfo {
    match e {
        ControllerEnd::Cancelled => ended(EndKind::Cancelled, ""),
        ControllerEnd::Rejected(r) => ended(EndKind::Rejected, format!("{r:?}")),
        ControllerEnd::Ended(r) => ended(end_kind(r), ""),
        ControllerEnd::ConnectFailed => ended(EndKind::ConnectFailed, ""),
        ControllerEnd::PairingFailed => ended(EndKind::PairingFailed, ""),
        ControllerEnd::Timeout => ended(EndKind::Timeout, ""),
    }
}

// ---- host ---------------------------------------------------------------

/// Accepts connections until aborted; one session at a time.
pub(crate) async fn host_loop(shared: Arc<Shared>, listener: Arc<dyn SessionListener>) {
    let ep = match shared.endpoint().await {
        Ok(ep) => ep.clone(),
        Err(e) => {
            listener.on_error(e.to_string());
            return;
        }
    };
    listener.on_state(SessionState::Listening);
    while let Some(res) = ep.accept_connection().await {
        let Ok(conn) = res else { continue };
        let Some(code) = shared.host_code() else {
            conn.close(1u32.into(), b"no code");
            continue;
        };
        let s = Session::new(Role::Host, Arc::clone(&listener));
        if !shared.set_session(&s) {
            conn.close(2u32.into(), b"busy");
            continue;
        }
        let task = tokio::spawn(host_flow(Arc::clone(&shared), Arc::clone(&s), conn, code));
        s.track(task);
    }
}

struct HostCtx<'a> {
    shared: &'a Shared,
    s: &'a Session,
    conn: &'a Connection,
    peer: DeviceId,
    controller_name: String,
    t0: Instant,
    /// Encoder config that arrived before Accept; sent right after it.
    pending_video: Option<VideoConfigInfo>,
}

async fn host_flow(shared: Arc<Shared>, s: Arc<Session>, conn: Connection, code: Arc<HostCode>) {
    let Some(mut cmds) = s.take_cmds() else {
        return;
    };
    s.listener.on_state(SessionState::Pairing);
    let out = match until_cancel(host_pair(&conn, shared.device_id(), &code), &mut cmds).await {
        None => return s.finish(&shared, ended(EndKind::Cancelled, ""), Some(&conn)),
        Some(Err(e)) => {
            return s.finish(
                &shared,
                ended(EndKind::PairingFailed, e.to_string()),
                Some(&conn),
            );
        }
        Some(Ok(o)) => o,
    };
    s.listener.on_sas(sas_info(out.sas));
    let ControlStream { mut send, recv, .. } = out.control;
    let (tx, mut ctl_rx) = mpsc::unbounded_channel();
    s.track(wire::spawn_reader(recv, tx.clone()));
    s.track(tokio::spawn(wire::accept_input_streams(conn.clone(), tx)));

    let mut ctx = HostCtx {
        shared: &shared,
        s: &s,
        conn: &conn,
        peer: out.peer,
        controller_name: String::new(),
        t0: Instant::now(),
        pending_video: None,
    };
    let mut hs = HostSession::new(Policy::default());
    let mut tick = tokio::time::interval(TICK);
    let mut ticks = 0u32;
    loop {
        let acts = tokio::select! {
            msg = ctl_rx.recv() => {
                let now = ms_since(ctx.t0);
                match msg.flatten() {
                    None => hs.handle(now, HostEvent::PeerDisconnected),
                    Some(p) => host_on_payload(&mut ctx, &mut hs, &mut send, p).await,
                }
            }
            c = cmds.recv() => host_on_cmd(&mut ctx, &mut hs, &mut send, c).await,
            _ = tick.tick() => {
                ticks = ticks.wrapping_add(1);
                if ticks.is_multiple_of(STATS_EVERY_TICKS) && !hs.granted().is_empty() {
                    s.emit_stats(&conn);
                }
                hs.on_tick(ms_since(ctx.t0))
            }
        };
        if let Some(end) = host_apply(&mut ctx, &hs, &mut send, acts).await {
            s.finish(&shared, end, Some(&conn));
            return;
        }
    }
}

async fn host_on_cmd(
    ctx: &mut HostCtx<'_>,
    hs: &mut HostSession,
    send: &mut SendStream,
    c: Option<Cmd>,
) -> Vec<HostAction> {
    let now = ms_since(ctx.t0);
    match c {
        None => hs.handle(now, HostEvent::UserStop),
        Some(Cmd::VideoConfig(cfg)) => {
            if hs.granted().is_empty() {
                ctx.pending_video = Some(cfg);
            } else {
                let _ = wire::send(send, wire::video_config_to_wire(cfg)).await;
            }
            Vec::new()
        }
        Some(c) => host_cmd_event(&c).map_or_else(Vec::new, |ev| hs.handle(now, ev)),
    }
}

async fn host_on_payload(
    ctx: &mut HostCtx<'_>,
    hs: &mut HostSession,
    send: &mut SendStream,
    p: Payload,
) -> Vec<HostAction> {
    let now = ms_since(ctx.t0);
    match p {
        Payload::SessionRequest(r) => {
            let requested = wire::perms_from_wire(&r.requested);
            // Mid-session, a request asks for more permissions (desktop engine).
            if matches!(hs.state(), HostState::Active { .. }) {
                let wanted = requested - hs.granted();
                return wanted
                    .iter()
                    .flat_map(|perm| hs.handle(now, HostEvent::PeerRequestPermission(perm)))
                    .collect();
            }
            ctx.controller_name = r.controller_name.chars().take(64).collect();
            hs.handle(
                now,
                HostEvent::Request {
                    peer: PeerId(ctx.peer.0),
                    // No account verification yet: every quick connect is anonymous (ADR-0009).
                    kind: SessionKind::Anonymous,
                    requested,
                },
            )
        }
        Payload::PermissionsUpdate(u) => {
            let wanted = wire::perms_from_wire(&u.granted) - hs.granted();
            wanted
                .iter()
                .flat_map(|perm| hs.handle(now, HostEvent::PeerRequestPermission(perm)))
                .collect()
        }
        Payload::SessionEnd(_) => hs.handle(now, HostEvent::PeerEnded),
        Payload::KeyframeRequest(_) => {
            ctx.s.listener.on_keyframe_request();
            Vec::new()
        }
        Payload::Ping(req) => {
            let t = micros_since(ctx.t0);
            let answer = v1::Pong {
                seq: req.seq,
                t1_us: req.t1_us,
                t2_us: t,
                t3_us: t,
            };
            let _ = wire::send(send, Payload::Pong(answer)).await;
            Vec::new()
        }
        p if wire::is_input(&p) => {
            if hs.granted().contains(Permission::Input) {
                for e in wire::input_from_wire(&p) {
                    ctx.s.listener.on_input(e);
                }
            }
            Vec::new()
        }
        // BitrateFeedback: the Android encoder runs CBR at a fixed target.
        _ => Vec::new(),
    }
}

fn host_cmd_event(c: &Cmd) -> Option<HostEvent> {
    Some(match c {
        Cmd::Accept(p) => HostEvent::UserAccept(*p),
        Cmd::Reject => HostEvent::UserReject,
        Cmd::Grant(p) => HostEvent::UserGrant(*p),
        Cmd::Revoke(p) => HostEvent::UserRevoke(*p),
        Cmd::AddTrust => HostEvent::UserAddTrust,
        Cmd::End { report: true } => HostEvent::StopAndReport,
        Cmd::End { report: false } => HostEvent::UserStop,
        Cmd::RequestPermission(_)
        | Cmd::Input(_)
        | Cmd::RequestKeyframe
        | Cmd::VideoConfig(_)
        | Cmd::Feedback(_) => {
            return None;
        }
    })
}

async fn host_send_accept(
    ctx: &mut HostCtx<'_>,
    hs: &HostSession,
    send: &mut SendStream,
    p: Permissions,
) {
    let max_ms = hs.kind().and_then(|k| hs.policy().max_duration_ms(k));
    let accept = v1::SessionAccept {
        granted: wire::perms_to_wire(p),
        displays: Vec::new(),
        max_duration_s: max_ms.map_or(0, |m| u32::try_from(m / 1000).unwrap_or(u32::MAX)),
    };
    let _ = wire::send(send, Payload::SessionAccept(accept)).await;
    if let Some(cfg) = ctx.pending_video.take() {
        let _ = wire::send(send, wire::video_config_to_wire(cfg)).await;
    }
    if let Ok(encoder) = FrameEncoder::new(MediaKind::Video, FEC_PARITY) {
        *lock(&ctx.s.media) = Some(MediaTx {
            conn: ctx.conn.clone(),
            encoder,
            next_frame: 0,
        });
    }
    ctx.s.set_granted(p);
    ctx.s.listener.on_state(SessionState::Active);
}

async fn host_apply(
    ctx: &mut HostCtx<'_>,
    hs: &HostSession,
    send: &mut SendStream,
    acts: Vec<HostAction>,
) -> Option<EndInfo> {
    let now = ms_since(ctx.t0);
    let l = Arc::clone(&ctx.s.listener);
    let mut end: Option<EndInfo> = None;
    for a in acts {
        match a {
            HostAction::ShowRequestDialog {
                kind,
                requested,
                allowed,
                accept_enabled_at,
                expires_at,
                ..
            } => {
                l.on_state(SessionState::IncomingRequest);
                l.on_incoming_request(IncomingRequest {
                    peer_id: ctx.peer.to_hex(),
                    peer_fingerprint: ctx.peer.fingerprint(),
                    verified: matches!(kind, SessionKind::Verified { .. }),
                    unattended: kind.is_unattended(),
                    controller_name: ctx.controller_name.clone(),
                    requested: perms_to_ffi(requested),
                    allowed: perms_to_ffi(allowed),
                    accept_in_ms: accept_enabled_at.saturating_sub(now),
                    expires_in_ms: expires_at.saturating_sub(now),
                });
            }
            HostAction::SendAccept(p) => host_send_accept(ctx, hs, send, p).await,
            HostAction::SendReject { reason, .. } => {
                let rej = v1::SessionReject {
                    reason: wire::reject_to_wire(reason).into(),
                    message: String::new(),
                };
                let _ = wire::send(send, Payload::SessionReject(rej)).await;
            }
            HostAction::SendPermissions(p) => {
                let upd = v1::PermissionsUpdate {
                    granted: wire::perms_to_wire(p),
                };
                let _ = wire::send(send, Payload::PermissionsUpdate(upd)).await;
                ctx.s.set_granted(p);
            }
            HostAction::AskGrant(p) => l.on_permission_asked(SessionPermission::from_core(p)),
            HostAction::PolicyDenied(p) => l.on_notice(Notice::PolicyDenied {
                permission: SessionPermission::from_core(p),
            }),
            HostAction::AddToTrustList(peer) => {
                let device = DeviceId(peer.0);
                let label = if ctx.controller_name.is_empty() {
                    device.fingerprint()
                } else {
                    ctx.controller_name.clone()
                };
                ctx.shared.add_trust(device, label, Profile::Support);
                l.on_notice(Notice::TrustAdded {
                    device_id: device.to_hex(),
                });
            }
            HostAction::TrustDenied => l.on_notice(Notice::TrustDenied),
            HostAction::ReportAbuse { peer } => l.on_notice(Notice::Reported {
                device_id: DeviceId(peer.0).to_hex(),
            }),
            HostAction::EndSession(reason) => {
                if !matches!(reason, EndReason::PeerEnded | EndReason::PeerDisconnected) {
                    goodbye(send, wire::end_to_wire(reason)).await;
                }
                end = Some(ended(end_kind(reason), ""));
            }
            HostAction::Notify(log) => {
                if let Some(e) = end.as_mut() {
                    e.duration_ms = Some(log.duration_ms());
                }
            }
            HostAction::HideRequestDialog | HostAction::ShowIndicator { .. } => {}
        }
    }
    end
}

// ---- controller -----------------------------------------------------------

pub(crate) async fn controller_flow(
    shared: Arc<Shared>,
    s: Arc<Session>,
    target: Target,
    code: String,
) {
    let Some(mut cmds) = s.take_cmds() else {
        return;
    };
    let t0 = Instant::now();
    let mut cs = ControllerSession::default();
    let requested = Permissions::support();
    let acts = cs.handle(ms_since(t0), ControllerEvent::Connect { requested });
    controller_apply(&s, &cs, None, acts).await;

    // Boxed: iroh's connect future is ~23 KB, too large to keep on the task stack.
    let dial = Box::pin(async {
        let addr = resolve_target(&shared, target).await?;
        let ep = shared.endpoint().await?;
        tokio::time::timeout(CONNECT_TIMEOUT, ep.connect(addr))
            .await
            .map_err(|_| ScrinError::Network {
                msg: "connect timed out".into(),
            })?
            .map_err(ScrinError::from)
    });
    let conn = match until_cancel(dial, &mut cmds).await {
        None => return s.finish(&shared, ended(EndKind::Cancelled, ""), None),
        Some(Err(e)) => {
            return s.finish(&shared, ended(EndKind::ConnectFailed, e.to_string()), None);
        }
        Some(Ok(c)) => c,
    };
    let acts = cs.handle(ms_since(t0), ControllerEvent::Connected);
    controller_apply(&s, &cs, None, acts).await;

    let unattended = code.trim().is_empty();
    let pair = async {
        if unattended {
            controller_auth_trusted(&conn, &shared.identity)
                .await
                .map(|c| (c, None))
        } else {
            controller_pair(&conn, shared.device_id(), &code)
                .await
                .map(|o| (o.control, Some(o.sas)))
        }
    };
    let (control, sas) = match until_cancel(pair, &mut cmds).await {
        None => return s.finish(&shared, ended(EndKind::Cancelled, ""), Some(&conn)),
        Some(Err(e)) => {
            return s.finish(
                &shared,
                ended(EndKind::PairingFailed, e.to_string()),
                Some(&conn),
            );
        }
        Some(Ok(v)) => v,
    };
    drop(code);
    let sas_text = sas.as_ref().map(Sas::emoji).unwrap_or_default();
    if let Some(sas) = sas {
        s.listener.on_sas(sas_info(sas));
    }
    let req = v1::SessionRequest {
        requested: Vec::new(),
        controller_name: shared.config.device_name.clone(),
        unattended,
    };
    let acts = cs.handle(ms_since(t0), ControllerEvent::Paired { sas: sas_text });
    controller_run(
        &shared,
        &s,
        &conn,
        control,
        Running {
            cs,
            cmds,
            t0,
            req,
            first: acts,
        },
    )
    .await;
}

/// A ticket/hex id dials directly; a 9-digit scrin ID goes through the server.
async fn resolve_target(shared: &Shared, target: Target) -> Result<EndpointAddr, ScrinError> {
    match target {
        Target::Addr(a) => Ok(a),
        Target::ScrinId(id) => {
            let server = shared
                .server
                .as_ref()
                .ok_or_else(|| ScrinError::input("a scrin ID needs a server; use a link"))?;
            Ok(server.resolve(&shared.identity, &id).await?)
        }
    }
}

struct Running {
    cs: ControllerSession,
    cmds: UnboundedReceiver<Cmd>,
    t0: Instant,
    req: v1::SessionRequest,
    first: Vec<ControllerAction>,
}

/// Lazily opened Input stream of a controller session.
struct InputOut {
    conn: Connection,
    tx: Option<UnboundedSender<Payload>>,
}

impl InputOut {
    fn send(&mut self, s: &Session, p: Payload) {
        let tx = self.tx.get_or_insert_with(|| {
            let (tx, rx) = mpsc::unbounded_channel();
            s.track(tokio::spawn(wire::input_writer(self.conn.clone(), rx)));
            tx
        });
        let _ = tx.send(p);
    }
}

async fn controller_run(
    shared: &Shared,
    s: &Arc<Session>,
    conn: &Connection,
    control: ControlStream,
    run: Running,
) {
    let Running {
        mut cs,
        mut cmds,
        t0,
        req,
        first,
    } = run;
    let ControlStream { mut send, recv, .. } = control;
    let (tx, mut ctl_rx) = mpsc::unbounded_channel();
    s.track(wire::spawn_reader(recv, tx));
    s.track(tokio::spawn(media_rx(conn.clone(), Arc::clone(s))));
    if let Some(end) = controller_apply(s, &cs, Some((&mut send, &req)), first).await {
        return s.finish(shared, end, Some(conn));
    }
    let mut input = InputOut {
        conn: conn.clone(),
        tx: None,
    };
    let mut tick = tokio::time::interval(TICK);
    let mut ticks = 0u32;
    loop {
        let acts = tokio::select! {
            msg = ctl_rx.recv() => {
                let now = ms_since(t0);
                match msg.flatten() {
                    None => cs.handle(now, ControllerEvent::Disconnected),
                    Some(p) => controller_on_payload(s, &mut cs, now, p),
                }
            }
            c = cmds.recv() => {
                controller_on_cmd(s, &mut cs, &mut send, &mut input, ms_since(t0), c).await
            }
            _ = tick.tick() => {
                ticks = ticks.wrapping_add(1);
                if ticks.is_multiple_of(STATS_EVERY_TICKS) && !cs.granted().is_empty() {
                    s.emit_stats(conn);
                }
                cs.on_tick(ms_since(t0))
            }
        };
        if let Some(end) = controller_apply(s, &cs, Some((&mut send, &req)), acts).await {
            s.finish(shared, end, Some(conn));
            return;
        }
    }
}

async fn controller_on_cmd(
    s: &Session,
    cs: &mut ControllerSession,
    send: &mut SendStream,
    input: &mut InputOut,
    now: u64,
    c: Option<Cmd>,
) -> Vec<ControllerAction> {
    match c {
        None | Some(Cmd::End { .. }) => cs.handle(now, ControllerEvent::Cancel),
        Some(Cmd::RequestPermission(p)) => cs.handle(now, ControllerEvent::RequestPermission(p)),
        Some(Cmd::Input(i)) => {
            if cs.granted().contains(Permission::Input) {
                input.send(s, wire::input_to_wire(i));
            }
            Vec::new()
        }
        Some(Cmd::RequestKeyframe) => {
            let kr = v1::KeyframeRequest {
                stream_id: 0,
                last_good_frame_id: 0,
            };
            let _ = wire::send(send, Payload::KeyframeRequest(kr)).await;
            Vec::new()
        }
        Some(Cmd::Feedback(fb)) => {
            let _ = wire::send(send, Payload::BitrateFeedback(fb)).await;
            Vec::new()
        }
        Some(_) => Vec::new(),
    }
}

fn controller_on_payload(
    s: &Session,
    cs: &mut ControllerSession,
    now: u64,
    p: Payload,
) -> Vec<ControllerAction> {
    match p {
        Payload::SessionAccept(a) => cs.handle(
            now,
            ControllerEvent::Accepted(wire::perms_from_wire(&a.granted)),
        ),
        Payload::SessionReject(r) => cs.handle(
            now,
            ControllerEvent::Rejected(wire::reject_from_wire(r.reason)),
        ),
        Payload::PermissionsUpdate(u) => cs.handle(
            now,
            ControllerEvent::PermissionsChanged(wire::perms_from_wire(&u.granted)),
        ),
        Payload::SessionEnd(e) => {
            cs.handle(now, ControllerEvent::Ended(wire::end_from_wire(e.reason)))
        }
        Payload::VideoConfig(c) => {
            if let Some(cfg) = wire::video_config_from_wire(&c) {
                s.listener.on_video_config(cfg);
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

async fn controller_apply(
    s: &Session,
    cs: &ControllerSession,
    mut ctl: Option<(&mut SendStream, &v1::SessionRequest)>,
    acts: Vec<ControllerAction>,
) -> Option<EndInfo> {
    let l = &s.listener;
    for a in acts {
        match a {
            ControllerAction::ShowStatus(st) => l.on_state(match st {
                ControllerStatus::Connecting => SessionState::Connecting,
                ControllerStatus::Pairing => SessionState::Pairing,
                ControllerStatus::AwaitingAccept => SessionState::AwaitingAccept,
                ControllerStatus::Active => SessionState::Active,
            }),
            ControllerAction::SendRequest(p) => {
                if let Some((w, req)) = ctl.as_mut() {
                    let mut r = (*req).clone();
                    r.requested = wire::perms_to_wire(p);
                    let _ = wire::send(w, Payload::SessionRequest(r)).await;
                }
            }
            ControllerAction::SendPermissionRequest(p) => {
                // Same shape as the desktop engine: a SessionRequest naming the extra permission.
                if let Some((w, req)) = ctl.as_mut() {
                    let r = v1::SessionRequest {
                        requested: wire::perms_to_wire(Permissions::only(p)),
                        controller_name: req.controller_name.clone(),
                        unattended: false,
                    };
                    let _ = wire::send(w, Payload::SessionRequest(r)).await;
                }
            }
            ControllerAction::ShowPermissions(p) => s.set_granted(p),
            ControllerAction::Disconnect => {
                if let Some((w, _)) = ctl.as_mut()
                    && matches!(
                        cs.state(),
                        scrin_session::ControllerState::Ended(ControllerEnd::Cancelled)
                    )
                {
                    goodbye(w, v1::SessionEndReason::ClosedByController).await;
                }
            }
            ControllerAction::ShowEnded(e) => return Some(controller_end(e)),
            ControllerAction::Dial
            | ControllerAction::StartPairing
            | ControllerAction::ShowSas(_)
            | ControllerAction::PermissionPending(_) => {}
        }
    }
    None
}

/// Controller: datagrams → FEC reassembly → complete access units to Kotlin;
/// arrivals → `BitrateFeedback` every 50 ms; losses → keyframe requests.
async fn media_rx(conn: Connection, s: Arc<Session>) {
    let started = Instant::now();
    let mut r = FrameReassembler::default();
    let mut log = ArrivalLog::default();
    let mut lost_seen = 0u64;
    let mut last_kf_request: Option<Instant> = None;
    let mut tick = tokio::time::interval(FEEDBACK_INTERVAL);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            d = recv_datagram(&conn) => {
                let Ok(d) = d else { break };
                lock(&s.meter).add_bytes(d.len());
                let Ok(header) = ShardHeader::decode(&d) else { continue };
                log.record(&header, micros_since(started), d.len());
                if let Ok(Some(f)) = r.push(&d) {
                    lock(&s.meter).add_frame();
                    s.listener
                        .on_video_frame(f.data, f.keyframe, f.frame_id, micros_since(started));
                }
            }
            _ = tick.tick() => {
                let rs = r.stats();
                {
                    let mut st = lock(&s.stats);
                    st.frames = rs.completed;
                    st.frames_recovered = rs.recovered;
                    st.frames_lost = rs.lost;
                }
                if let Some(fb) = log.take_report(0) && s.cmd.send(Cmd::Feedback(fb)).is_err() {
                    break;
                }
                if rs.lost > lost_seen {
                    lost_seen = rs.lost;
                    if last_kf_request.is_none_or(|t| t.elapsed() >= KEYFRAME_REQUEST_GAP) {
                        last_kf_request = Some(Instant::now());
                        let _ = s.cmd.send(Cmd::RequestKeyframe);
                    }
                }
            }
        }
    }
}
