//! Ending real-time subscriptions when access changes (PRD "Where rules
//! are enforced" #5): a private channel subscription is authorized once,
//! when it is made, so a user whose roles or permissions shrink would keep
//! receiving it. After such a change commits, their connections are closed
//! through `dyn Broadcaster::end_user_subscriptions` (navis-realtime: code
//! 4200, reconnect and authorize again).

use navis_contracts::{AuditEntry, Broadcaster};
use navis_db::Uow;
use navis_kernel::WeakAppContext;
use sea_orm::{ColumnTrait, ConnectionTrait, DbErr, EntityTrait, QueryFilter, QuerySelect};
use uuid::Uuid;

use crate::entity::{role, user_role};

/// Role-level changes after which a role's members may see less.
const NARROWING_ROLE_CHANGES: [&str; 2] = [
    "access.permission_revoked",
    "access.role_permissions_synced",
];

/// The users whose subscriptions a change described by `entry` (an access
/// audit entry) affects: the user of a user-level change (roles or direct
/// permissions), or the members of a role that lost permissions. Role
/// deletions are handled by `AccessService::delete_role`, which knows the
/// members before it removes them.
pub(crate) async fn affected_users<C: ConnectionTrait>(
    conn: &C,
    entry: &AuditEntry,
) -> Result<Vec<Uuid>, DbErr> {
    match entry.subject_type.as_deref() {
        Some("user") => Ok(entry
            .subject_id
            .as_deref()
            .and_then(|id| Uuid::parse_str(id).ok())
            .into_iter()
            .collect()),
        Some("role") if NARROWING_ROLE_CHANGES.contains(&entry.action.as_str()) => {
            let Some(name) = entry.subject_id.as_deref() else {
                return Ok(Vec::new());
            };
            match role::Entity::find()
                .filter(role::Column::Name.eq(name))
                .one(conn)
                .await?
            {
                Some(role) => role_members(conn, role.id).await,
                None => Ok(Vec::new()),
            }
        }
        _ => Ok(Vec::new()),
    }
}

/// The users holding role `role_id`.
pub(crate) async fn role_members<C: ConnectionTrait>(
    conn: &C,
    role_id: Uuid,
) -> Result<Vec<Uuid>, DbErr> {
    let mut users: Vec<Uuid> = user_role::Entity::find()
        .select_only()
        .column(user_role::Column::UserId)
        .filter(user_role::Column::RoleId.eq(role_id))
        .into_tuple()
        .all(conn)
        .await?;
    users.sort();
    users.dedup();
    Ok(users)
}

/// Once `uow` commits, ends the real-time subscriptions of `users` (when
/// the app has a `dyn Broadcaster`). A failure is logged: the change
/// itself stands.
pub(crate) async fn end_subscriptions_after_commit(
    app: &WeakAppContext,
    uow: &Uow,
    users: Vec<Uuid>,
) {
    if users.is_empty() {
        return;
    }
    let Some(ctx) = app.upgrade() else {
        return;
    };
    let Some(broadcaster) = ctx.try_get::<dyn Broadcaster>().await else {
        return;
    };
    uow.after_commit(move || {
        Box::pin(async move {
            for user in users {
                if let Err(error) = broadcaster.end_user_subscriptions(user).await {
                    tracing::warn!(
                        target: "navis::access",
                        %error,
                        user_id = %user,
                        "could not end the user's real-time subscriptions after an access change"
                    );
                }
            }
        })
    });
}
