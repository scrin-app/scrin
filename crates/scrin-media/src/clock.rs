//! NTP-style clock offset and round-trip estimation.
//!
//! For one exchange:
//! * `t0` – client sends request (client clock)
//! * `t1` – server receives it (server clock)
//! * `t2` – server sends reply (server clock)
//! * `t3` – client receives reply (client clock)
//!
//! `rtt = (t3 - t0) - (t2 - t1)` and `offset = ((t1 - t0) + (t2 - t3)) / 2`,
//! where `offset` is what to add to the client clock to get server time.
//!
//! Samples with the smallest RTT carry the least queueing asymmetry, so
//! [`ClockSync`] reports the offset of the minimum-RTT sample among the last
//! `N` exchanges.

use std::collections::VecDeque;

/// The four timestamps of one exchange, in microseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exchange {
    /// Client send time (client clock).
    pub t0: i64,
    /// Server receive time (server clock).
    pub t1: i64,
    /// Server send time (server clock).
    pub t2: i64,
    /// Client receive time (client clock).
    pub t3: i64,
}

/// Offset and round trip derived from one or more exchanges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockSample {
    /// Server clock minus client clock, µs.
    pub offset_us: i64,
    /// Network round-trip time, µs (server processing excluded).
    pub rtt_us: i64,
}

impl Exchange {
    /// Compute offset and RTT; `None` if the timestamps are inconsistent
    /// (negative RTT or server time running backwards) or overflow.
    pub fn sample(&self) -> Option<ClockSample> {
        let total = self.t3.checked_sub(self.t0)?;
        let processing = self.t2.checked_sub(self.t1)?;
        if total < 0 || processing < 0 {
            return None;
        }
        let rtt = total.checked_sub(processing)?;
        if rtt < 0 {
            return None;
        }
        let a = i128::from(self.t1) - i128::from(self.t0);
        let b = i128::from(self.t2) - i128::from(self.t3);
        let offset = i64::try_from((a + b).div_euclid(2)).ok()?;
        Some(ClockSample {
            offset_us: offset,
            rtt_us: rtt,
        })
    }
}

/// Min-RTT filter over the last `capacity` valid exchanges.
#[derive(Debug, Clone)]
pub struct ClockSync {
    capacity: usize,
    samples: VecDeque<ClockSample>,
    rejected: u64,
}

impl ClockSync {
    /// Keep the last `capacity` samples (at least 1).
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            capacity,
            samples: VecDeque::with_capacity(capacity),
            rejected: 0,
        }
    }

    /// Add an exchange; returns the current best estimate.
    pub fn add(&mut self, exchange: Exchange) -> Option<ClockSample> {
        match exchange.sample() {
            Some(sample) => {
                if self.samples.len() == self.capacity {
                    self.samples.pop_front();
                }
                self.samples.push_back(sample);
            }
            None => self.rejected += 1,
        }
        self.estimate()
    }

    /// Sample with the lowest RTT in the window (latest wins on ties).
    pub fn estimate(&self) -> Option<ClockSample> {
        self.samples.iter().rev().min_by_key(|s| s.rtt_us).copied()
    }

    /// Convert a client timestamp to server time with the current estimate.
    pub fn to_server_time(&self, client_us: i64) -> Option<i64> {
        self.estimate()
            .and_then(|s| client_us.checked_add(s.offset_us))
    }

    /// Exchanges rejected as inconsistent.
    pub fn rejected(&self) -> u64 {
        self.rejected
    }

    /// Valid samples currently held.
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Whether no valid sample is held.
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Simulate an exchange with true offset, one-way delays and processing time.
    fn exchange(t0: i64, offset: i64, up: i64, processing: i64, down: i64) -> Exchange {
        let t1 = t0 + up + offset;
        let t2 = t1 + processing;
        let t3 = t2 - offset + down;
        Exchange { t0, t1, t2, t3 }
    }

    #[test]
    fn symmetric_path_gives_exact_offset() {
        let s = exchange(1_000, 50_000, 10_000, 500, 10_000)
            .sample()
            .expect("valid");
        assert_eq!(
            s,
            ClockSample {
                offset_us: 50_000,
                rtt_us: 20_000
            }
        );
    }

    #[test]
    fn asymmetric_path_error_is_half_the_asymmetry() {
        let s = exchange(0, -7_000, 30_000, 0, 10_000)
            .sample()
            .expect("valid");
        assert_eq!(s.rtt_us, 40_000);
        assert_eq!(s.offset_us, -7_000 + 10_000);
    }

    #[test]
    fn inconsistent_exchanges_are_rejected() {
        let mut sync = ClockSync::new(4);
        assert_eq!(
            sync.add(Exchange {
                t0: 10,
                t1: 0,
                t2: 0,
                t3: 5
            }),
            None,
            "client time backwards"
        );
        assert_eq!(
            sync.add(Exchange {
                t0: 0,
                t1: 10,
                t2: 5,
                t3: 20
            }),
            None,
            "server time backwards"
        );
        assert_eq!(
            sync.add(Exchange {
                t0: 0,
                t1: 0,
                t2: 100,
                t3: 10
            }),
            None,
            "processing > total"
        );
        assert_eq!(
            sync.add(Exchange {
                t0: i64::MIN,
                t1: 0,
                t2: 0,
                t3: i64::MAX
            }),
            None,
            "overflow"
        );
        assert_eq!(sync.rejected(), 4);
        assert!(sync.is_empty());
    }

    #[test]
    fn min_rtt_filter_ignores_queued_samples() {
        let mut sync = ClockSync::new(8);
        let offset = 123_456;
        // Clean sample, then samples with heavy one-sided queueing.
        sync.add(exchange(0, offset, 5_000, 100, 5_000));
        for i in 1..6 {
            sync.add(exchange(i * 100_000, offset, 5_000 + 40_000, 100, 5_000));
        }
        let best = sync.estimate().expect("estimate");
        assert_eq!(best.offset_us, offset);
        assert_eq!(best.rtt_us, 10_000);
        assert_eq!(sync.to_server_time(1_000), Some(1_000 + offset));
    }

    #[test]
    fn window_forgets_old_samples() {
        let mut sync = ClockSync::new(3);
        sync.add(exchange(0, 0, 1_000, 0, 1_000)); // best rtt, old offset
        for i in 1..=3 {
            sync.add(exchange(i * 1_000_000, 900, 5_000, 0, 5_000));
        }
        assert_eq!(sync.len(), 3);
        assert_eq!(
            sync.estimate().expect("estimate").offset_us,
            900,
            "clock drift is followed"
        );
    }

    #[test]
    fn zero_capacity_is_one() {
        let mut sync = ClockSync::new(0);
        sync.add(exchange(0, 1, 10, 0, 10));
        sync.add(exchange(100, 2, 20, 0, 20));
        assert_eq!(sync.len(), 1);
        assert_eq!(sync.estimate().expect("estimate").offset_us, 2);
    }

    proptest! {
        /// With symmetric delays the offset is recovered exactly (±1 µs rounding).
        #[test]
        fn symmetric_delay_recovers_offset(
            t0 in -1_000_000_000i64..1_000_000_000,
            offset in -10_000_000_000i64..10_000_000_000,
            delay in 0i64..1_000_000,
            processing in 0i64..100_000,
        ) {
            let s = exchange(t0, offset, delay, processing, delay).sample().expect("valid");
            prop_assert_eq!(s.offset_us, offset);
            prop_assert_eq!(s.rtt_us, 2 * delay);
        }

        /// Error never exceeds half the RTT.
        #[test]
        fn error_bounded_by_half_rtt(
            offset in -1_000_000i64..1_000_000,
            up in 0i64..500_000,
            down in 0i64..500_000,
        ) {
            let s = exchange(0, offset, up, 0, down).sample().expect("valid");
            prop_assert!((s.offset_us - offset).abs() <= s.rtt_us / 2 + 1);
        }
    }
}
