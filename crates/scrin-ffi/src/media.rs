//! Media bookkeeping shared by both roles: the controller's arrival log that
//! becomes `BitrateFeedback` (same algorithm and wire shape as the desktop
//! engine's receiver, so a Windows host's GCC estimator adapts to a phone),
//! a per-interval fps/bitrate meter, and H.264 Annex B helpers.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use scrin_media::fec::ShardHeader;
use scrin_proto::v1;

/// How often the controller reports arrivals (the engine uses the same).
pub(crate) const FEEDBACK_INTERVAL: Duration = Duration::from_millis(50);
/// A frame this many ids behind the newest is counted as done for loss stats.
const LOSS_HORIZON: u32 = 16;
/// Arrivals kept per report (one report per [`FEEDBACK_INTERVAL`]).
const MAX_ARRIVALS: usize = 2048;

pub(crate) fn micros_since(t: Instant) -> u64 {
    u64::try_from(t.elapsed().as_micros()).unwrap_or(u64::MAX)
}

/// Controller-side record of received shards, drained into `BitrateFeedback`.
#[derive(Debug, Default)]
pub(crate) struct ArrivalLog {
    base_us: Option<u64>,
    arrivals: Vec<v1::DatagramArrival>,
    received: u32,
    /// frame id → (shard count, shards seen)
    frames: HashMap<u32, (u16, u16)>,
    newest: Option<u32>,
    lost: u32,
}

impl ArrivalLog {
    pub(crate) fn record(&mut self, h: &ShardHeader, now_us: u64, size: usize) {
        let base = *self.base_us.get_or_insert(now_us);
        self.received = self.received.saturating_add(1);
        if self.arrivals.len() < MAX_ARRIVALS {
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
                lost = lost.saturating_add(u32::from(count.saturating_sub(seen)));
            }
            !old
        });
        self.lost = self.lost.saturating_add(lost);
    }

    /// The report for the last interval, or `None` when nothing happened.
    /// `cap_bps` (0 = none) travels as the receiver's bitrate ceiling.
    pub(crate) fn take_report(&mut self, cap_bps: u32) -> Option<v1::BitrateFeedback> {
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

/// Frames and media bytes, turned into per-interval rates on demand.
#[derive(Debug, Default)]
pub(crate) struct Meter {
    frames: u64,
    bytes: u64,
    prev: Option<(Instant, u64, u64)>,
}

impl Meter {
    pub(crate) fn add_bytes(&mut self, n: usize) {
        self.bytes = self.bytes.saturating_add(n as u64);
    }

    pub(crate) fn add_frame(&mut self) {
        self.frames = self.frames.saturating_add(1);
    }

    /// `(fps, bits per second)` since the previous call (zeros on the first).
    pub(crate) fn rates(&mut self, now: Instant) -> (f32, u64) {
        let out = self.prev.map_or((0.0, 0), |(at, f, b)| {
            let dt = now.duration_since(at).as_secs_f64().max(0.001);
            #[expect(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
            let fps = (self.frames.saturating_sub(f) as f64 / dt) as f32;
            #[expect(
                clippy::cast_precision_loss,
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss
            )]
            let bps = (self.bytes.saturating_sub(b) as f64 * 8.0 / dt) as u64;
            (fps, bps)
        });
        self.prev = Some((now, self.frames, self.bytes));
        out
    }
}

/// NAL unit types in an Annex B buffer (3- and 4-byte start codes).
fn nal_types(data: &[u8]) -> impl Iterator<Item = u8> + '_ {
    let mut i = 0usize;
    std::iter::from_fn(move || {
        while i + 3 < data.len() {
            let hit = data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1;
            i += 1;
            if hit {
                i += 2;
                return Some(data[i] & 0x1f);
            }
        }
        None
    })
}

/// Whether an Annex B access unit carries an SPS (NAL type 7).
pub(crate) fn has_sps(data: &[u8]) -> bool {
    nal_types(data).any(|t| t == 7)
}

#[cfg(test)]
mod tests {
    use super::*;
    use scrin_media::fec::MediaKind;

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
        log.record(&header(1 + LOSS_HORIZON + 1, 0, 1), 9_000, 116);
        let fb = log.take_report(5_000_000).expect("report");
        assert_eq!(fb.datagrams_lost, 1);
        assert_eq!(fb.estimated_bps, 5_000_000);
        assert!(log.take_report(0).is_none());
    }

    #[test]
    fn meter_reports_rates_per_interval() {
        let mut m = Meter::default();
        let t0 = Instant::now();
        assert_eq!(m.rates(t0), (0.0, 0));
        for _ in 0..30 {
            m.add_frame();
            m.add_bytes(1000);
        }
        let (fps, bps) = m.rates(t0 + Duration::from_secs(1));
        assert!((fps - 30.0).abs() < 0.01, "{fps}");
        assert_eq!(bps, 240_000);
    }

    #[test]
    fn finds_sps_behind_both_start_code_lengths() {
        let idr = [0, 0, 0, 1, 0x65, 0x88];
        let with_sps = [
            0, 0, 0, 1, 0x67, 0x42, 0, 0, 1, 0x68, 0xce, 0, 0, 0, 1, 0x65, 0x88,
        ];
        assert!(!has_sps(&idr));
        assert!(has_sps(&with_sps));
        assert!(!has_sps(&[]));
        assert!(!has_sps(&[0, 0, 1]));
    }
}
