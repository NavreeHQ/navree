//! [`LoginThrottle`]: failed login attempts per email and client IP.

use std::sync::Arc;
use std::time::Duration;

use navree_cache::Cache;
use navree_contracts::Clock;
use navree_crypto::sha256_hex;

use crate::AuthError;

/// The throttle window.
const WINDOW: Duration = Duration::from_secs(60);

/// Counts failed logins per (email, client IP) key in the cache, Laravel
/// `RateLimiter` style: the first failure opens a one-minute window; once
/// `limit` failures are counted, logins for the key are refused until the
/// window ends. A successful login clears the key.
pub(crate) struct LoginThrottle {
    cache: Cache,
    limit: u32,
    clock: Arc<dyn Clock>,
}

impl LoginThrottle {
    /// A throttle allowing `limit` failures per minute (`0`: no limit).
    pub(crate) fn new(cache: Cache, limit: u32, clock: Arc<dyn Clock>) -> Self {
        Self {
            cache,
            limit,
            clock,
        }
    }

    /// The cache key of `email` (normalised) from `ip`: a digest, so the
    /// cache never holds email addresses.
    pub(crate) fn key(email: &str, ip: Option<&str>) -> String {
        let email = email.trim().to_lowercase();
        let digest = sha256_hex(format!("{email}|{}", ip.unwrap_or("-")).as_bytes());
        format!("navree-auth:login:{digest}")
    }

    /// [`AuthError::TooManyAttempts`] when the key has used up its window.
    pub(crate) async fn check(&self, key: &str) -> Result<(), AuthError> {
        if self.limit == 0 {
            return Ok(());
        }
        let attempts = self.cache.get::<i64>(key).await?.unwrap_or(0);
        if attempts < i64::from(self.limit) {
            return Ok(());
        }
        let available_at = self
            .cache
            .get::<i64>(&timer_key(key))
            .await?
            .unwrap_or_default();
        let wait = available_at.saturating_sub(self.clock.now().timestamp());
        Err(AuthError::TooManyAttempts {
            retry_after: Duration::from_secs(u64::try_from(wait).unwrap_or(0).max(1)),
        })
    }

    /// Counts a failed attempt.
    pub(crate) async fn hit(&self, key: &str) -> Result<(), AuthError> {
        if self.limit == 0 {
            return Ok(());
        }
        let available_at = self.clock.now().timestamp() + WINDOW.as_secs() as i64;
        self.cache
            .add(&timer_key(key), &available_at, WINDOW)
            .await?;
        self.cache.add(key, &0_i64, WINDOW).await?;
        self.cache.increment(key, 1).await?;
        Ok(())
    }

    /// Forgets the key's failures (after a successful login).
    pub(crate) async fn clear(&self, key: &str) -> Result<(), AuthError> {
        if self.limit == 0 {
            return Ok(());
        }
        self.cache.forget(key).await?;
        self.cache.forget(&timer_key(key)).await?;
        Ok(())
    }
}

fn timer_key(key: &str) -> String {
    format!("{key}:timer")
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
            LoginThrottle::new(cache.clone(), limit, clock.clone()),
            clock,
            cache,
        )
    }

    #[tokio::test(start_paused = true)]
    async fn blocks_after_limit_failures_until_the_window_ends() {
        let (throttle, clock, _) = throttle(3);
        let key = LoginThrottle::key("ada@example.com", Some("127.0.0.1"));
        for _ in 0..3 {
            throttle.check(&key).await.unwrap();
            throttle.hit(&key).await.unwrap();
        }
        clock.advance(Duration::from_secs(20));
        tokio::time::advance(Duration::from_secs(20)).await;
        match throttle.check(&key).await {
            Err(AuthError::TooManyAttempts { retry_after }) => {
                assert_eq!(retry_after, Duration::from_secs(40));
            }
            other => panic!("expected TooManyAttempts, got {other:?}"),
        }
        clock.advance(Duration::from_secs(41));
        tokio::time::advance(Duration::from_secs(41)).await;
        throttle.check(&key).await.unwrap();
    }

    #[tokio::test]
    async fn keys_are_separate_and_clear_resets() {
        let (throttle, _, _) = throttle(1);
        let ada = LoginThrottle::key("ada@example.com", Some("127.0.0.1"));
        let other_ip = LoginThrottle::key("ada@example.com", Some("10.0.0.1"));
        let grace = LoginThrottle::key("grace@example.com", Some("127.0.0.1"));
        throttle.hit(&ada).await.unwrap();
        assert!(throttle.check(&ada).await.is_err());
        throttle.check(&other_ip).await.unwrap();
        throttle.check(&grace).await.unwrap();
        throttle.clear(&ada).await.unwrap();
        throttle.check(&ada).await.unwrap();
    }

    #[tokio::test]
    async fn a_limit_of_zero_never_blocks() {
        let (throttle, _, _) = throttle(0);
        let key = LoginThrottle::key("ada@example.com", None);
        for _ in 0..10 {
            throttle.hit(&key).await.unwrap();
        }
        throttle.check(&key).await.unwrap();
    }

    #[test]
    fn keys_do_not_contain_the_email_address() {
        let key = LoginThrottle::key("Ada@Example.com", Some("127.0.0.1"));
        assert!(!key.to_lowercase().contains("ada"), "{key}");
        assert_eq!(
            key,
            LoginThrottle::key("ada@example.com", Some("127.0.0.1"))
        );
    }
}
