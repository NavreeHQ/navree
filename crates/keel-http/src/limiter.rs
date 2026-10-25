//! An in-memory keyed rate limiter (GCRA), used for the per-IP limit and
//! per-route throttles.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use tokio::time::Instant;

/// How often (in checks) expired keys are swept.
const SWEEP_EVERY: u64 = 1024;

/// Allows `limit` requests per `per` and key, as a burst, refilling one
/// request every `per / limit` (generic cell rate algorithm: one timestamp
/// per key).
#[derive(Debug)]
pub(crate) struct RateLimiter<K> {
    interval: Duration,
    period: Duration,
    /// Theoretical arrival time of the next request, per key.
    state: Mutex<HashMap<K, Instant>>,
    checks: AtomicU64,
}

impl<K: Hash + Eq + Clone> RateLimiter<K> {
    /// `limit` requests per `per` (a `limit` of 0 is treated as 1).
    pub(crate) fn new(limit: u32, per: Duration) -> Self {
        Self {
            interval: per / limit.max(1),
            period: per,
            state: Mutex::new(HashMap::new()),
            checks: AtomicU64::new(0),
        }
    }

    /// Counts a request for `key`: `Err(retry_after)` when over the limit.
    pub(crate) fn check(&self, key: &K) -> Result<(), Duration> {
        let now = Instant::now();
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if self.checks.fetch_add(1, Ordering::Relaxed) % SWEEP_EVERY == SWEEP_EVERY - 1 {
            state.retain(|_, tat| *tat > now);
        }
        let tat = state.get(key).copied().unwrap_or(now).max(now);
        let next = tat + self.interval;
        let ahead = next - now;
        if ahead > self.period {
            return Err(ahead - self.period);
        }
        state.insert(key.clone(), next);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn allows_a_burst_then_refills_one_per_interval() {
        let limiter = RateLimiter::new(2, Duration::from_secs(60));
        assert!(limiter.check(&"a").is_ok());
        assert!(limiter.check(&"a").is_ok());
        let retry = limiter.check(&"a").unwrap_err();
        assert_eq!(retry, Duration::from_secs(30));
        assert!(limiter.check(&"b").is_ok(), "keys are independent");

        tokio::time::advance(Duration::from_secs(30)).await;
        assert!(limiter.check(&"a").is_ok(), "one request refilled");
        assert!(limiter.check(&"a").is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn rejected_requests_do_not_count() {
        let limiter = RateLimiter::new(1, Duration::from_secs(10));
        assert!(limiter.check(&1).is_ok());
        for _ in 0..5 {
            assert!(limiter.check(&1).is_err());
        }
        tokio::time::advance(Duration::from_secs(10)).await;
        assert!(limiter.check(&1).is_ok());
    }
}
