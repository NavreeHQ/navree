//! The module's table as a navree-orm model (`setting::Model::query()`,
//! typed columns such as `setting::Model::SCOPE_TYPE`).
//!
//! `settings(key, scope_type, scope_id, value JSON, overridden_fields JSON,
//! updated_at, updated_by NULL)`, primary key `(key, scope_type,
//! scope_id)`, indexed by `(scope_type, scope_id)`: one row per settings
//! key and level that stores values. `value` holds only the fields set at
//! that level; `overridden_fields` lists them. The global level's
//! `scope_id` is the nil UUID.
//!
//! The row is identified by three columns, which a model (one key column)
//! does not describe: the model names `key` as its key column, and the
//! store addresses rows by filter and upserts them on the three columns,
//! never by the model's key alone.
//!
//! Scope ids are not foreign keys, so the module works with any user,
//! workspace and organization store. Change the table through the
//! `Settings` service, which validates, clears the cache, emits the events
//! and writes the audit entries; read it freely.

/// `settings`: the values stored for one key at one level.
pub mod setting {
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Serialize};
    use uuid::Uuid;

    /// The values stored for one key at one level.
    #[derive(Clone, Debug, PartialEq, navree_orm::Model, Serialize, Deserialize)]
    #[model(table = "settings", primary_key = "key")]
    pub struct Model {
        /// The settings key (`portal`).
        pub key: String,
        /// `global`, `organization`, `workspace` or `user`.
        pub scope_type: String,
        /// The organization's, workspace's or user's id; the nil UUID for
        /// the global level.
        pub scope_id: Uuid,
        /// The fields set at this level, as a JSON object.
        pub value: serde_json::Value,
        /// The names of those fields, sorted, as a JSON array.
        pub overridden_fields: serde_json::Value,
        /// When the values last changed.
        pub updated_at: DateTime<Utc>,
        /// Who changed them, when a user did.
        pub updated_by: Option<Uuid>,
    }
}
