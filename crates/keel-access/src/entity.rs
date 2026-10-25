//! The access module's tables as SeaORM entities.
//!
//! - `roles(id, name UNIQUE, description, created_at)`
//! - `permissions(id, name UNIQUE)`
//! - `role_permissions(role_id, permission_id)`
//! - `user_roles(id, user_id, role_id, team_id NULL)`
//! - `user_permissions(user_id, permission_id)`
//! - `access_meta(key PK, version)`: the access version, bumped in the same
//!   transaction as every role or permission change.
//!
//! User ids are not foreign keys, so the module works without keel-auth.
//! Change roles and permissions through
//! [`AccessService`](crate::AccessService), which keeps the version and the
//! audit trail up to date; read them freely.

use crate::engine::Subject;

/// `roles`: named groups of permissions.
pub mod role {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    /// A role.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "roles")]
    pub struct Model {
        /// The id.
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// The unique name, e.g. `admin`.
        #[sea_orm(unique)]
        pub name: String,
        /// What the role is for.
        pub description: Option<String>,
        /// When the role was created.
        pub created_at: DateTimeUtc,
    }

    /// No relations are declared.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `permissions`: permission strings (`posts.publish`).
pub mod permission {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    /// A permission.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "permissions")]
    pub struct Model {
        /// The id.
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// The unique permission string, `<prefix>.<action>`.
        #[sea_orm(unique)]
        pub name: String,
    }

    /// No relations are declared.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `role_permissions`: which permissions a role grants.
pub mod role_permission {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    /// A permission of a role.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "role_permissions")]
    pub struct Model {
        /// The role.
        #[sea_orm(primary_key, auto_increment = false)]
        pub role_id: Uuid,
        /// The permission.
        #[sea_orm(primary_key, auto_increment = false)]
        pub permission_id: Uuid,
    }

    /// No relations are declared.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `user_roles`: which roles a user has (globally, or in a team from Phase 2).
pub mod user_role {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    /// A role assignment.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "user_roles")]
    pub struct Model {
        /// The id of the assignment.
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// The user.
        pub user_id: Uuid,
        /// The role.
        pub role_id: Uuid,
        /// The team the role applies in; `None` for a global role.
        pub team_id: Option<Uuid>,
    }

    /// No relations are declared.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `user_permissions`: permissions given to a user directly.
pub mod user_permission {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    /// A direct permission of a user.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "user_permissions")]
    pub struct Model {
        /// The user.
        #[sea_orm(primary_key, auto_increment = false)]
        pub user_id: Uuid,
        /// The permission.
        #[sea_orm(primary_key, auto_increment = false)]
        pub permission_id: Uuid,
    }

    /// No relations are declared.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `access_meta`: the access version counter.
pub mod access_meta {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    /// The key of the access version row.
    pub const VERSION_KEY: &str = "version";

    /// A counter.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "access_meta")]
    pub struct Model {
        /// The counter name ([`VERSION_KEY`]).
        #[sea_orm(primary_key, auto_increment = false)]
        pub key: String,
        /// The value.
        pub version: i64,
    }

    /// No relations are declared.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// Roles are a subject themselves: `roles.manage` grants managing roles and
/// role assignments (the `/api/access` routes).
impl Subject for role::Entity {
    const SUBJECT: &'static str = "Role";
    const PERMISSION_PREFIX: &'static str = "roles";
}
