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

use crate::entity::{invitation, member, workspace};
use crate::{RoleTable, WorkspaceError, WorkspaceRole};

/// The permission prefixes of the module's own subjects, for apps without
/// `AccessModule` (whose `AbilityFactory` knows every registered subject).
fn own_subjects() -> BTreeMap<String, String> {
    [
        (
            workspace::Entity::PERMISSION_PREFIX,
            workspace::Entity::SUBJECT,
        ),
        (member::Entity::PERMISSION_PREFIX, member::Entity::SUBJECT),
        (
            invitation::Entity::PERMISSION_PREFIX,
            invitation::Entity::SUBJECT,
        ),
    ]
    .into_iter()
    .map(|(prefix, subject)| (prefix.to_owned(), subject.to_owned()))
    .collect()
}

/// The access of `principal` inside `workspace`, where they have `role`:
///
/// - the abilities function of `AccessOptions` (or navree-access's default,
///   the permissions alone) runs with an [`AbilityCtx`] whose user has the
///   single role `role`, whose permissions are the role's (built-in plus
///   `workspace.roles`), and whose `team_id` is the workspace;
/// - a global super-admin (`AccessOptions::super_admin_role`) keeps
///   `can manage all`;
/// - a token's abilities still narrow the result.
pub(crate) async fn workspace_access(
    ctx: &AppContext,
    principal: &Principal,
    workspace: Uuid,
    role: &WorkspaceRole,
    roles: &RoleTable,
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
    let mut permissions = roles.permissions(role).to_vec();
    permissions.sort();
    permissions.dedup();
    let ability_ctx = AbilityCtx {
        user: Some(AccessUser {
            id: principal.user_id,
            roles: vec![role.as_str().to_owned()],
        }),
        permissions: permissions.clone(),
        team_id: Some(workspace),
        permission_subjects: subjects.clone(),
    };
    let mut rules = Rules::new();
    (options.abilities)(&ability_ctx, &mut rules);
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
        roles: vec![role.as_str().to_owned()],
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
    }
}
