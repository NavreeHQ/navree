//! The module's own writes: navree-orm statements in a unit of work that
//! record the change events audit trails read.
//!
//! The events carry each model's `Serialize` form, so a field kept out of
//! it (an invitation's token digest) never reaches an observer; an update
//! records only the fields that changed. Rows are read back after a write, so callers get them as the
//! database stored them (timestamps at its precision).

use navree_db::{ChangeEvent, ChangeOp, Uow};
use navree_orm::{Assignment, Model, OrmError, PrimaryKey};

use crate::WorkspaceError;

/// Inserts `model` and records a `created` event; the stored row.
pub(crate) async fn insert<M: Model>(uow: &Uow, model: M) -> Result<M, WorkspaceError> {
    let key = model.key();
    M::insert(uow, [model]).await?;
    let stored = stored::<M>(uow, key.clone()).await?;
    uow.record_change(ChangeEvent {
        entity: M::meta().table().to_owned(),
        id: key.to_key_string(),
        op: ChangeOp::Created,
        old: None,
        new: Some(json(&stored)?),
    });
    Ok(stored)
}

/// Applies `change` to `current` and writes the columns that changed,
/// recording an `updated` event with those fields (none changed: nothing is
/// written or recorded). A 404 when the row is gone. The stored row.
pub(crate) async fn update<M: Model>(
    uow: &Uow,
    current: M,
    change: impl FnOnce(&mut M),
) -> Result<M, WorkspaceError> {
    let mut new = current.clone();
    change(&mut new);
    let mut sets = Vec::new();
    for column in new.changed_columns(&current)? {
        let value = new.value(column)?.ok_or_else(|| {
            OrmError::InvalidQuery(format!(
                "`{column}` is not a column of `{}`",
                M::meta().table()
            ))
        })?;
        sets.push(Assignment::value(column, value));
    }
    if sets.is_empty() {
        return Ok(current);
    }
    let key = current.key();
    if M::query().where_key(key.clone()).update(uow, sets).await? == 0 {
        return Err(missing::<M>(&key).into());
    }
    let stored = stored::<M>(uow, key.clone()).await?;
    let (before, after) = changed_fields(json(&current)?, json(&stored)?);
    if !after.is_empty() {
        uow.record_change(ChangeEvent {
            entity: M::meta().table().to_owned(),
            id: key.to_key_string(),
            op: ChangeOp::Updated,
            old: Some(before.into()),
            new: Some(after.into()),
        });
    }
    Ok(stored)
}

/// Deletes `model` and records a `deleted` event with it; a 404 when no row
/// was deleted.
pub(crate) async fn delete<M: Model>(uow: &Uow, model: M) -> Result<(), WorkspaceError> {
    let key = model.key();
    if M::query().where_key(key.clone()).delete(uow).await? == 0 {
        return Err(missing::<M>(&key).into());
    }
    uow.record_change(ChangeEvent {
        entity: M::meta().table().to_owned(),
        id: key.to_key_string(),
        op: ChangeOp::Deleted,
        old: Some(json(&model)?),
        new: None,
    });
    Ok(())
}

/// The row with `key` as the database stored it.
async fn stored<M: Model>(uow: &Uow, key: M::Key) -> Result<M, WorkspaceError> {
    Ok(M::query()
        .where_key(key.clone())
        .first(uow)
        .await?
        .ok_or_else(|| missing::<M>(&key))?
        .into_inner())
}

/// The error for a row that is not there (a 404).
fn missing<M: Model>(key: &M::Key) -> OrmError {
    OrmError::not_found(M::meta().name(), vec![key.to_key_string()])
}

/// `model` as a change event's values: its `Serialize` form.
fn json<M: Model>(model: &M) -> Result<serde_json::Value, WorkspaceError> {
    serde_json::to_value(model).map_err(|error| {
        WorkspaceError::Other(format!("cannot encode a {}: {error}", M::meta().name()).into())
    })
}

type Fields = serde_json::Map<String, serde_json::Value>;

/// The fields of `new` whose values differ from `old`: (old values, new
/// values).
fn changed_fields(old: serde_json::Value, new: serde_json::Value) -> (Fields, Fields) {
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
    use serde_json::json;

    use super::*;

    #[test]
    fn only_changed_fields_are_kept() {
        let (before, after) = changed_fields(
            json!({"name": "Acme", "slug": "acme"}),
            json!({"name": "Acme Inc", "slug": "acme"}),
        );
        assert_eq!(serde_json::Value::from(before), json!({"name": "Acme"}));
        assert_eq!(serde_json::Value::from(after), json!({"name": "Acme Inc"}));
    }
}
