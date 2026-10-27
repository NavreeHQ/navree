//! Where entries are kept: the [`LogbookStore`] trait, the
//! [`MemoryStore`] and the [`SqliteStore`] (the `file` store).

mod memory;
mod sqlite;

use chrono::{DateTime, Utc};
use navree_kernel::BoxFuture;

pub use memory::MemoryStore;
pub use sqlite::SqliteStore;

use crate::{Entry, Filter, LogbookError};

/// A page of the list: entries matching `filter`, newest first.
#[derive(Clone, Debug, PartialEq)]
pub struct ListQuery {
    /// What to show.
    pub filter: Filter,
    /// Only entries older than this `seq` (the next page).
    pub before: Option<u64>,
    /// Only entries newer than this `seq` (new arrivals).
    pub after: Option<u64>,
    /// At most this many (capped at 500).
    pub limit: usize,
}

impl ListQuery {
    /// The newest `limit` entries matching `filter`.
    pub fn new(filter: Filter, limit: usize) -> ListQuery {
        ListQuery {
            filter,
            before: None,
            after: None,
            limit,
        }
    }

    /// The limit, between 1 and 500.
    pub(crate) fn capped_limit(&self) -> usize {
        self.limit.clamp(1, 500)
    }
}

/// Keeps Logbook's entries and the tags of their batches. Implemented by
/// [`MemoryStore`] and [`SqliteStore`]; both pass the same conformance
/// tests.
///
/// Tags belong to a batch key ([`Entry::batch_key`]): the batch id, or the
/// entry's own id outside a batch. Entries come back with their batch's
/// tags in [`Entry::tags`], sorted.
pub trait LogbookStore: Send + Sync + 'static {
    /// Stores `entries` in order, giving each the next `seq`, and adds each
    /// entry's `tags` to its batch.
    fn insert(&self, entries: Vec<Entry>) -> BoxFuture<'_, Result<(), LogbookError>>;

    /// Adds `tags` to the batch `batch_key` (stored before or after its
    /// entries).
    fn add_tags(
        &self,
        batch_key: String,
        tags: Vec<String>,
    ) -> BoxFuture<'_, Result<(), LogbookError>>;

    /// A page of entries matching the query, newest first.
    fn list<'a>(&'a self, query: &'a ListQuery) -> BoxFuture<'a, Result<Vec<Entry>, LogbookError>>;

    /// One entry by id.
    fn entry<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<Option<Entry>, LogbookError>>;

    /// Every entry of batch `batch_id`, oldest first.
    fn batch<'a>(&'a self, batch_id: &'a str) -> BoxFuture<'a, Result<Vec<Entry>, LogbookError>>;

    /// How many entries there are.
    fn count(&self) -> BoxFuture<'_, Result<u64, LogbookError>>;

    /// Deletes every batch with an entry older than `older_than`, then, while
    /// more than `max_entries` remain, the oldest batches: batches go whole,
    /// never half. Returns how many entries went.
    fn prune(
        &self,
        max_entries: usize,
        older_than: DateTime<Utc>,
    ) -> BoxFuture<'_, Result<u64, LogbookError>>;

    /// Deletes everything; returns how many entries went.
    fn clear(&self) -> BoxFuture<'_, Result<u64, LogbookError>>;
}
