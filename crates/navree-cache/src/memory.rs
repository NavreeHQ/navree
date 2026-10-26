//! [`MemoryStore`]: the in-process [`CacheStore`] on moka.

use std::fmt;
use std::time::Duration;

use bytes::Bytes;
use moka::Expiry;
use moka::ops::compute::{CompResult, Op};
use navree_kernel::BoxFuture;
use tokio::time::Instant;

use crate::store::parse_integer;
use crate::{CacheError, CacheStore};

/// An in-process [`CacheStore`] on [moka](https://docs.rs/moka) with a TTL
/// per entry.
///
/// Expiry is checked against Tokio's clock, so `tokio::time::pause` and
/// `advance` control it in tests; moka evicts expired entries in the
/// background to reclaim memory. The store is unbounded and per process:
/// every app gets its own.
pub struct MemoryStore {
    entries: moka::future::Cache<String, Entry>,
}

/// A stored value and the instant it stops being visible.
#[derive(Clone)]
struct Entry {
    value: Bytes,
    expires_at: Option<Instant>,
}

impl Entry {
    fn new(value: Bytes, ttl: Option<Duration>) -> Self {
        Self {
            value,
            expires_at: deadline(ttl),
        }
    }

    fn is_live(&self) -> bool {
        self.expires_at.is_none_or(|at| Instant::now() < at)
    }

    /// The time left before expiry, for moka's eviction.
    fn remaining(&self) -> Option<Duration> {
        self.expires_at
            .map(|at| at.saturating_duration_since(Instant::now()))
    }
}

/// `now + ttl`; `None` (never expires) when there is no TTL or it overflows.
pub(crate) fn deadline(ttl: Option<Duration>) -> Option<Instant> {
    ttl.and_then(|ttl| Instant::now().checked_add(ttl))
}

/// Tells moka when to evict each entry (memory reclaim only: visibility is
/// decided by [`Entry::is_live`]).
struct EntryExpiry;

impl Expiry<String, Entry> for EntryExpiry {
    fn expire_after_create(
        &self,
        _key: &String,
        value: &Entry,
        _created_at: std::time::Instant,
    ) -> Option<Duration> {
        value.remaining()
    }

    fn expire_after_update(
        &self,
        _key: &String,
        value: &Entry,
        _updated_at: std::time::Instant,
        _duration_until_expiry: Option<Duration>,
    ) -> Option<Duration> {
        value.remaining()
    }
}

impl MemoryStore {
    /// An empty store.
    pub fn new() -> Self {
        Self {
            entries: moka::future::Cache::builder()
                .expire_after(EntryExpiry)
                .build(),
        }
    }

    async fn live(&self, key: &str) -> Option<Entry> {
        self.entries.get(key).await.filter(Entry::is_live)
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
            .field("entries", &self.entries.entry_count())
            .finish()
    }
}

impl CacheStore for MemoryStore {
    fn get<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, CacheError>> {
        Box::pin(async move { Ok(self.live(key).await.map(|entry| entry.value)) })
    }

    fn put<'a>(
        &'a self,
        key: &'a str,
        value: Bytes,
        ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<(), CacheError>> {
        Box::pin(async move {
            self.entries
                .insert(key.to_owned(), Entry::new(value, ttl))
                .await;
            Ok(())
        })
    }

    fn forget<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<bool, CacheError>> {
        Box::pin(async move {
            Ok(self
                .entries
                .remove(key)
                .await
                .is_some_and(|entry| entry.is_live()))
        })
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
            let result = self
                .entries
                .entry_by_ref(key)
                .and_compute_with(|existing| {
                    let op = match existing {
                        Some(entry) if entry.value().is_live() => Op::Nop,
                        _ => Op::Put(Entry::new(value, ttl)),
                    };
                    std::future::ready(op)
                })
                .await;
            Ok(matches!(
                result,
                CompResult::Inserted(_) | CompResult::ReplacedWith(_)
            ))
        })
    }

    fn increment<'a>(&'a self, key: &'a str, by: i64) -> BoxFuture<'a, Result<i64, CacheError>> {
        Box::pin(async move {
            let result = self
                .entries
                .entry_by_ref(key)
                .and_try_compute_with(|existing| {
                    let next = match existing.map(|e| e.into_value()).filter(Entry::is_live) {
                        Some(entry) => parse_integer(key, &entry.value).and_then(|current| {
                            add_checked(key, current, by).map(|next| Entry {
                                value: Bytes::from(next.to_string()),
                                expires_at: entry.expires_at,
                            })
                        }),
                        None => add_checked(key, 0, by)
                            .map(|next| Entry::new(Bytes::from(next.to_string()), None)),
                    };
                    std::future::ready(next.map(Op::Put))
                })
                .await?;
            match result {
                CompResult::Inserted(entry) | CompResult::ReplacedWith(entry) => {
                    parse_integer(key, &entry.into_value().value)
                }
                // Unreachable: the closure always returns `Op::Put`.
                _ => Err(CacheError::NotAnInteger {
                    key: key.to_owned(),
                }),
            }
        })
    }
}

fn add_checked(key: &str, current: i64, by: i64) -> Result<i64, CacheError> {
    current.checked_add(by).ok_or_else(|| CacheError::Overflow {
        key: key.to_owned(),
    })
}
