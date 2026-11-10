//! [`Scoped`]: providers resolved for the request (dependency injection).

use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

use http::request::Parts;
use navree_kernel::{AppContext, RequestScope};

use super::FromRequestParts;
use crate::{AppError, RouteMeta};

/// A provider resolved for the current request, with the visibility of the
/// module that registered the route (request-scoped injection).
///
/// Request-scoped providers (`Scope::Request`,
/// `#[injectable(scope = "request")]`) are built on first use within a
/// request and shared by every `Scoped<T>` of that request: the pipeline
/// keeps one `RequestScope` per request in its extensions. Singletons and
/// transient providers resolve as usual. A provider the route's module
/// cannot see, or one that fails to build, is a 500.
///
/// ```ignore
/// async fn show(cart: Scoped<Cart>) -> Json<CartView> { ... }
/// ```
pub struct Scoped<T: ?Sized>(pub Arc<T>);

impl<T: ?Sized> Deref for Scoped<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T: ?Sized> Clone for Scoped<T> {
    fn clone(&self) -> Self {
        Scoped(self.0.clone())
    }
}

impl<T: ?Sized> fmt::Debug for Scoped<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Scoped<{}>", std::any::type_name::<T>())
    }
}

impl<T> FromRequestParts for Scoped<T>
where
    T: ?Sized + Send + Sync + 'static,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        let unavailable = || {
            AppError::internal(format!(
                "Scoped<{}> is only available in a route mounted by navree-http",
                std::any::type_name::<T>()
            ))
        };
        let ctx = parts
            .extensions
            .get::<AppContext>()
            .cloned()
            .ok_or_else(unavailable)?;
        let module = parts
            .extensions
            .get::<Arc<RouteMeta>>()
            .and_then(|meta| meta.module_id)
            .ok_or_else(unavailable)?;
        let scope = match parts.extensions.get::<RequestScope>() {
            Some(scope) => scope.clone(),
            None => {
                let scope = RequestScope::new();
                parts.extensions.insert(scope.clone());
                scope
            }
        };
        ctx.request_resolver(module, scope)
            .get::<T>()
            .await
            .map(Scoped)
            .map_err(AppError::internal)
    }
}
