//! Rate limits: one algorithm (GCRA), counted where the app says.
//!
//! The per-IP limit and route throttles count in this process
//! ([`KeyedRateLimiter`]), and when `http.rate_limit_store` names a
//! [`RateLimitStore`] every process of the app shares, there too: `redis`
//! with navree-redis's `RedisModule`, or one the app registers under a name
//! of its own. [`KeyedRateLimiter`] is public too, for limits a route can't declare
//! (per OAuth client, per API key).

use std::cell::Cell;
use std::fmt;
use std::hash::Hash;
use std::sync::atomic::{AtomicU64, Ordering};
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
/// ([`RouteDef::throttle`](crate::RouteDef::throttle)) count like this,
/// and in a shared [`RateLimitStore`] too when `http.rate_limit_store` names
/// one.
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
/// on its own, with no store at all.
///
/// Each process also counts every request itself, first: it sees only some of
/// a client's requests, so its own count never refuses one the shared count
/// would allow, and a flood is refused there without a round trip to the
/// store. A store that fails or takes longer than 100 ms is not waited for,
/// and is left alone for 3 seconds: the process's own count decides, so a
/// limit holds per process while its store is down, a slow store does not
/// slow every request, and the log says so once a minute. Then one request
/// asks the store again, and the others go on without it until it answers.
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

/// How long a store may take to count a request before the process's own
/// count decides.
const STORE_WAIT: Duration = Duration::from_millis(100);

/// How long a store that failed or did not answer is left alone.
const STORE_REST: Duration = Duration::from_secs(3);

/// How often a failing store is logged.
const STORE_WARNING_EVERY: Duration = Duration::from_secs(60);

/// One of the app's limits, `limit` per `per` and key: counted in this
/// process, and then in the app's [`RateLimitStore`] under `name` when it has
/// one and the store is answering.
pub(crate) struct Limiter<K: Hash + Eq> {
    name: String,
    limit: u32,
    per: Duration,
    store: Option<Arc<dyn RateLimitStore>>,
    memory: KeyedRateLimiter<K>,
    breaker: Breaker,
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
            breaker: Breaker::new(),
            warned: Mutex::new(None),
        }
    }

    /// Counts a request for `key`: [`Limited`] when over the limit.
    pub(crate) async fn check(&self, key: &K) -> Result<(), Limited> {
        // Here first, always: see [`RateLimitStore`].
        self.memory.check(key)?;
        let Some(store) = &self.store else {
            return Ok(());
        };
        let Some(turn) = self.breaker.turn() else {
            return Ok(());
        };
        let id = format!("{}:{key}", self.name);
        let error =
            match tokio::time::timeout(STORE_WAIT, store.hit(&id, self.limit, self.per)).await {
                Ok(Ok(verdict)) => {
                    self.breaker.answered(turn);
                    return verdict;
                }
                Ok(Err(error)) => error.to_string(),
                Err(_) => format!("no answer in {STORE_WAIT:?}"),
            };
        self.breaker.failed();
        self.warn(&error);
        Ok(())
    }

    fn warn(&self, error: &str) {
        let now = Instant::now();
        let mut warned = self.warned.lock().unwrap_or_else(PoisonError::into_inner);
        if warned.is_none_or(|at| now.duration_since(at) >= STORE_WARNING_EVERY) {
            *warned = Some(now);
            tracing::warn!(target: "navree::http", limit = %self.name, %error, "the rate limit store could not count a request; this process counts on its own and asks the store again in a few seconds");
        }
    }
}

/// Whether a request asks the store: not for [`STORE_REST`] after it failed
/// or did not answer, then one request at a time until one gets an answer.
/// One atomic, as every request of the per-IP limit looks at it.
#[derive(Debug)]
struct Breaker {
    /// Until when the store is left alone, in milliseconds since `start`
    /// (never 0); 0 while it answers.
    rest_until: AtomicU64,
    start: Instant,
}

/// A request's turn to ask the store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Turn {
    /// The store answers: every request asks it.
    Ask,
    /// The store failed and has rested: this request asks for all of them,
    /// which leave it alone until this one has an answer, or until the
    /// time an answer may take, as given.
    Probe(u64),
}

impl Breaker {
    fn new() -> Self {
        Self {
            rest_until: AtomicU64::new(0),
            start: Instant::now(),
        }
    }

    /// Now, in milliseconds since `start`, from 1.
    fn now(&self) -> u64 {
        let since = u64::try_from(self.start.elapsed().as_millis()).unwrap_or(u64::MAX / 2);
        since.saturating_add(1)
    }

    /// Whether this request asks the store, and how.
    fn turn(&self) -> Option<Turn> {
        let rest_until = self.rest_until.load(Ordering::Acquire);
        if rest_until == 0 {
            return Some(Turn::Ask);
        }
        let now = self.now();
        if now < rest_until {
            return None;
        }
        // A probe that is dropped before its answer gives the next request
        // the turn once its time is up.
        let probing = now.saturating_add(millis(STORE_WAIT));
        self.rest_until
            .compare_exchange(rest_until, probing, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Turn::Probe(probing))
    }

    /// The store answered the request that had `turn`.
    fn answered(&self, turn: Turn) {
        if let Turn::Probe(probing) = turn {
            // Unless another request saw it fail meanwhile.
            let _ = self.rest_until.compare_exchange(
                probing,
                0,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
    }

    /// The store failed or did not answer: it rests from now.
    fn failed(&self) {
        let until = self.now().saturating_add(millis(STORE_REST));
        self.rest_until.store(until, Ordering::Release);
    }
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX / 2)
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
                started.elapsed() <= STORE_WAIT,
                "{:?}",
                started.elapsed()
            );
        }
    }

    /// A store that answers, fails or hangs as it is told, and counts the
    /// requests it was asked to count.
    #[derive(Default)]
    struct Scripted {
        asked: std::sync::atomic::AtomicUsize,
        /// 0 counts, 1 fails, 2 never answers.
        mode: std::sync::atomic::AtomicU8,
        counts: MemoryRateLimitStore,
    }

    const COUNTS: u8 = 0;
    const FAILS: u8 = 1;
    const HANGS: u8 = 2;

    impl Scripted {
        fn set(&self, mode: u8) {
            self.mode.store(mode, Ordering::Relaxed);
        }

        fn asked(&self) -> usize {
            self.asked.load(Ordering::Relaxed)
        }
    }

    impl RateLimitStore for Scripted {
        fn hit<'a>(&'a self, key: &'a str, limit: u32, per: Duration) -> BoxFuture<'a, StoreVerdict> {
            self.asked.fetch_add(1, Ordering::Relaxed);
            match self.mode.load(Ordering::Relaxed) {
                COUNTS => self.counts.hit(key, limit, per),
                FAILS => Box::pin(async { Err("connection refused".into()) }),
                _ => Box::pin(std::future::pending()),
            }
        }
    }

    fn limiter_on(store: &Arc<Scripted>, limit: u32) -> Limiter<&'static str> {
        let store: Arc<dyn RateLimitStore> = store.clone();
        Limiter::new(
            "http:ip".to_owned(),
            limit,
            Duration::from_secs(3600),
            Some(store),
        )
    }

    /// While a store is down or slow, requests do not wait for it: one finds
    /// out, the rest leave it alone for a while, then one asks again.
    #[tokio::test(start_paused = true)]
    async fn a_store_that_failed_is_left_alone_for_a_while_then_asked_again() {
        let store = Arc::new(Scripted::default());
        let limiter = limiter_on(&store, 1000);
        store.set(HANGS);
        let started = Instant::now();
        for _ in 0..50 {
            assert!(limiter.check(&"192.0.2.1").await.is_ok());
        }
        assert_eq!(started.elapsed(), STORE_WAIT, "only the first one waited");
        assert_eq!(store.asked(), 1);

        tokio::time::advance(STORE_REST).await;
        store.set(FAILS);
        assert!(limiter.check(&"192.0.2.1").await.is_ok());
        assert_eq!(store.asked(), 2, "one asks again, and finds it down");
        assert!(limiter.check(&"192.0.2.1").await.is_ok());
        assert_eq!(store.asked(), 2, "so it rests again");

        tokio::time::advance(STORE_REST).await;
        store.set(COUNTS);
        assert!(limiter.check(&"192.0.2.1").await.is_ok());
        assert!(limiter.check(&"192.0.2.1").await.is_ok());
        assert_eq!(store.asked(), 4, "it answers, and every request asks it");
    }

    /// A probe that is dropped before its answer (its client went away)
    /// does not keep the store from being asked again.
    #[tokio::test(start_paused = true)]
    async fn a_probe_that_never_finished_hands_the_turn_on() {
        let store = Arc::new(Scripted::default());
        let limiter = limiter_on(&store, 1000);
        store.set(FAILS);
        assert!(limiter.check(&"192.0.2.1").await.is_ok());
        tokio::time::advance(STORE_REST).await;
        store.set(HANGS);
        let dropped = tokio::time::timeout(STORE_WAIT / 10, limiter.check(&"192.0.2.1")).await;
        assert!(dropped.is_err(), "the probe was asking");
        assert_eq!(store.asked(), 2);
        assert!(limiter.check(&"192.0.2.1").await.is_ok());
        assert_eq!(store.asked(), 2, "the probe has the turn");

        tokio::time::advance(STORE_WAIT).await;
        store.set(COUNTS);
        assert!(limiter.check(&"192.0.2.1").await.is_ok());
        assert_eq!(store.asked(), 3, "its time is up: another asks");
        assert!(limiter.check(&"192.0.2.1").await.is_ok());
        assert_eq!(store.asked(), 4);
    }

    /// The process counts every request too: when the store goes down there
    /// is no fresh burst to hand out, and a flood is refused here without
    /// asking the store at all.
    #[tokio::test(start_paused = true)]
    async fn the_process_counts_every_request_so_a_failing_store_gives_no_fresh_burst() {
        let store = Arc::new(Scripted::default());
        let limiter = limiter_on(&store, 3);
        for _ in 0..3 {
            assert!(limiter.check(&"192.0.2.1").await.is_ok());
        }
        assert_eq!(store.asked(), 3);
        for _ in 0..20 {
            assert!(limiter.check(&"192.0.2.1").await.is_err());
        }
        assert_eq!(store.asked(), 3, "refused here, the store not asked");

        store.set(FAILS);
        assert!(
            limiter.check(&"192.0.2.1").await.is_err(),
            "the client's burst is spent here too"
        );
        assert!(limiter.check(&"192.0.2.2").await.is_ok(), "another client");
    }

    /// The store's count decides when it is lower: other processes spent the
    /// client's requests.
    #[tokio::test(start_paused = true)]
    async fn the_shared_count_refuses_what_this_process_would_allow() {
        let store = Arc::new(Scripted::default());
        let here = limiter_on(&store, 2);
        let there = limiter_on(&store, 2);
        assert!(there.check(&"192.0.2.1").await.is_ok());
        assert!(there.check(&"192.0.2.1").await.is_ok());
        assert!(here.check(&"192.0.2.1").await.is_err(), "spent elsewhere");
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
