//! [`with_backoff`]: an attempt that failed in a way that says "try again"
//! is run again, after a delay that grows.
//!
//! What is run again is decided by the failure
//! ([`Error::retryable`](super::Error::retryable)): a connection that could
//! not be made, a `408`, `429` or `5xx` answer, a provider that says to try
//! again, and, for a request whose second arrival does no harm
//! ([`Replay::Safe`]), a timeout or a dropped connection. How often and how
//! long is the [`RetryPolicy`], whose defaults are the counts the cloud
//! disks have always had.

use std::future::Future;
use std::time::{Duration, Instant};

use super::Result;

/// How often and for how long a request is tried again.
///
/// ```
/// use std::time::Duration;
///
/// use navree_storage::cloud::retry::RetryPolicy;
///
/// let policy = RetryPolicy::default();
/// assert_eq!(policy.max_retries, 10);
/// assert_eq!(policy.retry_timeout, Duration::from_secs(180));
/// assert_eq!(policy.first_delay, Duration::from_millis(100));
/// assert_eq!(policy.max_delay, Duration::from_secs(15));
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct RetryPolicy {
    /// The most times an attempt is run again after the first (default
    /// 10); 0 runs it once.
    pub max_retries: u32,
    /// How long after the first attempt started no new one is started
    /// (default 3 minutes). An attempt under way is not cut short: each
    /// has the request's own timeout.
    pub retry_timeout: Duration,
    /// The delay before the first retry (default 100 ms), and the least
    /// before any.
    pub first_delay: Duration,
    /// The longest delay before a retry (default 15 s).
    pub max_delay: Duration,
    /// How fast the delay may grow: each one is drawn at random between
    /// [`first_delay`](Self::first_delay) and this many times the delay
    /// before it (default 2).
    pub factor: f64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 10,
            retry_timeout: Duration::from_secs(3 * 60),
            first_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(15),
            factor: 2.0,
        }
    }
}

/// Whether a request may arrive twice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Replay {
    /// A second arrival does no harm: a read, or a write that leaves the
    /// same thing when it is done twice (`PUT` of an object, a part or a
    /// block; `DELETE`; the request that completes an upload). It is sent
    /// again after any failure worth it, also when nobody knows whether
    /// the first arrived (the time ran out, the connection dropped).
    Safe,
    /// A second arrival could do something twice. It is sent again only
    /// when the first cannot have been carried out: no connection was
    /// made, or the provider answered with a status that says to try
    /// again.
    IfNotSent,
}

/// Runs `attempt`, and runs it again while it fails in a way worth trying
/// again ([`Error::retryable`](super::Error::retryable) with `replay`),
/// within `policy`. It gives the first success, or the last failure once
/// the failure is not worth another attempt, the retries are used up or
/// [`RetryPolicy::retry_timeout`] has passed.
///
/// `attempt` is the whole of one try: it asks for the credential, builds
/// and signs the request, sends it and turns an answer that is not a
/// success into a failure ([`Response::success`](super::http::Response::success)).
/// So every try is signed afresh, with a credential that is still good,
/// however long the tries before it took.
///
/// ```no_run
/// use navree_storage::cloud::http::{Client, Request};
/// use navree_storage::cloud::retry::{Replay, RetryPolicy, with_backoff};
/// use navree_storage::store::{Method, Url};
///
/// # async fn example(client: &Client, url: &Url) -> navree_storage::cloud::Result<()> {
/// let policy = RetryPolicy::default();
/// let answer = with_backoff(&policy, Replay::Safe, || async {
///     let request = Request::new(Method::GET, url.clone());
///     client.send(request).await?.success().await
/// })
/// .await?;
/// # drop(answer);
/// # Ok(())
/// # }
/// ```
pub async fn with_backoff<T, F, Fut>(
    policy: &RetryPolicy,
    replay: Replay,
    mut attempt: F,
) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let mut budget = Budget::new(policy);
    loop {
        let error = match attempt().await {
            Ok(value) => return Ok(value),
            Err(error) => error,
        };
        if !error.retryable(replay) {
            return Err(error);
        }
        let Some(delay) = budget.next_delay(policy) else {
            return Err(error);
        };
        tracing::info!(
            kind = ?error.kind(),
            status = error.status().map(|status| status.as_u16()),
            retry = budget.retries(),
            max_retries = policy.max_retries,
            delay_ms = u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
            "a request to a cloud store failed and is sent again"
        );
        tokio::time::sleep(delay).await;
    }
}

/// What is left of a [`RetryPolicy`] for one thing that is tried again: a
/// request, or the rest of a body that stopped.
pub(super) struct Budget {
    started: Instant,
    retries: u32,
    delays: Delays,
}

impl Budget {
    /// The whole of `policy`, counted from now.
    pub(super) fn new(policy: &RetryPolicy) -> Self {
        Self {
            started: Instant::now(),
            retries: 0,
            delays: Delays::new(policy, rand::random::<f64>),
        }
    }

    /// How long to wait before the next try, or `None` when `policy`
    /// allows no other: its retries are used up, or its time has passed.
    pub(super) fn next_delay(&mut self, policy: &RetryPolicy) -> Option<Duration> {
        if self.retries >= policy.max_retries || self.started.elapsed() > policy.retry_timeout {
            return None;
        }
        self.retries += 1;
        Some(self.delays.next())
    }

    /// How many times it was tried again.
    pub(super) fn retries(&self) -> u32 {
        self.retries
    }
}

/// The delays between the attempts of one request: the first is
/// `first_delay`, and each one after is drawn between `first_delay` and
/// `factor` times the one before, and is at most `max_delay`. Drawing them
/// keeps clients that failed together from trying again together.
struct Delays {
    first: f64,
    next: f64,
    max: f64,
    factor: f64,
    /// A number in `[0, 1)`: how far between the least and the most the
    /// next delay lies.
    share: fn() -> f64,
}

impl Delays {
    fn new(policy: &RetryPolicy, share: fn() -> f64) -> Self {
        let first = policy.first_delay.as_secs_f64();
        Self {
            first,
            next: first,
            max: policy.max_delay.as_secs_f64().max(first),
            factor: policy.factor,
            share,
        }
    }

    /// The delay before the next retry.
    fn next(&mut self) -> Duration {
        let delay = self.next;
        // `f64::max` passes over a factor that is not a number, and a share
        // that is not one counts as none: the delay is then the first one.
        let most = (delay * self.factor).max(self.first);
        let share = (self.share)();
        let share = if share.is_nan() {
            0.0
        } else {
            share.clamp(0.0, 1.0)
        };
        self.next = (self.first + (most - self.first) * share).min(self.max);
        Duration::try_from_secs_f64(delay).unwrap_or(Duration::ZERO)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::super::{Error, ErrorKind};
    use super::*;

    const fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn policy(first: u64, max: u64, factor: f64) -> RetryPolicy {
        RetryPolicy {
            first_delay: ms(first),
            max_delay: ms(max),
            factor,
            ..RetryPolicy::default()
        }
    }

    fn delays(policy: &RetryPolicy, share: fn() -> f64, count: usize) -> Vec<Duration> {
        let mut delays = Delays::new(policy, share);
        (0..count).map(|_| delays.next()).collect()
    }

    fn close(actual: &[Duration], expected: &[u64]) {
        assert_eq!(actual.len(), expected.len(), "{actual:?}");
        for (actual, expected) in actual.iter().zip(expected) {
            let difference = actual.as_secs_f64() - ms(*expected).as_secs_f64();
            assert!(
                difference.abs() < 0.000_5,
                "{actual:?} is not {expected} ms"
            );
        }
    }

    #[test]
    fn the_defaults_are_the_counts_the_cloud_disks_have_always_had() {
        assert_eq!(
            RetryPolicy::default(),
            RetryPolicy {
                max_retries: 10,
                retry_timeout: Duration::from_secs(180),
                first_delay: ms(100),
                max_delay: ms(15_000),
                factor: 2.0,
            }
        );
    }

    #[test]
    fn delays_start_at_the_first_and_grow_up_to_the_longest() {
        // The largest draw: each delay is `factor` times the one before.
        close(
            &delays(&policy(100, 15_000, 2.0), || 1.0, 10),
            &[
                100, 200, 400, 800, 1_600, 3_200, 6_400, 12_800, 15_000, 15_000,
            ],
        );
        // The smallest draw: every delay is the first.
        close(&delays(&policy(100, 15_000, 2.0), || 0.0, 4), &[100; 4]);
        // Halfway between the first and twice the one before.
        close(
            &delays(&policy(100, 15_000, 2.0), || 0.5, 4),
            &[100, 150, 200, 250],
        );
    }

    #[test]
    fn a_policy_that_makes_no_sense_still_gives_delays() {
        // A longest delay under the first is the first.
        close(&delays(&policy(100, 10, 2.0), || 1.0, 3), &[100, 100, 100]);
        // A factor under one, or one that is not a number, never shrinks a
        // delay under the first.
        close(&delays(&policy(100, 15_000, 0.5), || 1.0, 3), &[100; 3]);
        close(
            &delays(&policy(100, 15_000, f64::NAN), || 1.0, 3),
            &[100; 3],
        );
        // A share outside zero to one is held to it.
        close(
            &delays(&policy(100, 15_000, 2.0), || 7.0, 3),
            &[100, 200, 400],
        );
        close(&delays(&policy(100, 15_000, 2.0), || -3.0, 3), &[100; 3]);
        close(
            &delays(&policy(100, 15_000, 2.0), || f64::NAN, 3),
            &[100; 3],
        );
        // No delay at all.
        assert_eq!(delays(&policy(0, 0, 2.0), || 1.0, 3), [Duration::ZERO; 3]);
    }

    #[test]
    fn random_delays_stay_between_the_first_and_the_longest() {
        let policy = policy(100, 1_000, 2.0);
        for _ in 0..200 {
            let mut delays = Delays::new(&policy, rand::random::<f64>);
            let mut before = delays.next();
            assert_eq!(before, ms(100));
            for _ in 0..8 {
                let delay = delays.next();
                assert!(delay >= ms(100) && delay <= ms(1_000), "{delay:?}");
                assert!(delay <= before * 2 + ms(1), "{before:?} then {delay:?}");
                before = delay;
            }
        }
    }

    /// A policy whose delays are too short to notice.
    fn quick(max_retries: u32) -> RetryPolicy {
        RetryPolicy {
            max_retries,
            first_delay: ms(1),
            max_delay: ms(2),
            ..RetryPolicy::default()
        }
    }

    #[tokio::test]
    async fn an_attempt_is_run_again_until_it_succeeds() {
        let attempts = AtomicU32::new(0);
        let value = with_backoff(&quick(5), Replay::IfNotSent, || async {
            match attempts.fetch_add(1, Ordering::SeqCst) {
                0 => Err(Error::new(ErrorKind::Connect, "no connection")),
                1 => Err(Error::busy("SlowDown")),
                attempt => Ok(attempt),
            }
        })
        .await
        .unwrap();
        assert_eq!(value, 2);
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn the_last_failure_is_given_when_the_retries_are_used_up() {
        for max_retries in [0, 1, 3] {
            let attempts = AtomicU32::new(0);
            let error = with_backoff(&quick(max_retries), Replay::Safe, || async {
                let attempt = attempts.fetch_add(1, Ordering::SeqCst);
                Err::<(), _>(Error::busy(format!("attempt {attempt}")))
            })
            .await
            .unwrap_err();
            assert_eq!(attempts.load(Ordering::SeqCst), max_retries + 1);
            assert_eq!(error.to_string(), format!("attempt {max_retries}"));
        }
    }

    #[tokio::test]
    async fn a_failure_not_worth_it_ends_at_once() {
        for (replay, kind, expected) in [
            (Replay::Safe, ErrorKind::Refused, 1),
            (Replay::Safe, ErrorKind::Credential, 1),
            (Replay::Safe, ErrorKind::Key, 1),
            (Replay::IfNotSent, ErrorKind::Timeout, 1),
            (Replay::IfNotSent, ErrorKind::Interrupted, 1),
            (Replay::Safe, ErrorKind::Timeout, 3),
            (Replay::Safe, ErrorKind::Interrupted, 3),
        ] {
            let attempts = AtomicU32::new(0);
            let error = with_backoff(&quick(2), replay, || async {
                attempts.fetch_add(1, Ordering::SeqCst);
                Err::<(), _>(Error::new(kind, "x"))
            })
            .await
            .unwrap_err();
            assert_eq!(error.kind(), kind);
            assert_eq!(
                attempts.load(Ordering::SeqCst),
                expected,
                "{replay:?} {kind:?}"
            );
        }
    }

    #[tokio::test]
    async fn no_attempt_starts_after_the_time_for_retries_has_passed() {
        let policy = RetryPolicy {
            retry_timeout: Duration::ZERO,
            ..quick(10)
        };
        let attempts = AtomicU32::new(0);
        let error = with_backoff(&policy, Replay::Safe, || async {
            attempts.fetch_add(1, Ordering::SeqCst);
            // Longer than the clock's smallest step, so time has passed.
            tokio::time::sleep(ms(20)).await;
            Err::<(), _>(Error::busy("SlowDown"))
        })
        .await
        .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Busy);
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }
}
