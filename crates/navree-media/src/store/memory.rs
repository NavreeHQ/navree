//! The `memory` driver.

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};

use futures_util::future::BoxFuture;
use navree_db::Uow;
use uuid::Uuid;

use super::{MediaQuery, MediaStore};
use crate::{MediaError, MediaItem};

/// Rows in process memory, lost on restart (`media.store = "memory"`, and
/// `MediaFake`). Writes apply at once, whatever happens to the unit of
/// work they were given.
#[derive(Debug, Default)]
pub struct MemoryStore {
    rows: Mutex<BTreeMap<Uuid, MediaItem>>,
}

impl MemoryStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    fn rows(&self) -> MutexGuard<'_, BTreeMap<Uuid, MediaItem>> {
        self.rows.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl MediaStore for MemoryStore {
    fn insert<'a>(
        &'a self,
        _uow: Option<&'a Uow>,
        item: &'a MediaItem,
    ) -> BoxFuture<'a, Result<(), MediaError>> {
        Box::pin(async move {
            let mut rows = self.rows();
            if rows.contains_key(&item.id) {
                return Err(MediaError::Db(sea_orm::DbErr::Custom(format!(
                    "media {} already exists",
                    item.id
                ))));
            }
            rows.insert(item.id, item.clone());
            Ok(())
        })
    }

    fn update<'a>(
        &'a self,
        _uow: Option<&'a Uow>,
        item: &'a MediaItem,
    ) -> BoxFuture<'a, Result<(), MediaError>> {
        Box::pin(async move {
            let mut rows = self.rows();
            match rows.get_mut(&item.id) {
                Some(row) => {
                    *row = item.clone();
                    Ok(())
                }
                None => Err(MediaError::NotFound { id: item.id }),
            }
        })
    }

    fn delete<'a>(
        &'a self,
        _uow: Option<&'a Uow>,
        ids: &'a [Uuid],
    ) -> BoxFuture<'a, Result<u64, MediaError>> {
        Box::pin(async move {
            let mut rows = self.rows();
            Ok(ids.iter().filter(|id| rows.remove(id).is_some()).count() as u64)
        })
    }

    fn find(&self, id: Uuid) -> BoxFuture<'_, Result<Option<MediaItem>, MediaError>> {
        Box::pin(async move { Ok(self.rows().get(&id).cloned()) })
    }

    fn list<'a>(
        &'a self,
        query: &'a MediaQuery,
    ) -> BoxFuture<'a, Result<Vec<MediaItem>, MediaError>> {
        Box::pin(async move {
            let rows = self.rows();
            let mut items: Vec<MediaItem> = rows
                .values()
                .filter(|item| query.matches(item))
                .cloned()
                .collect();
            if !query.pages() {
                items.sort_by(|a, b| a.order.cmp(&b.order).then(a.id.cmp(&b.id)));
            }
            if let Some(limit) = query.limit {
                items.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
            }
            Ok(items)
        })
    }
}
