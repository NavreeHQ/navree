//! [`RateLimiter`]: named rate limits shared by every process, on Redis,
//! and [`RedisRateLimitStore`], where the HTTP server's limits count.

use std::fmt;
use std::time::Duration;

use navree_http::{Limited, RateLimitStore, StoreVerdict};
use navree_kernel::BoxFuture;
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

/// [`HIT`] for the HTTP server's limits, which also say whether a refusal is
/// the first of its window, so a flood is reported once. One hash per key:
/// `tat` as in [`HIT`], and `reported`, when a refusal was last reported.
///
/// KEYS and ARGV as for [`HIT`]. Returns {1, 0, 0}, or {0, retry after in
/// microseconds, 1 when this refusal is to be reported}.
const HTTP_HIT: &str = r"
local time = redis.call('TIME')
local now = tonumber(time[1]) * 1000000 + tonumber(time[2])
local interval = tonumber(ARGV[1])
local period = tonumber(ARGV[2])
local state = redis.call('HMGET', KEYS[1], 'tat', 'reported')
local tat = tonumber(state[1]) or now
if tat < now then
  tat = now
end
local next = tat + interval
local ahead = next - now
if ahead > period then
  local reported = tonumber(state[2])
  if reported and now - reported < period then
    return {0, ahead - period, 0}
  end
  redis.call('HSET', KEYS[1], 'reported', string.format('%d', now))
  redis.call('PEXPIRE', KEYS[1], math.ceil(ahead / 1000))
  return {0, ahead - period, 1}
end
redis.call('HSET', KEYS[1], 'tat', string.format('%d', next))
redis.call('PEXPIRE', KEYS[1], math.ceil(ahead / 1000))
return {1, 0, 0}
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
        let (interval, period) = micros(limit, per);
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

/// The microseconds of a limit of `limit` per `per`: the interval between
/// requests and the period.
fn micros(limit: u32, per: Duration) -> (u64, u64) {
    let period = u64::try_from(per.as_micros()).unwrap_or(u64::MAX).max(1);
    let interval = (period / u64::from(limit.max(1))).max(1);
    (interval, period)
}

/// Where the HTTP server's per-IP limit and route throttles count, so every
/// process of the app counts them together: registered by `RedisModule` as
/// the [`RateLimitStore`] named `redis`, which `http.rate_limit_store =
/// "redis"` selects. It uses the connection named `rate_limiter` when there
/// is one, and keys of its own under `<prefix>http-limit:`, apart from
/// [`RateLimiter`]'s, so an app's limit named like one of the server's
/// neither counts with it nor resets it.
///
/// A key names a route as it is written, `{id}` and all; in a key the braces
/// are written `%7B` and `%7D` (and `%` as `%25`), as Redis Cluster would
/// take what is between them for the part of the key that places it, and put
/// the throttles of every route with an `{id}` in one place.
///
/// It counts as [`RateLimiter`] does, on Redis's clock, in one round trip
/// per request.
#[derive(Clone)]
pub struct RedisRateLimitStore {
    connection: RedisConnection,
    namespace: String,
    hit: Script,
}

impl RedisRateLimitStore {
    /// The store on `connection`, with keys under `<prefix>http-limit:`.
    pub fn new(connection: RedisConnection, prefix: &str) -> Self {
        Self {
            connection,
            namespace: format!("{prefix}http-limit:"),
            hit: Script::new(HTTP_HIT),
        }
    }

    /// Where the limit `key` is counted.
    pub fn redis_key(&self, key: &str) -> String {
        store_key(&self.namespace, key)
    }

    async fn count(&self, key: &str, limit: u32, per: Duration) -> StoreVerdict {
        let (interval, period) = micros(limit, per);
        let (allowed, retry_after, report): (i64, i64, i64) = self
            .hit
            .key(self.redis_key(key))
            .arg(interval)
            .arg(period)
            .invoke_async(&mut self.connection.manager())
            .await
            .map_err(|source| self.connection.error(source))?;
        if allowed == 1 {
            return Ok(Ok(()));
        }
        let retry_after = Duration::from_micros(u64::try_from(retry_after).unwrap_or_default());
        Ok(Err(Limited::new(retry_after, report == 1)))
    }
}

/// `key` under `namespace`, with no braces: `%`, `{` and `}` are written
/// `%25`, `%7B` and `%7D`, so two keys stay two keys.
fn store_key(namespace: &str, key: &str) -> String {
    let mut stored = String::with_capacity(namespace.len() + key.len());
    stored.push_str(namespace);
    for c in key.chars() {
        match c {
            '%' => stored.push_str("%25"),
            '{' => stored.push_str("%7B"),
            '}' => stored.push_str("%7D"),
            c => stored.push(c),
        }
    }
    stored
}

impl RateLimitStore for RedisRateLimitStore {
    fn hit<'a>(&'a self, key: &'a str, limit: u32, per: Duration) -> BoxFuture<'a, StoreVerdict> {
        Box::pin(self.count(key, limit, per))
    }
}

impl fmt::Debug for RedisRateLimitStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedisRateLimitStore")
            .field("connection", &self.connection.name())
            .field("namespace", &self.namespace)
            .finish_non_exhaustive()
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Redis Cluster places a key by what is between its first braces: the
    /// throttles of every route with an `{id}` would share one place.
    #[test]
    fn a_route_throttles_key_has_no_braces_and_stays_its_own() {
        let key = store_key("navree:http-limit:", "http:route:GET /deals/{id}:user:7");
        assert_eq!(
            key,
            "navree:http-limit:http:route:GET /deals/%7Bid%7D:user:7"
        );
        assert_ne!(
            store_key("p:", "http:route:GET /a/{id}"),
            store_key("p:", "http:route:GET /a/%7Bid%7D"),
            "a path written with the escape is another key"
        );
        assert_eq!(store_key("p:", "http:ip:192.0.2.1"), "p:http:ip:192.0.2.1");
    }
}
