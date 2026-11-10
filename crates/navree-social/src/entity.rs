//! The social module's table: a navree-orm model
//! (`social_account::Model::query()`, typed columns such as
//! `social_account::Model::PROVIDER`).
//!
//! `social_accounts(id, user_id → users ON DELETE CASCADE, provider,
//! provider_user_id, email, name, avatar_url, nickname, access_token,
//! refresh_token, token_expires_at, scopes, created_at, updated_at)`, one
//! row per linked provider account: unique per (provider,
//! provider_user_id) and per (user_id, provider). The token columns hold
//! navree-crypto ciphertexts (`social.store_tokens`), never plain tokens.

/// `social_accounts`: provider accounts linked to users.
///
/// The module's own writes of accounts go through `insert`, `update`
/// and `delete`: ORM statements in a unit of work that record a change
/// event for each write (the model's `Serialize` form, so the encrypted
/// tokens never reach an observer; an update carries only the changed
/// fields).
pub mod social_account {
    use std::fmt;

    use chrono::{DateTime, Utc};
    use navree_db::{ChangeEvent, ChangeOp, Uow};
    use navree_orm::{Assignment, Model as _, OrmError};
    use serde::Serialize;
    use uuid::Uuid;

    /// The table.
    const TABLE: &str = "social_accounts";

    /// A linked provider account.
    #[derive(Clone, PartialEq, Eq, navree_orm::Model, Serialize)]
    #[model(table = "social_accounts")]
    pub struct Model {
        /// The id (UUID v7).
        pub id: Uuid,
        /// The user the account signs in.
        pub user_id: Uuid,
        /// The provider key (`github`, `okta`).
        pub provider: String,
        /// The provider's user id.
        pub provider_user_id: String,
        /// The email address the provider reported.
        pub email: Option<String>,
        /// The name the provider reported.
        pub name: Option<String>,
        /// The avatar URL the provider reported.
        pub avatar_url: Option<String>,
        /// The handle the provider reported.
        pub nickname: Option<String>,
        /// The encrypted access token. Never serialized.
        #[serde(skip_serializing)]
        pub access_token: Option<String>,
        /// The encrypted refresh token. Never serialized.
        #[serde(skip_serializing)]
        pub refresh_token: Option<String>,
        /// When the access token expires.
        pub token_expires_at: Option<DateTime<Utc>>,
        /// The granted scopes, space-separated.
        pub scopes: Option<String>,
        /// When the account was linked.
        pub created_at: DateTime<Utc>,
        /// When the account last changed.
        pub updated_at: DateTime<Utc>,
    }

    /// The tokens are redacted.
    impl fmt::Debug for Model {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("SocialAccount")
                .field("id", &self.id)
                .field("user_id", &self.user_id)
                .field("provider", &self.provider)
                .field("provider_user_id", &self.provider_user_id)
                .field("email", &self.email)
                .field("name", &self.name)
                .field("has_tokens", &self.access_token.is_some())
                .field("token_expires_at", &self.token_expires_at)
                .field("created_at", &self.created_at)
                .finish_non_exhaustive()
        }
    }

    /// Inserts `account` through `uow` and records a `created` change event
    /// with the stored row; the stored row.
    pub(crate) async fn insert(uow: &Uow, account: Model) -> Result<Model, OrmError> {
        let id = account.id;
        Model::insert(uow, [account]).await?;
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

    /// Updates account `id` through `uow` with `change` and records an
    /// `updated` change event with the fields that changed (none: no
    /// event). A not-found error when there is no such account; the stored
    /// row.
    pub(crate) async fn update(
        uow: &Uow,
        id: Uuid,
        change: impl FnOnce(&mut Model),
    ) -> Result<Model, OrmError> {
        let old = stored(uow, id).await?;
        let mut new = old.clone();
        change(&mut new);
        let mut sets = Vec::new();
        for column in new.changed_columns(&old)? {
            let value = new.value(column)?.ok_or_else(|| {
                OrmError::InvalidQuery(format!("`{column}` is not a column of `{TABLE}`"))
            })?;
            sets.push(Assignment::value(column, value));
        }
        if sets.is_empty() {
            return Ok(old);
        }
        if Model::query().where_key(id).update(uow, sets).await? == 0 {
            return Err(not_found(id));
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

    /// Deletes `account` through `uow` and records a `deleted` change event
    /// with its row. A not-found error when it is gone.
    pub(crate) async fn delete(uow: &Uow, account: &Model) -> Result<(), OrmError> {
        if Model::query().where_key(account.id).delete(uow).await? == 0 {
            return Err(not_found(account.id));
        }
        uow.record_change(ChangeEvent {
            entity: TABLE.to_owned(),
            id: account.id.to_string(),
            op: ChangeOp::Deleted,
            old: Some(json(account)?),
            new: None,
        });
        Ok(())
    }

    /// The row of account `id` as the database stored it (timestamps at its
    /// precision).
    async fn stored(uow: &Uow, id: Uuid) -> Result<Model, OrmError> {
        Ok(Model::find(uow, id)
            .await?
            .ok_or_else(|| not_found(id))?
            .into_inner())
    }

    fn not_found(id: Uuid) -> OrmError {
        OrmError::not_found("SocialAccount", vec![id.to_string()])
    }

    /// `account` as a change event's values: its `Serialize` form, without
    /// the tokens.
    fn json(account: &Model) -> Result<serde_json::Value, OrmError> {
        serde_json::to_value(account)
            .map_err(|error| OrmError::InvalidQuery(format!("cannot encode an account: {error}")))
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
                json!({"name": "Anu", "email": "a@x"}),
                json!({"name": "Anu L", "email": "a@x"}),
            );
            assert_eq!(serde_json::Value::from(before), json!({"name": "Anu"}));
            assert_eq!(serde_json::Value::from(after), json!({"name": "Anu L"}));
        }

        #[test]
        fn tokens_are_not_serialized() {
            let now = Utc::now();
            let account = Model {
                id: Uuid::now_v7(),
                user_id: Uuid::now_v7(),
                provider: "github".into(),
                provider_user_id: "1".into(),
                email: None,
                name: None,
                avatar_url: None,
                nickname: None,
                access_token: Some("secret".into()),
                refresh_token: Some("secret".into()),
                token_expires_at: None,
                scopes: None,
                created_at: now,
                updated_at: now,
            };
            let value = json(&account).unwrap();
            assert!(value.get("access_token").is_none());
            assert!(value.get("refresh_token").is_none());
        }
    }
}
