//! Capture pacing on the host and latest-frame-wins presentation on the client.

/// Decides which captured frames to encode so the output approaches a target fps.
///
/// Uses a credit schedule: the next frame is due at `next_due_us`; a capture at
/// or after that time is taken and the schedule advances by one interval. If the
/// capture source stalled, the schedule resyncs instead of bursting to catch up.
/// A small tolerance (¼ interval) absorbs capture jitter so a 60 Hz source paced
/// to 60 fps is not decimated by timestamp noise.
#[derive(Debug, Clone)]
pub struct FramePacer {
    interval_us: u64,
    next_due_us: Option<u64>,
    captured: u64,
    skipped: u64,
}

/// Pacer verdict for one captured frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaceDecision {
    /// Encode and send this frame.
    Capture,
    /// Drop this frame.
    Skip,
}

impl FramePacer {
    /// Pacer for `fps` frames per second (0 is treated as 1).
    pub fn new(fps: u32) -> Self {
        Self {
            interval_us: interval_for(fps),
            next_due_us: None,
            captured: 0,
            skipped: 0,
        }
    }

    /// Change the target rate; takes effect from the next due time.
    pub fn set_fps(&mut self, fps: u32) {
        self.interval_us = interval_for(fps);
    }

    /// Current frame interval.
    pub fn interval_us(&self) -> u64 {
        self.interval_us
    }

    /// Frames accepted so far.
    pub fn captured(&self) -> u64 {
        self.captured
    }

    /// Frames skipped so far.
    pub fn skipped(&self) -> u64 {
        self.skipped
    }

    /// Force the next frame to be captured (e.g. after a keyframe request).
    pub fn reset(&mut self) {
        self.next_due_us = None;
    }

    /// Decide for a frame captured at `timestamp_us` (monotonic).
    pub fn on_frame(&mut self, timestamp_us: u64) -> PaceDecision {
        let tolerance = self.interval_us / 4;
        let take = match self.next_due_us {
            None => true,
            Some(due) => timestamp_us.saturating_add(tolerance) >= due,
        };
        if !take {
            self.skipped += 1;
            return PaceDecision::Skip;
        }
        self.captured += 1;
        let interval = self.interval_us;
        let next = self
            .next_due_us
            .unwrap_or(timestamp_us)
            .saturating_add(interval);
        // Stalled source or timestamp jump: resync rather than burst.
        let stalled = next.saturating_add(interval) <= timestamp_us;
        let jumped_back = next > timestamp_us.saturating_add(2 * interval);
        self.next_due_us = Some(if stalled || jumped_back {
            timestamp_us.saturating_add(interval)
        } else {
            next
        });
        PaceDecision::Capture
    }
}

fn interval_for(fps: u32) -> u64 {
    1_000_000 / u64::from(fps.max(1))
}

/// Depth-1 queue: a newer frame replaces an unpresented older one.
///
/// The decoder thread calls [`push`](Self::push); the render loop calls
/// [`take`](Self::take) on each vsync. There is no jitter buffer: stale frames
/// are dropped, never queued.
#[derive(Debug)]
pub struct PresentQueue<T> {
    slot: Option<T>,
    replaced: u64,
    presented: u64,
}

impl<T> Default for PresentQueue<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> PresentQueue<T> {
    /// Empty queue.
    pub fn new() -> Self {
        Self {
            slot: None,
            replaced: 0,
            presented: 0,
        }
    }

    /// Offer a decoded frame; returns the older frame it displaced, if any.
    pub fn push(&mut self, frame: T) -> Option<T> {
        let old = self.slot.replace(frame);
        if old.is_some() {
            self.replaced += 1;
        }
        old
    }

    /// Take the frame to present, leaving the queue empty.
    pub fn take(&mut self) -> Option<T> {
        let frame = self.slot.take();
        if frame.is_some() {
            self.presented += 1;
        }
        frame
    }

    /// Whether a frame is waiting.
    pub fn is_ready(&self) -> bool {
        self.slot.is_some()
    }

    /// Frames dropped because a newer one arrived first.
    pub fn replaced(&self) -> u64 {
        self.replaced
    }

    /// Frames handed to the presenter.
    pub fn presented(&self) -> u64 {
        self.presented
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count_captures(
        pacer: &mut FramePacer,
        source_fps: u64,
        seconds: u64,
        jitter_us: u64,
    ) -> u64 {
        let step = 1_000_000 / source_fps;
        let mut taken = 0;
        for i in 0..source_fps * seconds {
            let wobble = if i % 2 == 0 { 0 } else { jitter_us };
            if pacer.on_frame(1_000 + i * step + wobble) == PaceDecision::Capture {
                taken += 1;
            }
        }
        taken
    }

    #[test]
    fn decimates_high_rate_source() {
        let mut p = FramePacer::new(30);
        let taken = count_captures(&mut p, 144, 10, 0);
        assert!((295..=305).contains(&taken), "{taken}");
        assert_eq!(p.captured() + p.skipped(), 1440);
    }

    #[test]
    fn passes_matching_rate_despite_jitter() {
        let mut p = FramePacer::new(60);
        let taken = count_captures(&mut p, 60, 10, 2_000);
        assert_eq!(taken, 600);
    }

    #[test]
    fn halves_60_to_30_evenly() {
        use PaceDecision::{Capture as C, Skip as S};
        let mut p = FramePacer::new(30);
        let decisions: Vec<_> = (0..8u64).map(|i| p.on_frame(i * 16_667)).collect();
        assert_eq!(decisions, vec![C, S, C, S, C, S, C, S]);
    }

    #[test]
    fn slow_source_is_never_throttled_and_does_not_burst_after_stall() {
        let mut p = FramePacer::new(60);
        assert_eq!(count_captures(&mut p, 20, 2, 0), 40);
        // A 2 s stall, then a 120 Hz burst: must not take every frame to "catch up".
        let base = 10_000_000;
        let taken = (0..120u64)
            .filter(|i| p.on_frame(base + i * 8_333) == PaceDecision::Capture)
            .count();
        assert!((58..=62).contains(&taken), "{taken}");
    }

    #[test]
    fn fps_change_and_reset() {
        let mut p = FramePacer::new(60);
        assert_eq!(p.interval_us(), 16_666);
        p.set_fps(15);
        assert_eq!(p.interval_us(), 66_666);
        assert_eq!(p.on_frame(0), PaceDecision::Capture);
        assert_eq!(p.on_frame(10_000), PaceDecision::Skip);
        p.reset();
        assert_eq!(p.on_frame(10_001), PaceDecision::Capture);
        p.set_fps(0);
        assert_eq!(p.interval_us(), 1_000_000);
    }

    #[test]
    fn backwards_timestamps_do_not_panic() {
        let mut p = FramePacer::new(60);
        p.on_frame(u64::MAX - 10);
        p.on_frame(0);
        p.on_frame(u64::MAX);
    }

    #[test]
    fn present_queue_latest_wins() {
        let mut q = PresentQueue::new();
        assert!(!q.is_ready());
        assert_eq!(q.push(1), None);
        assert_eq!(q.push(2), Some(1));
        assert_eq!(q.push(3), Some(2));
        assert!(q.is_ready());
        assert_eq!(q.take(), Some(3));
        assert_eq!(q.take(), None);
        assert_eq!(q.replaced(), 2);
        assert_eq!(q.presented(), 1);
    }
}
