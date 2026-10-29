//! The module's tables as navree-orm models (`feature_flag::Model::query()`,
//! typed columns such as `feature_flag::Model::KILLED`).
//!
//! - `feature_flags(name PRIMARY KEY, everyone NULL, rollout JSON NULL,
//!   killed, updated_at, updated_by NULL)`: one row per flag that has
//!   state.
//! - `feature_flag_overrides(flag, scope_type, scope_id, active)`, primary
//!   key `(flag, scope_type, scope_id)`, indexed by `(scope_type,
//!   scope_id)`: one row per override. The row is identified by three
//!   columns, which a model (one key column) does not describe: the model
//!   names `flag` as its key column, and the store addresses rows by filter
//!   and upserts them on the three columns, never by the model's key alone.
//!
//! Scope ids are not foreign keys, so the module works with any user and
//! workspace store. Change the tables through the `Flags` service, which
//! clears the cache, emits the events and writes the audit entries; read
//! them freely.

/// `feature_flags`: the global state of each flag.
pub mod feature_flag {
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Serialize};
    use uuid::Uuid;

    /// The global state of one flag.
    #[derive(Clone, Debug, PartialEq, navree_orm::Model, Serialize, Deserialize)]
    #[model(table = "feature_flags", primary_key = "name")]
    pub struct Model {
        /// The flag's name.
        pub name: String,
        /// On or off for everyone; `NULL` when unset.
        pub everyone: Option<bool>,
        /// The rollout (`{"percent", "of", "plans"}`), if any.
        pub rollout: Option<serde_json::Value>,
        /// The kill switch.
        pub killed: bool,
        /// When the state last changed.
        pub updated_at: DateTime<Utc>,
        /// Who changed it, when a user did.
        pub updated_by: Option<Uuid>,
    }
}

/// `feature_flag_overrides`: a flag on or off for one user or workspace.
pub mod feature_flag_override {
    use serde::{Deserialize, Serialize};
    use uuid::Uuid;

    /// One override.
    #[derive(Clone, Debug, PartialEq, Eq, navree_orm::Model, Serialize, Deserialize)]
    #[model(table = "feature_flag_overrides", primary_key = "flag")]
    pub struct Model {
        /// The flag's name.
        pub flag: String,
        /// `user` or `workspace`.
        pub scope_type: String,
        /// The user's or workspace's id.
        pub scope_id: Uuid,
        /// On or off.
        pub active: bool,
    }
}
