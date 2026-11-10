//! Writes of the module's tables through the unit of work, with their
//! change events.
//!
//! Each write is an ORM statement that records no event of its own (a bulk
//! insert, a set-based update or delete); the event is recorded here from
//! the row's [`event_form`](Recorded::event_form), which leaves the
//! connection settings' secrets and certificates out. An update records
//! only the fields that changed (none: no event).

use navree_db::{ChangeEvent, ChangeOp, DbError, Uow};
use navree_orm::Assignment;
use serde_json::{Map, Value};

use crate::SsoError;
use crate::entity::Recorded;

/// Inserts `row` and records a `created` event with the stored row; the
/// stored row.
pub(crate) async fn insert<M: Recorded>(uow: &Uow, row: M) -> Result<M, SsoError> {
    let id = row.id();
    M::insert(uow, [row]).await?;
    let stored = stored::<M>(uow, id).await?;
    uow.record_change(ChangeEvent {
        entity: M::TABLE.to_owned(),
        id: id.to_string(),
        op: ChangeOp::Created,
        old: None,
        new: Some(stored.event_form()?),
    });
    Ok(stored)
}

/// Updates row `id` with `sets` and records an `updated` event with the
/// fields that changed. The row is read first, locked `FOR UPDATE` where
/// the database supports it; a missing row is a [`DbError`] of kind
/// `NotFound` (a 404 over HTTP). The stored row.
pub(crate) async fn update<M: Recorded>(
    uow: &Uow,
    id: uuid::Uuid,
    sets: Vec<Assignment>,
) -> Result<M, SsoError> {
    let mut query = M::query().where_key(id);
    if uow.lock_for_update() {
        query = query.lock_for_update();
    }
    let old = query
        .first(uow)
        .await?
        .ok_or_else(|| missing::<M>(id))?
        .into_inner();
    if M::query().where_key(id).update(uow, sets).await? == 0 {
        return Err(missing::<M>(id));
    }
    let stored = stored::<M>(uow, id).await?;
    let (before, after) = changed_fields(old.event_form()?, stored.event_form()?);
    if !after.is_empty() {
        uow.record_change(ChangeEvent {
            entity: M::TABLE.to_owned(),
            id: id.to_string(),
            op: ChangeOp::Updated,
            old: Some(before.into()),
            new: Some(after.into()),
        });
    }
    Ok(stored)
}

/// Deletes `row` and records a `deleted` event with it; a row that is gone
/// is a [`DbError`] of kind `NotFound`.
pub(crate) async fn delete<M: Recorded>(uow: &Uow, row: M) -> Result<(), SsoError> {
    let id = row.id();
    if M::query().where_key(id).delete(uow).await? == 0 {
        return Err(missing::<M>(id));
    }
    uow.record_change(ChangeEvent {
        entity: M::TABLE.to_owned(),
        id: id.to_string(),
        op: ChangeOp::Deleted,
        old: Some(row.event_form()?),
        new: None,
    });
    Ok(())
}

/// The row `id` as the database stored it (timestamps at its precision).
async fn stored<M: Recorded>(uow: &Uow, id: uuid::Uuid) -> Result<M, SsoError> {
    Ok(M::find(uow, id)
        .await?
        .ok_or_else(|| missing::<M>(id))?
        .into_inner())
}

fn missing<M: Recorded>(id: uuid::Uuid) -> SsoError {
    SsoError::Db(DbError::not_found(format!("{} {id}", M::TABLE)))
}

type Fields = Map<String, Value>;

/// The fields of `new` whose values differ from `old`: (old values, new
/// values).
fn changed_fields(old: Value, new: Value) -> (Fields, Fields) {
    let (Value::Object(old), Value::Object(new)) = (old, new) else {
        return (Fields::new(), Fields::new());
    };
    let mut before = Fields::new();
    let mut after = Fields::new();
    for (key, value) in new {
        let previous = old.get(&key).cloned().unwrap_or(Value::Null);
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
            json!({"name": "Acme SSO", "slug": "acme"}),
        );
        assert_eq!(Value::from(before), json!({"name": "Acme"}));
        assert_eq!(Value::from(after), json!({"name": "Acme SSO"}));
    }
}
