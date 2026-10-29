//! The `database` driver: the `media` table, through navree-orm.

use std::future::Future;

use futures_util::future::BoxFuture;
use navree_db::{Db, DbError, Uow};
use navree_orm::{Model as _, OrmError};
use uuid::Uuid;

use super::{MediaQuery, MediaStore};
use crate::model::MediaRecord;
use crate::{MediaError, MediaItem};

/// The `media` table of the app database (`media.store = "database"`, the
/// default). Writes go through the unit of work, so they record change
/// events (the audit trail sees `media` rows like any other).
#[derive(Clone, Debug)]
pub struct DatabaseStore {
    db: Db,
}

impl DatabaseStore {
    /// The store on `db` (its `media` table comes from the `media`
    /// migration set).
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Runs `write` in `uow`, in the current unit of work when there is an
    /// unfinished one, or in a unit of work of its own that it commits.
    async fn write<T, F, Fut>(&self, uow: Option<&Uow>, write: F) -> Result<T, MediaError>
    where
        F: FnOnce(Uow) -> Fut,
        Fut: Future<Output = Result<T, OrmError>>,
    {
        if let Some(uow) = uow {
            return Ok(write(uow.clone()).await?);
        }
        if let Some(current) = Uow::current()
            && !current.is_finished().await
        {
            return Ok(write(current).await?);
        }
        let own = self.db.uow(None, None).await;
        match write(own.clone()).await {
            Ok(value) => {
                own.commit().await.map_err(DbError::from)?;
                Ok(value)
            }
            Err(error) => {
                let _ = own.rollback().await;
                Err(error.into())
            }
        }
    }
}

impl MediaStore for DatabaseStore {
    fn insert<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        item: &'a MediaItem,
    ) -> BoxFuture<'a, Result<(), MediaError>> {
        Box::pin(async move {
            let row = MediaRecord::from(item);
            self.write(uow, |uow| async move {
                MediaRecord::create(&uow, row).await.map(|_| ())
            })
            .await
        })
    }

    fn update<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        item: &'a MediaItem,
    ) -> BoxFuture<'a, Result<(), MediaError>> {
        Box::pin(async move {
            let id = item.id;
            let row = MediaRecord::from(item);
            let found = self
                .write(uow, |uow| async move {
                    let Some(mut record) = MediaRecord::find(&uow, id).await? else {
                        return Ok(false);
                    };
                    let created_at = record.created_at;
                    *record.model_mut() = MediaRecord { created_at, ..row };
                    record.save(&uow).await?;
                    Ok(true)
                })
                .await?;
            if found {
                Ok(())
            } else {
                Err(MediaError::NotFound { id })
            }
        })
    }

    fn delete<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        ids: &'a [Uuid],
    ) -> BoxFuture<'a, Result<u64, MediaError>> {
        Box::pin(async move {
            if ids.is_empty() {
                return Ok(0);
            }
            let ids = ids.to_vec();
            self.write(
                uow,
                |uow| async move { MediaRecord::destroy(&uow, ids).await },
            )
            .await
        })
    }

    fn delete_owner<'a>(
        &'a self,
        uow: &'a Uow,
        model_type: &'a str,
        model_id: &'a str,
    ) -> BoxFuture<'a, Result<Vec<MediaItem>, MediaError>> {
        Box::pin(async move {
            let rows = MediaRecord::query()
                .where_(MediaRecord::MODEL_TYPE.eq(model_type))
                .where_(MediaRecord::MODEL_ID.eq(model_id))
                .get(uow)
                .await?;
            if !rows.is_empty() {
                MediaRecord::query()
                    .where_(MediaRecord::MODEL_TYPE.eq(model_type))
                    .where_(MediaRecord::MODEL_ID.eq(model_id))
                    .delete(uow)
                    .await?;
            }
            Ok(rows
                .into_iter()
                .map(|record| MediaItem::from(record.into_inner()))
                .collect())
        })
    }

    fn find(&self, id: Uuid) -> BoxFuture<'_, Result<Option<MediaItem>, MediaError>> {
        Box::pin(async move {
            let row = MediaRecord::find(&self.db, id).await?;
            Ok(row.map(|record| MediaItem::from(record.into_inner())))
        })
    }

    fn list<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        query: &'a MediaQuery,
    ) -> BoxFuture<'a, Result<Vec<MediaItem>, MediaError>> {
        Box::pin(async move {
            let mut select = MediaRecord::query();
            if let Some(model_type) = &query.model_type {
                select = select.where_(MediaRecord::MODEL_TYPE.eq(model_type.as_str()));
            }
            if let Some(model_id) = &query.model_id {
                select = select.where_(MediaRecord::MODEL_ID.eq(model_id.as_str()));
            }
            if let Some(collection) = &query.collection {
                select = select.where_(MediaRecord::COLLECTION.eq(collection.as_str()));
            }
            if let Some(ids) = &query.ids {
                select = select.where_in(MediaRecord::ID, ids.iter().copied());
            }
            if let Some(after) = query.after {
                select = select.where_(MediaRecord::ID.gt(after));
            }
            select = if query.pages() {
                select.order_by(MediaRecord::ID)
            } else {
                select
                    .order_by(MediaRecord::ORDER_COLUMN)
                    .order_by(MediaRecord::ID)
            };
            if let Some(limit) = query.limit {
                select = select.limit(limit);
            }
            let rows = match uow {
                Some(uow) => select.get(uow).await?,
                None => select.get(&self.db).await?,
            };
            Ok(rows
                .into_iter()
                .map(|record| MediaItem::from(record.into_inner()))
                .collect())
        })
    }
}
