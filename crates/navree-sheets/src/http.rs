//! What the export and import routes share: the request's workspace,
//! permission checks and route metadata.

use http::request::Parts;
use navree_contracts::{Principal, permitted};
use navree_http::extract::FromRequestParts;
use navree_http::{ApiMeta, AppError, RouteDef, SchemaFn};
use navree_kernel::AppContext;
use navree_workspace::{CurrentWorkspace, InWorkspace};
use serde::Serialize;
use uuid::Uuid;

/// The workspace a request acts in: the request's workspace in an app with
/// `WorkspaceModule` (resolved by the `InWorkspace` guard), else `None`.
pub(crate) struct Scope(pub(crate) Option<Uuid>);

impl FromRequestParts for Scope {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        Ok(Scope(
            parts
                .extensions
                .get::<CurrentWorkspace>()
                .map(CurrentWorkspace::id),
        ))
    }
}

/// A list response: `{"data": [...]}`.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct DataList<T> {
    pub(crate) data: Vec<T>,
}

/// 403 unless `principal` holds `permission` (app-wide or in `workspace`);
/// `None` lets everyone through.
pub(crate) async fn require(
    app: &AppContext,
    principal: &Principal,
    permission: Option<&str>,
    workspace: Option<Uuid>,
) -> Result<(), AppError> {
    let Some(permission) = permission else {
        return Ok(());
    };
    if permitted(app, principal, permission, workspace)
        .await
        .map_err(AppError::internal)?
    {
        Ok(())
    } else {
        Err(AppError::forbidden())
    }
}

/// Whether `principal` holds `permission` (`None`: yes).
pub(crate) async fn allowed(
    app: &AppContext,
    principal: &Principal,
    permission: Option<&str>,
    workspace: Option<Uuid>,
) -> Result<bool, AppError> {
    match permission {
        None => Ok(true),
        Some(permission) => permitted(app, principal, permission, workspace)
            .await
            .map_err(AppError::internal),
    }
}

/// Route metadata for the OpenAPI document.
pub(crate) fn api(
    tag: &str,
    summary: &str,
    responses: &[u16],
    request: Option<SchemaFn>,
    response: Option<SchemaFn>,
) -> ApiMeta {
    ApiMeta {
        summary: Some(summary.to_owned()),
        tags: vec![tag.to_owned()],
        responses: responses.to_vec(),
        request,
        response,
        ..ApiMeta::default()
    }
}

/// Signed-in users only, in the request's workspace when the app has
/// workspaces.
pub(crate) fn scoped(workspaces: bool) -> impl Fn(RouteDef) -> RouteDef + Clone {
    move |route: RouteDef| {
        let route = route.authenticated();
        if workspaces {
            route.guard(InWorkspace::guard())
        } else {
            route
        }
    }
}

/// The app behind a weak handle, or a 500 once it has shut down.
pub(crate) fn upgrade(app: &navree_kernel::WeakAppContext) -> Result<AppContext, AppError> {
    app.upgrade()
        .ok_or_else(|| AppError::internal("the app has shut down"))
}

#[cfg(test)]
mod tests {
    use navree_http::handler::Endpoint;

    use super::*;

    #[test]
    fn handlers_on_the_scope_are_native() {
        async fn handler(_: Scope) {}
        assert!(Endpoint::new(handler).is_native());
    }
}
