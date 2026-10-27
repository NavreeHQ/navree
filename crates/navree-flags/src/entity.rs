//! The module's tables as SeaORM entities; `feature_flags` is a navree-orm
//! model too (`feature_flag::Model::query()`, typed columns such as
//! `feature_flag::Model::KILLED`). The overrides table is keyed by three
//! columns, which an ORM model (one key column) does not describe, so it
//! stays an entity only.
//!
//! - `feature_flags(name PRIMARY KEY, everyone NULL, rollout JSON NULL,
//!   killed, updated_at, updated_by NULL)`: one row per flag that has
//!   state.
//! - `feature_flag_overrides(flag, scope_type, scope_id, active)`, primary
//!   key `(flag, scope_type, scope_id)`, indexed by `(scope_type,
//!   scope_id)`: one row per override.
//!
//! Scope ids are not foreign keys, so the module works with any user and
//! workspace store. Change the tables through the `Flags` service, which
//! clears the cache, emits the events and writes the audit entries; read
//! them freely.

/// `feature_flags`: the global state of each flag.
pub mod feature_flag {
    use sea_orm::entity::prelude::*;
    use serde::{Deserialize, Serialize};

    /// The global state of one flag.
    #[derive(
        Clone, Debug, PartialEq, DeriveEntityModel, navree_orm::Model, Serialize, Deserialize,
    )]
    #[sea_orm(table_name = "feature_flags")]
    #[model(table = "feature_flags", primary_key = "name")]
    pub struct Model {
        /// The flag's name.
        #[sea_orm(primary_key, auto_increment = false)]
        pub name: String,
        /// On or off for everyone; `NULL` when unset.
        pub everyone: Option<bool>,
        /// The rollout (`{"percent", "of", "plans"}`), if any.
        #[sea_orm(column_type = "JsonBinary", nullable)]
        pub rollout: Option<Json>,
        /// The kill switch.
        pub killed: bool,
        /// When the state last changed.
        pub updated_at: DateTimeUtc,
        /// Who changed it, when a user did.
        pub updated_by: Option<Uuid>,
    }

    /// `feature_flags` has no relations.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `feature_flag_overrides`: a flag on or off for one user or workspace.
pub mod feature_flag_override {
    use sea_orm::entity::prelude::*;
    use serde::{Deserialize, Serialize};

    /// One override.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
    #[sea_orm(table_name = "feature_flag_overrides")]
    pub struct Model {
        /// The flag's name.
        #[sea_orm(primary_key, auto_increment = false)]
        pub flag: String,
        /// `user` or `workspace`.
        #[sea_orm(primary_key, auto_increment = false)]
        pub scope_type: String,
        /// The user's or workspace's id.
        #[sea_orm(primary_key, auto_increment = false)]
        pub scope_id: Uuid,
        /// On or off.
        pub active: bool,
    }

    /// `feature_flag_overrides` has no relations.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
