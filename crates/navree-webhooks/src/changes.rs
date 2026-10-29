//! The change events of endpoint writes.
//!
//! An endpoint row holds its signing secrets, so it is written without the
//! ORM's own change events (which carry every stored column, the encrypted
//! secrets included) and recorded here by hand, in the serialized form of
//! the model: `secret` and `previous_secret` are `skip_serializing`, so the
//! event, and the audit entry written from it, never holds either.

use navree_db::{ChangeEvent, ChangeOp, Uow};
use serde_json::{Map, Value};

use crate::entity::endpoint;

const TABLE: &str = "webhook_endpoints";

/// `model` serialized: every field but the secrets.
fn visible(model: &endpoint::Model) -> Value {
    // Plain fields: serializing cannot fail.
    serde_json::to_value(model).unwrap_or(Value::Null)
}

/// Records that `model` was created (the whole visible record).
pub(crate) fn created(uow: &Uow, model: &endpoint::Model) {
    uow.record_change(ChangeEvent {
        entity: TABLE.to_owned(),
        id: model.id.to_string(),
        op: ChangeOp::Created,
        old: None,
        new: Some(visible(model)),
    });
}

/// Records that `old` became `new`: the visible fields that changed (and
/// nothing when only hidden ones did).
pub(crate) fn updated(uow: &Uow, old: &endpoint::Model, new: &endpoint::Model) {
    let (Value::Object(old_fields), Value::Object(new_fields)) = (visible(old), visible(new))
    else {
        return;
    };
    let mut before = Map::new();
    let mut after = Map::new();
    for (key, value) in new_fields {
        let previous = old_fields.get(&key).cloned().unwrap_or(Value::Null);
        if previous != value {
            before.insert(key.clone(), previous);
            after.insert(key, value);
        }
    }
    if after.is_empty() {
        return;
    }
    uow.record_change(ChangeEvent {
        entity: TABLE.to_owned(),
        id: new.id.to_string(),
        op: ChangeOp::Updated,
        old: Some(Value::Object(before)),
        new: Some(Value::Object(after)),
    });
}

/// Records that `old` was deleted (the whole visible record).
pub(crate) fn deleted(uow: &Uow, old: &endpoint::Model) {
    uow.record_change(ChangeEvent {
        entity: TABLE.to_owned(),
        id: old.id.to_string(),
        op: ChangeOp::Deleted,
        old: Some(visible(old)),
        new: None,
    });
}
