//! Workspace-scoped abilities: the member's role in the current workspace,
//! evaluated with navree-access's engine and the app's abilities function.

use std::collections::BTreeMap;
use std::sync::Arc;

use navree_access::{
    ALL, AbilityCtx, AbilityFactory, AccessOptions, AccessUser, MANAGE, PrincipalAccess, Rules,
    Subject,
};
use navree_contracts::Principal;
use navree_kernel::AppContext;
use uuid::Uuid;

use crate::entity::{
    AccessRequest, Invitation, Member, OrgMember, Organization, Partition, Workspace,
};
use crate::{RoleTable, WorkspaceError, WorkspaceRole};

/// The permission prefixes of the module's own subjects, for apps without
/// `AccessModule` (whose `AbilityFactory` knows every registered subject).
fn own_subjects() -> BTreeMap<String, String> {
    [
        (Workspace::PERMISSION_PREFIX, Workspace::SUBJECT),
        (Member::PERMISSION_PREFIX, Member::SUBJECT),
        (Invitation::PERMISSION_PREFIX, Invitation::SUBJECT),
        (Organization::PERMISSION_PREFIX, Organization::SUBJECT),
        (OrgMember::PERMISSION_PREFIX, OrgMember::SUBJECT),
        (AccessRequest::PERMISSION_PREFIX, AccessRequest::SUBJECT),
        (Partition::PERMISSION_PREFIX, Partition::SUBJECT),
    ]
    .into_iter()
    .map(|(prefix, subject)| (prefix.to_owned(), subject.to_owned()))
    .collect()
}

/// Where scoped access applies: a workspace (with the organization it
/// belongs to, if any) or an organization.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AccessScope {
    /// The workspace (`AbilityCtx::team_id`).
    pub(crate) workspace: Option<Uuid>,
    /// The organization (`AbilityCtx::organization_id`).
    pub(crate) organization: Option<Uuid>,
}

/// The access of `principal` inside `workspace`, where they have `role`:
///
/// - the abilities function of `AccessOptions` (or navree-access's default,
///   the permissions alone) runs with an [`AbilityCtx`] whose user has the
///   single role `role`, whose permissions are the role's (built-in plus
///   `workspace.roles`), whose `team_id` is the workspace and whose
///   `organization_id` is the workspace's organization;
/// - a global super-admin (`AccessOptions::super_admin_role`) keeps
///   `can manage all`;
/// - a token's abilities still narrow the result.
pub(crate) async fn workspace_access(
    ctx: &AppContext,
    principal: &Principal,
    workspace: Uuid,
    organization: Option<Uuid>,
    role: &WorkspaceRole,
    roles: &RoleTable,
) -> Result<Arc<PrincipalAccess>, WorkspaceError> {
    scoped_access(
        ctx,
        principal,
        AccessScope {
            workspace: Some(workspace),
            organization,
        },
        role.as_str(),
        roles.permissions(role).to_vec(),
    )
    .await
}

/// The access of `principal` in scope `scope` with the single role `role`
/// and its `permissions` (see [`workspace_access`]).
pub(crate) async fn scoped_access(
    ctx: &AppContext,
    principal: &Principal,
    scope: AccessScope,
    role: &str,
    mut permissions: Vec<String>,
) -> Result<Arc<PrincipalAccess>, WorkspaceError> {
    let options = ctx
        .try_get::<AccessOptions>()
        .await
        .map(|options| (*options).clone())
        .unwrap_or_default();
    let factory = ctx.try_get::<AbilityFactory>().await;
    let subjects = match &factory {
        Some(factory) => factory.permission_subjects().clone(),
        None => Arc::new(own_subjects()),
    };
    let (version, super_admin) = match &factory {
        Some(factory) => {
            let global = factory
                .access_for(Some(principal))
                .await
                .map_err(|error| WorkspaceError::Other(error.into()))?;
            (global.version, global.super_admin)
        }
        None => (0, false),
    };
    permissions.sort();
    permissions.dedup();
    let ability_ctx = AbilityCtx {
        user: Some(AccessUser {
            id: principal.user_id,
            roles: vec![role.to_owned()],
        }),
        permissions: permissions.clone(),
        team_id: scope.workspace,
        organization_id: scope.organization,
        permission_subjects: subjects.clone(),
    };
    let mut rules = Rules::new();
    match &factory {
        // The registered policies, then the app's abilities function.
        Some(factory) => factory.define_rules(&ability_ctx, &mut rules),
        None => (options.abilities)(&ability_ctx, &mut rules),
    }
    if super_admin {
        rules.can(MANAGE, ALL).because("super admin");
    }
    let mut ability = rules
        .try_build()
        .map_err(|error| WorkspaceError::Other(error.into()))?;
    if let Some(token_abilities) = &principal.token_abilities {
        ability = ability.restrict_to(token_abilities, &subjects);
    }
    Ok(Arc::new(PrincipalAccess {
        user: Some(principal.user_id),
        roles: vec![role.to_owned()],
        permissions,
        version,
        super_admin,
        ability: Arc::new(ability),
    }))
}

/// The (action, subject) a permission string checks, with `subjects`
/// mapping prefixes (`workspace.members.update` → (`update`,
/// `WorkspaceMember`)); `*` means `manage`, an unknown prefix stands for
/// itself.
pub(crate) fn permission_target<'a>(
    permission: &'a str,
    subjects: &'a BTreeMap<String, String>,
) -> (&'a str, &'a str) {
    let (prefix, action) = permission.rsplit_once('.').unwrap_or((permission, ""));
    let action = if action == "*" { MANAGE } else { action };
    let subject = subjects.get(prefix).map_or(prefix, String::as_str);
    (action, subject)
}

/// Every known permission prefix: the app's (when it has `AccessModule`) or
/// the module's own.
pub(crate) async fn subjects_of(ctx: &AppContext) -> Arc<BTreeMap<String, String>> {
    match ctx.try_get::<AbilityFactory>().await {
        Some(factory) => factory.permission_subjects().clone(),
        None => Arc::new(own_subjects()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permissions_name_their_subjects() {
        let subjects = own_subjects();
        assert_eq!(
            permission_target("workspace.members.update", &subjects),
            ("update", "WorkspaceMember")
        );
        assert_eq!(
            permission_target("workspace.invitations.*", &subjects),
            ("manage", "WorkspaceInvitation")
        );
        assert_eq!(
            permission_target("workspace.delete", &subjects),
            ("delete", "Workspace")
        );
        assert_eq!(
            permission_target("deals.read", &subjects),
            ("read", "deals")
        );
        assert_eq!(
            permission_target("organization.attach", &subjects),
            ("attach", "Organization")
        );
        assert_eq!(
            permission_target("organization.members.update", &subjects),
            ("update", "OrganizationMember")
        );
        assert_eq!(
            permission_target("organization.access_requests.*", &subjects),
            ("manage", "AccessRequest")
        );
        assert_eq!(
            permission_target("workspace.partitions.create", &subjects),
            ("create", "WorkspacePartition")
        );
    }
}
