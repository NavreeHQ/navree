//! Rate limits: one algorithm (GCRA), counted where the app says.
//!
//! The per-IP limit and route throttles count in this process
//! ([`KeyedRateLimiter`]) unless `http.rate_limit_store` names a
//! [`RateLimitStore`] every process of the app shares: `redis` with
//! navree-redis's `RedisModule`, or one the app registers under a name of its
//! own. [`KeyedRateLimiter`] is public too, for limits a route can't declare
//! (per OAuth client, per API key).

use std::cell::Cell;
use std::fmt;
use std::hash::Hash;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use dashmap::DashMap;
use navree_kernel::{BoxError, BoxFuture};
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
///
/// It counts in this process: each instance of the app keeps its own counts.
/// The per-IP limit and route throttles
/// ([`RouteDef::throttle`](crate::RouteDef::throttle)) count like this
/// unless `http.rate_limit_store` names a shared [`RateLimitStore`].
///
/// ```
/// use std::time::Duration;
/// use navree_http::KeyedRateLimiter;
///
/// let limiter = KeyedRateLimiter::new(2, Duration::from_secs(60));
/// assert!(limiter.check(&"client-a").is_ok());
/// assert!(limiter.check(&"client-a").is_ok());
/// let limited = limiter.check(&"client-a").unwrap_err();
/// assert!(limited.retry_after > Duration::ZERO);
/// assert!(limiter.check(&"client-b").is_ok());
/// ```
#[derive(Debug)]
pub struct KeyedRateLimiter<K: Hash + Eq> {
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
#[non_exhaustive]
pub struct Limited {
    /// When the key may send again.
    pub retry_after: Duration,
    /// Whether this refusal should be reported (audited): the first one for
    /// the key in a window of `per`, so a flood is reported once a window.
    pub report: bool,
}

impl Limited {
    /// A refusal: the key may send again after `retry_after`, and `report`
    /// says whether it is the first one of its window.
    pub fn new(retry_after: Duration, report: bool) -> Self {
        Self {
            retry_after,
            report,
        }
    }
}

impl Slot {
    fn new(now: Instant) -> Self {
        Self {
            tat: now,
            reported: None,
        }
    }

    /// Counts a request at `now`, one `interval` apart and a burst of
    /// `period / interval`. A refused request does not count.
    fn admit(&mut self, now: Instant, interval: Duration, period: Duration) -> Result<(), Limited> {
        let next = self.tat.max(now) + interval;
        let ahead = next - now;
        if ahead > period {
            let report = self
                .reported
                .is_none_or(|at| now.duration_since(at) >= period);
            if report {
                self.reported = Some(now);
            }
            return Err(Limited::new(ahead - period, report));
        }
        self.tat = next;
        Ok(())
    }
}

/// Whether this thread's turn to sweep expired keys has come.
fn sweep_now() -> bool {
    let checks = CHECKS.get().wrapping_add(1);
    CHECKS.set(checks);
    checks.is_multiple_of(SWEEP_EVERY)
}

/// The interval between requests of a limit of `limit` per `per`.
fn interval(limit: u32, per: Duration) -> Duration {
    per / limit.max(1)
}

/// What a [`RateLimitStore`] says of a request: allowed (`Ok`), or
/// [`Limited`]. The outer error is the store's own: it could not count.
pub type StoreVerdict = Result<Result<(), Limited>, BoxError>;

/// Where the per-IP limit and route throttles count their requests, when
/// every process of the app must count them together: behind a load balancer
/// N servers that each count on their own allow N times the limit.
///
/// `http.rate_limit_store` names the store, as a provider of
/// `dyn RateLimitStore` registered under that name: navree-redis's
/// `RedisModule` registers `redis`. Unset (or `memory`), each process counts
/// on its own, with no store at all. A store that fails or takes longer than
/// 250 ms is not waited for: the request is counted in this process instead,
/// so a limit holds per process while its store is down, and the log says so
/// once a minute.
///
/// Keys are the limit's name and the client's: `http:ip:203.0.113.9` for the
/// per-IP limit, `http:route:POST /api/exports:user:<uuid>` for a route
/// throttle.
pub trait RateLimitStore: Send + Sync + 'static {
    /// Counts a request against `key`: at most `limit` (at least 1) per
    /// `per`, as a burst, then one every `per / limit`, by the generic cell
    /// rate algorithm as [`KeyedRateLimiter`] counts. A refused request does
    /// not count, and [`Limited::report`] is set on the first refusal of `key`
    /// in a window of `per`.
    fn hit<'a>(&'a self, key: &'a str, limit: u32, per: Duration) -> BoxFuture<'a, StoreVerdict>;
}

/// A [`RateLimitStore`] in this process's memory: the limits of every app it
/// is registered in are counted together. For tests, and for several apps
/// in one process.
///
/// ```
/// use std::time::Duration;
/// use navree_http::MemoryRateLimitStore;
///
/// let store = MemoryRateLimitStore::default();
/// let minute = Duration::from_secs(60);
/// assert!(store.check("exports:ada", 1, minute).is_ok());
/// assert!(store.check("exports:ada", 1, minute).is_err());
/// ```
#[derive(Debug, Default)]
pub struct MemoryRateLimitStore {
    state: DashMap<String, Slot>,
}

impl MemoryRateLimitStore {
    /// Counts a request against `key`, as [`RateLimitStore::hit`] does.
    pub fn check(&self, key: &str, limit: u32, per: Duration) -> Result<(), Limited> {
        let now = Instant::now();
        if sweep_now() {
            self.state.retain(|_, slot| slot.tat > now);
        }
        let mut slot = match self.state.get_mut(key) {
            Some(slot) => slot,
            None => self
                .state
                .entry(key.to_owned())
                .or_insert_with(|| Slot::new(now)),
        };
        slot.admit(now, interval(limit, per), per)
    }
}

impl RateLimitStore for MemoryRateLimitStore {
    fn hit<'a>(&'a self, key: &'a str, limit: u32, per: Duration) -> BoxFuture<'a, StoreVerdict> {
        let verdict = self.check(key, limit, per);
        Box::pin(std::future::ready(Ok(verdict)))
    }
}

/// How long a store may take to count a request before the process counts
/// it instead.
const STORE_WAIT: Duration = Duration::from_millis(250);

/// How often a failing store is logged.
const STORE_WARNING_EVERY: Duration = Duration::from_secs(60);

/// One of the app's limits, `limit` per `per` and key: counted in the app's
/// [`RateLimitStore`] under `name` when it has one, else, and while the store
/// fails, in this process.
pub(crate) struct Limiter<K: Hash + Eq> {
    name: String,
    limit: u32,
    per: Duration,
    store: Option<Arc<dyn RateLimitStore>>,
    memory: KeyedRateLimiter<K>,
    warned: Mutex<Option<Instant>>,
}

impl<K: Hash + Eq + Clone + fmt::Display> Limiter<K> {
    pub(crate) fn new(
        name: String,
        limit: u32,
        per: Duration,
        store: Option<Arc<dyn RateLimitStore>>,
    ) -> Self {
        Self {
            name,
            limit,
            per,
            store,
            memory: KeyedRateLimiter::new(limit, per),
            warned: Mutex::new(None),
        }
    }

    /// Counts a request for `key`: [`Limited`] when over the limit.
    pub(crate) async fn check(&self, key: &K) -> Result<(), Limited> {
        let Some(store) = &self.store else {
            return self.memory.check(key);
        };
        let id = format!("{}:{key}", self.name);
        let error =
            match tokio::time::timeout(STORE_WAIT, store.hit(&id, self.limit, self.per)).await {
                Ok(Ok(verdict)) => return verdict,
                Ok(Err(error)) => error.to_string(),
                Err(_) => format!("no answer in {STORE_WAIT:?}"),
            };
        self.warn(&error);
        self.memory.check(key)
    }

    fn warn(&self, error: &str) {
        let now = Instant::now();
        let mut warned = self.warned.lock().unwrap_or_else(PoisonError::into_inner);
        if warned.is_none_or(|at| now.duration_since(at) >= STORE_WARNING_EVERY) {
            *warned = Some(now);
            tracing::warn!(target: "navree::http", limit = %self.name, %error, "the rate limit store could not count a request; this process counts it until the store answers again");
        }
    }
}

impl<K: Hash + Eq> fmt::Debug for Limiter<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Limiter")
            .field("name", &self.name)
            .field("limit", &self.limit)
            .field("per", &self.per)
            .field("shared", &self.store.is_some())
            .finish_non_exhaustive()
    }
}

impl<K: Hash + Eq + Clone> KeyedRateLimiter<K> {
    /// `limit` requests per `per` (a `limit` of 0 is treated as 1).
    pub fn new(limit: u32, per: Duration) -> Self {
        Self {
            interval: interval(limit, per),
            period: per,
            state: DashMap::new(),
        }
    }

    /// Counts a request for `key`: [`Limited`] when over the limit.
    pub fn check(&self, key: &K) -> Result<(), Limited> {
        let now = Instant::now();
        if sweep_now() {
            self.state.retain(|_, slot| slot.tat > now);
        }
        // Holds the lock of the key's shard until the slot is updated.
        let mut slot = self.state.entry(key.clone()).or_insert(Slot::new(now));
        slot.admit(now, self.interval, self.period)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn allows_a_burst_then_refills_one_per_interval() {
        let limiter = KeyedRateLimiter::new(2, Duration::from_secs(60));
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
        let limiter = KeyedRateLimiter::new(1, Duration::from_secs(10));
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
        let limiter = KeyedRateLimiter::new(100, Duration::from_secs(3600));
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
        assert!(
            limiter.check(&"another key").is_ok(),
            "keys are independent"
        );
    }

    /// The store counts every limit by its key, with the limit the request
    /// names, by the same rules.
    #[tokio::test(start_paused = true)]
    async fn the_memory_store_counts_like_the_limiter() {
        let store = MemoryRateLimitStore::default();
        let minute = Duration::from_secs(60);
        assert!(store.check("a", 2, minute).is_ok());
        assert!(store.check("a", 2, minute).is_ok());
        let limited = store.check("a", 2, minute).unwrap_err();
        assert_eq!(limited.retry_after, Duration::from_secs(30));
        assert!(limited.report);
        assert!(
            !store.check("a", 2, minute).unwrap_err().report,
            "once a window"
        );
        assert!(store.check("b", 2, minute).is_ok(), "keys are independent");
        tokio::time::advance(Duration::from_secs(30)).await;
        assert!(store.check("a", 2, minute).is_ok(), "one request refilled");
    }

    /// A store that is down: the limit still holds, in this process.
    struct Down;

    impl RateLimitStore for Down {
        fn hit<'a>(&'a self, _: &'a str, _: u32, _: Duration) -> BoxFuture<'a, StoreVerdict> {
            Box::pin(async { Err("connection refused".into()) })
        }
    }

    /// A store that never answers.
    struct Hung;

    impl RateLimitStore for Hung {
        fn hit<'a>(&'a self, _: &'a str, _: u32, _: Duration) -> BoxFuture<'a, StoreVerdict> {
            Box::pin(std::future::pending())
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_store_that_fails_or_hangs_leaves_the_limit_to_the_process() {
        for store in [Arc::new(Down) as Arc<dyn RateLimitStore>, Arc::new(Hung)] {
            let limiter = Limiter::new(
                "http:ip".to_owned(),
                1,
                Duration::from_secs(60),
                Some(store),
            );
            let started = Instant::now();
            assert!(limiter.check(&"192.0.2.1").await.is_ok());
            assert!(limiter.check(&"192.0.2.1").await.is_err());
            assert!(
                started.elapsed() <= STORE_WAIT * 2,
                "{:?}",
                started.elapsed()
            );
        }
    }

    /// Two limiters on one store are one limit: the store sees the limit's
    /// name and the client's key.
    #[tokio::test(start_paused = true)]
    async fn limiters_on_one_store_share_their_counts() {
        let store: Arc<dyn RateLimitStore> = Arc::new(MemoryRateLimitStore::default());
        let minute = Duration::from_secs(60);
        let here = Limiter::new("http:ip".to_owned(), 2, minute, Some(store.clone()));
        let there = Limiter::new("http:ip".to_owned(), 2, minute, Some(store.clone()));
        let other = Limiter::new("http:route:GET /x".to_owned(), 2, minute, Some(store));
        assert!(here.check(&"192.0.2.1").await.is_ok());
        assert!(there.check(&"192.0.2.1").await.is_ok());
        assert!(here.check(&"192.0.2.1").await.is_err(), "counted together");
        assert!(other.check(&"192.0.2.1").await.is_ok(), "another limit");
        assert!(there.check(&"192.0.2.2").await.is_ok(), "another client");
    }

    #[tokio::test(start_paused = true)]
    async fn rejected_requests_do_not_count() {
        let limiter = KeyedRateLimiter::new(1, Duration::from_secs(10));
        assert!(limiter.check(&1).is_ok());
        for _ in 0..5 {
            assert!(limiter.check(&1).is_err());
        }
        tokio::time::advance(Duration::from_secs(10)).await;
        assert!(limiter.check(&1).is_ok());
    }
}
