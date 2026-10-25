//! The [`CacheStore`] trait: the byte-level backend behind a [`Cache`](crate::Cache).

use std::time::Duration;

use bytes::Bytes;
use keel_kernel::BoxFuture;

use crate::CacheError;

/// A cache backend storing raw bytes under string keys.
///
/// [`Cache`](crate::Cache) adds the key prefix, JSON encoding, tags,
/// `remember` and `flexible` on top, so a store only implements these
/// primitives. Keys arrive fully prefixed. A `ttl` of `None` means the entry
/// never expires. [`MemoryStore`](crate::MemoryStore) is the built-in store;
/// a database or Redis store implements the same trait and is plugged in as
/// `Arc<dyn CacheStore>`.
///
/// `add` and `increment` must be atomic per key.
pub trait CacheStore: Send + Sync + 'static {
    /// The live (unexpired) value under `key`.
    fn get<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, CacheError>>;

    /// Stores `value` under `key`, replacing any previous value and TTL.
    fn put<'a>(
        &'a self,
        key: &'a str,
        value: Bytes,
        ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<(), CacheError>>;

    /// Removes `key`; `true` if a live value was removed.
    fn forget<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<bool, CacheError>>;

    /// Removes every entry of the store (not only those under one prefix).
    fn flush(&self) -> BoxFuture<'_, Result<(), CacheError>>;

    /// Stores `value` only if `key` has no live value; `true` if it was stored.
    fn add<'a>(
        &'a self,
        key: &'a str,
        value: Bytes,
        ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<bool, CacheError>>;

    /// Adds `by` to the integer stored under `key` (a missing key counts as
    /// 0) and returns the new value. The entry keeps its TTL; a new entry
    /// never expires. Values are stored as decimal text, which is also their
    /// JSON encoding.
    fn increment<'a>(&'a self, key: &'a str, by: i64) -> BoxFuture<'a, Result<i64, CacheError>>;
}

/// Parses the decimal integer stored in `value` (what `increment` reads).
pub(crate) fn parse_integer(key: &str, value: &[u8]) -> Result<i64, CacheError> {
    std::str::from_utf8(value)
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .ok_or_else(|| CacheError::NotAnInteger {
            key: key.to_owned(),
        })
}
