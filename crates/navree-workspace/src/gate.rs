//! The module's `dyn WorkspaceGate`: who may work in a workspace, for modules
//! that work in workspaces of their own accord.

use std::sync::Arc;

use http::request::Parts;
use navree_access::{WorkspaceEntry, WorkspaceGate, WorkspaceWrites};
use navree_contracts::AuthState;
use navree_db::Uow;
use navree_http::AppError;
use navree_kernel::BoxFuture;
use uuid::Uuid;

use crate::core::Core;
use crate::current::{app_error, enter_named, guest_member, writes};
use crate::demo::DemoIntent;
use crate::entity::Workspace;
use crate::role::WorkspaceRole;
use crate::{Isolation, ResolverKind, WorkspaceError, Workspaces};

/// Answers with the rules of [`CurrentWorkspace`](crate::CurrentWorkspace): a
/// membership, an organization's grant, or a demo's guest role.
pub(crate) struct Gate(pub(crate) Arc<Core>);

impl Gate {
    /// The workspace `id` and the role `user` works in it with, if any.
    async fn standing(
        &self,
        user: Uuid,
        id: Uuid,
    ) -> Result<Option<(Workspace, Option<WorkspaceRole>)>, AppError> {
        let core = &self.0;
        let workspace = match core.workspace_in(&*core.db, id).await {
            Ok(workspace) => workspace,
            Err(WorkspaceError::WorkspaceNotFound(_)) => return Ok(None),
            Err(error) => return Err(app_error(error)),
        };
        let ctx = core
            .app
            .upgrade()
            .ok_or_else(|| AppError::internal("the app has shut down"))?;
        let workspaces = ctx.get::<Workspaces>().await.map_err(AppError::internal)?;
        let role = match workspaces
            .admission(&workspace, user)
            .await
            .map_err(app_error)?
        {
            Some(admission) => Some(admission.role),
            None => guest_member(&core.config, &workspace, user).map(|member| member.role()),
        };
        Ok(Some((workspace, role)))
    }

    /// What `workspace` takes from a user with `role`, if they have one.
    fn writes_of(&self, workspace: &Workspace, role: Option<&WorkspaceRole>) -> WorkspaceWrites {
        let config = &self.0.config;
        let demo_editor = workspace.is_demo()
            && config.demo.allows_write(
                &http::Method::POST,
                None,
                None,
                role.map_or("", WorkspaceRole::as_str),
            );
        let read_only_demo =
            workspace.is_demo() && config.demo.mode == crate::DemoMode::ReadOnly && !demo_editor;
        let Some(role) = role else {
            return WorkspaceWrites {
                read_only_demo,
                ..WorkspaceWrites::default()
            };
        };
        // An owner has everything, and `workspace.roles` only adds to that.
        let role_permissions = match role {
            WorkspaceRole::Owner => Vec::new(),
            role => config
                .roles
                .get(role.as_str())
                .map(|options| options.permissions.clone())
                .unwrap_or_default(),
        };
        let edits_read_only_demo = demo_editor && config.demo.mode == crate::DemoMode::ReadOnly;
        WorkspaceWrites {
            read_only_demo,
            read_only_role: !edits_read_only_demo
                && !matches!(
                    role,
                    WorkspaceRole::Owner | WorkspaceRole::Admin | WorkspaceRole::Member
                ),
            role_permissions,
        }
    }
}

impl WorkspaceGate for Gate {
    fn header(&self) -> Option<&str> {
        let config = &self.0.config;
        config
            .resolvers
            .contains(&ResolverKind::Header)
            .then_some(config.header.as_str())
    }

    fn own_databases(&self) -> bool {
        self.0.config.isolation == Isolation::Database
    }

    fn check(
        &self,
        user: Uuid,
        workspace: Uuid,
        writes: bool,
    ) -> BoxFuture<'_, Result<WorkspaceEntry, AppError>> {
        Box::pin(async move {
            let Some((found, role)) = self.standing(user, workspace).await? else {
                return Ok(WorkspaceEntry::NoSuchWorkspace);
            };
            let Some(role) = role else {
                return Ok(WorkspaceEntry::NotAMember);
            };
            if writes && let Some(refusal) = self.writes_of(&found, Some(&role)).refusal() {
                return Err(refusal);
            }
            Ok(WorkspaceEntry::Allowed)
        })
    }

    fn enter_request<'a>(
        &'a self,
        parts: &'a mut Parts,
        workspace: Uuid,
    ) -> BoxFuture<'a, Result<WorkspaceEntry, AppError>> {
        Box::pin(async move {
            let user = parts
                .extensions
                .get::<AuthState>()
                .and_then(|state| state.0.as_ref())
                .map(|principal| principal.user_id)
                .ok_or_else(AppError::unauthenticated)?;
            match self.standing(user, workspace).await? {
                None => Ok(WorkspaceEntry::NoSuchWorkspace),
                Some((_, None)) => Ok(WorkspaceEntry::NotAMember),
                Some((found, Some(role))) => {
                    // A write to a read-only demo is refused on entering it, with
                    // the routes that stay open in mind.
                    let intent = parts.extensions.get::<DemoIntent>().copied();
                    let allowed = self.writes_of(&found, Some(&role));
                    if writes(&parts.method, intent)
                        && !allowed.read_only_demo
                        && allowed.role_is_read_only()
                    {
                        return Err(WorkspaceWrites::read_only_role_error());
                    }
                    enter_named(parts, workspace).await?;
                    Ok(WorkspaceEntry::Allowed)
                }
            }
        })
    }

    fn writes(
        &self,
        user: Uuid,
        workspace: Uuid,
    ) -> BoxFuture<'_, Result<WorkspaceWrites, AppError>> {
        Box::pin(async move {
            Ok(match self.standing(user, workspace).await? {
                Some((found, role)) => self.writes_of(&found, role.as_ref()),
                None => WorkspaceWrites::default(),
            })
        })
    }

    fn switch<'a>(
        &'a self,
        uow: &'a Uow,
        workspace: Option<Uuid>,
    ) -> BoxFuture<'a, Result<(), AppError>> {
        Box::pin(async move {
            let isolation = &self.0.isolation;
            match workspace {
                Some(id) => isolation.apply(uow, id).await,
                None => isolation.leave(uow).await,
            }
            .map_err(app_error)
        })
    }
}
