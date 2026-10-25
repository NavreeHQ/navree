//! Authentication and authorization seams: [`Authenticator`],
//! [`RouteAuthorizer`], [`Guard`] and the [`CurrentPrincipal`] extractor.

use std::fmt;

use axum::extract::FromRequestParts;
use http::request::Parts;
use keel_contracts::{AuthState, Principal};
use keel_kernel::BoxFuture;

use crate::{AppError, RouteMeta};

/// Establishes who is making a request (implemented by keel-auth; fakes in
/// tests). Registered as `dyn Authenticator` and resolved with
/// `AppContext::try_get`.
///
/// The pipeline calls it once per request, in the `AuthContext` layer slot,
/// and stores the result as [`AuthState`] in the request extensions.
/// `Ok(None)` means a guest; an error is returned to the client as is.
pub trait Authenticator: Send + Sync + 'static {
    /// The principal of the request, if any.
    fn authenticate<'a>(
        &'a self,
        parts: &'a mut Parts,
    ) -> BoxFuture<'a, Result<Option<Principal>, AppError>>;
}

/// Checks a route's roles, permissions and abilities (implemented by
/// keel-access). Registered as `dyn RouteAuthorizer` and resolved with
/// `AppContext::try_get`.
///
/// Called after authentication, throttling and guards, only for routes that
/// declare roles, permissions or `can`. It may store data for extractors in
/// `parts.extensions` (a record loaded for `can(.., from = "id")`).
pub trait RouteAuthorizer: Send + Sync + 'static {
    /// `Ok(())` to let the request through; usually 403 or 404 otherwise.
    fn authorize<'a>(
        &'a self,
        parts: &'a mut Parts,
        meta: &'a RouteMeta,
    ) -> BoxFuture<'a, Result<(), AppError>>;
}

/// A custom per-route check, run after authentication and throttling and
/// before authorization and the handler. A guard alone is not an access
/// declaration.
pub trait Guard: Send + Sync + 'static + fmt::Debug {
    /// `Ok(())` to continue; the error is returned to the client.
    fn check<'a>(&'a self, parts: &'a mut Parts) -> BoxFuture<'a, Result<(), AppError>>;
}

/// The authenticated principal of the request; 401 for guests.
#[derive(Clone, Debug)]
pub struct CurrentPrincipal(pub Principal);

impl<S: Send + Sync> FromRequestParts<S> for CurrentPrincipal {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<AuthState>()
            .and_then(|state| state.0.clone())
            .map(CurrentPrincipal)
            .ok_or_else(AppError::unauthenticated)
    }
}
