//! [`AccessDeclarations`]: how a crate whose handlers declare who may call
//! them (a route, a mount, a message pattern, a gRPC method, an MCP item, a
//! GraphQL field) shows those declarations to a boot check, whichever of
//! those crates' modules the app has.

use std::fmt;
use std::sync::Arc;

use navree_kernel::{AppContext, BoxError, BoxFuture, ModuleId};

use crate::AccessSpec;
use crate::mount::collect_mounts;
use crate::router::route_table;

/// What one handler declares about who may call it, as the crate that serves
/// the handler reads it at boot.
#[derive(Clone, Debug)]
pub struct DeclaredAccess {
    /// What declares it, as a boot text names it: "the route", "the mount" or
    /// "the message pattern", then its name between backticks.
    pub what: String,
    /// The module that registered the handler, when it is known.
    pub module: Option<ModuleId>,
    /// That module's name.
    pub module_name: &'static str,
    /// The declaration: the roles, the permissions and the ability it
    /// requires.
    pub access: AccessSpec,
}

/// The function behind an [`AccessDeclarations`].
type ReadDeclared = Arc<
    dyn Fn(AppContext) -> BoxFuture<'static, Result<Vec<DeclaredAccess>, BoxError>> + Send + Sync,
>;

/// Reads the access declaration of every handler of one kind: a contribution
/// of the module that serves that kind (`HttpModule` for routes and mounts),
/// so an app reads the kinds it has, with or without `HttpModule`.
/// [`declared_access`] calls every one the app's modules contribute.
#[derive(Clone)]
pub struct AccessDeclarations(ReadDeclared);

impl AccessDeclarations {
    /// The declarations that `read` finds in the app it is given. It runs at
    /// boot, once every singleton is built, and may run more than once.
    pub fn new<F, Fut>(read: F) -> Self
    where
        F: Fn(AppContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Vec<DeclaredAccess>, BoxError>> + Send + 'static,
    {
        Self(Arc::new(move |ctx| Box::pin(read(ctx))))
    }
}

impl fmt::Debug for AccessDeclarations {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AccessDeclarations").finish_non_exhaustive()
    }
}

/// The access declaration of every handler of the app, of every kind whose
/// module contributes an [`AccessDeclarations`], modules in dependency order.
/// navree-access reads it at boot, to hold what the app's handlers require
/// against the namespaces of the app's plugins.
pub async fn declared_access(ctx: &AppContext) -> Result<Vec<DeclaredAccess>, BoxError> {
    let mut declared = Vec::new();
    for source in ctx.contributions::<AccessDeclarations>() {
        declared.extend((source.value.0)(ctx.clone()).await?);
    }
    Ok(declared)
}

/// What the app's routes and mounts declare: what `HttpModule` contributes
/// as its [`AccessDeclarations`].
pub(crate) async fn routes_and_mounts(ctx: AppContext) -> Result<Vec<DeclaredAccess>, BoxError> {
    let mut declared = Vec::new();
    for route in route_table(&ctx).await? {
        declared.push(DeclaredAccess {
            what: format!("the route `{} {}`", route.method, route.full_path),
            module: route.module_id,
            module_name: route.module.unwrap_or("?"),
            access: route.access,
        });
    }
    for mount in collect_mounts(&ctx) {
        declared.push(DeclaredAccess {
            what: format!("the mount `{}`", mount.prefix),
            module: mount.meta.module_id,
            module_name: mount.module,
            access: mount.meta.access,
        });
    }
    Ok(declared)
}
