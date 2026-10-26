//! Reads and writes of the access tables, on any connection (the pool or a
//! unit of work's transaction).

use std::collections::BTreeSet;

use sea_orm::sea_query::{Expr, ExprTrait, OnConflict, Query};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DbErr, EntityTrait, QueryFilter, QueryOrder,
    Set,
};
use uuid::Uuid;

use crate::entity::access_meta::VERSION_KEY;
use crate::entity::{access_meta, permission, role, role_permission, user_permission, user_role};

/// The access version (0 before the first migration ran to completion).
pub(crate) async fn version<C: ConnectionTrait>(conn: &C) -> Result<i64, DbErr> {
    Ok(access_meta::Entity::find_by_id(VERSION_KEY.to_owned())
        .one(conn)
        .await?
        .map_or(0, |row| row.version))
}

/// Adds one to the access version.
pub(crate) async fn bump_version<C: ConnectionTrait>(conn: &C) -> Result<(), DbErr> {
    let upsert = Query::insert()
        .into_table(access_meta::Entity)
        .columns([access_meta::Column::Key, access_meta::Column::Version])
        .values_panic([VERSION_KEY.into(), 1_i64.into()])
        .on_conflict(
            OnConflict::column(access_meta::Column::Key)
                .value(
                    access_meta::Column::Version,
                    Expr::col((access_meta::Entity, access_meta::Column::Version)).add(1),
                )
                .to_owned(),
        )
        .to_owned();
    conn.execute(&upsert).await?;
    Ok(())
}

/// The global role names of `user`, sorted.
pub(crate) async fn roles_of<C: ConnectionTrait>(
    conn: &C,
    user: Uuid,
) -> Result<Vec<String>, DbErr> {
    let role_ids: Vec<Uuid> = user_role::Entity::find()
        .filter(user_role::Column::UserId.eq(user))
        .filter(user_role::Column::TeamId.is_null())
        .all(conn)
        .await?
        .into_iter()
        .map(|assignment| assignment.role_id)
        .collect();
    if role_ids.is_empty() {
        return Ok(Vec::new());
    }
    Ok(role::Entity::find()
        .filter(role::Column::Id.is_in(role_ids))
        .order_by_asc(role::Column::Name)
        .all(conn)
        .await?
        .into_iter()
        .map(|role| role.name)
        .collect())
}

/// The permissions of `user` (through global roles and directly), sorted
/// and without duplicates.
pub(crate) async fn permissions_of<C: ConnectionTrait>(
    conn: &C,
    user: Uuid,
) -> Result<Vec<String>, DbErr> {
    let role_ids: Vec<Uuid> = user_role::Entity::find()
        .filter(user_role::Column::UserId.eq(user))
        .filter(user_role::Column::TeamId.is_null())
        .all(conn)
        .await?
        .into_iter()
        .map(|assignment| assignment.role_id)
        .collect();
    let mut permission_ids: BTreeSet<Uuid> = BTreeSet::new();
    if !role_ids.is_empty() {
        permission_ids.extend(
            role_permission::Entity::find()
                .filter(role_permission::Column::RoleId.is_in(role_ids))
                .all(conn)
                .await?
                .into_iter()
                .map(|grant| grant.permission_id),
        );
    }
    permission_ids.extend(
        user_permission::Entity::find()
            .filter(user_permission::Column::UserId.eq(user))
            .all(conn)
            .await?
            .into_iter()
            .map(|grant| grant.permission_id),
    );
    permission_names(conn, permission_ids.into_iter().collect()).await
}

/// The names of the permissions with these ids, sorted.
async fn permission_names<C: ConnectionTrait>(
    conn: &C,
    ids: Vec<Uuid>,
) -> Result<Vec<String>, DbErr> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    Ok(permission::Entity::find()
        .filter(permission::Column::Id.is_in(ids))
        .order_by_asc(permission::Column::Name)
        .all(conn)
        .await?
        .into_iter()
        .map(|permission| permission.name)
        .collect())
}

/// The role named `name`.
pub(crate) async fn find_role<C: ConnectionTrait>(
    conn: &C,
    name: &str,
) -> Result<Option<role::Model>, DbErr> {
    role::Entity::find()
        .filter(role::Column::Name.eq(name))
        .one(conn)
        .await
}

/// Every role, by name.
pub(crate) async fn all_roles<C: ConnectionTrait>(conn: &C) -> Result<Vec<role::Model>, DbErr> {
    role::Entity::find()
        .order_by_asc(role::Column::Name)
        .all(conn)
        .await
}

/// The permission names of role `role_id`, sorted.
pub(crate) async fn role_permissions<C: ConnectionTrait>(
    conn: &C,
    role_id: Uuid,
) -> Result<Vec<String>, DbErr> {
    let ids = role_permission::Entity::find()
        .filter(role_permission::Column::RoleId.eq(role_id))
        .all(conn)
        .await?
        .into_iter()
        .map(|grant| grant.permission_id)
        .collect();
    permission_names(conn, ids).await
}

/// The permission named `name`.
pub(crate) async fn find_permission<C: ConnectionTrait>(
    conn: &C,
    name: &str,
) -> Result<Option<permission::Model>, DbErr> {
    permission::Entity::find()
        .filter(permission::Column::Name.eq(name))
        .one(conn)
        .await
}

/// The permission named `name`, created if missing.
pub(crate) async fn ensure_permission<C: ConnectionTrait>(
    conn: &C,
    name: &str,
) -> Result<permission::Model, DbErr> {
    if let Some(existing) = find_permission(conn, name).await? {
        return Ok(existing);
    }
    permission::ActiveModel {
        id: Set(Uuid::now_v7()),
        name: Set(name.to_owned()),
    }
    .insert(conn)
    .await
}
