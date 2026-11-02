//! [`WorkspaceGate`]: who may work in a workspace, for modules that work in
//! workspaces without being the workspace module.

use http::request::Parts;
use navree_db::Uow;
use navree_http::AppError;
use navree_kernel::BoxFuture;
use uuid::Uuid;

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
    /// workspace takes no writes from them (a read-only demo).
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
    /// another workspace).
    fn enter_request<'a>(
        &'a self,
        parts: &'a mut Parts,
        workspace: Uuid,
    ) -> BoxFuture<'a, Result<WorkspaceEntry, AppError>>;

    /// Makes `workspace` the one `uow` works in, or none, with no check: what
    /// the isolation strategy needs of its transaction (`navree.workspace`,
    /// the `search_path`).
    fn switch<'a>(
        &'a self,
        uow: &'a Uow,
        workspace: Option<Uuid>,
    ) -> BoxFuture<'a, Result<(), AppError>>;
}
