//! The module's `dyn PermissionCheck`: a member's role in a workspace
//! answering permission questions about that workspace outside a route.

use std::sync::Arc;

use navree_contracts::{BoxError, BoxFuture, PermissionCheck, Principal};
use uuid::Uuid;

use crate::access::{permission_target, subjects_of, workspace_access};
use crate::core::Core;

/// Answers [`PermissionCheck`] questions about a workspace with the user's
/// role there, evaluated exactly as behind the
/// [`InWorkspace`](crate::InWorkspace) guard: `Some(false)` for a
/// non-member, and `None` when no workspace is named (app-wide access is
/// navree-access's to answer).
pub(crate) struct WorkspacePermissionCheck(pub(crate) Arc<Core>);

impl PermissionCheck for WorkspacePermissionCheck {
    fn check<'a>(
        &'a self,
        principal: &'a Principal,
        permission: &'a str,
        workspace: Option<Uuid>,
    ) -> BoxFuture<'a, Result<Option<bool>, BoxError>> {
        Box::pin(async move {
            let Some(workspace) = workspace else {
                return Ok(None);
            };
            let core = &self.0;
            let reader = core.db.reader().await;
            let Some(member) = core
                .member_in(&reader, workspace, principal.user_id)
                .await?
            else {
                return Ok(Some(false));
            };
            let Some(ctx) = core.app.upgrade() else {
                return Ok(Some(false));
            };
            let access =
                workspace_access(&ctx, principal, workspace, &member.role(), &core.roles).await?;
            let subjects = subjects_of(&ctx).await;
            let (action, subject) = permission_target(permission, &subjects);
            Ok(Some(access.ability.has_unconditional(action, subject)))
        })
    }
}
