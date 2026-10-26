//! [`AccessService`]: manage roles, permissions and role assignments.

use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use navree_contracts::{AuditEntry, AuditKind, AuditSink, AuthVia, Principal};
use navree_db::{Db, Uow};
use navree_kernel::WeakAppContext;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, ModelTrait, QueryFilter, Set,
};
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::engine::Rule;
use crate::entity::{role, role_permission, user_permission, user_role};
use crate::error::{AccessError, check_permission, check_role_name};
use crate::factory::AbilityFactory;
use crate::{disconnect, store};

/// A role with its permissions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RoleInfo {
    /// The id.
    pub id: Uuid,
    /// The unique name.
    pub name: String,
    /// What the role is for.
    pub description: Option<String>,
    /// The permissions the role grants, sorted.
    pub permissions: Vec<String>,
    /// When the role was created.
    pub created_at: DateTime<Utc>,
}

/// Manages roles, permissions and role assignments. Provided and exported
/// by the access module.
///
/// Writes take the [`Uow`] to run in (an HTTP request's, or one from
/// `Db::uow` in jobs and commands). Every change bumps the access version
/// in the same transaction, so cached abilities are rebuilt everywhere once
/// it commits, and writes an [`AuditKind::Access`] entry after the commit
/// when an `AuditSink` is registered. Writes that change nothing (granting
/// a permission the role already has) do neither and return `false`.
///
/// Reads use the connection pool (committed data), or, inside a unit of
/// work whose transaction is open, that transaction (`Db::reader`): a
/// request holding SQLite's write lock must not wait for a second pool
/// connection. (Cached abilities are always built from the pool.)
pub struct AccessService {
    db: Arc<Db>,
    factory: Arc<AbilityFactory>,
    app: WeakAppContext,
}

impl AccessService {
    pub(crate) fn new(db: Arc<Db>, factory: Arc<AbilityFactory>, app: WeakAppContext) -> Self {
        Self { db, factory, app }
    }

    /// Creates role `name`; [`AccessError::RoleExists`] if it exists.
    pub async fn create_role(
        &self,
        uow: &Uow,
        name: &str,
        description: Option<&str>,
    ) -> Result<RoleInfo, AccessError> {
        check_role_name(name)?;
        let tx = uow.tx().await?;
        if store::find_role(&*tx, name).await?.is_some() {
            return Err(AccessError::RoleExists(name.to_owned()));
        }
        let role = role::ActiveModel {
            id: Set(Uuid::now_v7()),
            name: Set(name.to_owned()),
            description: Set(description.map(str::to_owned)),
            created_at: Set(Utc::now()),
        }
        .insert(&*tx)
        .await?;
        self.changed(
            uow,
            &*tx,
            AuditEntry::new(AuditKind::Access, "access.role_created")
                .subject("role", name)
                .new_values(json!({ "name": name, "description": description })),
        )
        .await?;
        Ok(info(role, Vec::new()))
    }

    /// Deletes role `name` with its grants and assignments;
    /// [`AccessError::RoleNotFound`] if there is none.
    pub async fn delete_role(&self, uow: &Uow, name: &str) -> Result<(), AccessError> {
        let tx = uow.tx().await?;
        let role = existing_role(&*tx, name).await?;
        let permissions = store::role_permissions(&*tx, role.id).await?;
        let members = disconnect::role_members(&*tx, role.id).await?;
        disconnect::end_subscriptions_after_commit(&self.app, uow, members).await;
        role_permission::Entity::delete_many()
            .filter(role_permission::Column::RoleId.eq(role.id))
            .exec(&*tx)
            .await?;
        user_role::Entity::delete_many()
            .filter(user_role::Column::RoleId.eq(role.id))
            .exec(&*tx)
            .await?;
        role.delete(&*tx).await?;
        self.changed(
            uow,
            &*tx,
            AuditEntry::new(AuditKind::Access, "access.role_deleted")
                .subject("role", name)
                .old_values(json!({ "name": name, "permissions": permissions })),
        )
        .await
    }

    /// Lets role `role` do `permission` (created if new). `false` if it
    /// already could.
    pub async fn grant(
        &self,
        uow: &Uow,
        role: &str,
        permission: &str,
    ) -> Result<bool, AccessError> {
        check_permission("permission", permission)?;
        let tx = uow.tx().await?;
        let role = existing_role(&*tx, role).await?;
        let permission_row = store::ensure_permission(&*tx, permission).await?;
        let exists = role_permission::Entity::find_by_id((role.id, permission_row.id))
            .one(&*tx)
            .await?
            .is_some();
        if exists {
            return Ok(false);
        }
        role_permission::ActiveModel {
            role_id: Set(role.id),
            permission_id: Set(permission_row.id),
        }
        .insert(&*tx)
        .await?;
        self.changed(
            uow,
            &*tx,
            AuditEntry::new(AuditKind::Access, "access.permission_granted")
                .subject("role", &role.name)
                .new_values(json!({ "permission": permission })),
        )
        .await?;
        Ok(true)
    }

    /// Takes `permission` from role `role`. `false` if it did not have it.
    pub async fn revoke(
        &self,
        uow: &Uow,
        role: &str,
        permission: &str,
    ) -> Result<bool, AccessError> {
        let tx = uow.tx().await?;
        let role = existing_role(&*tx, role).await?;
        let Some(permission_row) = store::find_permission(&*tx, permission).await? else {
            return Ok(false);
        };
        let deleted = role_permission::Entity::delete_by_id((role.id, permission_row.id))
            .exec(&*tx)
            .await?
            .rows_affected;
        if deleted == 0 {
            return Ok(false);
        }
        self.changed(
            uow,
            &*tx,
            AuditEntry::new(AuditKind::Access, "access.permission_revoked")
                .subject("role", &role.name)
                .old_values(json!({ "permission": permission })),
        )
        .await?;
        Ok(true)
    }

    /// Makes `permissions` exactly the permissions of role `role`; returns
    /// the role as it is now.
    pub async fn sync_role_permissions<I, S>(
        &self,
        uow: &Uow,
        role: &str,
        permissions: I,
    ) -> Result<RoleInfo, AccessError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let wanted: BTreeSet<String> = permissions
            .into_iter()
            .map(|p| p.as_ref().to_owned())
            .collect();
        for permission in &wanted {
            check_permission("permissions", permission)?;
        }
        let tx = uow.tx().await?;
        let role = existing_role(&*tx, role).await?;
        let current: BTreeSet<String> = store::role_permissions(&*tx, role.id)
            .await?
            .into_iter()
            .collect();
        if current == wanted {
            return Ok(info(role, current.into_iter().collect()));
        }
        for removed in current.difference(&wanted) {
            if let Some(permission) = store::find_permission(&*tx, removed).await? {
                role_permission::Entity::delete_by_id((role.id, permission.id))
                    .exec(&*tx)
                    .await?;
            }
        }
        for added in wanted.difference(&current) {
            let permission = store::ensure_permission(&*tx, added).await?;
            role_permission::ActiveModel {
                role_id: Set(role.id),
                permission_id: Set(permission.id),
            }
            .insert(&*tx)
            .await?;
        }
        self.changed(
            uow,
            &*tx,
            AuditEntry::new(AuditKind::Access, "access.role_permissions_synced")
                .subject("role", &role.name)
                .old_values(json!({ "permissions": current }))
                .new_values(json!({ "permissions": wanted })),
        )
        .await?;
        Ok(info(role, wanted.into_iter().collect()))
    }

    /// Gives `user` role `role` (globally). `false` if they had it.
    pub async fn assign_role(
        &self,
        uow: &Uow,
        user: Uuid,
        role: &str,
    ) -> Result<bool, AccessError> {
        let tx = uow.tx().await?;
        let role = existing_role(&*tx, role).await?;
        if assignment(&*tx, user, role.id).await?.is_some() {
            return Ok(false);
        }
        user_role::ActiveModel {
            id: Set(Uuid::now_v7()),
            user_id: Set(user),
            role_id: Set(role.id),
            team_id: Set(None),
        }
        .insert(&*tx)
        .await?;
        self.changed(
            uow,
            &*tx,
            AuditEntry::new(AuditKind::Access, "access.role_assigned")
                .subject("user", user.to_string())
                .new_values(json!({ "role": role.name })),
        )
        .await?;
        Ok(true)
    }

    /// Takes role `role` from `user`. `false` if they did not have it.
    pub async fn remove_role(
        &self,
        uow: &Uow,
        user: Uuid,
        role: &str,
    ) -> Result<bool, AccessError> {
        let tx = uow.tx().await?;
        let role = existing_role(&*tx, role).await?;
        let Some(existing) = assignment(&*tx, user, role.id).await? else {
            return Ok(false);
        };
        existing.delete(&*tx).await?;
        self.changed(
            uow,
            &*tx,
            AuditEntry::new(AuditKind::Access, "access.role_removed")
                .subject("user", user.to_string())
                .old_values(json!({ "role": role.name })),
        )
        .await?;
        Ok(true)
    }

    /// Makes `roles` exactly the global roles of `user`; returns them
    /// sorted. [`AccessError::UnknownRoles`] names roles that do not exist.
    pub async fn sync_user_roles<I, S>(
        &self,
        uow: &Uow,
        user: Uuid,
        roles: I,
    ) -> Result<Vec<String>, AccessError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let wanted: BTreeSet<String> = roles.into_iter().map(|r| r.as_ref().to_owned()).collect();
        let tx = uow.tx().await?;
        let mut wanted_roles = Vec::new();
        let mut unknown = Vec::new();
        for name in &wanted {
            match store::find_role(&*tx, name).await? {
                Some(role) => wanted_roles.push(role),
                None => unknown.push(name.clone()),
            }
        }
        if !unknown.is_empty() {
            return Err(AccessError::UnknownRoles(unknown));
        }
        let current: BTreeSet<String> = store::roles_of(&*tx, user).await?.into_iter().collect();
        if current == wanted {
            return Ok(wanted.into_iter().collect());
        }
        for removed in current.difference(&wanted) {
            if let Some(role) = store::find_role(&*tx, removed).await?
                && let Some(existing) = assignment(&*tx, user, role.id).await?
            {
                existing.delete(&*tx).await?;
            }
        }
        for role in wanted_roles.iter().filter(|r| !current.contains(&r.name)) {
            user_role::ActiveModel {
                id: Set(Uuid::now_v7()),
                user_id: Set(user),
                role_id: Set(role.id),
                team_id: Set(None),
            }
            .insert(&*tx)
            .await?;
        }
        self.changed(
            uow,
            &*tx,
            AuditEntry::new(AuditKind::Access, "access.user_roles_synced")
                .subject("user", user.to_string())
                .old_values(json!({ "roles": current }))
                .new_values(json!({ "roles": wanted })),
        )
        .await?;
        Ok(wanted.into_iter().collect())
    }

    /// Gives `user` `permission` directly (created if new). `false` if they
    /// had it directly already.
    pub async fn grant_to_user(
        &self,
        uow: &Uow,
        user: Uuid,
        permission: &str,
    ) -> Result<bool, AccessError> {
        check_permission("permission", permission)?;
        let tx = uow.tx().await?;
        let permission_row = store::ensure_permission(&*tx, permission).await?;
        let exists = user_permission::Entity::find_by_id((user, permission_row.id))
            .one(&*tx)
            .await?
            .is_some();
        if exists {
            return Ok(false);
        }
        user_permission::ActiveModel {
            user_id: Set(user),
            permission_id: Set(permission_row.id),
        }
        .insert(&*tx)
        .await?;
        self.changed(
            uow,
            &*tx,
            AuditEntry::new(AuditKind::Access, "access.user_permission_granted")
                .subject("user", user.to_string())
                .new_values(json!({ "permission": permission })),
        )
        .await?;
        Ok(true)
    }

    /// Takes the direct `permission` from `user`. `false` if they did not
    /// have it directly.
    pub async fn revoke_from_user(
        &self,
        uow: &Uow,
        user: Uuid,
        permission: &str,
    ) -> Result<bool, AccessError> {
        let tx = uow.tx().await?;
        let Some(permission_row) = store::find_permission(&*tx, permission).await? else {
            return Ok(false);
        };
        let deleted = user_permission::Entity::delete_by_id((user, permission_row.id))
            .exec(&*tx)
            .await?
            .rows_affected;
        if deleted == 0 {
            return Ok(false);
        }
        self.changed(
            uow,
            &*tx,
            AuditEntry::new(AuditKind::Access, "access.user_permission_revoked")
                .subject("user", user.to_string())
                .old_values(json!({ "permission": permission })),
        )
        .await?;
        Ok(true)
    }

    /// Role `name` with its permissions, if it exists.
    pub async fn role(&self, name: &str) -> Result<Option<RoleInfo>, AccessError> {
        role_info(&self.db.reader().await, name).await
    }

    /// Every role with its permissions, by name.
    pub async fn roles(&self) -> Result<Vec<RoleInfo>, AccessError> {
        let conn = &self.db.reader().await;
        let mut roles = Vec::new();
        for role in store::all_roles(conn).await? {
            let permissions = store::role_permissions(conn, role.id).await?;
            roles.push(info(role, permissions));
        }
        Ok(roles)
    }

    /// The global role names of `user`, sorted.
    pub async fn roles_of(&self, user: Uuid) -> Result<Vec<String>, AccessError> {
        Ok(store::roles_of(&self.db.reader().await, user).await?)
    }

    /// The permissions of `user` (through roles and directly), sorted.
    pub async fn permissions_of(&self, user: Uuid) -> Result<Vec<String>, AccessError> {
        Ok(store::permissions_of(&self.db.reader().await, user).await?)
    }

    /// The access version, bumped by every change.
    pub async fn version(&self) -> Result<i64, AccessError> {
        Ok(store::version(&self.db.reader().await).await?)
    }

    /// Why `user` may or may not do `action` on `subject` (the record with
    /// primary key `id`, or the subject type when `id` is `None`): the rule
    /// that decides, as `access:explain` prints it.
    /// [`AccessError::RecordNotFound`] when there is no such record.
    pub async fn explain(
        &self,
        user: Uuid,
        action: &str,
        subject: &str,
        id: Option<&str>,
    ) -> Result<Explanation, AccessError> {
        let principal = Principal {
            user_id: user,
            via: AuthVia::Session,
            token_abilities: None,
            session_id: None,
        };
        let access = self.factory.access_for(Some(&principal)).await?;
        let record = match id {
            Some(id) => Some(self.load_record(subject, id).await?),
            None => None,
        };
        let ability = &access.ability;
        let rule = ability
            .relevant_rule(action, subject, record.as_ref(), None)
            .map(|rule| {
                let number = ability
                    .rules()
                    .iter()
                    .position(|candidate| std::ptr::eq(candidate, rule))
                    .map_or(0, |index| index + 1);
                (number, rule.clone())
            });
        Ok(Explanation {
            user,
            action: action.to_owned(),
            subject: subject.to_owned(),
            id: id.map(str::to_owned),
            allowed: rule.as_ref().is_some_and(|(_, rule)| !rule.inverted),
            rule,
            roles: access.roles.clone(),
        })
    }

    /// The record of `subject` with primary key `id`, as conditions see it.
    async fn load_record(&self, subject: &str, id: &str) -> Result<Value, AccessError> {
        let loader = self.factory.subjects().loader(subject).ok_or_else(|| {
            AccessError::Config(format!(
                "no record loader for subject `{subject}`: import `AccessModule::for_feature::<E>()` for its entity"
            ))
        })?;
        let uow = self.db.uow(None, None).await;
        let loaded = loader.load(&uow, id).await;
        uow.rollback().await?;
        loaded?
            .map(|record| record.json)
            .ok_or_else(|| AccessError::RecordNotFound {
                subject: subject.to_owned(),
                id: id.to_owned(),
            })
    }

    /// Records a change: bumps the access version in the transaction and
    /// writes `entry` (with the unit of work's actor and request id) once
    /// it commits.
    async fn changed<C: ConnectionTrait>(
        &self,
        uow: &Uow,
        conn: &C,
        mut entry: AuditEntry,
    ) -> Result<(), AccessError> {
        store::bump_version(conn).await?;
        // Subscriptions authorized under the old access end (see
        // `crate::disconnect`).
        let affected = disconnect::affected_users(conn, &entry).await?;
        disconnect::end_subscriptions_after_commit(&self.app, uow, affected).await;
        let Some(ctx) = self.app.upgrade() else {
            return Ok(());
        };
        let Some(sink) = ctx.try_get::<dyn AuditSink>().await else {
            return Ok(());
        };
        if let Some(actor) = uow.actor() {
            entry = entry.actor(actor);
        }
        if let Some(request_id) = uow.request_id() {
            entry = entry.request_id(request_id);
        }
        uow.after_commit(move || {
            Box::pin(async move {
                let action = entry.action.clone();
                if let Err(error) = sink.record(entry).await {
                    tracing::warn!(target: "navree::audit", %error, action, "could not write an access audit entry");
                }
            })
        });
        Ok(())
    }
}

/// Why a user may or may not do something: see [`AccessService::explain`].
///
/// Displays as, for example:
///
/// ```text
/// User 0192… may not `delete` Post 0193…
/// Decided by rule #6: cannot delete Post where {"status":"locked"} (locked posts cannot be deleted)
/// Roles: writer
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Explanation {
    /// The user.
    pub user: Uuid,
    /// The action checked.
    pub action: String,
    /// The subject type checked.
    pub subject: String,
    /// The record id, for a record check.
    pub id: Option<String>,
    /// The decision.
    pub allowed: bool,
    /// The deciding rule and its number (1-based, in definition order);
    /// `None` when no rule matches (denied by default).
    pub rule: Option<(usize, Rule)>,
    /// The user's roles.
    pub roles: Vec<String>,
}

impl fmt::Display for Explanation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let verdict = if self.allowed { "may" } else { "may not" };
        write!(
            f,
            "User {} {verdict} `{}` {}",
            self.user, self.action, self.subject
        )?;
        if let Some(id) = &self.id {
            write!(f, " {id}")?;
        }
        writeln!(f)?;
        match &self.rule {
            Some((number, rule)) => {
                writeln!(f, "Decided by rule #{number}: {}", DisplayRule(rule))?
            }
            None => writeln!(f, "No rule matches: denied by default.")?,
        }
        if self.roles.is_empty() {
            write!(f, "Roles: (none)")
        } else {
            write!(f, "Roles: {}", self.roles.join(", "))
        }
    }
}

/// `can update Post fields [title] where {..} (reason)`.
struct DisplayRule<'a>(&'a Rule);

impl fmt::Display for DisplayRule<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rule = self.0;
        let verb = if rule.inverted { "cannot" } else { "can" };
        write!(
            f,
            "{verb} {} {}",
            rule.action.join(", "),
            rule.subject.join(", ")
        )?;
        if let Some(fields) = &rule.fields {
            write!(f, " fields [{}]", fields.join(", "))?;
        }
        if let Some(conditions) = rule.conditions.as_ref().filter(|c| !c.is_empty()) {
            write!(f, " where {}", conditions.to_json())?;
        }
        if let Some(reason) = &rule.reason {
            write!(f, " ({reason})")?;
        }
        Ok(())
    }
}

impl fmt::Debug for AccessService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AccessService").finish_non_exhaustive()
    }
}

/// Role `name` with its permissions, read on `conn`.
pub(crate) async fn role_info<C: ConnectionTrait>(
    conn: &C,
    name: &str,
) -> Result<Option<RoleInfo>, AccessError> {
    let Some(role) = store::find_role(conn, name).await? else {
        return Ok(None);
    };
    let permissions = store::role_permissions(conn, role.id).await?;
    Ok(Some(info(role, permissions)))
}

fn info(role: role::Model, permissions: Vec<String>) -> RoleInfo {
    RoleInfo {
        id: role.id,
        name: role.name,
        description: role.description,
        permissions,
        created_at: role.created_at,
    }
}

async fn existing_role<C: ConnectionTrait>(
    conn: &C,
    name: &str,
) -> Result<role::Model, AccessError> {
    store::find_role(conn, name)
        .await?
        .ok_or_else(|| AccessError::RoleNotFound(name.to_owned()))
}

async fn assignment<C: ConnectionTrait>(
    conn: &C,
    user: Uuid,
    role_id: Uuid,
) -> Result<Option<user_role::Model>, AccessError> {
    Ok(user_role::Entity::find()
        .filter(user_role::Column::UserId.eq(user))
        .filter(user_role::Column::RoleId.eq(role_id))
        .filter(user_role::Column::TeamId.is_null())
        .one(conn)
        .await?)
}
