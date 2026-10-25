//! [`Controller`], [`GlobalLayer`] and [`ModuleBuilderHttpExt`]: how modules
//! contribute routes, global middleware and error mappings.

use std::error::Error as StdError;
use std::fmt;
use std::sync::Arc;

use axum::Router;
use keel_kernel::{
    AppContext, BoxError, BoxFuture, Injectable, KernelError, ModuleBuilder, ModuleId, Resolver,
};

use crate::{AppError, ErrorMapper, RouteDef};

/// A group of routes sharing a prefix and an API version (what
/// `#[controller("/deals", version = "1")]` implements). Controllers are
/// singletons registered with [`ModuleBuilderHttpExt::controller`]; handlers
/// capture `Arc<Self>`.
///
/// Routes mount at `{api_prefix}/v{version}{prefix}{path}`; a prefix or a
/// route path starting with `!` is absolute (no API prefix, no version).
pub trait Controller: Send + Sync + 'static {
    /// The path prefix, e.g. `/deals`.
    fn prefix(&self) -> &'static str;

    /// The API version (`"1"` mounts under `/v1`).
    fn version(&self) -> Option<&'static str> {
        None
    }

    /// The routes, with paths relative to the prefix.
    fn routes(self: Arc<Self>) -> Vec<RouteDef>;
}

/// The routes of one controller, resolved from its module.
pub(crate) struct ControllerRoutes {
    pub(crate) type_name: &'static str,
    pub(crate) prefix: &'static str,
    pub(crate) version: Option<&'static str>,
    pub(crate) routes: Vec<RouteDef>,
}

type ResolveRoutes = Arc<
    dyn Fn(Resolver) -> BoxFuture<'static, Result<ControllerRoutes, KernelError>> + Send + Sync,
>;

/// A controller registered by a module (a contribution).
#[derive(Clone)]
pub(crate) struct ControllerRegistration {
    pub(crate) module: ModuleId,
    pub(crate) resolve: ResolveRoutes,
}

impl ControllerRegistration {
    fn of<C: Controller + Injectable>(module: ModuleId) -> Self {
        Self {
            module,
            resolve: Arc::new(|r: Resolver| {
                Box::pin(async move {
                    let controller = r.get::<C>().await?;
                    Ok(ControllerRoutes {
                        type_name: std::any::type_name::<C>(),
                        prefix: controller.prefix(),
                        version: controller.version(),
                        routes: controller.routes(),
                    })
                })
            }),
        }
    }
}

/// Where a [`GlobalLayer`] sits in the request pipeline, outermost first:
/// the session layer, then authentication context, then the unit of work.
/// All slots run after the fixed layers (request id, tracing, CORS, panic
/// catcher, timeout, security headers, compression, maintenance mode, per-IP
/// rate limit, body limit) and before routing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LayerSlot {
    /// Session and CSRF (keel-session).
    Session,
    /// Authentication: keel-http runs the `Authenticator` first in this slot.
    AuthContext,
    /// The unit of work (keel-db).
    Uow,
}

/// The function that wraps the app router in a [`GlobalLayer`].
pub type ApplyLayer = Arc<dyn Fn(&AppContext, Router) -> Result<Router, BoxError> + Send + Sync>;

/// App-wide middleware contributed by a module with
/// [`ModuleBuilderHttpExt::http_layer`]. `apply` wraps the router
/// (`router.layer(...)`); layers in the same slot run in module dependency
/// order (the first registered is outermost). `apply` may run more than once
/// per app, so it should not have side effects, and it must only add
/// layers: the router already has keel's single 404 fallback.
///
/// Inside the pipeline the request extensions already hold the
/// `AppContext`, the `keel_contracts::RequestId` and the [`ClientIp`](crate::ClientIp);
/// from the `AuthContext` slot inward they also hold the
/// `keel_contracts::AuthState`. Error responses should be [`AppError`]s, so
/// they render as problem+json with the request id.
///
/// ```
/// use axum::extract::Request;
/// use axum::middleware::{self, Next};
/// use axum::response::{IntoResponse, Response};
/// use keel_http::{AppError, GlobalLayer, LayerSlot, ModuleBuilderHttpExt};
/// use keel_kernel::{AppContext, Module, ModuleBuilder};
///
/// async fn unit_of_work(ctx: AppContext, request: Request, next: Next) -> Response {
///     if request.extensions().get::<AppContext>().is_none() {
///         return AppError::internal("outside the keel pipeline").into_response();
///     }
///     // open a transaction with `ctx`, run the request, commit or roll back
///     let _ = ctx;
///     next.run(request).await
/// }
///
/// struct DatabaseModule;
///
/// impl Module for DatabaseModule {
///     fn register(&self, m: &mut ModuleBuilder) {
///         m.http_layer(GlobalLayer::new(LayerSlot::Uow, "uow", |ctx, router| {
///             let ctx = ctx.clone();
///             Ok(router.layer(middleware::from_fn(move |request: Request, next: Next| {
///                 unit_of_work(ctx.clone(), request, next)
///             })))
///         }));
///     }
/// }
/// ```
#[derive(Clone)]
pub struct GlobalLayer {
    /// The pipeline slot.
    pub slot: LayerSlot,
    /// A name for logs and `routes` output.
    pub name: &'static str,
    /// Wraps the router.
    pub apply: ApplyLayer,
}

impl GlobalLayer {
    /// A layer in `slot` applied by `apply`.
    pub fn new<F>(slot: LayerSlot, name: &'static str, apply: F) -> Self
    where
        F: Fn(&AppContext, Router) -> Result<Router, BoxError> + Send + Sync + 'static,
    {
        Self {
            slot,
            name,
            apply: Arc::new(apply),
        }
    }
}

impl fmt::Debug for GlobalLayer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GlobalLayer")
            .field("slot", &self.slot)
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// HTTP registrations on a [`ModuleBuilder`].
pub trait ModuleBuilderHttpExt {
    /// Registers controller `C` as a singleton provider of this module and
    /// mounts its routes.
    fn controller<C: Controller + Injectable>(&mut self) -> &mut Self;

    /// Adds app-wide middleware in a [`LayerSlot`].
    fn http_layer(&mut self, layer: GlobalLayer) -> &mut Self;

    /// Maps errors of type `E` found in a 500 [`AppError`]'s source chain to
    /// a specific error (e.g. a database "record not found" to 404).
    fn map_error<E: StdError + 'static>(&mut self, f: fn(&E) -> Option<AppError>) -> &mut Self;
}

impl ModuleBuilderHttpExt for ModuleBuilder {
    fn controller<C: Controller + Injectable>(&mut self) -> &mut Self {
        let registration = ControllerRegistration::of::<C>(self.module_id());
        self.provider::<C>().contribute(registration)
    }

    fn http_layer(&mut self, layer: GlobalLayer) -> &mut Self {
        self.contribute(layer)
    }

    fn map_error<E: StdError + 'static>(&mut self, f: fn(&E) -> Option<AppError>) -> &mut Self {
        self.contribute(ErrorMapper::new(f))
    }
}
