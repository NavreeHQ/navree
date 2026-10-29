//! [`Members`]: who belongs to a workspace, with which role.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use navree_db::Uow;
use navree_orm::Model as _;
use serde::Serialize;
use uuid::Uuid;

use crate::core::Core;
use crate::directory::UserSummary;
use crate::entity::Member;
use crate::events::{MemberAdded, MemberRemoved, MemberRoleChanged};
use crate::store;
use crate::workspaces::set_role;
use crate::{WorkspaceError, WorkspaceRole};

/// A member with the user's name and email, when the
/// [`UserDirectory`](crate::UserDirectory) knows them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MemberInfo {
    /// The membership.
    #[serde(flatten)]
    pub member: Member,
    /// The user, if the directory knows them.
    pub user: Option<UserSummary>,
}

/// Adds members, changes their roles and removes them. Provided and
/// exported by the workspace module.
///
/// The workspace's owner (`workspaces.owner_id`) always keeps the `owner`
/// role: removing or demoting them is [`WorkspaceError::OwnerRequired`]
/// (a 409) until ownership is transferred, so a workspace never loses its
/// last owner. Writes take the [`Uow`] to run in and emit their events once
/// it commits.
pub struct Members {
    core: Arc<Core>,
}

impl Members {
    pub(crate) fn new(core: Arc<Core>) -> Self {
        Self { core }
    }

    /// The members of `workspace`, oldest first, with their users.
    pub async fn list(&self, workspace: Uuid) -> Result<Vec<MemberInfo>, WorkspaceError> {
        let members = Member::query()
            .where_(Member::WORKSPACE_ID.eq(workspace))
            .order_by(Member::CREATED_AT)
            .order_by(Member::ID)
            .get(&*self.core.db)
            .await?
            .into_models();
        let ids: Vec<Uuid> = members.iter().map(|m| m.user_id).collect();
        let mut users: HashMap<Uuid, UserSummary> = self
            .core
            .directory
            .users(&ids)
            .await
            .map_err(WorkspaceError::Other)?
            .into_iter()
            .map(|user| (user.id, user))
            .collect();
        Ok(members
            .into_iter()
            .map(|member| MemberInfo {
                user: users.remove(&member.user_id),
                member,
            })
            .collect())
    }

    /// The membership of `user` in `workspace`, if any.
    pub async fn find(
        &self,
        workspace: Uuid,
        user: Uuid,
    ) -> Result<Option<Member>, WorkspaceError> {
        self.core.member_in(&*self.core.db, workspace, user).await
    }

    /// Adds `user` to `workspace` with `role` (a built-in or configured
    /// role). [`WorkspaceError::AlreadyMember`] if they belong already.
    pub async fn add(
        &self,
        uow: &Uow,
        workspace: Uuid,
        user: Uuid,
        role: WorkspaceRole,
    ) -> Result<Member, WorkspaceError> {
        self.core.check_role(&role)?;
        self.core.workspace_in(uow, workspace).await?;
        if self.core.member_in(uow, workspace, user).await?.is_some() {
            return Err(WorkspaceError::AlreadyMember);
        }
        self.insert(uow, workspace, user, role).await
    }

    /// Inserts a membership (no checks) and emits `MemberAdded`.
    pub(crate) async fn insert(
        &self,
        uow: &Uow,
        workspace: Uuid,
        user: Uuid,
        role: WorkspaceRole,
    ) -> Result<Member, WorkspaceError> {
        let now = self.core.now();
        let added = store::insert(
            uow,
            Member {
                id: Uuid::now_v7(),
                workspace_id: workspace,
                user_id: user,
                role: role.as_str().to_owned(),
                partition_scope: crate::org_role::SCOPE_ALL.to_owned(),
                created_at: now,
                updated_at: now,
            },
        )
        .await?;
        self.core
            .emit_after_commit(
                uow,
                MemberAdded {
                    workspace_id: workspace,
                    user_id: user,
                    role: role.as_str().to_owned(),
                },
            )
            .await;
        Ok(added)
    }

    /// Gives `user` the role `role` in `workspace`.
    /// [`WorkspaceError::MemberNotFound`] if they are not a member,
    /// [`WorkspaceError::OwnerRequired`] when `user` is the workspace's
    /// owner and `role` is not `owner`.
    pub async fn change_role(
        &self,
        uow: &Uow,
        workspace: Uuid,
        user: Uuid,
        role: WorkspaceRole,
    ) -> Result<Member, WorkspaceError> {
        self.core.check_role(&role)?;
        let owner = self.core.workspace_in(uow, workspace).await?.owner_id;
        let current = self
            .core
            .member_in(uow, workspace, user)
            .await?
            .ok_or(WorkspaceError::MemberNotFound)?;
        if user == owner && role != WorkspaceRole::Owner {
            return Err(WorkspaceError::OwnerRequired);
        }
        let from = current.role.clone();
        if from == role.as_str() {
            return Ok(current);
        }
        let changed = set_role(uow, current, role.clone(), self.core.now()).await?;
        self.core
            .emit_after_commit(
                uow,
                MemberRoleChanged {
                    workspace_id: workspace,
                    user_id: user,
                    from,
                    to: role.as_str().to_owned(),
                },
            )
            .await;
        Ok(changed)
    }

    /// Removes `user` from `workspace` (also how a member leaves).
    /// [`WorkspaceError::MemberNotFound`] if they are not a member,
    /// [`WorkspaceError::OwnerRequired`] for the workspace's owner.
    pub async fn remove(
        &self,
        uow: &Uow,
        workspace: Uuid,
        user: Uuid,
    ) -> Result<(), WorkspaceError> {
        let owner = self.core.workspace_in(uow, workspace).await?.owner_id;
        let current = self
            .core
            .member_in(uow, workspace, user)
            .await?
            .ok_or(WorkspaceError::MemberNotFound)?;
        if user == owner {
            return Err(WorkspaceError::OwnerRequired);
        }
        let role = current.role.clone();
        store::delete(uow, current).await?;
        self.core
            .emit_after_commit(
                uow,
                MemberRemoved {
                    workspace_id: workspace,
                    user_id: user,
                    role,
                },
            )
            .await;
        Ok(())
    }
}

impl fmt::Debug for Members {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Members").finish_non_exhaustive()
    }
}
