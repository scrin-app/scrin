//! Reconnect backoff: exponential from 250 ms to 30 s with jitter.
//!
//! Pure: the caller supplies the randomness, so tests are deterministic and
//! the policy can run anywhere (FFI, wasm) without a runtime.

use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Backoff {
    base: Duration,
    max: Duration,
    /// Fraction of the delay that is randomised, in `0.0..=1.0`.
    jitter: f64,
    attempt: u32,
}

impl Default for Backoff {
    fn default() -> Self {
        Self::new(Duration::from_millis(250), Duration::from_secs(30), 0.2)
    }
}

impl Backoff {
    #[must_use]
    pub fn new(base: Duration, max: Duration, jitter: f64) -> Self {
        Self {
            base,
            max: max.max(base),
            jitter: jitter.clamp(0.0, 1.0),
            attempt: 0,
        }
    }

    /// Attempts made since the last success.
    #[must_use]
    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    /// The un-jittered delay for the current attempt.
    #[must_use]
    pub fn current_ceiling(&self) -> Duration {
        let factor = 1u32.checked_shl(self.attempt.min(31)).unwrap_or(u32::MAX);
        self.base.saturating_mul(factor).min(self.max)
    }

    /// Delay before the next attempt. `unit` is a uniform random in `[0, 1)`;
    /// the result lies in `[ceiling * (1 - jitter), ceiling]`.
    pub fn next_delay(&mut self, unit: f64) -> Duration {
        let ceiling = self.current_ceiling();
        self.attempt = self.attempt.saturating_add(1);
        let unit = if unit.is_finite() {
            unit.clamp(0.0, 1.0)
        } else {
            0.0
        };
        ceiling.mul_f64(1.0 - self.jitter * unit)
    }

    /// Delay using the OS random source for jitter.
    pub fn next_delay_random(&mut self) -> Duration {
        let mut b = [0u8; 4];
        let unit = if getrandom::fill(&mut b).is_ok() {
            f64::from(u32::from_le_bytes(b)) / (f64::from(u32::MAX) + 1.0)
        } else {
            0.5
        };
        self.next_delay(unit)
    }

    /// A connection succeeded: start again from the base delay.
    pub fn reset(&mut self) {
        self.attempt = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grows_exponentially_and_caps() {
        let mut b = Backoff::new(Duration::from_millis(250), Duration::from_secs(30), 0.0);
        let delays: Vec<_> = (0..10).map(|_| b.next_delay(0.5)).collect();
        assert_eq!(delays[0], Duration::from_millis(250));
        assert_eq!(delays[1], Duration::from_millis(500));
        assert_eq!(delays[2], Duration::from_secs(1));
        assert_eq!(delays[6], Duration::from_secs(16));
        assert_eq!(delays[7], Duration::from_secs(30));
        assert_eq!(delays[9], Duration::from_secs(30));
    }

    #[test]
    fn jitter_stays_in_band() {
        let mut b = Backoff::default();
        for _ in 0..5 {
            b.next_delay(0.0);
        }
        let ceiling = b.current_ceiling();
        let lo = b.clone().next_delay(1.0);
        let hi = b.clone().next_delay(0.0);
        assert_eq!(hi, ceiling);
        assert_eq!(lo, ceiling.mul_f64(0.8));
        for _ in 0..100 {
            let d = b.clone().next_delay_random();
            assert!(d >= lo && d <= hi, "{d:?} outside [{lo:?}, {hi:?}]");
        }
    }

    #[test]
    fn reset_returns_to_base() {
        let mut b = Backoff::new(Duration::from_millis(250), Duration::from_secs(30), 0.0);
        for _ in 0..8 {
            b.next_delay(0.0);
        }
        assert_eq!(b.attempt(), 8);
        b.reset();
        assert_eq!(b.next_delay(0.0), Duration::from_millis(250));
    }

    #[test]
    fn huge_attempt_counts_do_not_overflow() {
        let mut b = Backoff::default();
        for _ in 0..1000 {
            assert!(b.next_delay(f64::NAN) <= Duration::from_secs(30));
        }
    }
}
