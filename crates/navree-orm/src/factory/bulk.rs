//! One `INSERT` for many new models ([`Factory::insert`](super::Factory::insert)):
//! keys and timestamps are generated as a save generates them, but no hook
//! runs and no change event is recorded.

use std::borrow::Cow;

use chrono::{DateTime, Utc};

use crate::error::{OrmError, Result};
use crate::exec::Target;
use crate::ir::{Expr, InsertIr};
use crate::model::{KeyKind, Model, PrimaryKey};
use crate::value::Field;

/// Inserts `models` in one statement on `target`; returns how many rows
/// were inserted.
///
/// Empty keys are generated (UUIDs, ULIDs) or left to the database
/// (incrementing keys: then every model must leave its key empty, or none);
/// empty creation and update timestamps are set to `now`.
pub(crate) async fn insert_models<M: Model>(
    target: &Target<'_>,
    mut models: Vec<M>,
    now: DateTime<Utc>,
) -> Result<u64> {
    if models.is_empty() {
        return Ok(0);
    }
    let meta = M::meta();
    let empty_keys = models.iter().filter(|model| model.key().is_empty()).count();
    let assigned_by_database = match meta.key_kind() {
        KeyKind::Increments if empty_keys == models.len() => true,
        KeyKind::Increments if empty_keys == 0 => false,
        KeyKind::Increments => {
            return Err(OrmError::factory(
                meta.name(),
                "a bulk insert needs every record to leave its incrementing key empty, or none to",
            ));
        }
        KeyKind::Manual if empty_keys > 0 => {
            return Err(OrmError::MissingKey {
                model: Cow::Borrowed(meta.name()),
                column: meta.key_column(),
            });
        }
        _ => false,
    };
    for model in &mut models {
        if model.key().is_empty() && !assigned_by_database {
            let kind = meta.key_kind();
            let key = M::Key::generate(kind).ok_or_else(|| {
                OrmError::invalid(format!(
                    "{}'s key type {kind:?} cannot be generated for its key field of type {}",
                    meta.name(),
                    M::Key::type_name()
                ))
            })?;
            model.set_key(key);
        }
        for column in [meta.created_at(), meta.updated_at()].into_iter().flatten() {
            if model.timestamp_is_empty(column) {
                model.touch_timestamp(column, now)?;
            }
        }
    }
    let mut columns: Vec<Cow<'static, str>> = Vec::new();
    let mut rows = Vec::with_capacity(models.len());
    for (index, model) in models.iter().enumerate() {
        let values = model.values()?;
        let row: Vec<(&'static str, Expr)> = values
            .into_iter()
            .filter(|(column, _)| !(assigned_by_database && *column == meta.key_column()))
            .map(|(column, value)| (column, Expr::Value(value)))
            .collect();
        if index == 0 {
            columns = row.iter().map(|(column, _)| Cow::Borrowed(*column)).collect();
        }
        rows.push(row.into_iter().map(|(_, value)| value).collect());
    }
    let insert = InsertIr {
        table: Cow::Borrowed(meta.table()),
        columns,
        rows,
        returning: Vec::new(),
    };
    Ok(target.insert(&insert).await?.rows_affected)
}
