//! [`MemoryStore`]: the in-process [`CacheStore`], on navree-bounded.

use std::fmt;
use std::time::Duration;

use bytes::Bytes;
use navree_bounded::{Cache, Compute, Computed};
use navree_kernel::BoxFuture;
use tokio::time::Instant;

use crate::store::parse_integer;
use crate::{CacheError, CacheStore};

/// The most entries a [`MemoryStore`] holds unless built with
/// [`MemoryStore::with_capacity`] (there is no configuration key for it).
pub const DEFAULT_MAX_ENTRIES: usize = 1_000_000;

/// An in-process [`CacheStore`]: a bounded cache with a TTL per entry.
///
/// Expiry is checked against Tokio's clock, so `tokio::time::pause` and
/// `advance` control it in tests: a read never returns an expired entry, and
/// expired entries are removed as reads, writes and `sweep` meet them. The
/// store holds at most [`capacity`](MemoryStore::capacity) entries (a hard
/// bound, never exceeded); when it is full, a new key evicts the entry
/// least likely to be used again (SIEVE: see navree-bounded). It is per
/// process: every app gets its own.
pub struct MemoryStore {
    entries: Cache<String, Bytes>,
}

/// `now + ttl`; `None` (never expires) when there is no TTL or it overflows.
pub(crate) fn deadline(ttl: Option<Duration>) -> Option<Instant> {
    ttl.and_then(|ttl| Instant::now().checked_add(ttl))
}

impl MemoryStore {
    /// An empty store holding at most [`DEFAULT_MAX_ENTRIES`] entries.
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_MAX_ENTRIES)
    }

    /// An empty store holding at most `max_entries` entries.
    pub fn with_capacity(max_entries: usize) -> Self {
        Self {
            entries: Cache::new(max_entries),
        }
    }

    /// The most entries the store holds.
    pub fn capacity(&self) -> usize {
        self.entries.capacity()
    }

    /// How many entries the store holds now (expired ones that nothing has
    /// removed yet included).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the store holds no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Looks at up to `budget` slots of every shard and removes the expired
    /// entries among them; returns how many it removed. The store also
    /// reclaims expired entries as it is used, so a periodic task only helps
    /// an app that wants its memory back during a quiet spell.
    pub fn sweep(&self, budget: usize) -> usize {
        self.entries.sweep(budget)
    }
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for MemoryStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MemoryStore")
            .field("entries", &self.entries.len())
            .field("capacity", &self.entries.capacity())
            .finish()
    }
}

impl CacheStore for MemoryStore {
    fn get<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, CacheError>> {
        Box::pin(async move { Ok(self.entries.get(key)) })
    }

    fn put<'a>(
        &'a self,
        key: &'a str,
        value: Bytes,
        ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<(), CacheError>> {
        Box::pin(async move {
            self.entries.insert_with_ttl(key.to_owned(), value, ttl);
            Ok(())
        })
    }

    fn forget<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<bool, CacheError>> {
        Box::pin(async move { Ok(self.entries.remove(key).is_some()) })
    }

    fn flush(&self) -> BoxFuture<'_, Result<(), CacheError>> {
        Box::pin(async move {
            self.entries.invalidate_all();
            Ok(())
        })
    }

    fn add<'a>(
        &'a self,
        key: &'a str,
        value: Bytes,
        ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<bool, CacheError>> {
        Box::pin(async move {
            let outcome = self.entries.compute(key, |existing| match existing {
                Some(_) => Compute::Keep,
                None => Compute::Store(value, ttl),
            });
            Ok(outcome == Computed::Inserted)
        })
    }

    fn increment<'a>(&'a self, key: &'a str, by: i64) -> BoxFuture<'a, Result<i64, CacheError>> {
        Box::pin(async move {
            let mut result = Ok(0);
            self.entries.compute(key, |existing| {
                let next = match existing {
                    Some(value) => {
                        parse_integer(key, value).and_then(|current| add_checked(key, current, by))
                    }
                    None => add_checked(key, 0, by),
                };
                match next {
                    Ok(next) => {
                        result = Ok(next);
                        let value = Bytes::from(next.to_string());
                        match existing {
                            // The entry keeps its TTL; a new one never expires.
                            Some(_) => Compute::Update(value),
                            None => Compute::Store(value, None),
                        }
                    }
                    Err(error) => {
                        result = Err(error);
                        Compute::Keep
                    }
                }
            });
            result
        })
    }
}

fn add_checked(key: &str, current: i64, by: i64) -> Result<i64, CacheError> {
    current.checked_add(by).ok_or_else(|| CacheError::Overflow {
        key: key.to_owned(),
    })
}
