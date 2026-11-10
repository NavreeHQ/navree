//! [`CurrentDomain`] and the request layer that marks requests to a
//! verified custom domain.

use http::request::Parts;
use navree_http::extract::Request;
use navree_http::extract::{FromRequestParts, OptionalFromRequestParts};
use navree_http::middleware::Next;
use navree_http::{AppError, Response};
use navree_kernel::AppContext;
use navree_openapi::ToSchema;
use navree_workspace::DomainWorkspace;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::host::request_host;
use crate::{Domains, DomainsError};

/// The verified custom domain a request came in on.
///
/// As an extractor it answers 404 (`domain_not_found`) for any other host;
/// `Option<CurrentDomain>` gives `None` instead. It needs no signed-in
/// user, so public pages served on a workspace's domain (a landing page,
/// a booking form) know their workspace:
///
/// ```no_run
/// use navree_domains::CurrentDomain;
///
/// async fn landing(domain: CurrentDomain) -> String {
///     format!("Welcome to {} (workspace {})", domain.host, domain.workspace_id)
/// }
/// ```
///
/// Signed-in requests use `CurrentWorkspace` with the `domain` resolver
/// (`workspace.resolvers = ["domain", ...]`), which also checks the
/// membership.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CurrentDomain {
    /// The domain's id.
    pub id: Uuid,
    /// The workspace it belongs to.
    pub workspace_id: Uuid,
    /// The host, lower-case.
    pub host: String,
    /// What the workspace uses it for.
    pub purpose: String,
}

async fn domains_of(parts: &Parts) -> Result<std::sync::Arc<Domains>, AppError> {
    let ctx = parts
        .extensions
        .get::<AppContext>()
        .ok_or_else(|| AppError::internal("CurrentDomain needs the navree request pipeline"))?;
    ctx.get::<Domains>()
        .await
        .map_err(|_| AppError::internal("CurrentDomain needs DomainsModule in the app"))
}

/// The request's verified custom domain, resolved once.
async fn resolve(parts: &mut Parts) -> Result<Option<CurrentDomain>, AppError> {
    if let Some(found) = parts.extensions.get::<Option<CurrentDomain>>() {
        return Ok(found.clone());
    }
    let domains = domains_of(parts).await?;
    let found = match request_host(parts) {
        Some(host) => domains
            .resolve(&host)
            .await
            .map_err(DomainsError::into_app_error)?,
        None => None,
    };
    parts.extensions.insert(found.clone());
    Ok(found)
}

impl FromRequestParts for CurrentDomain {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        resolve(parts)
            .await?
            .ok_or_else(|| DomainsError::DomainNotFound.into_app_error())
    }
}

impl OptionalFromRequestParts for CurrentDomain {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts) -> Result<Option<Self>, Self::Rejection> {
        resolve(parts).await
    }
}

/// The request layer (`domains-host`): with `domain` in
/// `workspace.resolvers`, a request to a verified custom domain carries a
/// [`DomainWorkspace`] for the `domain` resolver, and its
/// [`CurrentDomain`]. Other requests pass untouched.
pub(crate) async fn mark_domain(request: Request, next: Next) -> Response {
    let (mut parts, body) = request.into_parts();
    let domains = match parts.extensions.get::<AppContext>() {
        Some(ctx) => ctx.try_get::<Domains>().await,
        None => None,
    };
    if let Some(domains) = domains
        && domains.resolves_workspaces()
        && let Some(host) = request_host(&parts)
    {
        match domains.resolve(&host).await {
            Ok(found) => {
                if let Some(domain) = &found {
                    parts.extensions.insert(DomainWorkspace {
                        workspace_id: domain.workspace_id,
                        host: domain.host.clone(),
                    });
                }
                parts.extensions.insert(found);
            }
            Err(error) => {
                tracing::warn!(target: "navree::domains", host, %error, "could not look up a custom domain");
            }
        }
    }
    next.run(Request::from_parts(parts, body)).await
}

#[cfg(test)]
mod tests {
    use navree_http::handler::Endpoint;

    use super::*;

    #[test]
    fn handlers_on_the_current_domain_are_native() {
        async fn handler(_: CurrentDomain, _: Option<CurrentDomain>) {}
        let _ = Endpoint::new(handler);
    }
}
