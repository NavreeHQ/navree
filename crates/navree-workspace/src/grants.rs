//! Grants: which workspaces an organization member reaches, which partitions
//! a workspace member sees, and the workspace role an organization grant
//! gives.

use chrono::{DateTime, Utc};
use navree_db::Uow;
use navree_orm::{Executor, Model as _};
use uuid::Uuid;

use crate::core::Core;
use crate::entity::{Member, OrgMember, Workspace, organization_grant, partition_grant};
use crate::org_role::{SCOPE_ALL, SCOPE_ONLY};
use crate::store;
use crate::{
    OrgRole, PartitionSelector, Selector, WorkspaceError, WorkspaceRole, WorkspaceSelector,
};

/// The workspaces organization member `member` reaches: `All` for the
/// owner and for `all` members, else the granted ones.
pub(crate) async fn org_selector<E: Executor + ?Sized>(
    exec: &E,
    member: &OrgMember,
) -> Result<WorkspaceSelector, WorkspaceError> {
    if !member.is_limited() || member.role() == OrgRole::Owner {
        return Ok(Selector::All);
    }
    type Grant = organization_grant::Model;
    let ids = Grant::query()
        .where_(Grant::MEMBER_ID.eq(member.id))
        .order_by(Grant::CREATED_AT)
        .order_by(Grant::ID)
        .get(exec)
        .await?
        .into_models()
        .into_iter()
        .map(|grant| grant.workspace_id);
    Ok(Selector::only(ids))
}

/// Stores `selector` for organization member `member` in `uow`: replaces
/// the member's grants and sets their scope.
pub(crate) async fn set_org_selector(
    uow: &Uow,
    member: OrgMember,
    selector: &WorkspaceSelector,
    now: DateTime<Utc>,
) -> Result<OrgMember, WorkspaceError> {
    type Grant = organization_grant::Model;
    let existing = Grant::query()
        .where_(Grant::MEMBER_ID.eq(member.id))
        .get(uow)
        .await?
        .into_models();
    for grant in existing {
        store::delete(uow, grant).await?;
    }
    if let Some(ids) = selector.ids() {
        for workspace in ids {
            store::insert(
                uow,
                Grant {
                    id: Uuid::now_v7(),
                    organization_id: member.organization_id,
                    member_id: member.id,
                    workspace_id: *workspace,
                    created_at: now,
                },
            )
            .await?;
        }
    }
    let scope = selector.scope_name();
    if member.scope == scope {
        return Ok(member);
    }
    store::update(uow, member, |row| {
        row.scope = scope.to_owned();
        row.updated_at = now;
    })
    .await
}

/// The workspaces attached to organization `org`, by name.
pub(crate) async fn attached<E: Executor + ?Sized>(
    exec: &E,
    org: Uuid,
) -> Result<Vec<Workspace>, WorkspaceError> {
    Ok(Workspace::query()
        .where_(Workspace::ORGANIZATION_ID.eq(org))
        .order_by(Workspace::NAME)
        .order_by(Workspace::ID)
        .get(exec)
        .await?
        .into_models())
}

/// The workspaces of organization `org` that `selector` reaches, by name.
pub(crate) async fn reached<E: Executor + ?Sized>(
    exec: &E,
    org: Uuid,
    selector: &WorkspaceSelector,
) -> Result<Vec<Workspace>, WorkspaceError> {
    Ok(attached(exec, org)
        .await?
        .into_iter()
        .filter(|workspace| selector.includes(workspace.id))
        .collect())
}

/// Checks that every listed workspace is attached to `org` (a 400 on
/// `workspaces` otherwise).
pub(crate) async fn check_workspaces<E: Executor + ?Sized>(
    exec: &E,
    org: Uuid,
    selector: &WorkspaceSelector,
) -> Result<(), WorkspaceError> {
    let Some(ids) = selector.ids() else {
        return Ok(());
    };
    let attached: Vec<Uuid> = attached(exec, org).await?.iter().map(|w| w.id).collect();
    if ids.iter().all(|id| attached.contains(id)) {
        Ok(())
    } else {
        Err(WorkspaceError::invalid(
            "workspaces",
            "workspaces must list workspaces of the organization",
        ))
    }
}

/// How a user may enter a workspace through its organization.
#[derive(Clone, Debug)]
pub(crate) struct OrgGrant {
    /// Their organization membership.
    pub(crate) member: OrgMember,
    /// Their organization role.
    pub(crate) role: OrgRole,
    /// The workspace role it gives.
    pub(crate) workspace_role: WorkspaceRole,
}

/// The grant through which `user` may enter `workspace` as a member of its
/// organization: `None` when the workspace has no organization, the user
/// is not a member of it, their role is unknown, or they are limited to
/// other workspaces.
pub(crate) async fn org_grant<E: Executor + ?Sized>(
    core: &Core,
    exec: &E,
    workspace: &Workspace,
    user: Uuid,
) -> Result<Option<OrgGrant>, WorkspaceError> {
    let Some(org) = workspace.organization_id else {
        return Ok(None);
    };
    let Some(member) = core.org_member_in(exec, org, user).await? else {
        return Ok(None);
    };
    let role = member.role();
    let Some(workspace_role) = core.org_roles.workspace_role(&role).cloned() else {
        return Ok(None);
    };
    if member.is_limited() && role != OrgRole::Owner {
        type Grant = organization_grant::Model;
        let granted = Grant::query()
            .where_(Grant::MEMBER_ID.eq(member.id))
            .where_(Grant::WORKSPACE_ID.eq(workspace.id))
            .exists(exec)
            .await?;
        if !granted {
            return Ok(None);
        }
    }
    Ok(Some(OrgGrant {
        member,
        role,
        workspace_role,
    }))
}

/// The partitions workspace member `member` sees: `All` for the owner and
/// for `all` members, else the granted ones.
pub(crate) async fn partition_selector<E: Executor + ?Sized>(
    exec: &E,
    member: &Member,
) -> Result<PartitionSelector, WorkspaceError> {
    if !member.partitions_limited() || member.role() == WorkspaceRole::Owner {
        return Ok(Selector::All);
    }
    type Grant = partition_grant::Model;
    let ids = Grant::query()
        .where_(Grant::MEMBER_ID.eq(member.id))
        .order_by(Grant::CREATED_AT)
        .order_by(Grant::ID)
        .get(exec)
        .await?
        .into_models()
        .into_iter()
        .map(|grant| grant.partition_id);
    Ok(Selector::only(ids))
}

/// Stores `selector` for workspace member `member` in `uow`: replaces the
/// member's partition grants and sets their partition scope.
pub(crate) async fn set_partition_selector(
    uow: &Uow,
    member: Member,
    selector: &PartitionSelector,
    now: DateTime<Utc>,
) -> Result<Member, WorkspaceError> {
    type Grant = partition_grant::Model;
    let existing = Grant::query()
        .where_(Grant::MEMBER_ID.eq(member.id))
        .get(uow)
        .await?
        .into_models();
    for grant in existing {
        store::delete(uow, grant).await?;
    }
    if let Some(ids) = selector.ids() {
        for partition in ids {
            store::insert(
                uow,
                Grant {
                    id: Uuid::now_v7(),
                    workspace_id: member.workspace_id,
                    member_id: member.id,
                    partition_id: *partition,
                    created_at: now,
                },
            )
            .await?;
        }
    }
    let scope = if selector.is_all() {
        SCOPE_ALL
    } else {
        SCOPE_ONLY
    };
    if member.partition_scope == scope {
        return Ok(member);
    }
    store::update(uow, member, |row| {
        row.partition_scope = scope.to_owned();
        row.updated_at = now;
    })
    .await
}
