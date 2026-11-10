//! [`PermissionCheck`]: permission questions asked outside a route's access
//! declaration (a channel authorizer, a service deciding who may see a
//! record).

use navree_kernel::AppContext;
use uuid::Uuid;

use crate::{BoxError, BoxFuture, Principal};

/// Answers "does this principal hold this permission?", app-wide or in one
/// workspace. Registered as multi-bindings of `dyn PermissionCheck`:
/// navree-access answers for the user's app-wide roles and permissions,
/// navree-workspace for their role in a workspace. Ask them all with
/// [`permitted`].
///
/// A check returns `Some(true)` or `Some(false)` when the question is its to
/// answer, and `None` when it is not (navree-workspace's check, asked about
/// no workspace).
pub trait PermissionCheck: Send + Sync + 'static {
    /// Whether `principal` holds `permission` (`deals.read`,
    /// `operations.view`), in `workspace` when one is given.
    fn check<'a>(
        &'a self,
        principal: &'a Principal,
        permission: &'a str,
        workspace: Option<Uuid>,
    ) -> BoxFuture<'a, Result<Option<bool>, BoxError>>;
}

/// Whether any registered [`PermissionCheck`] grants `principal` the
/// `permission`, app-wide or in `workspace`. False when no check is
/// registered or none grants it; a check that fails is an error, not a
/// denial.
///
/// ```no_run
/// # async fn example(ctx: &navree_kernel::AppContext, principal: &navree_contracts::Principal, workspace: uuid::Uuid) -> Result<(), navree_contracts::BoxError> {
/// if navree_contracts::permitted(ctx, principal, "operations.view", Some(workspace)).await? {
///     // show the operation
/// }
/// # Ok(())
/// # }
/// ```
pub async fn permitted(
    ctx: &AppContext,
    principal: &Principal,
    permission: &str,
    workspace: Option<Uuid>,
) -> Result<bool, BoxError> {
    for check in ctx.get_all::<dyn PermissionCheck>().await? {
        if check.check(principal, permission, workspace).await? == Some(true) {
            return Ok(true);
        }
    }
    Ok(false)
}
