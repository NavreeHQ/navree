//! An in-memory keyed rate limiter (GCRA), used for the per-IP limit and
//! per-route throttles.

use std::cell::Cell;
use std::hash::Hash;
use std::time::Duration;

use dashmap::DashMap;
use tokio::time::Instant;

/// How often (in checks on one thread) expired keys are swept.
const SWEEP_EVERY: u64 = 1024;

thread_local! {
    /// Checks made on this thread, counting to the next sweep: a shared
    /// counter would be one more cache line every request writes.
    static CHECKS: Cell<u64> = const { Cell::new(0) };
}

/// Allows `limit` requests per `per` and key, as a burst, refilling one
/// request every `per / limit` (generic cell rate algorithm: one timestamp
/// per key). The keys are sharded, so concurrent requests from different
/// keys rarely wait for each other.
#[derive(Debug)]
pub(crate) struct RateLimiter<K: Hash + Eq> {
    interval: Duration,
    period: Duration,
    state: DashMap<K, Slot>,
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
            state: DashMap::new(),
        }
    }

    /// Counts a request for `key`: [`Limited`] when over the limit.
    pub(crate) fn check(&self, key: &K) -> Result<(), Limited> {
        let now = Instant::now();
        let checks = CHECKS.get().wrapping_add(1);
        CHECKS.set(checks);
        if checks % SWEEP_EVERY == 0 {
            self.state.retain(|_, slot| slot.tat > now);
        }
        // Holds the lock of the key's shard until the slot is updated.
        let mut slot = self.state.entry(key.clone()).or_insert(Slot {
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

    /// Checks of one key from many threads at once allow exactly the burst.
    #[test]
    fn concurrent_checks_never_allow_more_than_the_limit() {
        let limiter = RateLimiter::new(100, Duration::from_secs(3600));
        let allowed = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    for _ in 0..50 {
                        if limiter.check(&"shared").is_ok() {
                            allowed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                });
            }
        });
        assert_eq!(allowed.into_inner(), 100);
        assert!(limiter.check(&"another key").is_ok(), "keys are independent");
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
