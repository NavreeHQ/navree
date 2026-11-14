//! [`RequestWorkspace`]: the workspace a request is in, asked for without
//! entering it.

use http::request::Parts;
use navree_kernel::BoxFuture;
use uuid::Uuid;

/// The workspace a request is in, as the app's workspace module resolves it
/// (navree-workspace registers it as the `dyn RequestWorkspace` provider).
///
/// Resolves the id only. It does not enter the workspace on the request's unit of work and
/// does not apply its isolation, so a page whose shell asks for links runs its queries as it
/// did without them.
///
/// ```
/// use http::request::Parts;
/// use navree_http::RequestWorkspace;
/// use navree_kernel::AppContext;
/// use uuid::Uuid;
///
/// /// The workspace the request of `parts` names and its principal may
/// /// enter; `None` in an app without a workspace module.
/// async fn workspace_of(ctx: &AppContext, parts: &mut Parts) -> Option<Uuid> {
///     let resolver = ctx.try_get::<dyn RequestWorkspace>().await?;
///     resolver.resolve(parts).await
/// }
/// ```
pub trait RequestWorkspace: Send + Sync + 'static {
    /// `Some(id)` when the request names a workspace its principal may enter; never an error.
    fn resolve<'a>(&'a self, parts: &'a mut Parts) -> BoxFuture<'a, Option<Uuid>>;
}
