//! Token-bucket rate limiting and the pairing-failure lockout.
//!
//! Both are pure: time is passed in, so tests are deterministic and the
//! server can use `Instant::now()`.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Bucket parameters: `burst` tokens, refilled at `per_sec` tokens/second.
#[derive(Debug, Clone, Copy)]
pub struct Rate {
    pub burst: f64,
    pub per_sec: f64,
}

impl Rate {
    #[must_use]
    pub const fn new(burst: f64, per_sec: f64) -> Self {
        Self { burst, per_sec }
    }
}

#[derive(Debug, Clone, Copy)]
struct Bucket {
    tokens: f64,
    last: Instant,
}

/// One token bucket per key. Idle full buckets are pruned periodically so the
/// map cannot grow without bound under a spray of source addresses.
#[derive(Debug)]
pub struct RateLimiter<K> {
    rate: Rate,
    buckets: Mutex<HashMap<K, Bucket>>,
    max_keys: usize,
}

impl<K: Eq + Hash + Clone> RateLimiter<K> {
    #[must_use]
    pub fn new(rate: Rate, max_keys: usize) -> Self {
        Self {
            rate,
            buckets: Mutex::new(HashMap::new()),
            max_keys,
        }
    }

    /// Takes one token for `key`; `false` means rate limited.
    pub fn check(&self, key: &K, now: Instant) -> bool {
        let mut map = self
            .buckets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if map.len() >= self.max_keys && !map.contains_key(key) {
            let rate = self.rate;
            map.retain(|_, b| refill(rate, *b, now).tokens < rate.burst);
            if map.len() >= self.max_keys {
                // Still full of active keys: fail closed for newcomers.
                return false;
            }
        }
        let entry = map.entry(key.clone()).or_insert(Bucket {
            tokens: self.rate.burst,
            last: now,
        });
        *entry = refill(self.rate, *entry, now);
        if entry.tokens >= 1.0 {
            entry.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

fn refill(rate: Rate, b: Bucket, now: Instant) -> Bucket {
    let dt = now.saturating_duration_since(b.last).as_secs_f64();
    Bucket {
        tokens: (b.tokens + dt * rate.per_sec).min(rate.burst),
        last: now,
    }
}

/// Failure counting per target ID: `threshold` reports inside `window` lock the
/// target for `lock_for` (anonymous resolves get 429).
#[derive(Debug)]
pub struct Lockout {
    threshold: usize,
    window: Duration,
    lock_for: Duration,
    state: Mutex<HashMap<u64, LockState>>,
}

#[derive(Debug, Default)]
struct LockState {
    failures: Vec<Instant>,
    locked_until: Option<Instant>,
}

impl Lockout {
    #[must_use]
    pub fn new(threshold: usize, window: Duration, lock_for: Duration) -> Self {
        Self {
            threshold,
            window,
            lock_for,
            state: Mutex::new(HashMap::new()),
        }
    }

    /// The ADR-0009 default: 5 failures in 10 minutes lock for 10 minutes.
    #[must_use]
    pub fn standard() -> Self {
        Self::new(5, Duration::from_secs(600), Duration::from_secs(600))
    }

    /// Records one failed pairing; returns `true` if the target is now locked.
    pub fn record_failure(&self, id: u64, now: Instant) -> bool {
        let mut map = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let st = map.entry(id).or_default();
        let window = self.window;
        st.failures
            .retain(|t| now.saturating_duration_since(*t) < window);
        st.failures.push(now);
        if st.failures.len() >= self.threshold {
            st.locked_until = Some(now + self.lock_for);
            st.failures.clear();
        }
        st.locked_until.is_some_and(|u| now < u)
    }

    #[must_use]
    pub fn is_locked(&self, id: u64, now: Instant) -> bool {
        let mut map = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(st) = map.get_mut(&id) else {
            return false;
        };
        match st.locked_until {
            Some(u) if now < u => true,
            Some(_) => {
                st.locked_until = None;
                false
            }
            None => false,
        }
    }

    /// Drops state with no recent failures and no active lock.
    pub fn prune(&self, now: Instant) {
        let window = self.window;
        let mut map = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        map.retain(|_, st| {
            st.failures
                .retain(|t| now.saturating_duration_since(*t) < window);
            !st.failures.is_empty() || st.locked_until.is_some_and(|u| now < u)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_allows_burst_then_limits_then_refills() {
        let rl = RateLimiter::new(Rate::new(3.0, 1.0), 100);
        let t0 = Instant::now();
        assert!(rl.check(&"a", t0));
        assert!(rl.check(&"a", t0));
        assert!(rl.check(&"a", t0));
        assert!(!rl.check(&"a", t0));
        // Other keys are independent.
        assert!(rl.check(&"b", t0));
        // One second refills one token.
        assert!(rl.check(&"a", t0 + Duration::from_secs(1)));
        assert!(!rl.check(&"a", t0 + Duration::from_secs(1)));
        // Refill is capped at burst.
        let later = t0 + Duration::from_secs(100);
        for _ in 0..3 {
            assert!(rl.check(&"a", later));
        }
        assert!(!rl.check(&"a", later));
    }

    #[test]
    fn full_map_prunes_idle_keys_and_fails_closed_when_all_active() {
        let rl = RateLimiter::new(Rate::new(2.0, 1.0), 2);
        let t0 = Instant::now();
        assert!(rl.check(&1, t0));
        assert!(rl.check(&2, t0));
        // Both keys are active (not full): newcomer is refused.
        assert!(!rl.check(&3, t0));
        // After both refilled to full, they are pruned and 3 gets in.
        assert!(rl.check(&3, t0 + Duration::from_secs(5)));
    }

    #[test]
    fn five_failures_in_window_lock_the_target() {
        let lo = Lockout::standard();
        let t0 = Instant::now();
        for i in 0..4 {
            assert!(!lo.record_failure(7, t0 + Duration::from_secs(i)));
        }
        assert!(!lo.is_locked(7, t0 + Duration::from_secs(4)));
        assert!(lo.record_failure(7, t0 + Duration::from_secs(5)));
        assert!(lo.is_locked(7, t0 + Duration::from_secs(6)));
        assert!(!lo.is_locked(8, t0 + Duration::from_secs(6)));
        // Unlocks after 10 minutes.
        assert!(lo.is_locked(7, t0 + Duration::from_secs(604)));
        assert!(!lo.is_locked(7, t0 + Duration::from_secs(606)));
    }

    #[test]
    fn failures_outside_window_do_not_count() {
        let lo = Lockout::standard();
        let t0 = Instant::now();
        for i in 0..4 {
            lo.record_failure(7, t0 + Duration::from_secs(i));
        }
        // 11 minutes later the first four have expired.
        assert!(!lo.record_failure(7, t0 + Duration::from_mins(11)));
        assert!(!lo.is_locked(7, t0 + Duration::from_secs(661)));
        lo.prune(t0 + Duration::from_secs(2000));
        assert!(!lo.is_locked(7, t0 + Duration::from_secs(2000)));
    }
}
