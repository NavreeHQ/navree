//! The `database` driver: the `media` table.

use futures_util::future::BoxFuture;
use navree_db::{Db, Repo, Uow};
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DbErr, EntityTrait, IntoActiveModel,
    QueryFilter, QueryOrder, QuerySelect,
};
use serde_json::Value;
use uuid::Uuid;

use super::{MediaQuery, MediaStore};
use crate::entity::{self, Column, Entity};
use crate::{MediaError, MediaItem};

/// The `media` table of the app database (`media.store = "database"`, the
/// default). Writes go through `Repo`, so they record change events (the
/// audit trail sees `media` rows like any other).
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
    async fn write<T, F>(&self, uow: Option<&Uow>, write: F) -> Result<T, MediaError>
    where
        F: for<'u> FnOnce(&'u Uow) -> BoxFuture<'u, Result<T, DbErr>>,
    {
        if let Some(uow) = uow {
            return Ok(write(uow).await?);
        }
        if let Some(current) = Uow::current()
            && !current.is_finished().await
        {
            return Ok(write(&current).await?);
        }
        let own = self.db.uow(None, None).await;
        match write(&own).await {
            Ok(value) => {
                own.commit().await?;
                Ok(value)
            }
            Err(error) => {
                let _ = own.rollback().await;
                Err(error.into())
            }
        }
    }

    /// Deletes the media of one model inside a transaction an observer was
    /// given, without change events, and returns the deleted rows.
    pub(crate) async fn delete_owner_in<C: ConnectionTrait>(
        conn: &C,
        model_type: &str,
        model_id: &str,
    ) -> Result<Vec<MediaItem>, DbErr> {
        let rows = Entity::find()
            .filter(Column::ModelType.eq(model_type))
            .filter(Column::ModelId.eq(model_id))
            .all(conn)
            .await?;
        if !rows.is_empty() {
            Entity::delete_many()
                .filter(Column::ModelType.eq(model_type))
                .filter(Column::ModelId.eq(model_id))
                .exec(conn)
                .await?;
        }
        Ok(rows.into_iter().map(from_model).collect())
    }
}

impl MediaStore for DatabaseStore {
    fn insert<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        item: &'a MediaItem,
    ) -> BoxFuture<'a, Result<(), MediaError>> {
        Box::pin(async move {
            let model = to_model(item);
            self.write(uow, move |uow| {
                Box::pin(async move {
                    let am = model.into_active_model().reset_all();
                    Repo::<Entity>::insert(uow, am).await.map(|_| ())
                })
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
            let model = to_model(item);
            let id = item.id;
            self.write(uow, move |uow| {
                Box::pin(async move {
                    let am = model.into_active_model().reset_all();
                    Repo::<Entity>::update(uow, am).await.map(|_| ())
                })
            })
            .await
            .map_err(|error| match error {
                MediaError::Db(DbErr::RecordNotFound(_)) => MediaError::NotFound { id },
                other => other,
            })
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
            self.write(uow, move |uow| {
                Box::pin(async move {
                    let tx = uow.tx().await?;
                    let rows = Entity::find()
                        .filter(Column::Id.is_in(ids))
                        .all(&*tx)
                        .await?;
                    drop(tx);
                    let count = rows.len() as u64;
                    for row in rows {
                        Repo::<Entity>::delete(uow, row).await?;
                    }
                    Ok(count)
                })
            })
            .await
        })
    }

    fn find(&self, id: Uuid) -> BoxFuture<'_, Result<Option<MediaItem>, MediaError>> {
        Box::pin(async move {
            let row = Entity::find_by_id(id).one(&self.db.reader().await).await?;
            Ok(row.map(from_model))
        })
    }

    fn list<'a>(
        &'a self,
        query: &'a MediaQuery,
    ) -> BoxFuture<'a, Result<Vec<MediaItem>, MediaError>> {
        Box::pin(async move {
            let mut select = Entity::find();
            if let Some(model_type) = &query.model_type {
                select = select.filter(Column::ModelType.eq(model_type.as_str()));
            }
            if let Some(model_id) = &query.model_id {
                select = select.filter(Column::ModelId.eq(model_id.as_str()));
            }
            if let Some(collection) = &query.collection {
                select = select.filter(Column::Collection.eq(collection.as_str()));
            }
            if let Some(ids) = &query.ids {
                select = select.filter(Column::Id.is_in(ids.clone()));
            }
            if let Some(after) = query.after {
                select = select.filter(Expr::col(Column::Id).gt(after));
            }
            select = if query.pages() {
                select.order_by_asc(Column::Id)
            } else {
                select
                    .order_by_asc(Column::OrderColumn)
                    .order_by_asc(Column::Id)
            };
            if let Some(limit) = query.limit {
                select = select.limit(limit);
            }
            let rows = select.all(&self.db.reader().await).await?;
            Ok(rows.into_iter().map(from_model).collect())
        })
    }
}

fn json_object(value: Value) -> serde_json::Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => serde_json::Map::new(),
    }
}

pub(crate) fn from_model(model: entity::Model) -> MediaItem {
    MediaItem {
        id: model.id,
        model_type: model.model_type,
        model_id: model.model_id,
        collection: model.collection,
        name: model.name,
        file_name: model.file_name,
        mime_type: model.mime_type,
        disk: model.disk,
        conversions_disk: model.conversions_disk,
        path: model.path,
        size: u64::try_from(model.size).unwrap_or_default(),
        width: model.width.and_then(|w| u32::try_from(w).ok()),
        height: model.height.and_then(|h| u32::try_from(h).ok()),
        conversions: serde_json::from_value(model.conversions).unwrap_or_default(),
        metadata: json_object(model.metadata),
        custom_properties: json_object(model.custom_properties),
        order: model.order_column,
        created_at: model.created_at,
        updated_at: model.updated_at,
    }
}

fn to_model(item: &MediaItem) -> entity::Model {
    entity::Model {
        id: item.id,
        model_type: item.model_type.clone(),
        model_id: item.model_id.clone(),
        collection: item.collection.clone(),
        name: item.name.clone(),
        file_name: item.file_name.clone(),
        mime_type: item.mime_type.clone(),
        disk: item.disk.clone(),
        conversions_disk: item.conversions_disk.clone(),
        path: item.path.clone(),
        size: i64::try_from(item.size).unwrap_or(i64::MAX),
        width: item.width.and_then(|w| i32::try_from(w).ok()),
        height: item.height.and_then(|h| i32::try_from(h).ok()),
        conversions: serde_json::to_value(&item.conversions).unwrap_or(Value::Null),
        metadata: Value::Object(item.metadata.clone()),
        custom_properties: Value::Object(item.custom_properties.clone()),
        order_column: item.order,
        created_at: item.created_at,
        updated_at: item.updated_at,
    }
}
