//! [`OrgMembers`]: who belongs to an organization, with which role, in
//! which of its workspaces.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use navree_db::Uow;
use navree_orm::Model as _;
use serde::Serialize;
use uuid::Uuid;

use crate::core::Core;
use crate::directory::UserSummary;
use crate::entity::OrgMember;
use crate::events::{OrgMemberAdded, OrgMemberChanged, OrgMemberRemoved};
use crate::grants::{check_workspaces, org_selector, set_org_selector};
use crate::organizations::set_org_role;
use crate::store;
use crate::{OrgRole, WorkspaceError, WorkspaceSelector};

/// An organization member with the workspaces they reach and the user's
/// name and email, when the [`UserDirectory`](crate::UserDirectory) knows
/// them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OrgMemberInfo {
    /// The membership.
    #[serde(flatten)]
    pub member: OrgMember,
    /// The organization's workspaces they reach.
    pub workspaces: WorkspaceSelector,
    /// The user, if the directory knows them.
    pub user: Option<UserSummary>,
}

/// Adds organization members, changes their roles and the workspaces they
/// reach, and removes them. Provided and exported by the workspace module;
/// its methods answer [`WorkspaceError::FeatureDisabled`] unless
/// `workspace.organizations.enabled` is on.
///
/// A member's [`OrgRole`] gives permissions in the organization and maps to
/// a workspace role, which they have in the workspaces their
/// [`WorkspaceSelector`] reaches: `All` of the organization's workspaces
/// (including ones attached later) or `Only` the listed ones. The owner
/// (`organizations.owner_id`) keeps the `owner` role and every workspace
/// until ownership is transferred.
pub struct OrgMembers {
    core: Arc<Core>,
}

impl OrgMembers {
    pub(crate) fn new(core: Arc<Core>) -> Self {
        Self { core }
    }

    /// The members of organization `org`, oldest first, with their
    /// workspaces and users.
    pub async fn list(&self, org: Uuid) -> Result<Vec<OrgMemberInfo>, WorkspaceError> {
        self.core.require_organizations()?;
        let db = &*self.core.db;
        let members = OrgMember::query()
            .where_(OrgMember::ORGANIZATION_ID.eq(org))
            .order_by(OrgMember::CREATED_AT)
            .order_by(OrgMember::ID)
            .get(db)
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
        let mut list = Vec::with_capacity(members.len());
        for member in members {
            list.push(OrgMemberInfo {
                workspaces: org_selector(db, &member).await?,
                user: users.remove(&member.user_id),
                member,
            });
        }
        Ok(list)
    }

    /// The membership of `user` in organization `org`, if any.
    pub async fn find(&self, org: Uuid, user: Uuid) -> Result<Option<OrgMember>, WorkspaceError> {
        self.core.require_organizations()?;
        self.core.org_member_in(&*self.core.db, org, user).await
    }

    /// The workspaces `user` reaches as a member of organization `org`
    /// (`None` for non-members).
    pub async fn selector(
        &self,
        org: Uuid,
        user: Uuid,
    ) -> Result<Option<WorkspaceSelector>, WorkspaceError> {
        self.core.require_organizations()?;
        let db = &*self.core.db;
        match self.core.org_member_in(db, org, user).await? {
            Some(member) => Ok(Some(org_selector(db, &member).await?)),
            None => Ok(None),
        }
    }

    /// Adds `user` to organization `org` with `role` (a built-in or
    /// configured organization role other than `owner`), reaching
    /// `workspaces` (which must be the organization's).
    /// [`WorkspaceError::AlreadyOrganizationMember`] if they belong already.
    pub async fn add(
        &self,
        uow: &Uow,
        org: Uuid,
        user: Uuid,
        role: OrgRole,
        workspaces: WorkspaceSelector,
    ) -> Result<OrgMember, WorkspaceError> {
        self.core.require_organizations()?;
        self.check_grant(uow, org, &role, &workspaces).await?;
        if role == OrgRole::Owner {
            return Err(owner_role());
        }
        if self.core.org_member_in(uow, org, user).await?.is_some() {
            return Err(WorkspaceError::AlreadyOrganizationMember);
        }
        let now = self.core.now();
        let added = store::insert(
            uow,
            OrgMember {
                id: Uuid::now_v7(),
                organization_id: org,
                user_id: user,
                role: role.as_str().to_owned(),
                scope: workspaces.scope_name().to_owned(),
                created_at: now,
                updated_at: now,
            },
        )
        .await?;
        let added = set_org_selector(uow, added, &workspaces, now).await?;
        self.core
            .emit_after_commit(
                uow,
                OrgMemberAdded {
                    organization_id: org,
                    user_id: user,
                    role: role.as_str().to_owned(),
                    workspaces,
                },
            )
            .await;
        Ok(added)
    }

    /// Changes the role and/or the workspaces of member `user` of
    /// organization `org` (`None` keeps one). [`WorkspaceError::MemberNotFound`]
    /// if they are not a member; [`WorkspaceError::OrganizationOwnerRequired`]
    /// when the owner would lose the `owner` role or any workspace.
    pub async fn update(
        &self,
        uow: &Uow,
        org: Uuid,
        user: Uuid,
        role: Option<OrgRole>,
        workspaces: Option<WorkspaceSelector>,
    ) -> Result<OrgMember, WorkspaceError> {
        self.core.require_organizations()?;
        let organization = self.core.organization_in(uow, org).await?;
        let current = self
            .core
            .org_member_in(uow, org, user)
            .await?
            .ok_or(WorkspaceError::MemberNotFound)?;
        let current_selector = org_selector(uow, &current).await?;
        let new_role = role.unwrap_or_else(|| current.role());
        let new_selector = workspaces.unwrap_or_else(|| current_selector.clone());
        if user == organization.owner_id {
            if new_role != OrgRole::Owner || !new_selector.is_all() {
                return Err(WorkspaceError::OrganizationOwnerRequired);
            }
            return Ok(current);
        }
        self.check_grant(uow, org, &new_role, &new_selector).await?;
        if new_role == OrgRole::Owner {
            return Err(owner_role());
        }
        let from = current.role.clone();
        if from == new_role.as_str() && current_selector == new_selector {
            return Ok(current);
        }
        let now = self.core.now();
        let changed = set_org_role(uow, current, new_role.clone(), now).await?;
        let changed = set_org_selector(uow, changed, &new_selector, now).await?;
        self.core
            .emit_after_commit(
                uow,
                OrgMemberChanged {
                    organization_id: org,
                    user_id: user,
                    from,
                    to: new_role.as_str().to_owned(),
                    workspaces: new_selector,
                },
            )
            .await;
        Ok(changed)
    }

    /// Gives `user` organization role `role` reaching `workspaces`: adds
    /// them, or changes their membership when they belong already.
    pub async fn grant(
        &self,
        uow: &Uow,
        org: Uuid,
        user: Uuid,
        role: OrgRole,
        workspaces: WorkspaceSelector,
    ) -> Result<OrgMember, WorkspaceError> {
        self.core.require_organizations()?;
        let exists = self
            .core
            .org_member_in(uow, org, user)
            .await?
            .is_some();
        if exists {
            self.update(uow, org, user, Some(role), Some(workspaces))
                .await
        } else {
            self.add(uow, org, user, role, workspaces).await
        }
    }

    /// Removes `user` from organization `org` (also how a member leaves).
    /// [`WorkspaceError::MemberNotFound`] if they are not a member,
    /// [`WorkspaceError::OrganizationOwnerRequired`] for the owner.
    pub async fn remove(&self, uow: &Uow, org: Uuid, user: Uuid) -> Result<(), WorkspaceError> {
        self.core.require_organizations()?;
        let organization = self.core.organization_in(uow, org).await?;
        let current = self
            .core
            .org_member_in(uow, org, user)
            .await?
            .ok_or(WorkspaceError::MemberNotFound)?;
        if user == organization.owner_id {
            return Err(WorkspaceError::OrganizationOwnerRequired);
        }
        let role = current.role.clone();
        store::delete(uow, current).await?;
        self.core
            .emit_after_commit(
                uow,
                OrgMemberRemoved {
                    organization_id: org,
                    user_id: user,
                    role,
                },
            )
            .await;
        Ok(())
    }

    /// Checks the organization, the role and the listed workspaces.
    async fn check_grant(
        &self,
        uow: &Uow,
        org: Uuid,
        role: &OrgRole,
        workspaces: &WorkspaceSelector,
    ) -> Result<(), WorkspaceError> {
        self.core.check_org_role(role)?;
        self.core.organization_in(uow, org).await?;
        check_workspaces(uow, org, workspaces).await
    }
}

impl fmt::Debug for OrgMembers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OrgMembers").finish_non_exhaustive()
    }
}

/// The 400 for granting `owner`.
fn owner_role() -> WorkspaceError {
    WorkspaceError::invalid(
        "role",
        "role must not be owner: transfer ownership of the organization instead",
    )
}
