//! [`RateLimiter`]: named rate limits shared by every process, on Redis.

use std::fmt;
use std::time::Duration;

use redis::Script;

use crate::{RedisConnection, RedisError};

/// The generic cell rate algorithm in one step, on Redis's clock (so every
/// app server counts the same time): one timestamp per key, the
/// theoretical arrival time of the next request, in microseconds.
///
/// KEYS: the limit's key. ARGV: the interval between requests and the
/// period, in microseconds. Returns {1, remaining} or {0, retry after in
/// microseconds}.
const HIT: &str = r"
local time = redis.call('TIME')
local now = tonumber(time[1]) * 1000000 + tonumber(time[2])
local interval = tonumber(ARGV[1])
local period = tonumber(ARGV[2])
local tat = tonumber(redis.call('GET', KEYS[1]) or now)
if tat < now then
  tat = now
end
local next = tat + interval
local ahead = next - now
if ahead > period then
  return {0, ahead - period}
end
redis.call('SET', KEYS[1], string.format('%d', next), 'PX', math.ceil(ahead / 1000))
return {1, math.floor((period - ahead) / interval)}
";

/// The answer of [`RateLimiter::hit`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RateLimit {
    /// The request is allowed; `remaining` more are allowed right now.
    Allowed {
        /// Requests still allowed before the limit refuses.
        remaining: u32,
    },
    /// Over the limit: the next request is allowed after `retry_after`.
    Limited {
        /// How long until a request is allowed again.
        retry_after: Duration,
    },
}

impl RateLimit {
    /// Whether the request is allowed.
    pub fn is_allowed(&self) -> bool {
        matches!(self, RateLimit::Allowed { .. })
    }
}

/// Rate limits by name, counted in Redis, so every process of the app
/// shares them: the same algorithm as the per-IP limit and route throttles
/// of navree-http (a burst of `limit`, then one request every
/// `per / limit`), for limits those cannot express, such as a limit per
/// plan, per API key or shared by several routes. Provided by
/// `RedisModule`; it uses the connection named `rate_limiter` when there
/// is one.
///
/// ```no_run
/// use std::time::Duration;
///
/// use navree_redis::{RateLimit, RateLimiter};
///
/// # async fn export(limiter: &RateLimiter, user_id: &str) -> Result<(), navree_redis::RedisError> {
/// match limiter.hit(&format!("exports:{user_id}"), 10, Duration::from_secs(3600)).await? {
///     RateLimit::Allowed { remaining } => { /* export; `remaining` more this hour */ }
///     RateLimit::Limited { retry_after } => { /* answer 429 with Retry-After */ }
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct RateLimiter {
    connection: RedisConnection,
    namespace: String,
    hit: Script,
}

impl RateLimiter {
    /// The limiter on `connection`, with keys under `<prefix>limit:`.
    pub fn new(connection: RedisConnection, prefix: &str) -> Self {
        Self {
            connection,
            namespace: format!("{prefix}limit:"),
            hit: Script::new(HIT),
        }
    }

    /// Where the limit `key` is counted.
    pub fn redis_key(&self, key: &str) -> String {
        format!("{}{key}", self.namespace)
    }

    /// Counts a request against `key`: at most `limit` (at least 1) per
    /// `per`, as a burst. A refused request does not count.
    pub async fn hit(&self, key: &str, limit: u32, per: Duration) -> Result<RateLimit, RedisError> {
        let period = u64::try_from(per.as_micros()).unwrap_or(u64::MAX).max(1);
        let interval = (period / u64::from(limit.max(1))).max(1);
        let (allowed, value): (i64, i64) = self
            .hit
            .key(self.redis_key(key))
            .arg(interval)
            .arg(period)
            .invoke_async(&mut self.connection.manager())
            .await
            .map_err(|source| self.connection.error(source))?;
        let value = u64::try_from(value).unwrap_or_default();
        Ok(if allowed == 1 {
            RateLimit::Allowed {
                remaining: u32::try_from(value).unwrap_or(u32::MAX),
            }
        } else {
            RateLimit::Limited {
                retry_after: Duration::from_micros(value),
            }
        })
    }

    /// Forgets the requests counted against `key`.
    pub async fn reset(&self, key: &str) -> Result<(), RedisError> {
        redis::cmd("DEL")
            .arg(self.redis_key(key))
            .query_async::<i64>(&mut self.connection.manager())
            .await
            .map(drop)
            .map_err(|source| self.connection.error(source))
    }
}

impl fmt::Debug for RateLimiter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RateLimiter")
            .field("connection", &self.connection.name())
            .field("namespace", &self.namespace)
            .finish_non_exhaustive()
    }
}
