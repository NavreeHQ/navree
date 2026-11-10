//! Reads and writes of the access tables, on any executor (a pool, a
//! reader or a unit of work).

use std::collections::BTreeSet;

use navree_db::record::{self, Filter};
use navree_db::{Backend, DbError, Executor, Record as _, Statement};
use uuid::Uuid;

use crate::entity::{
    AccessMetaRow, PermissionRow, RolePermissionRow, RoleRow, UserPermissionRow, UserRoleRow,
    VERSION_KEY,
};

/// The access version (0 before the first migration ran to completion).
pub(crate) async fn version<E: Executor + ?Sized>(exec: &E) -> Result<i64, DbError> {
    Ok(AccessMetaRow::select()
        .eq("key", VERSION_KEY)
        .one(exec)
        .await?
        .map_or(0, |row| row.version))
}

/// Adds one to the access version (creating it at 1).
pub(crate) async fn bump_version<E: Executor + ?Sized>(exec: &E) -> Result<(), DbError> {
    let backend = exec.backend();
    let (table, key, version) = (
        backend.quote_identifier(AccessMetaRow::TABLE),
        backend.quote_part("key"),
        backend.quote_part("version"),
    );
    let sql = match backend {
        Backend::MySql => format!(
            "INSERT INTO {table} ({key}, {version}) VALUES (?, 1) \
             ON DUPLICATE KEY UPDATE {version} = {version} + 1"
        ),
        Backend::Postgres | Backend::Sqlite => format!(
            "INSERT INTO {table} ({key}, {version}) VALUES (?, 1) \
             ON CONFLICT ({key}) DO UPDATE SET {version} = {table}.{version} + 1"
        ),
    };
    // The identifiers are constants quoted for the backend; the key is bound.
    // nosemgrep: navree-sql-built-from-strings
    exec.execute(&Statement::portable(sql, [VERSION_KEY.into()]))
        .await?;
    Ok(())
}

/// The ids of the global roles of `user`.
async fn global_role_ids<E: Executor + ?Sized>(exec: &E, user: Uuid) -> Result<Vec<Uuid>, DbError> {
    Ok(UserRoleRow::select()
        .eq("user_id", user)
        .filter(Filter::all().is_null("team_id"))
        .all(exec)
        .await?
        .into_iter()
        .map(|assignment| assignment.role_id)
        .collect())
}

/// The global role names of `user`, sorted.
pub(crate) async fn roles_of<E: Executor + ?Sized>(
    exec: &E,
    user: Uuid,
) -> Result<Vec<String>, DbError> {
    let role_ids = global_role_ids(exec, user).await?;
    if role_ids.is_empty() {
        return Ok(Vec::new());
    }
    Ok(RoleRow::select()
        .filter(Filter::all().is_in("id", role_ids))
        .order_by("name")
        .all(exec)
        .await?
        .into_iter()
        .map(|role| role.name)
        .collect())
}

/// The permissions of `user` (through global roles and directly), sorted
/// and without duplicates.
pub(crate) async fn permissions_of<E: Executor + ?Sized>(
    exec: &E,
    user: Uuid,
) -> Result<Vec<String>, DbError> {
    let role_ids = global_role_ids(exec, user).await?;
    let mut permission_ids: BTreeSet<Uuid> = BTreeSet::new();
    if !role_ids.is_empty() {
        permission_ids.extend(
            RolePermissionRow::select()
                .filter(Filter::all().is_in("role_id", role_ids))
                .all(exec)
                .await?
                .into_iter()
                .map(|grant| grant.permission_id),
        );
    }
    permission_ids.extend(
        UserPermissionRow::select()
            .eq("user_id", user)
            .all(exec)
            .await?
            .into_iter()
            .map(|grant| grant.permission_id),
    );
    permission_names(exec, permission_ids.into_iter().collect()).await
}

/// The names of the permissions with these ids, sorted.
async fn permission_names<E: Executor + ?Sized>(
    exec: &E,
    ids: Vec<Uuid>,
) -> Result<Vec<String>, DbError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    Ok(PermissionRow::select()
        .filter(Filter::all().is_in("id", ids))
        .order_by("name")
        .all(exec)
        .await?
        .into_iter()
        .map(|permission| permission.name)
        .collect())
}

/// The role named `name`.
pub(crate) async fn find_role<E: Executor + ?Sized>(
    exec: &E,
    name: &str,
) -> Result<Option<RoleRow>, DbError> {
    RoleRow::select().eq("name", name).one(exec).await
}

/// Every role, by name.
pub(crate) async fn all_roles<E: Executor + ?Sized>(exec: &E) -> Result<Vec<RoleRow>, DbError> {
    RoleRow::select().order_by("name").all(exec).await
}

/// The permission names of role `role_id`, sorted.
pub(crate) async fn role_permissions<E: Executor + ?Sized>(
    exec: &E,
    role_id: Uuid,
) -> Result<Vec<String>, DbError> {
    let ids = RolePermissionRow::select()
        .eq("role_id", role_id)
        .all(exec)
        .await?
        .into_iter()
        .map(|grant| grant.permission_id)
        .collect();
    permission_names(exec, ids).await
}

/// The permission named `name`.
pub(crate) async fn find_permission<E: Executor + ?Sized>(
    exec: &E,
    name: &str,
) -> Result<Option<PermissionRow>, DbError> {
    PermissionRow::select().eq("name", name).one(exec).await
}

/// The permission named `name`, created if missing.
pub(crate) async fn ensure_permission<E: Executor + ?Sized>(
    exec: &E,
    name: &str,
) -> Result<PermissionRow, DbError> {
    if let Some(existing) = find_permission(exec, name).await? {
        return Ok(existing);
    }
    let permission = PermissionRow {
        id: Uuid::now_v7(),
        name: name.to_owned(),
    };
    record::insert(exec, &permission).await?;
    Ok(permission)
}

/// Whether role `role_id` grants permission `permission_id`.
pub(crate) async fn role_grants<E: Executor + ?Sized>(
    exec: &E,
    role_id: Uuid,
    permission_id: Uuid,
) -> Result<bool, DbError> {
    Ok(RolePermissionRow::select()
        .eq("role_id", role_id)
        .eq("permission_id", permission_id)
        .one(exec)
        .await?
        .is_some())
}

/// Removes permission `permission_id` from role `role_id`; the number of
/// rows removed.
pub(crate) async fn remove_grant<E: Executor + ?Sized>(
    exec: &E,
    role_id: Uuid,
    permission_id: Uuid,
) -> Result<u64, DbError> {
    RolePermissionRow::delete_where(
        Filter::all()
            .eq("role_id", role_id)
            .eq("permission_id", permission_id),
    )
    .execute(exec)
    .await
}

/// The global assignment of role `role_id` to `user`.
pub(crate) async fn assignment<E: Executor + ?Sized>(
    exec: &E,
    user: Uuid,
    role_id: Uuid,
) -> Result<Option<UserRoleRow>, DbError> {
    UserRoleRow::select()
        .eq("user_id", user)
        .eq("role_id", role_id)
        .filter(Filter::all().is_null("team_id"))
        .one(exec)
        .await
}

/// Gives `user` role `role_id` globally.
pub(crate) async fn assign<E: Executor + ?Sized>(
    exec: &E,
    user: Uuid,
    role_id: Uuid,
) -> Result<(), DbError> {
    record::insert(
        exec,
        &UserRoleRow {
            id: Uuid::now_v7(),
            user_id: user,
            role_id,
            team_id: None,
        },
    )
    .await
}

/// The users holding role `role_id`, sorted, without duplicates.
pub(crate) async fn role_members<E: Executor + ?Sized>(
    exec: &E,
    role_id: Uuid,
) -> Result<Vec<Uuid>, DbError> {
    let backend = exec.backend();
    let sql = format!(
        "SELECT DISTINCT {user} FROM {table} WHERE {role} = ? ORDER BY {user}",
        user = backend.quote_part("user_id"),
        table = backend.quote_identifier(UserRoleRow::TABLE),
        role = backend.quote_part("role_id"),
    );
    // Constant identifiers quoted for the backend; the role is bound.
    // nosemgrep: navree-sql-built-from-strings
    let rows = exec
        .fetch_all(&Statement::portable(sql, [role_id.into()]))
        .await?;
    let mut users = rows
        .iter()
        .map(|row| row.get::<Uuid>(0))
        .collect::<Result<Vec<_>, _>>()?;
    // Byte order on MySQL and SQLite, UUID order on PostgreSQL: sort here.
    users.sort();
    users.dedup();
    Ok(users)
}
