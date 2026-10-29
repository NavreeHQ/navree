//! The access module's tables as navree-db records (migration set `access`).
//!
//! - [`RoleRow`] `roles(id, name UNIQUE, description, created_at)`
//! - [`PermissionRow`] `permissions(id, name UNIQUE)`
//! - [`RolePermissionRow`] `role_permissions(role_id, permission_id)`
//! - [`UserRoleRow`] `user_roles(id, user_id, role_id, team_id NULL)`
//! - [`UserPermissionRow`] `user_permissions(user_id, permission_id)`
//! - [`AccessMetaRow`] `access_meta(key PK, version)`: the access version,
//!   bumped in the same transaction as every role or permission change.
//!
//! User ids are not foreign keys, so the module works without navree-auth.
//! Change roles and permissions through
//! [`AccessService`](crate::AccessService), which keeps the version and the
//! audit trail up to date; read them freely with
//! [`navree_db::Record::select`] or [`navree_db::record::find`].

use std::sync::OnceLock;

use chrono::{DateTime, Utc};
use navree_db::record::Filter;
use navree_db::{Kind, Record as _, Uow, Value};
use navree_kernel::{BoxError, BoxFuture};
use serde::Serialize;
use uuid::Uuid;

use crate::engine::{Subject, TypedColumns};
use crate::records::SubjectRecords;

/// The key of the access version row in `access_meta`.
pub const VERSION_KEY: &str = "version";

navree_db::record! {
    /// A role: a named group of permissions (`roles`).
    #[derive(Clone, Debug, PartialEq, Eq, Serialize)]
    pub struct RoleRow in "roles" {
        /// The id.
        pub id: Uuid,
        /// The unique name, e.g. `admin`.
        pub name: String,
        /// What the role is for.
        pub description: Option<String>,
        /// When the role was created.
        pub created_at: DateTime<Utc>,
    }
}

navree_db::record! {
    /// A permission string, `<prefix>.<action>` (`permissions`).
    #[derive(Clone, Debug, PartialEq, Eq, Serialize)]
    pub struct PermissionRow in "permissions" {
        /// The id.
        pub id: Uuid,
        /// The unique permission string, e.g. `posts.publish`.
        pub name: String,
    }
}

navree_db::record! {
    /// A permission a role grants (`role_permissions`, keyed by both
    /// columns).
    #[derive(Clone, Debug, PartialEq, Eq, Serialize)]
    pub struct RolePermissionRow in "role_permissions" key "role_id" {
        /// The role.
        pub role_id: Uuid,
        /// The permission.
        pub permission_id: Uuid,
    }
}

navree_db::record! {
    /// A role assignment (`user_roles`): global, or in a team.
    #[derive(Clone, Debug, PartialEq, Eq, Serialize)]
    pub struct UserRoleRow in "user_roles" {
        /// The id of the assignment.
        pub id: Uuid,
        /// The user.
        pub user_id: Uuid,
        /// The role.
        pub role_id: Uuid,
        /// The team the role applies in; `None` for a global role.
        pub team_id: Option<Uuid>,
    }
}

navree_db::record! {
    /// A permission given to a user directly (`user_permissions`, keyed by
    /// both columns).
    #[derive(Clone, Debug, PartialEq, Eq, Serialize)]
    pub struct UserPermissionRow in "user_permissions" key "user_id" {
        /// The user.
        pub user_id: Uuid,
        /// The permission.
        pub permission_id: Uuid,
    }
}

navree_db::record! {
    /// A counter (`access_meta`): the access version under [`VERSION_KEY`].
    #[derive(Clone, Debug, PartialEq, Eq, Serialize)]
    pub struct AccessMetaRow in "access_meta" key "key" {
        /// The counter name ([`VERSION_KEY`]).
        pub key: String,
        /// The value.
        pub version: i64,
    }
}

/// Roles are a subject themselves: `roles.manage` grants managing roles and
/// role assignments (the `/api/access` routes).
impl Subject for RoleRow {
    const SUBJECT: &'static str = "Role";
    const PERMISSION_PREFIX: &'static str = "roles";
}

impl RoleRow {
    /// The columns of `roles` with their types, for list filters
    /// ([`Accessible::sql_filter`](crate::Accessible::sql_filter)).
    pub fn columns() -> &'static TypedColumns {
        static COLUMNS: OnceLock<TypedColumns> = OnceLock::new();
        COLUMNS.get_or_init(|| {
            TypedColumns::new(RoleRow::TABLE)
                .with("id", Kind::Uuid)
                .with("name", Kind::String)
                .with("description", Kind::String)
                .with("created_at", Kind::Timestamp)
        })
    }
}

/// `can(.., Role, from = "id")` routes load roles by id.
impl SubjectRecords for RoleRow {
    type Record = RoleRow;

    fn load<'a>(uow: &'a Uow, id: &'a str) -> BoxFuture<'a, Result<Option<RoleRow>, BoxError>> {
        Box::pin(async move {
            let Ok(id) = Uuid::parse_str(id) else {
                return Ok(None);
            };
            Ok(load_row::<RoleRow>(uow, Value::from(id)).await?)
        })
    }

    fn record_json(record: &RoleRow) -> serde_json::Value {
        crate::row_json(record)
    }
}

/// The record of `R` whose key is `key`, read in `uow`'s transaction (locked
/// `FOR UPDATE` where the database supports it).
pub(crate) async fn load_row<R: navree_db::Record>(
    uow: &Uow,
    key: Value,
) -> Result<Option<R>, navree_db::DbError> {
    let mut select = R::select().filter(Filter::all().eq(R::KEY, key));
    if uow.lock_for_update() {
        select = select.for_update();
    }
    select.one(uow).await
}
