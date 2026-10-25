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
    state: Mutex<HashMap<K, Slot>>,
    checks: AtomicU64,
}

/// A key's state.
#[derive(Clone, Copy, Debug)]
struct Slot {
    /// Theoretical arrival time of the next request.
    tat: Instant,
    /// When a refusal was last reported.
    reported: Option<Instant>,
}

/// A refused request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Limited {
    /// When the key may send again.
    pub(crate) retry_after: Duration,
    /// Whether this refusal should be reported (audited): the first one for
    /// the key in a window of `per`, so a flood is reported once a window.
    pub(crate) report: bool,
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

    /// Counts a request for `key`: [`Limited`] when over the limit.
    pub(crate) fn check(&self, key: &K) -> Result<(), Limited> {
        let now = Instant::now();
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if self.checks.fetch_add(1, Ordering::Relaxed) % SWEEP_EVERY == SWEEP_EVERY - 1 {
            state.retain(|_, slot| slot.tat > now);
        }
        let slot = state.entry(key.clone()).or_insert(Slot {
            tat: now,
            reported: None,
        });
        let next = slot.tat.max(now) + self.interval;
        let ahead = next - now;
        if ahead > self.period {
            let report = slot
                .reported
                .is_none_or(|at| now.duration_since(at) >= self.period);
            if report {
                slot.reported = Some(now);
            }
            return Err(Limited {
                retry_after: ahead - self.period,
                report,
            });
        }
        slot.tat = next;
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
        assert_eq!(retry.retry_after, Duration::from_secs(30));
        assert!(limiter.check(&"b").is_ok(), "keys are independent");

        tokio::time::advance(Duration::from_secs(30)).await;
        assert!(limiter.check(&"a").is_ok(), "one request refilled");
        assert!(limiter.check(&"a").is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn refusals_are_reported_once_per_window() {
        let limiter = RateLimiter::new(1, Duration::from_secs(10));
        assert!(limiter.check(&1).is_ok());
        assert!(limiter.check(&1).unwrap_err().report);
        assert!(!limiter.check(&1).unwrap_err().report);
        assert!(limiter.check(&2).is_ok(), "keys are independent");
        assert!(limiter.check(&2).unwrap_err().report);
        tokio::time::advance(Duration::from_secs(10)).await;
        assert!(limiter.check(&1).is_ok());
        assert!(limiter.check(&1).unwrap_err().report, "a new window");
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
