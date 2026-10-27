//! [`MediaStore`]: where media rows are kept, the `database` and `memory`
//! drivers.

mod database;
mod memory;

use futures_util::future::BoxFuture;
use navree_db::Uow;
use navree_db::sea_orm::DatabaseTransaction;
use uuid::Uuid;

pub use database::DatabaseStore;
pub use memory::MemoryStore;

use crate::{MediaError, MediaItem};

/// Which rows [`MediaStore::list`] returns.
///
/// Rows come in collection order (`order`, then id), or in id order when
/// the query pages (`after` or `limit` set).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MediaQuery {
    /// Only this model type.
    pub model_type: Option<String>,
    /// Only this model key.
    pub model_id: Option<String>,
    /// Only this collection.
    pub collection: Option<String>,
    /// Only these ids.
    pub ids: Option<Vec<Uuid>>,
    /// Only ids after this one (paging in id order).
    pub after: Option<Uuid>,
    /// At most this many rows (paging in id order).
    pub limit: Option<u64>,
}

impl MediaQuery {
    /// Every row of `model_type`'s model `model_id`.
    pub fn owner(model_type: impl Into<String>, model_id: impl Into<String>) -> Self {
        Self {
            model_type: Some(model_type.into()),
            model_id: Some(model_id.into()),
            ..Self::default()
        }
    }

    /// Only the collection `collection`.
    pub fn collection(mut self, collection: impl Into<String>) -> Self {
        self.collection = Some(collection.into());
        self
    }

    pub(crate) fn pages(&self) -> bool {
        self.after.is_some() || self.limit.is_some()
    }

    pub(crate) fn matches(&self, item: &MediaItem) -> bool {
        self.model_type.as_ref().is_none_or(|t| *t == item.model_type)
            && self.model_id.as_ref().is_none_or(|k| *k == item.model_id)
            && self.collection.as_ref().is_none_or(|c| *c == item.collection)
            && self.ids.as_ref().is_none_or(|ids| ids.contains(&item.id))
            && self.after.is_none_or(|after| item.id > after)
    }
}

/// Keeps the rows of [`MediaItem`]s. Files are the [`Media`](crate::Media)
/// service's business; a store only keeps rows.
///
/// Writes take the unit of work to join: with `Some(uow)` the
/// `database` driver writes in its transaction (rolled back with it);
/// with `None` it joins `Uow::current()` when there is an unfinished one,
/// and commits a unit of work of its own otherwise. The `memory` driver
/// applies every write at once.
pub trait MediaStore: Send + Sync + 'static {
    /// Inserts a row.
    fn insert<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        item: &'a MediaItem,
    ) -> BoxFuture<'a, Result<(), MediaError>>;

    /// Replaces a row (by id).
    fn update<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        item: &'a MediaItem,
    ) -> BoxFuture<'a, Result<(), MediaError>>;

    /// Deletes rows by id and returns how many existed.
    fn delete<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        ids: &'a [Uuid],
    ) -> BoxFuture<'a, Result<u64, MediaError>>;

    /// Deletes every row of one model inside `tx`, the transaction a
    /// change observer was given (the model's own delete is committing),
    /// without change events; returns the deleted rows. The `memory`
    /// driver ignores `tx`.
    fn delete_owner<'a>(
        &'a self,
        tx: &'a DatabaseTransaction,
        model_type: &'a str,
        model_id: &'a str,
    ) -> BoxFuture<'a, Result<Vec<MediaItem>, MediaError>>;

    /// The row `id`.
    fn find(&self, id: Uuid) -> BoxFuture<'_, Result<Option<MediaItem>, MediaError>>;

    /// The rows `query` selects; with `Some(uow)` read in its
    /// transaction, so rows it wrote and has not committed are included.
    fn list<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        query: &'a MediaQuery,
    ) -> BoxFuture<'a, Result<Vec<MediaItem>, MediaError>>;
}
