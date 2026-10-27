//! [`RedisCacheStore`] and [`RedisLockStore`]: navree-cache's stores on Redis.

use std::fmt;
use std::time::Duration;

use bytes::Bytes;
use navree_cache::{CacheError, CacheStore, LockStore};
use navree_kernel::BoxFuture;
use redis::AsyncCommands;
use redis::aio::ConnectionManager;

use crate::{RedisConnection, RedisError};

/// How many keys one `SCAN` step asks for when flushing.
const SCAN_COUNT: usize = 1000;

/// Releases a lock only for its owner.
const RELEASE_LOCK: &str = r"
if redis.call('GET', KEYS[1]) == ARGV[1] then
  return redis.call('DEL', KEYS[1])
end
return 0
";

/// navree-cache's [`CacheStore`] on Redis (`cache.store = "redis"`).
///
/// Each entry is a Redis string under `<prefix>cache:<key>` (the key as
/// `Cache` passes it, with `cache.prefix`), with the entry's TTL as the
/// key's expiry: `put` is `SET … PX`, `add` is `SET … NX PX` and
/// `increment` is `INCRBY`, which keeps the TTL. `flush` deletes the keys
/// under `<prefix>cache:` (with `SCAN`), not the whole database, so the
/// queue, sessions and locks on the same server survive `cache:clear`.
#[derive(Clone)]
pub struct RedisCacheStore {
    connection: RedisConnection,
    namespace: String,
}

impl RedisCacheStore {
    /// The store on `connection`, with keys under `<prefix>cache:`.
    pub fn new(connection: RedisConnection, prefix: &str) -> Self {
        Self {
            connection,
            namespace: format!("{prefix}cache:"),
        }
    }

    /// Where `key` is stored.
    pub fn redis_key(&self, key: &str) -> String {
        format!("{}{key}", self.namespace)
    }

    fn manager(&self) -> ConnectionManager {
        self.connection.manager()
    }

    fn fail(&self, source: redis::RedisError) -> CacheError {
        CacheError::Store(Box::new(self.connection.error(source)))
    }
}

impl fmt::Debug for RedisCacheStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedisCacheStore")
            .field("connection", &self.connection.name())
            .field("namespace", &self.namespace)
            .finish()
    }
}

/// The longest TTL sent to Redis: 100 years (Redis refuses expiry times
/// that overflow its clock).
const MAX_TTL_MS: u64 = 100 * 365 * 24 * 60 * 60 * 1000;

/// A TTL in whole milliseconds, from 1 (Redis refuses 0) to 100 years.
pub(crate) fn millis(ttl: Duration) -> u64 {
    u64::try_from(ttl.as_millis())
        .unwrap_or(u64::MAX)
        .clamp(1, MAX_TTL_MS)
}

impl CacheStore for RedisCacheStore {
    fn get<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, CacheError>> {
        Box::pin(async move {
            let value: Option<Vec<u8>> = self
                .manager()
                .get(self.redis_key(key))
                .await
                .map_err(|e| self.fail(e))?;
            Ok(value.map(Bytes::from))
        })
    }

    fn put<'a>(
        &'a self,
        key: &'a str,
        value: Bytes,
        ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<(), CacheError>> {
        Box::pin(async move {
            let mut command = redis::cmd("SET");
            command.arg(self.redis_key(key)).arg(value.as_ref());
            if let Some(ttl) = ttl {
                command.arg("PX").arg(millis(ttl));
            }
            command
                .query_async::<()>(&mut self.manager())
                .await
                .map_err(|e| self.fail(e))
        })
    }

    fn forget<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<bool, CacheError>> {
        Box::pin(async move {
            let removed: u64 = self
                .manager()
                .del(self.redis_key(key))
                .await
                .map_err(|e| self.fail(e))?;
            Ok(removed > 0)
        })
    }

    fn flush(&self) -> BoxFuture<'_, Result<(), CacheError>> {
        Box::pin(async move {
            delete_matching(&self.connection, &self.namespace)
                .await
                .map(drop)
                .map_err(|error| CacheError::Store(Box::new(error)))
        })
    }

    fn add<'a>(
        &'a self,
        key: &'a str,
        value: Bytes,
        ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<bool, CacheError>> {
        Box::pin(async move {
            let mut command = redis::cmd("SET");
            command.arg(self.redis_key(key)).arg(value.as_ref()).arg("NX");
            if let Some(ttl) = ttl {
                command.arg("PX").arg(millis(ttl));
            }
            let stored: Option<String> = command
                .query_async(&mut self.manager())
                .await
                .map_err(|e| self.fail(e))?;
            Ok(stored.is_some())
        })
    }

    fn increment<'a>(&'a self, key: &'a str, by: i64) -> BoxFuture<'a, Result<i64, CacheError>> {
        Box::pin(async move {
            self.manager()
                .incr(self.redis_key(key), by)
                .await
                .map_err(|error: redis::RedisError| {
                    let message = error.to_string();
                    if message.contains("overflow") {
                        CacheError::Overflow {
                            key: key.to_owned(),
                        }
                    } else if message.contains("not an integer") {
                        CacheError::NotAnInteger {
                            key: key.to_owned(),
                        }
                    } else {
                        self.fail(error)
                    }
                })
        })
    }
}

/// navree-cache's [`LockStore`] on Redis: each lock is a key under
/// `<prefix>locks:` holding its owner token, taken with `SET … NX PX` and
/// released by a Lua script that deletes it only for that token. Locks are
/// shared by every process using the server.
#[derive(Clone)]
pub struct RedisLockStore {
    connection: RedisConnection,
    namespace: String,
    release: redis::Script,
}

impl RedisLockStore {
    /// The store on `connection`, with locks under `<prefix>locks:`.
    pub fn new(connection: RedisConnection, prefix: &str) -> Self {
        Self {
            connection,
            namespace: format!("{prefix}locks:"),
            release: redis::Script::new(RELEASE_LOCK),
        }
    }

    /// Where the lock `name` is stored.
    pub fn redis_key(&self, name: &str) -> String {
        format!("{}{name}", self.namespace)
    }

    fn fail(&self, source: redis::RedisError) -> CacheError {
        CacheError::Store(Box::new(self.connection.error(source)))
    }
}

impl fmt::Debug for RedisLockStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedisLockStore")
            .field("connection", &self.connection.name())
            .field("namespace", &self.namespace)
            .finish()
    }
}

impl LockStore for RedisLockStore {
    fn acquire<'a>(
        &'a self,
        name: &'a str,
        owner: &'a str,
        ttl: Duration,
    ) -> BoxFuture<'a, Result<bool, CacheError>> {
        Box::pin(async move {
            let taken: Option<String> = redis::cmd("SET")
                .arg(self.redis_key(name))
                .arg(owner)
                .arg("NX")
                .arg("PX")
                .arg(millis(ttl))
                .query_async(&mut self.connection.manager())
                .await
                .map_err(|e| self.fail(e))?;
            Ok(taken.is_some())
        })
    }

    fn release<'a>(
        &'a self,
        name: &'a str,
        owner: &'a str,
    ) -> BoxFuture<'a, Result<bool, CacheError>> {
        Box::pin(async move {
            let released: i64 = self
                .release
                .key(self.redis_key(name))
                .arg(owner)
                .invoke_async(&mut self.connection.manager())
                .await
                .map_err(|e| self.fail(e))?;
            Ok(released > 0)
        })
    }
}

/// Deletes every key starting with `prefix` (`SCAN MATCH` + `UNLINK`, in
/// steps); returns how many.
pub(crate) async fn delete_matching(
    connection: &RedisConnection,
    prefix: &str,
) -> Result<u64, RedisError> {
    let pattern = format!("{}*", escape_glob(prefix));
    let mut manager = connection.manager();
    let mut cursor: u64 = 0;
    let mut deleted = 0;
    loop {
        let (next, keys): (u64, Vec<String>) = redis::cmd("SCAN")
            .arg(cursor)
            .arg("MATCH")
            .arg(&pattern)
            .arg("COUNT")
            .arg(SCAN_COUNT)
            .query_async(&mut manager)
            .await
            .map_err(|source| connection.error(source))?;
        if !keys.is_empty() {
            let removed: u64 = redis::cmd("UNLINK")
                .arg(&keys)
                .query_async(&mut manager)
                .await
                .map_err(|source| connection.error(source))?;
            deleted += removed;
        }
        if next == 0 {
            return Ok(deleted);
        }
        cursor = next;
    }
}

/// `text` with the glob characters of `SCAN MATCH` escaped.
fn escape_glob(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '*' | '?' | '[' | ']' | '\\' | '^') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_characters_are_escaped() {
        assert_eq!(escape_glob("navree:cache:"), "navree:cache:");
        assert_eq!(escape_glob("a*b?[c]\\"), "a\\*b\\?\\[c\\]\\\\");
    }

    #[test]
    fn ttls_are_whole_milliseconds_of_at_least_one() {
        assert_eq!(millis(Duration::from_secs(2)), 2000);
        assert_eq!(millis(Duration::from_micros(10)), 1);
        assert_eq!(millis(Duration::MAX), MAX_TTL_MS);
    }
}
