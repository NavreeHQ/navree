//! `users`: the accounts. Emails are stored trimmed and lower-cased.
//!
//! [`Model`] (exported as [`User`](crate::User)) is a navree-orm model and
//! the access subject `User` (permission prefix `users`): register it with
//! `AccessModule::for_feature::<navree_auth::User>()` to load users for
//! `can(.., User, from = "id")` checks (through the ORM, in the request's
//! unit of work) and to make `users.*` permissions grant abilities on
//! `User`. The
//! password hash and the two-factor columns are never serialized (the
//! fields are skipped by `Serialize`, so a `Model` can be returned as JSON
//! and recorded in change events and audit trails) and never appear in
//! `Debug` output.
//!
//! The module's own writes of users go through [`insert`] and [`update`]:
//! ORM statements in a unit of work that record the same change events
//! `navree_db::Repo` recorded (the model's `Serialize` form, so no secret
//! reaches an observer; an update carries only the changed fields).

use std::fmt;

use chrono::{DateTime, Utc};
use navree_db::{ChangeEvent, ChangeOp, DbError, Uow};
use navree_orm::{Assignment, Model as _, OrmError};
use serde::Serialize;
use uuid::Uuid;

use crate::AuthError;

/// The table.
pub(crate) const TABLE: &str = "users";

/// A user account. The struct is called `Model`, so the derive names the
/// trait by path.
#[derive(Clone, PartialEq, Eq, navree_orm::Model, Serialize)]
#[model(table = "users", subject = "User", permission_prefix = "users")]
pub struct Model {
    /// The id (UUID v7).
    pub id: Uuid,
    /// The display name.
    pub name: String,
    /// The unique, lower-cased email address.
    pub email: String,
    /// The Argon2id PHC string. Never serialized.
    #[serde(skip_serializing)]
    pub password_hash: String,
    /// When the email address was verified, if it was.
    pub email_verified_at: Option<DateTime<Utc>>,
    /// The encrypted TOTP secret. Never serialized.
    #[serde(skip_serializing)]
    pub two_factor_secret: Option<String>,
    /// The encrypted recovery codes. Never serialized.
    #[serde(skip_serializing)]
    pub two_factor_recovery_codes: Option<String>,
    /// When two-factor authentication was confirmed. Never serialized.
    #[serde(skip_serializing)]
    pub two_factor_confirmed_at: Option<DateTime<Utc>>,
    /// When the account was created.
    pub created_at: DateTime<Utc>,
    /// When the account last changed.
    pub updated_at: DateTime<Utc>,
}

/// The password hash and two-factor data are redacted.
impl fmt::Debug for Model {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("User")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("email", &self.email)
            .field("email_verified_at", &self.email_verified_at)
            .field(
                "two_factor_enabled",
                &self.two_factor_confirmed_at.is_some(),
            )
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish_non_exhaustive()
    }
}

/// Inserts `user` through `uow` and records a `created` change event with
/// the stored row (its `Serialize` form, so no secret reaches an observer);
/// the stored row. Other modules that create users (social sign-in, single
/// sign-on) write them with this.
pub async fn insert(uow: &Uow, user: Model) -> Result<Model, AuthError> {
    let id = user.id;
    Model::insert(uow, [user]).await?;
    let stored = stored(uow, id).await?;
    uow.record_change(ChangeEvent {
        entity: TABLE.to_owned(),
        id: id.to_string(),
        op: ChangeOp::Created,
        old: None,
        new: Some(json(&stored)?),
    });
    Ok(stored)
}

/// Updates user `id` through `uow` with `change` and records an `updated`
/// change event with the fields that changed (none: no event). The row is
/// read first, locked `FOR UPDATE` where the database supports it;
/// [`AuthError::UserNotFound`] when there is none. The stored row.
pub(crate) async fn update(
    uow: &Uow,
    id: Uuid,
    change: impl FnOnce(&mut Model),
) -> Result<Model, AuthError> {
    let mut query = Model::query().where_key(id);
    if uow.lock_for_update() {
        query = query.lock_for_update();
    }
    let old = query
        .first(uow)
        .await?
        .ok_or(AuthError::UserNotFound)?
        .into_inner();
    let mut new = old.clone();
    change(&mut new);
    let mut sets = Vec::new();
    for column in new.changed_columns(&old)? {
        let value = new.value(column)?.ok_or_else(|| {
            OrmError::InvalidQuery(format!("`{column}` is not a column of `users`"))
        })?;
        sets.push(Assignment::value(column, value));
    }
    if sets.is_empty() {
        return Ok(old);
    }
    if Model::query().where_key(id).update(uow, sets).await? == 0 {
        return Err(AuthError::UserNotFound);
    }
    let stored = stored(uow, id).await?;
    let (before, after) = changed_fields(json(&old)?, json(&stored)?);
    if !after.is_empty() {
        uow.record_change(ChangeEvent {
            entity: TABLE.to_owned(),
            id: id.to_string(),
            op: ChangeOp::Updated,
            old: Some(before.into()),
            new: Some(after.into()),
        });
    }
    Ok(stored)
}

/// The row of user `id` as the database stored it (timestamps at its
/// precision).
async fn stored(uow: &Uow, id: Uuid) -> Result<Model, AuthError> {
    Ok(Model::find(uow, id)
        .await?
        .ok_or(AuthError::UserNotFound)?
        .into_inner())
}

/// `user` as a change event's values: its `Serialize` form, without the
/// secrets.
fn json(user: &Model) -> Result<serde_json::Value, AuthError> {
    serde_json::to_value(user)
        .map_err(|error| AuthError::Db(DbError::invalid(format!("cannot encode a user: {error}"))))
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
            json!({"name": "Ada", "email": "a@x"}),
            json!({"name": "Ada L", "email": "a@x"}),
        );
        assert_eq!(serde_json::Value::from(before), json!({"name": "Ada"}));
        assert_eq!(serde_json::Value::from(after), json!({"name": "Ada L"}));
    }

    #[test]
    fn secrets_are_not_serialized() {
        let now = Utc::now();
        let user = Model {
            id: Uuid::now_v7(),
            name: "Ada".into(),
            email: "ada@example.com".into(),
            password_hash: "$argon2id$secret".into(),
            email_verified_at: None,
            two_factor_secret: Some("secret".into()),
            two_factor_recovery_codes: Some("codes".into()),
            two_factor_confirmed_at: Some(now),
            created_at: now,
            updated_at: now,
        };
        let value = json(&user).unwrap();
        for secret in [
            "password_hash",
            "two_factor_secret",
            "two_factor_recovery_codes",
            "two_factor_confirmed_at",
        ] {
            assert!(value.get(secret).is_none(), "{secret} is serialized");
        }
    }
}
