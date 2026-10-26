//! [`Repo`]: entity writes through the unit of work that record [`ChangeEvent`]s.

use std::marker::PhantomData;

use sea_orm::sea_query::ValueTuple;
use sea_orm::sea_query::value::sea_value_to_json_value;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, DbErr, EntityTrait, IntoActiveModel, Iterable,
    ModelTrait, PrimaryKeyToColumn, QueryFilter, QuerySelect, Select, Value,
};
use serde::Serialize;

use crate::{ChangeEvent, ChangeOp, Uow};

/// Generic entity helpers that run in a [`Uow`]'s transaction and record a
/// [`ChangeEvent`] for every write, so audit trails and other
/// [`ChangeObserver`](crate::ChangeObserver)s see them.
///
/// ```ignore
/// type Deals = Repo<deal::Entity>;
/// let deal = Deals::insert(&uow, deal::ActiveModel { .. }).await?;
/// ```
pub struct Repo<E>(PhantomData<E>);

impl<E> Repo<E>
where
    E: EntityTrait,
    E::Model: Serialize + IntoActiveModel<E::ActiveModel> + Sync,
    E::ActiveModel: Send + Sync,
{
    /// Inserts `am` and records a `created` event with the new record.
    pub async fn insert(uow: &Uow, am: E::ActiveModel) -> Result<E::Model, DbErr> {
        let tx = uow.tx().await?;
        let model = am.insert(&*tx).await?;
        uow.record_change(ChangeEvent {
            entity: table::<E>(),
            id: key_string(&model.get_primary_key_value()),
            op: ChangeOp::Created,
            old: None,
            new: Some(to_json(&model)?),
        });
        Ok(model)
    }

    /// Updates `am` (its primary key must be set) and records an `updated`
    /// event with the changed fields only (none if nothing changed). The
    /// current row is read first (`FOR UPDATE` where supported);
    /// `RecordNotFound` if it does not exist.
    pub async fn update(uow: &Uow, am: E::ActiveModel) -> Result<E::Model, DbErr> {
        let key = am.get_primary_key_value().ok_or(DbErr::PrimaryKeyNotSet {
            ctx: "Repo::update",
        })?;
        let tx = uow.tx().await?;
        let old = lockable(by_key::<E>(&key), uow)
            .one(&*tx)
            .await?
            .ok_or_else(|| not_found::<E>(&key))?;
        let new = am.update(&*tx).await?;
        let (old_fields, new_fields) = diff(to_json(&old)?, to_json(&new)?);
        if !new_fields.is_empty() {
            uow.record_change(ChangeEvent {
                entity: table::<E>(),
                id: key_string(&key),
                op: ChangeOp::Updated,
                old: Some(old_fields.into()),
                new: Some(new_fields.into()),
            });
        }
        Ok(new)
    }

    /// Deletes `model` and records a `deleted` event with the old record;
    /// `RecordNotFound` if no row was deleted.
    pub async fn delete(uow: &Uow, model: E::Model) -> Result<(), DbErr> {
        let key = model.get_primary_key_value();
        let tx = uow.tx().await?;
        let result = model.clone().into_active_model().delete(&*tx).await?;
        if result.rows_affected == 0 {
            return Err(not_found::<E>(&key));
        }
        uow.record_change(ChangeEvent {
            entity: table::<E>(),
            id: key_string(&key),
            op: ChangeOp::Deleted,
            old: Some(to_json(&model)?),
            new: None,
        });
        Ok(())
    }

    /// The record with primary key `id` (single-column keys), read in the
    /// unit of work's transaction.
    pub async fn find(uow: &Uow, id: impl Into<Value>) -> Result<Option<E::Model>, DbErr> {
        let select = by_id::<E>(id.into())?;
        select.one(&*uow.tx().await?).await
    }

    /// Like [`find`](Repo::find), but `RecordNotFound` (a 404 over HTTP)
    /// when there is no such record.
    pub async fn find_or_fail(uow: &Uow, id: impl Into<Value>) -> Result<E::Model, DbErr> {
        let id = id.into();
        Self::find(uow, id.clone())
            .await?
            .ok_or_else(|| not_found::<E>(&ValueTuple::One(id)))
    }

    /// Like [`find`](Repo::find), but locks the row (`FOR UPDATE`) where the
    /// backend supports it (see [`Uow::lock_for_update`]).
    pub async fn find_for_update(
        uow: &Uow,
        id: impl Into<Value>,
    ) -> Result<Option<E::Model>, DbErr> {
        let select = lockable(by_id::<E>(id.into())?, uow);
        select.one(&*uow.tx().await?).await
    }
}

impl<E> std::fmt::Debug for Repo<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Repo<{}>", std::any::type_name::<E>())
    }
}

fn table<E: EntityTrait>() -> String {
    E::default().table_name().to_owned()
}

fn lockable<E: EntityTrait>(select: Select<E>, uow: &Uow) -> Select<E> {
    if uow.lock_for_update() {
        select.lock_exclusive()
    } else {
        select
    }
}

/// `SELECT ... WHERE pk = id` for a single-column primary key.
fn by_id<E: EntityTrait>(id: Value) -> Result<Select<E>, DbErr> {
    let mut columns = E::PrimaryKey::iter();
    match (columns.next(), columns.next()) {
        (Some(pk), None) => Ok(E::find().filter(pk.into_column().eq(id))),
        _ => Err(DbErr::KeyArityMismatch {
            expected: u8::try_from(E::PrimaryKey::iter().count()).unwrap_or(u8::MAX),
            received: 1,
        }),
    }
}

/// `SELECT ... WHERE pk1 = v1 AND pk2 = v2 ...`.
fn by_key<E: EntityTrait>(key: &ValueTuple) -> Select<E> {
    let condition = E::PrimaryKey::iter()
        .zip(key.clone())
        .fold(Condition::all(), |condition, (pk, value)| {
            condition.add(pk.into_column().eq(value))
        });
    E::find().filter(condition)
}

fn not_found<E: EntityTrait>(key: &ValueTuple) -> DbErr {
    DbErr::RecordNotFound(format!("{} {}", table::<E>(), key_string(key)))
}

/// The key as a string: `0199...`, or `1,2` for composite keys.
fn key_string(key: &ValueTuple) -> String {
    key.clone()
        .into_iter()
        .map(|value| match sea_value_to_json_value(&value) {
            serde_json::Value::String(s) => s,
            other => other.to_string(),
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn to_json<T: Serialize>(value: &T) -> Result<serde_json::Value, DbErr> {
    serde_json::to_value(value).map_err(|e| DbErr::Json(e.to_string()))
}

type Fields = serde_json::Map<String, serde_json::Value>;

/// The fields of `new` whose values differ from `old`: (old values, new values).
fn diff(old: serde_json::Value, new: serde_json::Value) -> (Fields, Fields) {
    let (serde_json::Value::Object(old), serde_json::Value::Object(new)) = (old, new) else {
        return (Fields::new(), Fields::new());
    };
    let mut before = Fields::new();
    let mut after = Fields::new();
    for (key, value) in new {
        let previous = old.get(&key).cloned().unwrap_or(serde_json::Value::Null);
        if previous != value {
            before.insert(key.clone(), previous);
            after.insert(key, value);
        }
    }
    (before, after)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn diff_keeps_changed_fields_only() {
        let (old, new) = diff(
            json!({"id": 1, "name": "a", "stage": "lead"}),
            json!({"id": 1, "name": "a", "stage": "won"}),
        );
        assert_eq!(serde_json::Value::from(old), json!({"stage": "lead"}));
        assert_eq!(serde_json::Value::from(new), json!({"stage": "won"}));
    }

    #[test]
    fn composite_keys_join_with_commas() {
        let key = ValueTuple::Two(Value::from(1i32), Value::from("x"));
        assert_eq!(key_string(&key), "1,x");
    }
}
