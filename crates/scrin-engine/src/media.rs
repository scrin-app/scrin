//! Video over QUIC datagrams.
//!
//! **Datagram format.** Every video datagram is one `scrin_media::fec` shard:
//! the 16-byte [`ShardHeader`] (version, kind, flags, `frame_id`,
//! `shard_index`, `shard_count`, `data_shards`, `payload_len`) followed by
//! `payload_len` bytes. The `scrin_proto::media` header has the same size but
//! no `payload_len`, which Reed-Solomon reassembly needs, and the frame length
//! travels inside the protected data; so the media path uses the scrin-media
//! header end to end and the proto header is not put on the wire by this
//! engine. Send times for bandwidth estimation stay on the host (looked up by
//! `frame_id`/`shard_index` when `BitrateFeedback` arrives), so no timestamp
//! is needed in the datagram.
//!
//! **Host** ([`HostStream`]): a dedicated thread runs capture → encode →
//! shard → `send_datagram`, paced to the adaptive fps. `BitrateFeedback` from
//! the controller feeds the GCC estimator, whose target drives the quality
//! ladder (bitrate, then fps, then scale) and the FEC parity ratio.
//!
//! **Controller** ([`Receiver`]): an async task reads datagrams into a
//! `FrameReassembler`, reports arrivals every 50 ms, and hands complete frames
//! to a decoder thread, which emits BGRA frames for the native renderer.
//! Latest-frame-wins: when the decoder falls behind, frames are dropped and a
//! keyframe is requested instead of queueing.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc as std_mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use bytes::Bytes;
use scrin_media::adapt::{EncoderSettings, Mode, QualityController};
use scrin_media::bwe::{BandwidthEstimator, BweConfig, PacketFeedback};
use scrin_media::fec::{CompletedFrame, FrameEncoder, FrameReassembler, MediaKind, ShardHeader};
use scrin_net::Connection;
use scrin_net::datagram::{recv_datagram, send_datagram};
use scrin_proto::v1::{self, envelope::Payload};
use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::api::VideoFrame;
use crate::backend::{DecoderConfig, EncoderConfig, MediaBackend};
use crate::gw::{DATAGRAM_LANE, Seal};
use crate::wire::{Outgoing, env};

/// How often the controller reports arrivals.
pub(crate) const FEEDBACK_INTERVAL: Duration = Duration::from_millis(50);
/// Periodic keyframe so a late joiner or a missed request recovers.
const KEYFRAME_INTERVAL: Duration = Duration::from_secs(5);
/// Frames of send history kept for feedback lookups.
const SEND_HISTORY_FRAMES: usize = 512;
/// Decode jobs allowed in flight before frames are dropped.
const MAX_PENDING_DECODES: usize = 4;
/// A frame this many ids behind the newest is counted as done for loss stats.
const LOSS_HORIZON: u32 = 16;

fn micros_since(t: Instant) -> u64 {
    u64::try_from(t.elapsed().as_micros()).unwrap_or(u64::MAX)
}

/// Messages from the actor to the host media thread.
#[derive(Debug)]
pub(crate) enum MediaCtl {
    Feedback(v1::BitrateFeedback),
    Keyframe,
    Mode(Mode),
}

/// The running host stream; dropping it stops the thread.
#[derive(Debug)]
pub(crate) struct HostStream {
    stop: Arc<AtomicBool>,
    ctl: std_mpsc::Sender<MediaCtl>,
    thread: Option<JoinHandle<()>>,
}

impl HostStream {
    pub(crate) fn control(&self, msg: MediaCtl) {
        let _ = self.ctl.send(msg);
    }
}

impl Drop for HostStream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Send times of recent shards, per frame.
#[derive(Debug, Default)]
struct SendLog {
    frames: VecDeque<(u32, Vec<(u64, u32)>)>,
}

impl SendLog {
    fn push(&mut self, frame_id: u32, shards: Vec<(u64, u32)>) {
        if self.frames.len() == SEND_HISTORY_FRAMES {
            self.frames.pop_front();
        }
        self.frames.push_back((frame_id, shards));
    }

    fn lookup(&self, frame_id: u32, shard: u32) -> Option<(u64, u32)> {
        let (_, shards) = self.frames.iter().rev().find(|(id, _)| *id == frame_id)?;
        shards.get(usize::try_from(shard).ok()?).copied()
    }
}

pub(crate) struct HostStreamConfig {
    pub conn: Connection,
    pub backend: Arc<dyn MediaBackend>,
    pub control: mpsc::UnboundedSender<Outgoing>,
    pub display: u32,
    pub mode: Mode,
    /// Gateway sessions seal every datagram on [`DATAGRAM_LANE`].
    pub seal: Seal,
    pub on_error: Box<dyn Fn(String) + Send>,
}

pub(crate) fn start_host_stream(cfg: HostStreamConfig) -> std::io::Result<HostStream> {
    let stop = Arc::new(AtomicBool::new(false));
    let (ctl, rx) = std_mpsc::channel();
    let flag = stop.clone();
    let thread = std::thread::Builder::new()
        .name("scrin-host-video".into())
        .spawn(move || {
            let on_error = cfg.on_error;
            if let Err(e) = host_loop(
                &cfg.conn,
                &*cfg.backend,
                &cfg.control,
                cfg.display,
                cfg.mode,
                cfg.seal.as_deref(),
                &rx,
                &flag,
            ) {
                warn!(error = %e, "host video stream stopped");
                on_error(e);
            }
        })?;
    Ok(HostStream {
        stop,
        ctl,
        thread: Some(thread),
    })
}

struct Adaptation {
    bwe: BandwidthEstimator,
    ladder: QualityController,
    settings: EncoderSettings,
}

impl Adaptation {
    fn new(mode: Mode) -> Self {
        let bwe = BandwidthEstimator::new(BweConfig::default());
        let mut ladder = QualityController::with_mode(mode);
        let settings = ladder.update(bwe.target_bps(), 0);
        Self {
            bwe,
            ladder,
            settings,
        }
    }

    /// Feeds one report; returns the parity ratio to use.
    fn on_feedback(&mut self, fb: &v1::BitrateFeedback, log: &SendLog, now_us: u64) -> f32 {
        let base = fb.base_receive_us;
        let mut packets: Vec<PacketFeedback> = fb
            .arrivals
            .iter()
            .filter_map(|a| {
                let (sent, size) = log.lookup(a.frame_id, a.shard_index)?;
                Some(PacketFeedback::received(
                    sent,
                    base.saturating_add(u64::from(a.receive_delta_us)),
                    size.max(a.size_bytes),
                ))
            })
            .collect();
        packets.sort_by_key(|p| p.send_time_us);
        let newest_send = packets.last().map_or(now_us, |p| p.send_time_us);
        let lost = fb.datagrams_lost.min(4096);
        packets.extend((0..lost).map(|_| PacketFeedback::lost(newest_send, 1200)));
        let mut target = self.bwe.on_feedback(&packets, now_us);
        if fb.estimated_bps > 0 {
            target = target.min(fb.estimated_bps);
        }
        self.settings = self.ladder.update(target, now_us);
        // 5 % baseline, two parity shards per observed loss, capped at 30 %.
        #[expect(clippy::cast_possible_truncation)] // a ratio in 0.05..=0.3
        let ratio = (0.05 + 2.0 * self.bwe.loss_fraction()).clamp(0.05, 0.3) as f32;
        ratio
    }
}

#[expect(clippy::too_many_arguments)] // one call site; a struct would only rename them
fn host_loop(
    conn: &Connection,
    backend: &dyn MediaBackend,
    control: &mpsc::UnboundedSender<Outgoing>,
    display: u32,
    mode: Mode,
    seal: Option<&crate::gw::Channel>,
    rx: &std_mpsc::Receiver<MediaCtl>,
    stop: &AtomicBool,
) -> Result<(), String> {
    let mut capture = backend.open_capture(display).map_err(|e| e.to_string())?;
    let (width, height) = capture.size();
    let mut adapt = Adaptation::new(mode);
    let mut encoder = backend
        .open_encoder(EncoderConfig {
            width,
            height,
            settings: adapt.settings,
        })
        .map_err(|e| e.to_string())?;
    let mut applied = adapt.settings;
    let video_config = v1::VideoConfig {
        stream_id: 0,
        codec: encoder.codec().into(),
        width,
        height,
        fps: applied.fps,
        bitrate_bps: applied.bitrate_bps,
        chroma: v1::ChromaSubsampling::ChromaSubsampling420.into(),
        hdr: false,
        display_id: display,
        codec_config: encoder.codec_config(),
    };
    control
        .send(Outgoing::Env(env(Payload::VideoConfig(video_config))))
        .map_err(|_| "control stream closed".to_owned())?;

    let mut fec = FrameEncoder::new(MediaKind::Video, 0.1).map_err(|e| e.to_string())?;
    let mut log = SendLog::default();
    let started = Instant::now();
    let mut frame_id: u32 = 0;
    let mut force_key = true;
    let mut last_key = Instant::now();
    let mut next_due = Instant::now();
    let mut sent = SendCounters::default();

    while !stop.load(Ordering::Acquire) {
        while let Ok(msg) = rx.try_recv() {
            match msg {
                MediaCtl::Feedback(fb) => {
                    let ratio = adapt.on_feedback(&fb, &log, micros_since(started));
                    let _ = fec.set_parity_ratio(ratio);
                }
                MediaCtl::Keyframe => force_key = true,
                MediaCtl::Mode(m) => adapt.ladder = QualityController::with_mode(m),
            }
        }
        if adapt.settings != applied {
            encoder
                .reconfigure(adapt.settings)
                .map_err(|e| e.to_string())?;
            applied = adapt.settings;
        }
        let interval = Duration::from_micros(1_000_000 / u64::from(applied.fps.max(1)));
        let Some(frame) = capture.capture(interval).map_err(|e| e.to_string())? else {
            continue;
        };
        if last_key.elapsed() >= KEYFRAME_INTERVAL {
            force_key = true;
        }
        if let Some(au) = encoder
            .encode(&frame, force_key)
            .map_err(|e| e.to_string())?
        {
            if au.keyframe {
                force_key = false;
                last_key = Instant::now();
            }
            let shards = fec
                .encode(frame_id, au.keyframe, &au.data)
                .map_err(|e| e.to_string())?;
            let Some(times) = send_shards(conn, seal, &shards, started, &mut sent)? else {
                return Ok(());
            };
            log.push(frame_id, times);
            frame_id = frame_id.wrapping_add(1);
        }
        // Pace to the target fps; a blocking capturer already waited.
        next_due += interval;
        let now = Instant::now();
        if next_due > now {
            std::thread::sleep(next_due - now);
        } else if now - next_due > interval * 4 {
            next_due = now;
        }
    }
    Ok(())
}

#[derive(Debug, Default)]
struct SendCounters {
    ok: u64,
    err: u64,
}

/// Sends one frame's shards (sealed on the gateway path). Returns the send
/// log entries, or `None` once the connection is gone.
fn send_shards(
    conn: &Connection,
    seal: Option<&crate::gw::Channel>,
    shards: &[scrin_media::fec::Shard],
    started: Instant,
    sent: &mut SendCounters,
) -> Result<Option<Vec<(u64, u32)>>, String> {
    let mut times = Vec::with_capacity(shards.len());
    for shard in shards {
        let bytes = shard.to_bytes();
        let size = u32::try_from(bytes.len()).unwrap_or(u32::MAX);
        times.push((micros_since(started), size));
        let bytes = match seal {
            None => bytes,
            Some(c) => c.seal(DATAGRAM_LANE, &bytes).map_err(|e| e.to_string())?,
        };
        match send_datagram(conn, Bytes::from(bytes)) {
            Ok(()) => sent.ok += 1,
            Err(scrin_net::NetError::Connection(_)) => return Ok(None),
            Err(e) => {
                sent.err += 1;
                if sent.err.is_power_of_two() {
                    debug!(error = %e, sent_ok = sent.ok, sent_err = sent.err, "datagram not sent");
                }
            }
        }
    }
    Ok(Some(times))
}

/// Counters the actor turns into `Stats` events.
#[derive(Debug, Default)]
pub(crate) struct ReceiverStats {
    pub decoded: AtomicU64,
    pub bytes: AtomicU64,
    pub completed: AtomicU64,
    pub lost: AtomicU64,
    pub decode_us: AtomicU64,
    pub width: AtomicU32,
    pub height: AtomicU32,
}

enum DecodeJob {
    Config(DecoderConfig),
    Frame(CompletedFrame),
}

/// The running controller pipeline; dropping it stops both halves.
#[derive(Debug)]
pub(crate) struct Receiver {
    task: tokio::task::JoinHandle<()>,
    jobs: std_mpsc::Sender<DecodeJob>,
    pending: Arc<AtomicUsize>,
}

impl std::fmt::Debug for DecodeJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Config(_) => "Config",
            Self::Frame(_) => "Frame",
        })
    }
}

impl Receiver {
    pub(crate) fn configure(&self, cfg: DecoderConfig) {
        self.pending.fetch_add(1, Ordering::AcqRel);
        let _ = self.jobs.send(DecodeJob::Config(cfg));
    }
}

impl Drop for Receiver {
    fn drop(&mut self) {
        self.task.abort();
        // The decoder thread exits when `jobs` (its only sender) drops.
    }
}

pub(crate) struct ReceiverConfig {
    pub conn: Connection,
    pub backend: Arc<dyn MediaBackend>,
    pub control: mpsc::UnboundedSender<Outgoing>,
    pub stats: Arc<ReceiverStats>,
    /// Receiver bitrate cap (0 = none), sent as `estimated_bps`.
    pub cap_bps: Arc<AtomicU32>,
    pub on_frame: Box<dyn Fn(VideoFrame) + Send>,
}

pub(crate) fn start_receiver(cfg: ReceiverConfig) -> std::io::Result<Receiver> {
    let (jobs, rx) = std_mpsc::channel::<DecodeJob>();
    let pending = Arc::new(AtomicUsize::new(0));
    let need_key = Arc::new(AtomicBool::new(true));
    let awaiting_key = Arc::new(AtomicBool::new(true));

    {
        let backend = cfg.backend.clone();
        let stats = cfg.stats.clone();
        let pending = pending.clone();
        let need_key = need_key.clone();
        let awaiting_key = awaiting_key.clone();
        let on_frame = cfg.on_frame;
        std::thread::Builder::new()
            .name("scrin-decode".into())
            .spawn(move || {
                decode_loop(
                    &rx,
                    &*backend,
                    &stats,
                    &pending,
                    &need_key,
                    &awaiting_key,
                    &*on_frame,
                );
            })?;
    }

    let task = tokio::spawn(receive_loop(
        cfg.conn,
        cfg.control,
        cfg.stats,
        cfg.cap_bps,
        jobs.clone(),
        pending.clone(),
        need_key,
        awaiting_key,
    ));
    Ok(Receiver {
        task,
        jobs,
        pending,
    })
}

fn decode_loop(
    rx: &std_mpsc::Receiver<DecodeJob>,
    backend: &dyn MediaBackend,
    stats: &ReceiverStats,
    pending: &AtomicUsize,
    need_key: &AtomicBool,
    awaiting_key: &AtomicBool,
    on_frame: &dyn Fn(VideoFrame),
) {
    let mut decoder = None;
    while let Ok(job) = rx.recv() {
        pending.fetch_sub(1, Ordering::AcqRel);
        match job {
            DecodeJob::Config(cfg) => match backend.open_decoder(&cfg) {
                Ok(d) => {
                    decoder = Some(d);
                    awaiting_key.store(true, Ordering::Release);
                    need_key.store(true, Ordering::Release);
                }
                Err(e) => warn!(error = %e, "no decoder for the host's stream"),
            },
            DecodeJob::Frame(f) => {
                let Some(dec) = decoder.as_mut() else {
                    need_key.store(true, Ordering::Release);
                    continue;
                };
                if awaiting_key.load(Ordering::Acquire) && !f.keyframe {
                    continue;
                }
                awaiting_key.store(false, Ordering::Release);
                let t = Instant::now();
                match dec.decode(&f.data, f.keyframe) {
                    Ok(Some(d)) => {
                        stats
                            .decode_us
                            .fetch_add(micros_since(t), Ordering::Relaxed);
                        stats.decoded.fetch_add(1, Ordering::Relaxed);
                        stats.width.store(d.width, Ordering::Relaxed);
                        stats.height.store(d.height, Ordering::Relaxed);
                        on_frame(VideoFrame {
                            width: d.width,
                            height: d.height,
                            stride: d.stride,
                            bgra: d.bgra,
                            frame_id: f.frame_id,
                        });
                    }
                    Ok(None) => {}
                    Err(e) => {
                        debug!(error = %e, "decode failed; waiting for a keyframe");
                        awaiting_key.store(true, Ordering::Release);
                        need_key.store(true, Ordering::Release);
                    }
                }
            }
        }
    }
}

#[derive(Default)]
struct ArrivalLog {
    base_us: Option<u64>,
    arrivals: Vec<v1::DatagramArrival>,
    received: u32,
    /// frame id → (shard count, shards seen)
    frames: HashMap<u32, (u16, u16)>,
    newest: Option<u32>,
    lost: u32,
}

impl ArrivalLog {
    fn record(&mut self, h: &ShardHeader, now_us: u64, size: usize) {
        let base = *self.base_us.get_or_insert(now_us);
        self.received += 1;
        if self.arrivals.len() < 2048 {
            self.arrivals.push(v1::DatagramArrival {
                frame_id: h.frame_id,
                shard_index: u32::from(h.shard_index),
                receive_delta_us: u32::try_from(now_us.saturating_sub(base)).unwrap_or(u32::MAX),
                size_bytes: u32::try_from(size).unwrap_or(u32::MAX),
            });
        }
        let e = self.frames.entry(h.frame_id).or_insert((h.shard_count, 0));
        e.1 = e.1.saturating_add(1);
        if self
            .newest
            .is_none_or(|n| h.frame_id.wrapping_sub(n) < 0x8000_0000)
        {
            self.newest = Some(h.frame_id);
        }
    }

    /// Counts shards of frames that fell behind the horizon as lost.
    fn settle(&mut self) {
        let Some(newest) = self.newest else { return };
        let mut lost = 0u32;
        self.frames.retain(|&id, &mut (count, seen)| {
            let old = newest.wrapping_sub(id) > LOSS_HORIZON;
            if old {
                lost += u32::from(count.saturating_sub(seen));
            }
            !old
        });
        self.lost += lost;
    }

    fn take_report(&mut self, cap_bps: u32) -> Option<v1::BitrateFeedback> {
        self.settle();
        if self.arrivals.is_empty() && self.lost == 0 {
            return None;
        }
        let fb = v1::BitrateFeedback {
            stream_id: 0,
            base_receive_us: self.base_us.unwrap_or(0),
            arrivals: std::mem::take(&mut self.arrivals),
            datagrams_received: self.received,
            datagrams_lost: self.lost,
            shards_recovered_by_fec: 0,
            frames_dropped: 0,
            estimated_bps: cap_bps,
        };
        self.base_us = None;
        self.received = 0;
        self.lost = 0;
        Some(fb)
    }
}

#[expect(clippy::too_many_arguments)] // one call site; a struct would only rename them
async fn receive_loop(
    conn: Connection,
    control: mpsc::UnboundedSender<Outgoing>,
    stats: Arc<ReceiverStats>,
    cap_bps: Arc<AtomicU32>,
    jobs: std_mpsc::Sender<DecodeJob>,
    pending: Arc<AtomicUsize>,
    need_key: Arc<AtomicBool>,
    awaiting_key: Arc<AtomicBool>,
) {
    let started = Instant::now();
    let mut reasm = FrameReassembler::default();
    let mut log = ArrivalLog::default();
    let mut tick = tokio::time::interval(FEEDBACK_INTERVAL);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            d = recv_datagram(&conn) => {
                let Ok(bytes) = d else { break };
                stats.bytes.fetch_add(bytes.len() as u64, Ordering::Relaxed);
                let Ok(header) = ShardHeader::decode(&bytes) else { continue };
                log.record(&header, micros_since(started), bytes.len());
                if let Ok(Some(frame)) = reasm.push(&bytes) {
                    if pending.load(Ordering::Acquire) >= MAX_PENDING_DECODES {
                        awaiting_key.store(true, Ordering::Release);
                        need_key.store(true, Ordering::Release);
                        continue;
                    }
                    pending.fetch_add(1, Ordering::AcqRel);
                    if jobs.send(DecodeJob::Frame(frame)).is_err() {
                        break;
                    }
                }
            }
            _ = tick.tick() => {
                let s = reasm.stats();
                stats.completed.store(s.completed, Ordering::Relaxed);
                stats.lost.store(s.lost, Ordering::Relaxed);
                if let Some(fb) = log.take_report(cap_bps.load(Ordering::Relaxed))
                    && control.send(Outgoing::Env(env(Payload::BitrateFeedback(fb)))).is_err()
                {
                    break;
                }
                if need_key.swap(false, Ordering::AcqRel) {
                    let req = v1::KeyframeRequest { stream_id: 0, last_good_frame_id: 0 };
                    if control.send(Outgoing::Env(env(Payload::KeyframeRequest(req)))).is_err() {
                        break;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(frame_id: u32, idx: u16, count: u16) -> ShardHeader {
        ShardHeader {
            kind: MediaKind::Video,
            keyframe: false,
            frame_id,
            shard_index: idx,
            shard_count: count,
            data_shards: count,
            payload_len: 100,
        }
    }

    #[test]
    fn arrival_log_reports_and_counts_losses() {
        let mut log = ArrivalLog::default();
        log.record(&header(1, 0, 3), 1_000, 116);
        log.record(&header(1, 2, 3), 1_500, 116);
        let fb = log.take_report(0).expect("report");
        assert_eq!(fb.arrivals.len(), 2);
        assert_eq!(fb.arrivals[1].receive_delta_us, 500);
        assert_eq!(fb.datagrams_lost, 0);
        // Frame 1 falls behind the horizon with one shard missing.
        log.record(&header(1 + LOSS_HORIZON + 1, 0, 1), 9_000, 116);
        let fb = log.take_report(5_000_000).expect("report");
        assert_eq!(fb.datagrams_lost, 1);
        assert_eq!(fb.estimated_bps, 5_000_000);
        assert!(log.take_report(0).is_none());
    }

    #[test]
    fn send_log_finds_recent_shards() {
        let mut log = SendLog::default();
        log.push(7, vec![(10, 1200), (11, 1200)]);
        assert_eq!(log.lookup(7, 1), Some((11, 1200)));
        assert_eq!(log.lookup(7, 2), None);
        assert_eq!(log.lookup(8, 0), None);
    }

    #[test]
    fn feedback_lowers_quality_under_heavy_loss() {
        let mut a = Adaptation::new(Mode::Quality);
        let mut log = SendLog::default();
        let mut ratio = 0.0;
        for i in 0..40u32 {
            let t = u64::from(i) * 50_000;
            log.push(i, vec![(t, 1200)]);
            let fb = v1::BitrateFeedback {
                base_receive_us: t + 5_000,
                arrivals: vec![v1::DatagramArrival {
                    frame_id: i,
                    shard_index: 0,
                    receive_delta_us: 0,
                    size_bytes: 1200,
                }],
                datagrams_received: 1,
                datagrams_lost: 3,
                ..Default::default()
            };
            ratio = a.on_feedback(&fb, &log, t + 10_000);
        }
        assert!(ratio > 0.2, "parity ratio {ratio}");
        assert!(a.settings.bitrate_bps < 2_000_000, "{:?}", a.settings);
    }
}
