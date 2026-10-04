//! Delay- and loss-based bandwidth estimation in the spirit of Google
//! Congestion Control (GCC, draft-ietf-rmcat-gcc).
//!
//! The receiver reports, per media packet, the send and arrival time; the
//! sender feeds those reports into [`BandwidthEstimator::on_feedback`].
//!
//! Pipeline:
//! 1. **Grouping** – packets sent within [`BweConfig::burst_us`] of the first
//!    packet of a group form one group (one encoded frame, roughly).
//! 2. **Trendline** – the inter-group delay variation
//!    `d = (arrival_i - arrival_{i-1}) - (send_i - send_{i-1})` is accumulated,
//!    exponentially smoothed (0.9) and a least-squares slope is fitted over the
//!    last 20 groups. A positive slope means queues are building.
//! 3. **Overuse detector** – the slope (scaled) is compared with an adaptive
//!    threshold `γ` that tracks the signal with gains `k_u`/`k_d`.
//! 4. **AIMD** – on *normal* the rate grows ~8 %/s; on *overuse* it drops to
//!    0.85 × measured receive rate; on *underuse* it holds.
//! 5. **Loss** – loss > 10 % multiplies the rate by `1 - 0.5 × loss`; loss
//!    < 2 % allows increase; in between holds.
//!
//! The final target is `min(delay_rate, loss_rate)` clamped to the configured
//! bounds.

use std::collections::VecDeque;

/// Timing report for one received media packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacketFeedback {
    /// Sender clock, microseconds.
    pub send_time_us: u64,
    /// Receiver clock, microseconds; `None` when the packet was lost.
    pub arrival_time_us: Option<u64>,
    /// Packet size in bytes (including headers).
    pub size: u32,
}

impl PacketFeedback {
    /// A packet that arrived.
    pub fn received(send_time_us: u64, arrival_time_us: u64, size: u32) -> Self {
        Self {
            send_time_us,
            arrival_time_us: Some(arrival_time_us),
            size,
        }
    }

    /// A packet reported lost.
    pub fn lost(send_time_us: u64, size: u32) -> Self {
        Self {
            send_time_us,
            arrival_time_us: None,
            size,
        }
    }
}

/// Estimator tuning.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BweConfig {
    /// Lowest target bitrate.
    pub min_bps: u32,
    /// Highest target bitrate.
    pub max_bps: u32,
    /// Starting estimate.
    pub start_bps: u32,
    /// Send-time span that forms one packet group.
    pub burst_us: u64,
    /// Groups in the trendline regression window.
    pub trendline_window: usize,
    /// Exponential smoothing of the accumulated delay.
    pub smoothing: f64,
    /// Gain applied to the trendline slope before thresholding.
    pub threshold_gain: f64,
    /// Multiplicative increase per second while the link is not overused.
    pub increase_per_s: f64,
    /// Fraction of the measured receive rate taken on overuse.
    pub beta: f64,
}

impl Default for BweConfig {
    fn default() -> Self {
        Self {
            min_bps: 100_000,
            max_bps: 100_000_000,
            start_bps: 2_000_000,
            burst_us: 5_000,
            trendline_window: 20,
            smoothing: 0.9,
            threshold_gain: 4.0,
            increase_per_s: 0.08,
            beta: 0.85,
        }
    }
}

/// Link state from the overuse detector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BandwidthUsage {
    /// Delay is stable.
    Normal,
    /// Queues are building.
    Overusing,
    /// Queues are draining.
    Underusing,
}

#[derive(Debug, Clone, Copy)]
#[allow(clippy::struct_field_names)] // all are µs timestamps; the suffix carries the unit
struct Group {
    first_send_us: u64,
    last_send_us: u64,
    last_arrival_us: u64,
}

/// Least-squares slope over smoothed accumulated delay.
#[derive(Debug)]
struct Trendline {
    window: usize,
    smoothing: f64,
    accumulated_ms: f64,
    smoothed_ms: f64,
    first_arrival_ms: Option<f64>,
    samples: VecDeque<(f64, f64)>,
    slope: f64,
}

impl Trendline {
    fn new(window: usize, smoothing: f64) -> Self {
        Self {
            window: window.max(2),
            smoothing,
            accumulated_ms: 0.0,
            smoothed_ms: 0.0,
            first_arrival_ms: None,
            samples: VecDeque::new(),
            slope: 0.0,
        }
    }

    fn update(&mut self, delta_ms: f64, arrival_ms: f64) {
        let first = *self.first_arrival_ms.get_or_insert(arrival_ms);
        self.accumulated_ms += delta_ms;
        self.smoothed_ms =
            self.smoothing * self.smoothed_ms + (1.0 - self.smoothing) * self.accumulated_ms;
        self.samples
            .push_back((arrival_ms - first, self.smoothed_ms));
        if self.samples.len() > self.window {
            self.samples.pop_front();
        }
        if self.samples.len() == self.window {
            self.slope = Self::fit(&self.samples).unwrap_or(self.slope);
        }
    }

    fn fit(samples: &VecDeque<(f64, f64)>) -> Option<f64> {
        #[allow(clippy::cast_precision_loss)] // window is small
        let n = samples.len() as f64;
        let (sx, sy) = samples
            .iter()
            .fold((0.0, 0.0), |(sx, sy), &(x, y)| (sx + x, sy + y));
        let (mx, my) = (sx / n, sy / n);
        let (num, den) = samples.iter().fold((0.0, 0.0), |(num, den), &(x, y)| {
            (num + (x - mx) * (y - my), den + (x - mx) * (x - mx))
        });
        (den > f64::EPSILON).then(|| num / den)
    }

    fn ready(&self) -> bool {
        self.samples.len() == self.window
    }
}

/// Adaptive-threshold overuse detector.
#[derive(Debug)]
struct OveruseDetector {
    threshold: f64,
    last_update_ms: Option<f64>,
    overuse_ms: f64,
    overuse_count: u32,
    prev_trend: f64,
    state: BandwidthUsage,
}

impl OveruseDetector {
    const K_UP: f64 = 0.0087;
    const K_DOWN: f64 = 0.039;
    const MIN_THRESHOLD: f64 = 6.0;
    const MAX_THRESHOLD: f64 = 600.0;
    const OVERUSE_TIME_MS: f64 = 10.0;

    fn new() -> Self {
        Self {
            threshold: 12.5,
            last_update_ms: None,
            overuse_ms: 0.0,
            overuse_count: 0,
            prev_trend: 0.0,
            state: BandwidthUsage::Normal,
        }
    }

    fn detect(&mut self, trend: f64, group_delta_ms: f64, now_ms: f64) -> BandwidthUsage {
        if trend > self.threshold {
            self.overuse_ms += group_delta_ms.max(0.0);
            self.overuse_count += 1;
            if self.overuse_ms > Self::OVERUSE_TIME_MS
                && self.overuse_count > 1
                && trend >= self.prev_trend
            {
                self.overuse_ms = 0.0;
                self.overuse_count = 0;
                self.state = BandwidthUsage::Overusing;
            }
        } else if trend < -self.threshold {
            self.overuse_ms = 0.0;
            self.overuse_count = 0;
            self.state = BandwidthUsage::Underusing;
        } else {
            self.overuse_ms = 0.0;
            self.overuse_count = 0;
            self.state = BandwidthUsage::Normal;
        }
        self.prev_trend = trend;
        self.adapt_threshold(trend, now_ms);
        self.state
    }

    fn adapt_threshold(&mut self, trend: f64, now_ms: f64) {
        let last = self.last_update_ms.replace(now_ms).unwrap_or(now_ms);
        let abs = trend.abs();
        // Ignore spikes far above the threshold (e.g. route changes).
        if abs > self.threshold + 15.0 {
            return;
        }
        let k = if abs < self.threshold {
            Self::K_DOWN
        } else {
            Self::K_UP
        };
        let dt = (now_ms - last).clamp(0.0, 100.0);
        self.threshold = (self.threshold + k * (abs - self.threshold) * dt)
            .clamp(Self::MIN_THRESHOLD, Self::MAX_THRESHOLD);
    }
}

/// Sliding-window receive-rate meter.
#[derive(Debug)]
struct RateMeter {
    window_us: u64,
    packets: VecDeque<(u64, u32)>,
    bytes: u64,
}

impl RateMeter {
    fn new(window_us: u64) -> Self {
        Self {
            window_us,
            packets: VecDeque::new(),
            bytes: 0,
        }
    }

    fn add(&mut self, arrival_us: u64, size: u32) {
        self.packets.push_back((arrival_us, size));
        self.bytes += u64::from(size);
        let newest = self.packets.iter().map(|p| p.0).max().unwrap_or(arrival_us);
        while let Some(&(t, s)) = self.packets.front() {
            if newest.saturating_sub(t) <= self.window_us {
                break;
            }
            self.bytes -= u64::from(s);
            self.packets.pop_front();
        }
    }

    fn rate_bps(&self) -> Option<f64> {
        let first = self.packets.iter().map(|p| p.0).min()?;
        let last = self.packets.iter().map(|p| p.0).max()?;
        let span = last.saturating_sub(first);
        if self.packets.len() < 2 || span < self.window_us / 4 {
            return None;
        }
        #[allow(clippy::cast_precision_loss)] // byte counts and µs spans fit f64 mantissa
        Some(self.bytes as f64 * 8.0 * 1e6 / span as f64)
    }
}

/// GCC-style estimator; see the module docs.
#[derive(Debug)]
pub struct BandwidthEstimator {
    config: BweConfig,
    current: Option<Group>,
    previous: Option<Group>,
    trendline: Trendline,
    detector: OveruseDetector,
    meter: RateMeter,
    delay_bps: f64,
    loss_bps: f64,
    last_update_us: Option<u64>,
    last_decrease_us: Option<u64>,
    loss_fraction: f64,
}

impl BandwidthEstimator {
    /// Create an estimator; `start_bps` is clamped into `[min_bps, max_bps]`.
    pub fn new(config: BweConfig) -> Self {
        let max = config.max_bps.max(config.min_bps);
        let config = BweConfig {
            max_bps: max,
            start_bps: config.start_bps.clamp(config.min_bps, max),
            ..config
        };
        let start = f64::from(config.start_bps);
        Self {
            config,
            current: None,
            previous: None,
            trendline: Trendline::new(config.trendline_window, config.smoothing),
            detector: OveruseDetector::new(),
            meter: RateMeter::new(500_000),
            delay_bps: start,
            loss_bps: start,
            last_update_us: None,
            last_decrease_us: None,
            loss_fraction: 0.0,
        }
    }

    /// Latest detector state.
    pub fn usage(&self) -> BandwidthUsage {
        self.detector.state
    }

    /// Loss fraction of the last feedback batch.
    pub fn loss_fraction(&self) -> f64 {
        self.loss_fraction
    }

    /// Measured receive rate, when enough data has arrived.
    pub fn receive_rate_bps(&self) -> Option<u32> {
        self.meter.rate_bps().map(to_bps)
    }

    /// Current target bitrate.
    pub fn target_bps(&self) -> u32 {
        to_bps(self.delay_bps.min(self.loss_bps)).clamp(self.config.min_bps, self.config.max_bps)
    }

    /// Process one feedback batch (ordered by send time) and return the new target.
    pub fn on_feedback(&mut self, feedback: &[PacketFeedback], now_us: u64) -> u32 {
        let mut lost_packets = 0usize;
        for packet in feedback {
            match packet.arrival_time_us {
                Some(arrival) => self.on_packet(packet.send_time_us, arrival, packet.size),
                None => lost_packets += 1,
            }
        }
        if !feedback.is_empty() {
            #[allow(clippy::cast_precision_loss)] // batch sizes are small
            let loss = lost_packets as f64 / feedback.len() as f64;
            self.loss_fraction = loss;
            self.update_loss(loss);
        }
        self.update_delay(now_us);
        self.target_bps()
    }

    fn on_packet(&mut self, send_us: u64, arrival_us: u64, size: u32) {
        self.meter.add(arrival_us, size);
        match &mut self.current {
            Some(group)
                if send_us >= group.first_send_us
                    && send_us - group.first_send_us <= self.config.burst_us =>
            {
                group.last_send_us = group.last_send_us.max(send_us);
                group.last_arrival_us = group.last_arrival_us.max(arrival_us);
            }
            Some(group) if send_us < group.first_send_us => {} // reordered into an old group: ignore for delay
            _ => {
                let finished = self.current.replace(Group {
                    first_send_us: send_us,
                    last_send_us: send_us,
                    last_arrival_us: arrival_us,
                });
                if let Some(finished) = finished {
                    self.on_group(finished);
                }
            }
        }
    }

    fn on_group(&mut self, group: Group) {
        if let Some(prev) = self.previous.replace(group) {
            let send_delta = signed_delta_ms(group.last_send_us, prev.last_send_us);
            let arrival_delta = signed_delta_ms(group.last_arrival_us, prev.last_arrival_us);
            let delta = arrival_delta - send_delta;
            #[allow(clippy::cast_precision_loss)] // sub-µs precision is irrelevant at ms scale
            let arrival_ms = group.last_arrival_us as f64 / 1000.0;
            self.trendline.update(delta, arrival_ms);
            if self.trendline.ready() {
                let window = f64::from(u32::try_from(self.trendline.window).unwrap_or(u32::MAX));
                let trend = self.trendline.slope * window.min(60.0) * self.config.threshold_gain;
                self.detector.detect(trend, arrival_delta, arrival_ms);
            }
        }
    }

    fn update_delay(&mut self, now_us: u64) {
        let elapsed_s = self.last_update_us.map_or(0.0, |last| {
            #[allow(clippy::cast_precision_loss)] // result is clamped to ≤ 1 s
            let dt = now_us.saturating_sub(last) as f64 / 1e6;
            dt.min(1.0)
        });
        self.last_update_us = Some(now_us);
        let received = self.meter.rate_bps();
        match self.detector.state {
            BandwidthUsage::Overusing => {
                // At most one decrease per ~RTT-ish interval so a single queue episode
                // is not punished on every batch.
                let recent = self
                    .last_decrease_us
                    .is_some_and(|t| now_us.saturating_sub(t) < 200_000);
                if !recent {
                    let basis = received.unwrap_or(self.delay_bps).min(self.delay_bps);
                    self.delay_bps = self.config.beta * basis;
                    self.last_decrease_us = Some(now_us);
                }
            }
            BandwidthUsage::Normal => {
                let grown = self.delay_bps * (1.0 + self.config.increase_per_s).powf(elapsed_s);
                // Do not run far ahead of what the link demonstrably carries.
                let cap = received.map_or(f64::INFINITY, |r| 1.5 * r + 10_000.0);
                self.delay_bps = grown.min(cap.max(self.delay_bps));
            }
            BandwidthUsage::Underusing => {}
        }
        self.delay_bps = self.clamp(self.delay_bps);
    }

    fn update_loss(&mut self, loss: f64) {
        if loss > 0.10 {
            self.loss_bps *= 1.0 - 0.5 * loss;
        } else if loss < 0.02 {
            // Loss is not limiting: follow the delay-based estimate.
            self.loss_bps = self.loss_bps.max(self.delay_bps);
        }
        self.loss_bps = self.clamp(self.loss_bps);
        if loss > 0.10 {
            // The delay controller must not climb above what loss allows.
            self.delay_bps = self.delay_bps.min(self.loss_bps);
        }
    }

    fn clamp(&self, bps: f64) -> f64 {
        bps.clamp(
            f64::from(self.config.min_bps),
            f64::from(self.config.max_bps),
        )
    }
}

fn signed_delta_ms(a: u64, b: u64) -> f64 {
    #[allow(clippy::cast_precision_loss)] // µs deltas between consecutive groups are small
    if a >= b {
        (a - b) as f64 / 1000.0
    } else {
        -((b - a) as f64 / 1000.0)
    }
}

fn to_bps(bps: f64) -> u32 {
    if bps.is_nan() || bps <= 0.0 {
        0
    } else if bps >= f64::from(u32::MAX) {
        u32::MAX
    } else {
        // Range checked above.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let v = bps as u32;
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Simulated bottleneck link: FIFO queue with fixed capacity, optional random loss.
    struct Link {
        capacity_bps: f64,
        busy_until_us: f64,
        base_delay_us: f64,
        loss: f64,
        rng: u64,
    }

    impl Link {
        fn new(capacity_bps: f64) -> Self {
            Self {
                capacity_bps,
                busy_until_us: 0.0,
                base_delay_us: 20_000.0,
                loss: 0.0,
                rng: 0x9E37_79B9_7F4A_7C15,
            }
        }

        fn send(&mut self, send_us: u64, size: u32) -> PacketFeedback {
            self.rng ^= self.rng << 13;
            self.rng ^= self.rng >> 7;
            self.rng ^= self.rng << 17;
            #[allow(clippy::cast_precision_loss)] // value < 1e6, exact in f64
            let draw = (self.rng % 1_000_000) as f64 / 1e6;
            if draw < self.loss {
                return PacketFeedback::lost(send_us, size);
            }
            #[allow(clippy::cast_precision_loss)] // simulated times are < 2^53 µs
            let now = send_us as f64;
            let start = self.busy_until_us.max(now);
            self.busy_until_us = start + f64::from(size) * 8.0 * 1e6 / self.capacity_bps;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            // positive, sub-µs dropped
            let arrival = (self.busy_until_us + self.base_delay_us) as u64;
            PacketFeedback::received(send_us, arrival, size)
        }
    }

    /// Send at the estimator's target for `seconds`, 60 frames/s, feedback every 50 ms.
    fn run(
        est: &mut BandwidthEstimator,
        link: &mut Link,
        start_us: u64,
        seconds: u64,
    ) -> (u64, Vec<u32>) {
        let frame_us = 16_667;
        let mut t = start_us;
        let end = start_us + seconds * 1_000_000;
        let mut batch = Vec::new();
        let mut targets = Vec::new();
        let mut next_feedback = t + 50_000;
        while t < end {
            let frame_bytes = u64::from(est.target_bps()) * frame_us / 8 / 1_000_000;
            let mut left = frame_bytes;
            let mut offset = 0;
            while left > 0 {
                let size = u32::try_from(left.min(1200)).expect("≤ 1200");
                batch.push(link.send(t + offset, size));
                left -= u64::from(size);
                offset += 50;
            }
            t += frame_us;
            if t >= next_feedback {
                targets.push(est.on_feedback(&batch, t));
                batch.clear();
                next_feedback += 50_000;
            }
        }
        (t, targets)
    }

    fn config() -> BweConfig {
        BweConfig {
            min_bps: 200_000,
            max_bps: 50_000_000,
            start_bps: 1_000_000,
            ..BweConfig::default()
        }
    }

    #[test]
    fn steady_link_converges_toward_capacity() {
        let capacity = 8_000_000.0;
        let mut est = BandwidthEstimator::new(config());
        let mut link = Link::new(capacity);
        let (_, targets) = run(&mut est, &mut link, 0, 60);
        let tail = &targets[targets.len() - 100..];
        #[allow(clippy::cast_precision_loss)] // len is 100
        let avg = tail.iter().map(|&v| f64::from(v)).sum::<f64>() / tail.len() as f64;
        assert!(avg > 0.5 * capacity, "avg {avg} should approach capacity");
        assert!(
            avg < 1.3 * capacity,
            "avg {avg} should not run far above capacity"
        );
    }

    #[test]
    fn grows_from_start_when_link_is_wide() {
        let mut est = BandwidthEstimator::new(config());
        let mut link = Link::new(1e9);
        let (_, targets) = run(&mut est, &mut link, 0, 10);
        let last = *targets.last().expect("targets");
        assert!(
            last > 1_800_000,
            "8%/s for 10 s should give > 1.8 Mbps, got {last}"
        );
        assert!(last < 2_500_000, "growth should be gradual, got {last}");
    }

    #[test]
    fn capacity_drop_triggers_decrease() {
        let mut est = BandwidthEstimator::new(BweConfig {
            start_bps: 10_000_000,
            ..config()
        });
        let mut link = Link::new(12_000_000.0);
        let (t, _) = run(&mut est, &mut link, 0, 5);
        let before = est.target_bps();
        link.capacity_bps = 3_000_000.0;
        let (_, targets) = run(&mut est, &mut link, t, 5);
        let min_after = targets.iter().copied().min().expect("targets");
        assert!(
            min_after < before / 2,
            "before {before}, min after {min_after}"
        );
        assert!(targets.iter().any(|_| true));
        assert!(*targets.last().expect("targets") < 4_500_000);
    }

    #[test]
    fn detects_overuse_from_growing_delay() {
        let mut est = BandwidthEstimator::new(config());
        let mut saw_overuse = false;
        // Each 10 ms group arrives 3 ms later than its send spacing: a building queue.
        for i in 0..200u64 {
            let send = i * 10_000;
            let arrival = 20_000 + i * 13_000;
            est.on_feedback(&[PacketFeedback::received(send, arrival, 1000)], arrival);
            saw_overuse |= est.usage() == BandwidthUsage::Overusing;
        }
        assert!(saw_overuse);
        assert!(est.target_bps() < 1_000_000);
    }

    #[test]
    fn heavy_loss_decreases_and_light_loss_does_not() {
        let mut est = BandwidthEstimator::new(BweConfig {
            start_bps: 5_000_000,
            ..config()
        });
        let mut link = Link::new(1e9);
        link.loss = 0.3;
        let (t, _) = run(&mut est, &mut link, 0, 3);
        let after_loss = est.target_bps();
        assert!(
            after_loss < 2_000_000,
            "30% loss should cut hard, got {after_loss}"
        );
        link.loss = 0.01;
        run(&mut est, &mut link, t, 10);
        assert!(
            est.target_bps() > after_loss,
            "1% loss should allow recovery"
        );
    }

    #[test]
    fn moderate_loss_holds() {
        let mut est = BandwidthEstimator::new(config());
        let batch: Vec<_> = (0..100u64)
            .map(|i| {
                if i % 20 == 0 {
                    PacketFeedback::lost(i * 1000, 1000)
                } else {
                    PacketFeedback::received(i * 1000, i * 1000 + 20_000, 1000)
                }
            })
            .collect();
        est.on_feedback(&batch, 200_000);
        assert!((est.loss_fraction() - 0.05).abs() < 1e-9);
        assert_eq!(est.target_bps(), 1_000_000);
    }

    #[test]
    fn never_leaves_bounds() {
        let cfg = BweConfig {
            min_bps: 300_000,
            max_bps: 3_000_000,
            start_bps: 50_000_000,
            ..config()
        };
        let mut est = BandwidthEstimator::new(cfg);
        assert_eq!(est.target_bps(), 3_000_000, "start is clamped");
        for (capacity, loss) in [(1e9, 0.0), (100_000.0, 0.0), (1e9, 0.9), (50_000.0, 0.5)] {
            let mut link = Link::new(capacity);
            link.loss = loss;
            let (_, targets) = run(&mut est, &mut link, 0, 5);
            assert!(
                targets.iter().all(|&v| (300_000..=3_000_000).contains(&v)),
                "{targets:?}"
            );
        }
    }

    #[test]
    fn tolerates_degenerate_feedback() {
        let mut est = BandwidthEstimator::new(config());
        assert_eq!(est.on_feedback(&[], 0), 1_000_000);
        // Reordered, duplicated and time-travelling packets must not panic.
        let fb = [
            PacketFeedback::received(10_000, 5_000, 0),
            PacketFeedback::received(0, u64::MAX, u32::MAX),
            PacketFeedback::received(u64::MAX, 0, 1),
            PacketFeedback::received(10_000, 5_000, 0),
        ];
        let target = est.on_feedback(&fb, u64::MAX);
        assert!((200_000..=50_000_000).contains(&target));
    }
}
