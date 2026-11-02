//! [`WorkspaceGate`]: who may work in a workspace, for modules that work in
//! workspaces without being the workspace module.

use http::request::Parts;
use navree_db::Uow;
use navree_http::AppError;
use navree_kernel::BoxFuture;
use uuid::Uuid;

/// A request extension that limits the request to one workspace: set by an
/// authentication strategy whose credential belongs to a workspace (an OAuth
/// client created for one workspace). A request that names no workspace works
/// in this one, and one that names another is refused, whoever holds the
/// credential.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PinnedWorkspace(pub Uuid);

impl PinnedWorkspace {
    /// The answer to a request that names another workspace than the one its
    /// credential is limited to (a 403).
    pub fn mismatch() -> AppError {
        AppError::forbidden()
            .with_code("token_workspace_mismatch")
            .with_detail("This token is limited to another workspace.")
    }
}

/// What a [`WorkspaceGate`] says of a user and a workspace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceEntry {
    /// They may work in it, as a member or through its organization.
    Allowed,
    /// The workspace exists, and they are not in it.
    NotAMember,
    /// There is no such workspace.
    NoSuchWorkspace,
}

/// What a workspace takes from a user: what a [`WorkspaceGate`] reports of the
/// demo it may be and of the user's role in it, for a module that works in
/// workspaces and limits what they do there.
///
/// A role writes by default if it is `owner`, `admin` or `member`, and doesn't if
/// it is `viewer` or one of the app's own. The permissions an app sets for a role
/// (`workspace.roles.<name>`) decide for the models they name, either way:
/// `notes.update` lets a viewer update notes and a `member` with only `notes.read`
/// no longer writes them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkspaceWrites {
    /// The workspace is a read-only demo, and the user's role doesn't edit it.
    pub read_only_demo: bool,
    /// The user has a role that can't write where its permissions say nothing.
    pub read_only_role: bool,
    /// The permissions the app set for the user's role.
    pub role_permissions: Vec<String>,
}

impl WorkspaceWrites {
    /// The answer to a write the user's role can't make (a 403).
    pub fn read_only_role_error() -> AppError {
        AppError::forbidden()
            .with_code("workspace_read_only")
            .with_detail("Your role in this workspace is read-only.")
    }

    /// Whether the user's role writes `action` on the model with the permission
    /// prefix `prefix`.
    pub fn role_writes(&self, prefix: &str, action: &str) -> bool {
        let mut named = false;
        for permission in &self.role_permissions {
            let Some((on, granted)) = permission.rsplit_once('.') else {
                continue;
            };
            if on == "all" && granted == "*" {
                return true;
            }
            if on == prefix {
                if granted == "*" || granted == action || granted == "manage" {
                    return true;
                }
                named = true;
            }
        }
        !named && !self.read_only_role
    }

    /// Whether the user's role writes nothing at all.
    pub fn role_is_read_only(&self) -> bool {
        self.read_only_role
            && !self.role_permissions.iter().any(|permission| {
                permission
                    .rsplit_once('.')
                    .is_some_and(|(_, action)| !matches!(action, "read" | "view"))
            })
    }

    /// The answer to any write, if the workspace takes none from the user.
    pub fn refusal(&self) -> Option<AppError> {
        if self.read_only_demo {
            Some(AppError::demo_read_only())
        } else if self.role_is_read_only() {
            Some(Self::read_only_role_error())
        } else {
            None
        }
    }
}

/// How a module enters a workspace on a user's behalf: navree-workspace's
/// `WorkspaceModule` registers it, and a module that finds none has no way of
/// knowing who may work in a workspace.
///
/// ```no_run
/// # use navree_access::{WorkspaceEntry, WorkspaceGate};
/// # async fn demo(ctx: navree_kernel::AppContext, user: uuid::Uuid, workspace: uuid::Uuid) {
/// let Some(gate) = ctx.try_get::<dyn WorkspaceGate>().await else {
///     return; // No workspace module: refuse, don't trust what a request names.
/// };
/// if gate.check(user, workspace, false).await.unwrap() == WorkspaceEntry::Allowed {
///     // ...
/// }
/// # }
/// ```
pub trait WorkspaceGate: Send + Sync + 'static {
    /// The header a request names its workspace with, if requests may.
    fn header(&self) -> Option<&str>;

    /// Whether each workspace keeps its records in a database of its own,
    /// which a unit of work on the app's database never reaches.
    fn own_databases(&self) -> bool;

    /// Whether `user` may work in `workspace`. With `writes`, an error if the
    /// workspace takes no writes from them (a read-only demo, a role that can't
    /// write).
    fn check(
        &self,
        user: Uuid,
        workspace: Uuid,
        writes: bool,
    ) -> BoxFuture<'_, Result<WorkspaceEntry, AppError>>;

    /// For a request: checks the signed-in user as [`check`](Self::check)
    /// does and, if they may, enters `workspace` on the request's unit of
    /// work and treats the request as one inside it. An error if the request
    /// may not go on in it (a write to a read-only demo, a token limited to
    /// another workspace, a role that can't write).
    fn enter_request<'a>(
        &'a self,
        parts: &'a mut Parts,
        workspace: Uuid,
    ) -> BoxFuture<'a, Result<WorkspaceEntry, AppError>>;

    /// What `workspace` takes from `user`, member or not: nothing is refused of
    /// a workspace that doesn't exist, for [`check`](Self::check) to say.
    fn writes(
        &self,
        user: Uuid,
        workspace: Uuid,
    ) -> BoxFuture<'_, Result<WorkspaceWrites, AppError>>;

    /// Makes `workspace` the one `uow` works in, or none, with no check: what
    /// the isolation strategy needs of its transaction (`navree.workspace`,
    /// the `search_path`).
    fn switch<'a>(
        &'a self,
        uow: &'a Uow,
        workspace: Option<Uuid>,
    ) -> BoxFuture<'a, Result<(), AppError>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn writes(read_only_role: bool, permissions: &[&str]) -> WorkspaceWrites {
        WorkspaceWrites {
            read_only_demo: false,
            read_only_role,
            role_permissions: permissions.iter().map(|p| (*p).to_owned()).collect(),
        }
    }

    #[test]
    fn a_role_writes_by_its_default_where_its_permissions_say_nothing() {
        assert!(writes(false, &[]).role_writes("notes", "update"));
        assert!(!writes(true, &[]).role_writes("notes", "update"));
        assert!(writes(true, &[]).role_is_read_only());
        assert!(!writes(false, &[]).role_is_read_only());
        // Permissions on something else change nothing for notes.
        assert!(writes(false, &["deals.read"]).role_writes("notes", "update"));
        assert!(!writes(true, &["deals.update"]).role_writes("notes", "update"));
    }

    #[test]
    fn the_permissions_of_a_role_decide_for_the_models_they_name() {
        let viewer = writes(true, &["notes.update"]);
        assert!(viewer.role_writes("notes", "update"));
        assert!(!viewer.role_writes("notes", "delete"));
        assert!(!viewer.role_is_read_only());
        let member = writes(false, &["notes.read"]);
        assert!(!member.role_writes("notes", "update"));
        assert!(member.role_writes("deals", "update"));
        assert!(writes(true, &["notes.*"]).role_writes("notes", "delete"));
        assert!(writes(true, &["notes.manage"]).role_writes("notes", "delete"));
        assert!(writes(true, &["all.*"]).role_writes("notes", "delete"));
        // A dotted prefix is one prefix.
        assert!(writes(true, &["crm.notes.update"]).role_writes("crm.notes", "update"));
        assert!(!writes(true, &["crm.notes.update"]).role_writes("notes", "update"));
        // Reading is no reason to let a read-only role in to write.
        assert!(writes(true, &["notes.read", "workspace.read"]).role_is_read_only());
    }

    #[test]
    fn a_refusal_says_which() {
        assert!(writes(false, &[]).refusal().is_none());
        assert_eq!(
            writes(true, &[]).refusal().unwrap().code,
            "workspace_read_only"
        );
        let demo = WorkspaceWrites {
            read_only_demo: true,
            ..WorkspaceWrites::default()
        };
        assert_eq!(demo.refusal().unwrap().code, "demo_read_only");
    }
}
