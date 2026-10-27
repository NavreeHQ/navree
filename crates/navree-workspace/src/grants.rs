//! Grants: which workspaces an organization member reaches, which partitions
//! a workspace member sees, and the workspace role an organization grant
//! gives.

use chrono::{DateTime, Utc};
use navree_db::{Repo, Uow};
use sea_orm::{
    ColumnTrait, ConnectionTrait, EntityTrait, IntoActiveModel, QueryFilter, QueryOrder, Set,
};
use uuid::Uuid;

use crate::core::Core;
use crate::entity::{
    Member, OrgMember, Workspace, member, organization_grant, organization_member,
    partition_grant, workspace,
};
use crate::org_role::{SCOPE_ALL, SCOPE_ONLY};
use crate::{OrgRole, PartitionSelector, Selector, WorkspaceError, WorkspaceRole, WorkspaceSelector};

/// The workspaces organization member `member` reaches: `All` for the
/// owner and for `all` members, else the granted ones.
pub(crate) async fn org_selector<C: ConnectionTrait>(
    conn: &C,
    member: &OrgMember,
) -> Result<WorkspaceSelector, WorkspaceError> {
    if !member.is_limited() || member.role() == OrgRole::Owner {
        return Ok(Selector::All);
    }
    let ids = organization_grant::Entity::find()
        .filter(organization_grant::Column::MemberId.eq(member.id))
        .order_by_asc(organization_grant::Column::CreatedAt)
        .order_by_asc(organization_grant::Column::Id)
        .all(conn)
        .await?
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
    let existing = organization_grant::Entity::find()
        .filter(organization_grant::Column::MemberId.eq(member.id))
        .all(&*uow.tx().await?)
        .await?;
    for grant in existing {
        Repo::<organization_grant::Entity>::delete(uow, grant).await?;
    }
    if let Some(ids) = selector.ids() {
        for workspace in ids {
            Repo::<organization_grant::Entity>::insert(
                uow,
                organization_grant::ActiveModel {
                    id: Set(Uuid::now_v7()),
                    organization_id: Set(member.organization_id),
                    member_id: Set(member.id),
                    workspace_id: Set(*workspace),
                    created_at: Set(now),
                },
            )
            .await?;
        }
    }
    let scope = selector.scope_name();
    if member.scope == scope {
        return Ok(member);
    }
    let mut row = member.into_active_model();
    row.scope = Set(scope.to_owned());
    row.updated_at = Set(now);
    Ok(Repo::<organization_member::Entity>::update(uow, row).await?)
}

/// The workspaces attached to organization `org`, by name.
pub(crate) async fn attached<C: ConnectionTrait>(
    conn: &C,
    org: Uuid,
) -> Result<Vec<Workspace>, WorkspaceError> {
    Ok(workspace::Entity::find()
        .filter(workspace::Column::OrganizationId.eq(org))
        .order_by_asc(workspace::Column::Name)
        .order_by_asc(workspace::Column::Id)
        .all(conn)
        .await?)
}

/// The workspaces of organization `org` that `selector` reaches, by name.
pub(crate) async fn reached<C: ConnectionTrait>(
    conn: &C,
    org: Uuid,
    selector: &WorkspaceSelector,
) -> Result<Vec<Workspace>, WorkspaceError> {
    Ok(attached(conn, org)
        .await?
        .into_iter()
        .filter(|workspace| selector.includes(workspace.id))
        .collect())
}

/// Checks that every listed workspace is attached to `org` (a 400 on
/// `workspaces` otherwise).
pub(crate) async fn check_workspaces<C: ConnectionTrait>(
    conn: &C,
    org: Uuid,
    selector: &WorkspaceSelector,
) -> Result<(), WorkspaceError> {
    let Some(ids) = selector.ids() else {
        return Ok(());
    };
    let attached: Vec<Uuid> = attached(conn, org).await?.iter().map(|w| w.id).collect();
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
pub(crate) async fn org_grant<C: ConnectionTrait>(
    core: &Core,
    conn: &C,
    workspace: &Workspace,
    user: Uuid,
) -> Result<Option<OrgGrant>, WorkspaceError> {
    let Some(org) = workspace.organization_id else {
        return Ok(None);
    };
    let Some(member) = core.org_member_in(conn, org, user).await? else {
        return Ok(None);
    };
    let role = member.role();
    let Some(workspace_role) = core.org_roles.workspace_role(&role).cloned() else {
        return Ok(None);
    };
    if member.is_limited() && role != OrgRole::Owner {
        let granted = organization_grant::Entity::find()
            .filter(organization_grant::Column::MemberId.eq(member.id))
            .filter(organization_grant::Column::WorkspaceId.eq(workspace.id))
            .one(conn)
            .await?
            .is_some();
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
pub(crate) async fn partition_selector<C: ConnectionTrait>(
    conn: &C,
    member: &Member,
) -> Result<PartitionSelector, WorkspaceError> {
    if !member.partitions_limited() || member.role() == WorkspaceRole::Owner {
        return Ok(Selector::All);
    }
    let ids = partition_grant::Entity::find()
        .filter(partition_grant::Column::MemberId.eq(member.id))
        .order_by_asc(partition_grant::Column::CreatedAt)
        .order_by_asc(partition_grant::Column::Id)
        .all(conn)
        .await?
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
    let existing = partition_grant::Entity::find()
        .filter(partition_grant::Column::MemberId.eq(member.id))
        .all(&*uow.tx().await?)
        .await?;
    for grant in existing {
        Repo::<partition_grant::Entity>::delete(uow, grant).await?;
    }
    if let Some(ids) = selector.ids() {
        for partition in ids {
            Repo::<partition_grant::Entity>::insert(
                uow,
                partition_grant::ActiveModel {
                    id: Set(Uuid::now_v7()),
                    workspace_id: Set(member.workspace_id),
                    member_id: Set(member.id),
                    partition_id: Set(*partition),
                    created_at: Set(now),
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
    let mut row = member.into_active_model();
    row.partition_scope = Set(scope.to_owned());
    row.updated_at = Set(now);
    Ok(Repo::<member::Entity>::update(uow, row).await?)
}
