//! [`MemoryStore`]: entries in the process's memory.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Mutex, MutexGuard, PoisonError};

use chrono::{DateTime, Utc};
use navree_kernel::BoxFuture;

use super::{ListQuery, LogbookStore};
use crate::{Entry, LogbookError};

/// Keeps entries in memory (`logbook.store = "memory"`): for tests, and for
/// machines where you would rather not write the file.
#[derive(Default)]
pub struct MemoryStore {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    next_seq: u64,
    entries: BTreeMap<u64, Entry>,
    by_id: HashMap<String, u64>,
    by_key: HashMap<String, BTreeSet<u64>>,
    tags: HashMap<String, BTreeSet<String>>,
}

impl MemoryStore {
    /// An empty store.
    pub fn new() -> MemoryStore {
        MemoryStore::default()
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Inner {
    /// `entry` with its batch's tags.
    fn with_tags(&self, entry: &Entry) -> Entry {
        let mut entry = entry.clone();
        entry.tags = self
            .tags
            .get(entry.batch_key())
            .map(|tags| tags.iter().cloned().collect())
            .unwrap_or_default();
        entry
    }

    /// Deletes every entry and the tags of the batch `key`.
    fn remove_key(&mut self, key: &str) -> u64 {
        self.tags.remove(key);
        let Some(seqs) = self.by_key.remove(key) else {
            return 0;
        };
        for seq in &seqs {
            if let Some(entry) = self.entries.remove(seq) {
                self.by_id.remove(&entry.id);
            }
        }
        seqs.len() as u64
    }
}

impl LogbookStore for MemoryStore {
    fn insert(&self, entries: Vec<Entry>) -> BoxFuture<'_, Result<(), LogbookError>> {
        let mut inner = self.lock();
        for mut entry in entries {
            inner.next_seq += 1;
            entry.seq = inner.next_seq;
            let key = entry.batch_key().to_owned();
            let tags = std::mem::take(&mut entry.tags);
            inner.tags.entry(key.clone()).or_default().extend(tags);
            inner.by_id.insert(entry.id.clone(), entry.seq);
            inner.by_key.entry(key).or_default().insert(entry.seq);
            inner.entries.insert(entry.seq, entry);
        }
        Box::pin(async { Ok(()) })
    }

    fn add_tags(
        &self,
        batch_key: String,
        tags: Vec<String>,
    ) -> BoxFuture<'_, Result<(), LogbookError>> {
        self.lock().tags.entry(batch_key).or_default().extend(tags);
        Box::pin(async { Ok(()) })
    }

    fn list<'a>(&'a self, query: &'a ListQuery) -> BoxFuture<'a, Result<Vec<Entry>, LogbookError>> {
        let inner = self.lock();
        let low = query.after.map_or(0, |after| after.saturating_add(1));
        let high = query.before.unwrap_or(u64::MAX);
        let found: Vec<Entry> = if low >= high {
            Vec::new()
        } else {
            inner
                .entries
                .range(low..high)
                .rev()
                .map(|(_, entry)| inner.with_tags(entry))
                .filter(|entry| query.filter.matches(entry))
                .take(query.capped_limit())
                .collect()
        };
        Box::pin(async { Ok(found) })
    }

    fn entry<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<Option<Entry>, LogbookError>> {
        let inner = self.lock();
        let found = inner
            .by_id
            .get(id)
            .and_then(|seq| inner.entries.get(seq))
            .map(|entry| inner.with_tags(entry));
        Box::pin(async { Ok(found) })
    }

    fn batch<'a>(&'a self, batch_id: &'a str) -> BoxFuture<'a, Result<Vec<Entry>, LogbookError>> {
        let inner = self.lock();
        let found: Vec<Entry> = inner
            .by_key
            .get(batch_id)
            .into_iter()
            .flatten()
            .filter_map(|seq| inner.entries.get(seq))
            .filter(|entry| entry.batch_id.as_deref() == Some(batch_id))
            .map(|entry| inner.with_tags(entry))
            .collect();
        Box::pin(async { Ok(found) })
    }

    fn count(&self) -> BoxFuture<'_, Result<u64, LogbookError>> {
        let count = self.lock().entries.len() as u64;
        Box::pin(async move { Ok(count) })
    }

    fn prune(
        &self,
        max_entries: usize,
        older_than: DateTime<Utc>,
    ) -> BoxFuture<'_, Result<u64, LogbookError>> {
        let mut inner = self.lock();
        let mut deleted = 0;
        let old: BTreeSet<String> = inner
            .entries
            .values()
            .filter(|entry| entry.at < older_than)
            .map(|entry| entry.batch_key().to_owned())
            .collect();
        for key in old {
            deleted += inner.remove_key(&key);
        }
        let excess = inner.entries.len().saturating_sub(max_entries);
        if excess > 0 {
            let doomed: BTreeSet<String> = inner
                .entries
                .values()
                .take(excess)
                .map(|entry| entry.batch_key().to_owned())
                .collect();
            for key in doomed {
                deleted += inner.remove_key(&key);
            }
        }
        Box::pin(async move { Ok(deleted) })
    }

    fn clear(&self) -> BoxFuture<'_, Result<u64, LogbookError>> {
        let mut inner = self.lock();
        let deleted = inner.entries.len() as u64;
        let next_seq = inner.next_seq;
        *inner = Inner {
            next_seq,
            ..Inner::default()
        };
        Box::pin(async move { Ok(deleted) })
    }
}

impl std::fmt::Debug for MemoryStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryStore")
            .field("entries", &self.lock().entries.len())
            .finish()
    }
}
