//! [`LoginThrottle`]: failed login attempts per email and client IP.

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use navree_cache::Cache;
use navree_contracts::Clock;
use navree_crypto::sha256_hex;
use navree_http::rate_limit_key;

use crate::AuthError;

/// The throttle window.
const WINDOW: Duration = Duration::from_secs(60);

/// Counts attempts per key (failed logins per email and client IP, wrong
/// two-factor codes per user) in the cache, in fixed windows:
/// the first attempt opens a one-minute window; once `limit` attempts are
/// counted, further ones are refused until the window ends.
///
/// An attempt is counted **before** the credential is checked
/// ([`attempt`](LoginThrottle::attempt) increments atomically and refuses
/// when the new count is over the limit), so parallel requests cannot all
/// pass a check made while the counter was still low. A successful attempt
/// then either clears the key ([`clear`](LoginThrottle::clear), after a
/// login) or gives its slot back ([`release`](LoginThrottle::release),
/// after a password confirmation), so only failures use up the window.
///
/// Each window has its own counter, named after the time the window ends
/// (stored under the `:timer` key), so a counter that outlives its window
/// is never read again.
pub(crate) struct LoginThrottle {
    cache: Cache,
    limit: u32,
    clock: Arc<dyn Clock>,
    /// `http.rate_limit_ipv6_prefix`: IPv6 clients are counted by network.
    ipv6_prefix: u8,
}

impl LoginThrottle {
    /// A throttle allowing `limit` failures per minute (`0`: no limit),
    /// counting IPv6 clients by their first `ipv6_prefix` bits.
    pub(crate) fn new(cache: Cache, limit: u32, clock: Arc<dyn Clock>, ipv6_prefix: u8) -> Self {
        Self {
            cache,
            limit,
            clock,
            ipv6_prefix,
        }
    }

    /// The cache key of `email` (normalised) from `ip`: a digest, so the
    /// cache never holds email addresses. An IPv6 address counts as its
    /// network (see [`navree_http::rate_limit_key`]), so rotating through
    /// the addresses of one allocation does not give fresh attempts.
    pub(crate) fn key(&self, email: &str, ip: Option<&str>) -> String {
        let email = email.trim().to_lowercase();
        let client = match ip {
            Some(ip) => match ip.parse::<IpAddr>() {
                Ok(addr) => rate_limit_key(addr, self.ipv6_prefix).to_string(),
                Err(_) => ip.to_owned(),
            },
            None => "-".to_owned(),
        };
        let digest = sha256_hex(format!("{email}|{client}").as_bytes());
        format!("navree-auth:login:{digest}")
    }

    /// Counts an attempt for `key` before it is made:
    /// [`AuthError::TooManyAttempts`] (and nothing to check) when the
    /// window already holds `limit` attempts. (`AuthService` uses
    /// [`count`](LoginThrottle::count) to audit refusals.)
    #[cfg(test)]
    pub(crate) async fn attempt(&self, key: &str) -> Result<(), AuthError> {
        match self.count(key).await? {
            Attempt::Allowed => Ok(()),
            Attempt::Refused { retry_after, .. } => Err(AuthError::TooManyAttempts { retry_after }),
        }
    }

    /// Counts an attempt for `key` (see [`attempt`](LoginThrottle::attempt)),
    /// telling whether a refusal is the window's first.
    pub(crate) async fn count(&self, key: &str) -> Result<Attempt, AuthError> {
        if self.limit == 0 {
            return Ok(Attempt::Allowed);
        }
        let now = self.clock.now().timestamp();
        let available_at = self.window(key, now).await?;
        let counter = counter_key(key, available_at);
        self.cache.add(&counter, &0_i64, WINDOW).await?;
        let attempts = self.cache.increment(&counter, 1).await?;
        let limit = i64::from(self.limit);
        if attempts <= limit {
            return Ok(Attempt::Allowed);
        }
        let wait = available_at.saturating_sub(now);
        Ok(Attempt::Refused {
            retry_after: Duration::from_secs(u64::try_from(wait).unwrap_or(0).max(1)),
            // The counter is incremented atomically: exactly one attempt
            // per window sees `limit + 1`.
            first: attempts == limit + 1,
        })
    }

    /// Gives back the slot of an attempt that succeeded but should not
    /// count (a correct password confirmation).
    pub(crate) async fn release(&self, key: &str) -> Result<(), AuthError> {
        if self.limit == 0 {
            return Ok(());
        }
        let Some(available_at) = self.cache.get::<i64>(&timer_key(key)).await? else {
            return Ok(());
        };
        let counter = counter_key(key, available_at);
        if self
            .cache
            .get::<i64>(&counter)
            .await?
            .is_some_and(|attempts| attempts > 0)
        {
            self.cache.decrement(&counter, 1).await?;
        }
        Ok(())
    }

    /// Forgets the key's attempts (after a successful login).
    pub(crate) async fn clear(&self, key: &str) -> Result<(), AuthError> {
        if self.limit == 0 {
            return Ok(());
        }
        let timer = timer_key(key);
        if let Some(available_at) = self.cache.get::<i64>(&timer).await? {
            self.cache.forget(&counter_key(key, available_at)).await?;
        }
        self.cache.forget(&timer).await?;
        Ok(())
    }

    /// When the key's current window ends (unix seconds), opening a new
    /// window at `now` when there is none.
    async fn window(&self, key: &str, now: i64) -> Result<i64, AuthError> {
        let timer = timer_key(key);
        let opened = now.saturating_add(WINDOW.as_secs() as i64);
        self.cache.add(&timer, &opened, WINDOW).await?;
        Ok(self.cache.get::<i64>(&timer).await?.unwrap_or(opened))
    }
}

/// The outcome of [`LoginThrottle::count`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Attempt {
    /// Within the limit: go ahead.
    Allowed,
    /// Over the limit.
    Refused {
        /// When the window ends.
        retry_after: Duration,
        /// The window's first refusal (audit it; skip the rest).
        first: bool,
    },
}

fn timer_key(key: &str) -> String {
    format!("{key}:timer")
}

/// The attempt counter of the window of `key` ending at `available_at`.
fn counter_key(key: &str, available_at: i64) -> String {
    format!("{key}:{available_at}")
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use chrono::Utc;
    use navree_cache::Cache;
    use navree_contracts::FixedClock;

    use super::*;
    use crate::AuthError;

    fn throttle(limit: u32) -> (LoginThrottle, Arc<FixedClock>, Cache) {
        let clock = Arc::new(FixedClock::new(Utc::now()));
        let cache = Cache::memory();
        (
            LoginThrottle::new(cache.clone(), limit, clock.clone(), 64),
            clock,
            cache,
        )
    }

    #[tokio::test(start_paused = true)]
    async fn blocks_after_limit_attempts_until_the_window_ends() {
        let (throttle, clock, _) = throttle(3);
        let key = throttle.key("anu@example.com", Some("127.0.0.1"));
        for _ in 0..3 {
            throttle.attempt(&key).await.unwrap();
        }
        clock.advance(Duration::from_secs(20));
        tokio::time::advance(Duration::from_secs(20)).await;
        match throttle.attempt(&key).await {
            Err(AuthError::TooManyAttempts { retry_after }) => {
                assert_eq!(retry_after, Duration::from_secs(40));
            }
            other => panic!("expected TooManyAttempts, got {other:?}"),
        }
        clock.advance(Duration::from_secs(41));
        tokio::time::advance(Duration::from_secs(41)).await;
        throttle.attempt(&key).await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn parallel_attempts_never_exceed_the_limit() {
        let (throttle, _, _) = throttle(5);
        let throttle = Arc::new(throttle);
        let key = throttle.key("anu@example.com", Some("127.0.0.1"));
        let tasks: Vec<_> = (0..64)
            .map(|_| {
                let (throttle, key) = (throttle.clone(), key.clone());
                tokio::spawn(async move { throttle.attempt(&key).await.is_ok() })
            })
            .collect();
        let mut allowed = 0;
        for task in tasks {
            allowed += usize::from(task.await.unwrap());
        }
        assert_eq!(allowed, 5);
    }

    #[tokio::test]
    async fn only_the_first_refusal_of_a_window_is_marked() {
        let (throttle, _, _) = throttle(1);
        let key = throttle.key("anu@example.com", None);
        assert_eq!(throttle.count(&key).await.unwrap(), Attempt::Allowed);
        let mut marked = Vec::new();
        for _ in 0..3 {
            match throttle.count(&key).await.unwrap() {
                Attempt::Refused { first, .. } => marked.push(first),
                Attempt::Allowed => panic!("over the limit"),
            }
        }
        assert_eq!(marked, [true, false, false]);
    }

    #[tokio::test]
    async fn keys_are_separate_and_clear_resets() {
        let (throttle, _, _) = throttle(1);
        let ada = throttle.key("anu@example.com", Some("127.0.0.1"));
        let other_ip = throttle.key("anu@example.com", Some("10.0.0.1"));
        let grace = throttle.key("gauri@example.com", Some("127.0.0.1"));
        throttle.attempt(&ada).await.unwrap();
        assert!(throttle.attempt(&ada).await.is_err());
        throttle.attempt(&other_ip).await.unwrap();
        throttle.attempt(&grace).await.unwrap();
        throttle.clear(&ada).await.unwrap();
        throttle.attempt(&ada).await.unwrap();
    }

    #[tokio::test]
    async fn released_attempts_do_not_count() {
        let (throttle, _, _) = throttle(2);
        let key = throttle.key("anu@example.com", None);
        for _ in 0..5 {
            throttle.attempt(&key).await.unwrap();
            throttle.release(&key).await.unwrap();
        }
        throttle.attempt(&key).await.unwrap();
        throttle.attempt(&key).await.unwrap();
        assert!(throttle.attempt(&key).await.is_err());
    }

    #[tokio::test]
    async fn a_limit_of_zero_never_blocks() {
        let (throttle, _, _) = throttle(0);
        let key = throttle.key("anu@example.com", None);
        for _ in 0..10 {
            throttle.attempt(&key).await.unwrap();
        }
    }

    #[test]
    fn keys_do_not_contain_the_email_address() {
        let (throttle, _, _) = throttle(1);
        let key = throttle.key("Anu@Example.com", Some("127.0.0.1"));
        assert!(!key.to_lowercase().contains("anu"), "{key}");
        assert_eq!(key, throttle.key("anu@example.com", Some("127.0.0.1")));
    }

    #[test]
    fn ipv6_clients_share_a_key_per_network() {
        let (throttle, _, _) = throttle(1);
        let ada = |ip| throttle.key("anu@example.com", Some(ip));
        assert_eq!(ada("2001:db8:1:2::1"), ada("2001:db8:1:2:ffff:eeee:dddd:1"));
        assert_ne!(ada("2001:db8:1:2::1"), ada("2001:db8:1:3::1"));
        assert_eq!(ada("192.0.2.7"), ada("::ffff:192.0.2.7"), "mapped is IPv4");
        assert_ne!(ada("192.0.2.7"), ada("192.0.2.8"));
        assert_ne!(
            throttle.key("gauri@example.com", Some("2001:db8:1:2::1")),
            ada("2001:db8:1:2::1")
        );
        // The prefix is the one of `http.rate_limit_ipv6_prefix`.
        let wide = LoginThrottle::new(
            Cache::memory(),
            1,
            Arc::new(FixedClock::new(Utc::now())),
            48,
        );
        assert_eq!(
            wide.key("anu@example.com", Some("2001:db8:1:2::1")),
            wide.key("anu@example.com", Some("2001:db8:1:9::1"))
        );
    }
}
