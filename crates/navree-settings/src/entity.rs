//! The module's table as a SeaORM entity.
//!
//! `settings(key, scope_type, scope_id, value JSON, overridden_fields JSON,
//! updated_at, updated_by NULL)`, primary key `(key, scope_type,
//! scope_id)`, indexed by `(scope_type, scope_id)`: one row per settings
//! key and level that stores values. `value` holds only the fields set at
//! that level; `overridden_fields` lists them. The global level's
//! `scope_id` is the nil UUID.
//!
//! Scope ids are not foreign keys, so the module works with any user,
//! workspace and organization store. Change the table through the
//! `Settings` service, which validates, clears the cache, emits the events
//! and writes the audit entries; read it freely.

/// `settings`: the values stored for one key at one level.
pub mod setting {
    use sea_orm::entity::prelude::*;
    use serde::{Deserialize, Serialize};

    /// The values stored for one key at one level.
    #[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
    #[sea_orm(table_name = "settings")]
    pub struct Model {
        /// The settings key (`portal`).
        #[sea_orm(primary_key, auto_increment = false)]
        pub key: String,
        /// `global`, `organization`, `workspace` or `user`.
        #[sea_orm(primary_key, auto_increment = false)]
        pub scope_type: String,
        /// The organization's, workspace's or user's id; the nil UUID for
        /// the global level.
        #[sea_orm(primary_key, auto_increment = false)]
        pub scope_id: Uuid,
        /// The fields set at this level, as a JSON object.
        #[sea_orm(column_type = "JsonBinary")]
        pub value: Json,
        /// The names of those fields, sorted, as a JSON array.
        #[sea_orm(column_type = "JsonBinary")]
        pub overridden_fields: Json,
        /// When the values last changed.
        pub updated_at: DateTimeUtc,
        /// Who changed them, when a user did.
        pub updated_by: Option<Uuid>,
    }

    /// `settings` has no relations.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
