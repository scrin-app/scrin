//! Per-session limits: maximum duration, idle timeout, bandwidth and byte caps.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use super::code;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_session: Duration,
    pub idle: Duration,
    /// Bytes per second across both directions; 0 = unlimited.
    pub max_bps: u64,
    /// Total bytes; 0 = unlimited.
    pub max_bytes: u64,
}

#[derive(Debug)]
pub struct Quota {
    limits: Limits,
    start: Instant,
    /// Milliseconds since `start` of the last forwarded byte.
    last_ms: AtomicU64,
    bytes: AtomicU64,
    /// Token bucket (tokens may go negative; the debt is paid by waiting).
    bucket: Mutex<(f64, Instant)>,
}

impl Quota {
    #[must_use]
    pub fn new(limits: Limits, now: Instant) -> Self {
        #[allow(clippy::cast_precision_loss)] // bandwidth caps are far below 2^52
        let burst = limits.max_bps as f64;
        Self {
            limits,
            start: now,
            last_ms: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
            bucket: Mutex::new((burst, now)),
        }
    }

    fn touch(&self, now: Instant) {
        let ms = u64::try_from(now.saturating_duration_since(self.start).as_millis())
            .unwrap_or(u64::MAX);
        self.last_ms.fetch_max(ms, Ordering::Relaxed);
    }

    /// Total bytes forwarded so far.
    #[must_use]
    pub fn bytes(&self) -> u64 {
        self.bytes.load(Ordering::Relaxed)
    }

    /// Accounts `n` bytes; returns the delay the caller must wait before
    /// forwarding (bandwidth cap), or a close code when the byte cap is hit.
    pub fn charge(&self, n: usize, now: Instant) -> Result<Duration, u32> {
        self.touch(now);
        let n64 = n as u64;
        let total = self.bytes.fetch_add(n64, Ordering::Relaxed) + n64;
        if self.limits.max_bytes > 0 && total > self.limits.max_bytes {
            return Err(code::QUOTA);
        }
        if self.limits.max_bps == 0 {
            return Ok(Duration::ZERO);
        }
        #[allow(clippy::cast_precision_loss)]
        let (rate, cost) = (self.limits.max_bps as f64, n as f64);
        let mut b = self
            .bucket
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let dt = now.saturating_duration_since(b.1).as_secs_f64();
        b.0 = (b.0 + dt * rate).min(rate) - cost;
        b.1 = now;
        if b.0 >= 0.0 {
            Ok(Duration::ZERO)
        } else {
            Ok(Duration::from_secs_f64(-b.0 / rate))
        }
    }

    /// Stream bytes: waits out the bandwidth cap.
    pub async fn spend(&self, n: usize) -> Result<(), u32> {
        let wait = self.charge(n, Instant::now())?;
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
        Ok(())
    }

    /// Datagrams are never delayed: `Ok(false)` means drop this one.
    pub fn spend_datagram(&self, n: usize) -> Result<bool, u32> {
        Ok(self.charge(n, Instant::now())?.is_zero())
    }

    /// A close code if the session must end now.
    #[must_use]
    pub fn expired(&self, now: Instant) -> Option<u32> {
        let age = now.saturating_duration_since(self.start);
        if age >= self.limits.max_session {
            return Some(code::SESSION_LIMIT);
        }
        let last = Duration::from_millis(self.last_ms.load(Ordering::Relaxed));
        if age.saturating_sub(last) >= self.limits.idle {
            return Some(code::IDLE);
        }
        if self.limits.max_bytes > 0 && self.bytes() > self.limits.max_bytes {
            return Some(code::QUOTA);
        }
        None
    }

    /// Resolves with a close code once [`Self::expired`] fires (1 s resolution).
    pub async fn watchdog(&self) -> u32 {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tick.tick().await;
            if let Some(c) = self.expired(Instant::now()) {
                return c;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> Limits {
        Limits {
            max_session: Duration::from_secs(3600),
            idle: Duration::from_secs(120),
            max_bps: 1000,
            max_bytes: 0,
        }
    }

    #[test]
    fn session_limit_and_idle_fire() {
        let t0 = Instant::now();
        let q = Quota::new(limits(), t0);
        assert_eq!(q.expired(t0 + Duration::from_secs(60)), None);
        assert_eq!(q.expired(t0 + Duration::from_secs(121)), Some(code::IDLE));
        q.charge(1, t0 + Duration::from_secs(119)).expect("charge");
        assert_eq!(q.expired(t0 + Duration::from_secs(200)), None);
        assert_eq!(
            q.expired(t0 + Duration::from_secs(3600)),
            Some(code::SESSION_LIMIT)
        );
    }

    #[test]
    fn bandwidth_cap_delays_and_datagrams_drop() {
        let t0 = Instant::now();
        let q = Quota::new(limits(), t0);
        assert_eq!(q.charge(1000, t0), Ok(Duration::ZERO));
        let wait = q.charge(500, t0).expect("charge");
        assert!((wait.as_secs_f64() - 0.5).abs() < 1e-6, "{wait:?}");
        // Bucket is in debt: a datagram now is dropped.
        assert_eq!(q.spend_datagram(10), Ok(false));
    }

    #[test]
    fn byte_cap_closes() {
        let t0 = Instant::now();
        let q = Quota::new(
            Limits {
                max_bytes: 100,
                max_bps: 0,
                ..limits()
            },
            t0,
        );
        assert!(q.charge(100, t0).is_ok());
        assert_eq!(q.charge(1, t0), Err(code::QUOTA));
        assert_eq!(q.expired(t0), Some(code::QUOTA));
    }
}
