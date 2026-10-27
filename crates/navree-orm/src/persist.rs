//! The persistence flows of a [`Record`]: insert, update, delete, refresh,
//! touch and increment, with their lifecycle hooks, timestamps, generated
//! keys and change events.
//!
//! Insert: `saving` → `creating` → generate an empty key → fill empty
//! timestamps → `INSERT` → read a database-assigned key back → `created`
//! change event → `created` → `saved` → sync the original.
//!
//! Update: `saving` → (only when dirty) `updating` → `updated_at` unless
//! the caller changed it → `UPDATE` of the dirty columns by the original
//! key → sync the changes → `updated` change event → `updated` → `saved` →
//! sync the original. A clean record still runs `saving` and `saved`.
//!
//! Delete: `deleting` → `DELETE` (soft-deleting models: `UPDATE` of the
//! deleted-at and updated-at columns, unless the record is being force
//! deleted) → `deleted` change event → (`trashed` for a soft delete) →
//! `deleted`.

use std::borrow::Cow;

use navree_db::{ChangeEvent, ChangeOp};

use crate::column::{Assignment, Column};
use crate::error::{OrmError, Result};
use crate::exec::{ExecContext, Target};
use crate::hooks::{Flow, Hooks, ModelEvent};
use crate::ir::{
    ArithmeticOp, ColumnRef, DeleteIr, Expr, InsertIr, Operator, TableRef, UpdateIr, Where,
    WhereClause,
};
use crate::model::{KeyKind, Model, PrimaryKey};
use crate::query::Builder;
use crate::record::Record;
use crate::value::{Field, Numeric, Value, is_null, to_json};

/// `WHERE table.key = key`.
fn by_key<M: Model>(key: &M::Key) -> Vec<WhereClause> {
    let meta = M::meta();
    vec![WhereClause::and(Where::Compare {
        left: Expr::Column(ColumnRef::new(Some(meta.table()), meta.key_column())),
        op: Operator::Eq,
        right: Expr::Value(key.to_value()),
    })]
}

/// The key the stored row has: the original one (the key column may have
/// been changed in memory).
fn stored_key<M: Model>(record: &Record<M>) -> M::Key {
    match &record.original {
        Some(original) => original.key(),
        None => record.model.key(),
    }
}

/// The conditions identifying `record`'s stored row: its key and, for a
/// pivot model without an `id` column (`#[model(pivot(..))]`, keyed by one
/// pivot column), the other pivot column too.
fn identity<M: Model>(record: &Record<M>) -> Result<Vec<WhereClause>> {
    let meta = M::meta();
    let mut wheres = by_key::<M>(&stored_key(record));
    // A pivot model keyed by one of its two pivot keys is identified by
    // both; one with an id of its own, by the id alone.
    let pivot_keys = meta.pivot_key_columns().filter(|(foreign, related)| {
        meta.key_column() == *foreign || meta.key_column() == *related
    });
    if let Some((foreign, related)) = pivot_keys {
        let other = if meta.key_column() == related {
            foreign
        } else {
            related
        };
        let source = record.original.as_ref().unwrap_or(&record.model);
        let value = source
            .value(other)?
            .ok_or_else(|| crate::model::derive_support::unknown_column::<M>(other))?;
        wheres.push(WhereClause::and(Where::Compare {
            left: Expr::Column(ColumnRef::new(Some(meta.table()), other)),
            op: Operator::Eq,
            right: Expr::Value(value),
        }));
    }
    Ok(wheres)
}

/// `values` as a JSON object (change events).
fn json_object(values: &[(&'static str, Value)]) -> serde_json::Value {
    serde_json::Value::Object(
        values
            .iter()
            .map(|(column, value)| ((*column).to_owned(), to_json(value)))
            .collect(),
    )
}

/// Records a change in the unit of work the statement ran in, if any.
fn record_change<M: Model>(
    target: &Target<'_>,
    key: &M::Key,
    op: ChangeOp,
    old: Option<serde_json::Value>,
    new: Option<serde_json::Value>,
) {
    if let Some(uow) = target.uow() {
        uow.record_change(ChangeEvent {
            entity: M::meta().table().to_owned(),
            id: key.to_key_string(),
            op,
            old,
            new,
        });
    }
}

/// Inserts or updates `record`.
pub(crate) async fn save<M: Model>(target: &Target<'_>, record: &mut Record<M>) -> Result<bool> {
    let ctx = target.context();
    let hooks = Hooks::<M>::resolve(&ctx).await?;
    if hooks.before(ModelEvent::Saving, &ctx, record).await? == Flow::Cancel {
        return Ok(false);
    }
    let changed = !record.exists || record.is_dirty();
    let saved = if record.exists {
        if record.is_dirty() {
            update(target, &ctx, &hooks, record).await?
        } else {
            true
        }
    } else {
        insert(target, &ctx, &hooks, record).await?
    };
    if !saved {
        return Ok(false);
    }
    hooks.after(ModelEvent::Saved, &ctx, record).await?;
    if changed {
        crate::relations::touch::touch_owners(target, record).await?;
    }
    record.sync_original();
    Ok(true)
}

async fn insert<M: Model>(
    target: &Target<'_>,
    ctx: &ExecContext,
    hooks: &Hooks<M>,
    record: &mut Record<M>,
) -> Result<bool> {
    if hooks.before(ModelEvent::Creating, ctx, record).await? == Flow::Cancel {
        return Ok(false);
    }
    let meta = M::meta();

    let mut assigned_by_database = false;
    if record.model.key().is_empty() {
        match meta.key_kind() {
            KeyKind::Increments => assigned_by_database = true,
            KeyKind::Manual => {
                return Err(OrmError::MissingKey {
                    model: Cow::Borrowed(meta.name()),
                    column: meta.key_column(),
                });
            }
            kind => {
                let key = M::Key::generate(kind).ok_or_else(|| {
                    OrmError::invalid(format!(
                        "{}'s key type {kind:?} cannot be generated for its key field of type {}",
                        meta.name(),
                        M::Key::type_name()
                    ))
                })?;
                record.model.set_key(key);
            }
        }
    }

    if meta.uses_timestamps() {
        let now = ctx.now().await;
        for column in [meta.created_at(), meta.updated_at()].into_iter().flatten() {
            if record.model.timestamp_is_empty(column) {
                record.model.touch_timestamp(column, now)?;
            }
        }
    }

    let values = record.model.values()?;
    let (columns, row): (Vec<Cow<'static, str>>, Vec<Expr>) = values
        .iter()
        .filter(|(column, _)| !(assigned_by_database && *column == meta.key_column()))
        .map(|(column, value)| (Cow::Borrowed(*column), Expr::Value(value.clone())))
        .unzip();
    let insert = InsertIr {
        table: Cow::Borrowed(meta.table()),
        columns,
        rows: vec![row],
        returning: if assigned_by_database {
            vec![Cow::Borrowed(meta.key_column())]
        } else {
            Vec::new()
        },
    };
    let outcome = target.insert(&insert).await?;

    if assigned_by_database {
        let key = match (outcome.returned.first(), outcome.last_insert_id) {
            (Some(row), _) => row.value(meta.key_column())?,
            (None, Some(id)) => Value::BigUnsigned(Some(id)),
            (None, None) => {
                return Err(OrmError::invalid(format!(
                    "the database did not return the key of the new {}",
                    meta.name()
                )));
            }
        };
        record.model.set_value(meta.key_column(), key)?;
    }

    record.exists = true;
    record.recently_created = true;
    record.changes.clear();
    record.previous.clear();
    let key = record.model.key();
    let stored = record.model.values()?;
    record_change::<M>(
        target,
        &key,
        ChangeOp::Created,
        None,
        Some(json_object(&stored)),
    );
    hooks.after(ModelEvent::Created, ctx, record).await?;
    Ok(true)
}

async fn update<M: Model>(
    target: &Target<'_>,
    ctx: &ExecContext,
    hooks: &Hooks<M>,
    record: &mut Record<M>,
) -> Result<bool> {
    if hooks.before(ModelEvent::Updating, ctx, record).await? == Flow::Cancel {
        return Ok(false);
    }
    let meta = M::meta();
    if let Some(updated_at) = meta.updated_at()
        && !record.is_dirty_on([updated_at])
    {
        let now = ctx.now().await;
        record.model.touch_timestamp(updated_at, now)?;
    }
    let dirty = record.get_dirty()?;
    if dirty.is_empty() {
        return Ok(true);
    }
    let key = stored_key(record);
    let update = UpdateIr {
        table: TableRef::new(meta.table()),
        sets: dirty
            .iter()
            .map(|(column, value)| (Cow::Borrowed(*column), Expr::Value(value.clone())))
            .collect(),
        wheres: identity(record)?,
    };
    target.update(&update).await?;
    record.sync_changes(dirty)?;
    record_change::<M>(
        target,
        &key,
        ChangeOp::Updated,
        Some(json_object(&record.previous)),
        Some(json_object(&record.changes)),
    );
    hooks.after(ModelEvent::Updated, ctx, record).await?;
    Ok(true)
}

/// Deletes `record`: soft-deletes it when the model soft-deletes (firing
/// `deleting`, `trashed`, `deleted`), removes the row otherwise or while
/// the record is being force deleted (`deleting`, `deleted`).
pub(crate) async fn delete<M: Model>(target: &Target<'_>, record: &mut Record<M>) -> Result<bool> {
    if !record.exists {
        return Ok(false);
    }
    let ctx = target.context();
    let hooks = Hooks::<M>::resolve(&ctx).await?;
    if hooks.before(ModelEvent::Deleting, &ctx, record).await? == Flow::Cancel {
        return Ok(false);
    }
    // The owners are touched while the row (and its pivot rows) still
    // exist.
    crate::relations::touch::touch_owners(target, record).await?;
    let meta = M::meta();
    let key = stored_key(record);
    let wheres = identity(record)?;
    let before = match &record.original {
        Some(original) => original.values()?,
        None => record.model.values()?,
    };

    match meta.deleted_at().filter(|_| !record.force_deleting) {
        Some(deleted_at) => {
            let now = ctx.now().await;
            record.model.touch_timestamp(deleted_at, now)?;
            let mut columns = vec![deleted_at];
            if let Some(updated_at) = meta.updated_at() {
                record.model.touch_timestamp(updated_at, now)?;
                columns.push(updated_at);
            }
            let mut sets = Vec::with_capacity(columns.len());
            for column in &columns {
                let value = record
                    .model
                    .value(column)?
                    .ok_or_else(|| crate::model::derive_support::unknown_column::<M>(column))?;
                sets.push((Cow::Borrowed(*column), Expr::Value(value)));
            }
            let update = UpdateIr {
                table: TableRef::new(meta.table()),
                sets,
                wheres,
            };
            target.update(&update).await?;
            record.sync_original_on(columns)?;
            record_change::<M>(
                target,
                &key,
                ChangeOp::Deleted,
                Some(json_object(&before)),
                None,
            );
            hooks.after(ModelEvent::Trashed, &ctx, record).await?;
            hooks.after(ModelEvent::Deleted, &ctx, record).await?;
        }
        None => {
            let delete = DeleteIr {
                table: TableRef::new(meta.table()),
                wheres,
            };
            target.delete(&delete).await?;
            record.exists = false;
            record_change::<M>(
                target,
                &key,
                ChangeOp::Deleted,
                Some(json_object(&before)),
                None,
            );
            hooks.after(ModelEvent::Deleted, &ctx, record).await?;
        }
    }
    Ok(true)
}

/// The stored row of `record`, read without global scopes.
async fn reload<M: Model>(target: &Target<'_>, record: &Record<M>) -> Result<Option<Record<M>>> {
    let mut query = Builder::<M>::new().without_global_scopes().limit(1);
    query.ir.wheres = identity(record)?;
    query
        .fetch(target)
        .await
        .map(|records| records.into_vec().into_iter().next())
}

/// Reloads `record`'s columns, and the relations it has loaded (pivots and
/// values set by hand stay as they are).
pub(crate) async fn refresh<M: Model>(target: &Target<'_>, record: &mut Record<M>) -> Result<()> {
    if !record.exists {
        return Ok(());
    }
    let fresh = reload(target, record).await?.ok_or_else(|| {
        OrmError::not_found(M::meta().name(), vec![stored_key(record).to_key_string()])
    })?;
    record.model = fresh.model;
    record.extras = fresh.extras;
    record.sync_original();
    crate::relations::lazy::reload_relations(target, record).await
}

/// `record` as stored now.
pub(crate) async fn fresh<M: Model>(
    target: &Target<'_>,
    record: &Record<M>,
) -> Result<Option<Record<M>>> {
    if !record.exists {
        return Ok(None);
    }
    reload(target, record).await
}

/// Sets the update timestamp (or `column`) to now and saves.
pub(crate) async fn touch<M: Model>(
    target: &Target<'_>,
    record: &mut Record<M>,
    column: Option<&str>,
) -> Result<bool> {
    let ctx = target.context();
    let meta = M::meta();
    match column {
        None => {
            let Some(updated_at) = meta.updated_at() else {
                return Ok(false);
            };
            let now = ctx.now().await;
            record.model.touch_timestamp(updated_at, now)?;
            if !record.exists
                && let Some(created_at) = meta.created_at()
                && record.model.timestamp_is_empty(created_at)
            {
                record.model.touch_timestamp(created_at, now)?;
            }
        }
        Some(column) => {
            let now = ctx.now().await;
            record
                .model
                .set_value(column, Value::ChronoDateTimeUtc(Some(now)))?;
        }
    }
    save(target, record).await
}

/// `column = column ± amount` for each of `steps` on the stored record (in
/// one statement), plus `extra` columns and the update timestamp.
pub(crate) async fn increment<M: Model, T: Numeric>(
    target: &Target<'_>,
    record: &mut Record<M>,
    steps: Vec<(Column<M, T>, T)>,
    decrement: bool,
    extra: Vec<Assignment>,
) -> Result<bool> {
    let action = if decrement { "decrement" } else { "increment" };
    if !record.exists {
        return Err(Record::<M>::not_persisted(action));
    }
    let meta = M::meta();
    let names: Vec<String> = steps
        .iter()
        .map(|(column, _)| column.column_ref().name.into_owned())
        .collect();
    if names.is_empty() {
        return Err(OrmError::invalid(format!(
            "{action} needs at least one column of {}",
            meta.name()
        )));
    }
    if let Some(twice) = names
        .iter()
        .enumerate()
        .find_map(|(at, name)| names[..at].contains(name).then_some(name))
    {
        return Err(OrmError::invalid(format!(
            "{} `{twice}` of {} is listed twice: each column steps once per statement",
            if decrement {
                "decrementing"
            } else {
                "incrementing"
            },
            meta.name()
        )));
    }

    let mut sets: Vec<(Cow<'static, str>, Expr)> = Vec::with_capacity(steps.len());
    let mut written = Vec::with_capacity(steps.len());
    for ((_, amount), name) in steps.into_iter().zip(names) {
        let current = record
            .model
            .value(&name)?
            .ok_or_else(|| crate::model::derive_support::unknown_column::<M>(&name))?;
        let next = if is_null(&current) {
            current
        } else {
            let current = T::from_value(current).map_err(|error| {
                crate::row::decode_error(meta.name(), &name, T::type_name(), &error)
            })?;
            let next = if decrement {
                current.checked_sub(amount)
            } else {
                current.checked_add(amount)
            };
            next.ok_or_else(|| {
                OrmError::invalid(format!(
                    "{} `{name}` would overflow {}",
                    if decrement {
                        "decrementing"
                    } else {
                        "incrementing"
                    },
                    T::type_name()
                ))
            })?
            .to_value()
        };
        record.model.set_value(&name, next)?;
        sets.push((
            Cow::Owned(name.clone()),
            Expr::Arithmetic {
                left: Box::new(Expr::Column(ColumnRef::new(None::<&str>, name.clone()))),
                op: if decrement {
                    ArithmeticOp::Sub
                } else {
                    ArithmeticOp::Add
                },
                right: Box::new(Expr::Value(amount.to_value())),
            },
        ));
        written.push(name);
    }
    for assignment in extra {
        let (extra_column, value) = assignment.into_set().map_err(OrmError::invalid)?;
        if let Expr::Value(value) = &value {
            record.model.set_value(&extra_column, value.clone())?;
        }
        written.push(extra_column.to_string());
        sets.push((extra_column, value));
    }
    let ctx = target.context();
    if let Some(updated_at) = meta.updated_at()
        && !written.iter().any(|column| column == updated_at)
    {
        let now = ctx.now().await;
        record.model.touch_timestamp(updated_at, now)?;
        let value = record
            .model
            .value(updated_at)?
            .unwrap_or(Value::String(None));
        sets.push((Cow::Borrowed(updated_at), Expr::Value(value)));
        written.push(updated_at.to_owned());
    }

    let hooks = Hooks::<M>::resolve(&ctx).await?;
    // Cancelled: the new values stay on the record, unsaved (dirty).
    if hooks.before(ModelEvent::Updating, &ctx, record).await? == Flow::Cancel {
        return Ok(false);
    }
    let key = stored_key(record);
    let update = UpdateIr {
        table: TableRef::new(meta.table()),
        sets,
        wheres: identity(record)?,
    };
    target.update(&update).await?;
    let dirty = record.get_dirty()?;
    record.sync_changes(dirty)?;
    record_change::<M>(
        target,
        &key,
        ChangeOp::Updated,
        Some(json_object(&record.previous)),
        Some(json_object(&record.changes)),
    );
    hooks.after(ModelEvent::Updated, &ctx, record).await?;
    record.sync_original_on(written.iter().map(String::as_str))?;
    Ok(true)
}
