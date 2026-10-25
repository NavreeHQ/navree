//! # navree-http
//!
//! `HttpModule`: Axum integration, the default Tower layer stack, RFC 9457
//! problem+json errors, controllers and route metadata.
//!
//! - Modules contribute routes with [`Controller`]s
//!   ([`ModuleBuilderHttpExt::controller`]); each [`RouteDef`] carries a
//!   [`RouteMeta`] with its access declaration, guards, throttle, timeout,
//!   body limit, audit action and OpenAPI metadata. **Deny by default:** a
//!   route without [`public`](RouteDef::public),
//!   [`authenticated`](RouteDef::authenticated), [`roles`](RouteDef::roles),
//!   [`permissions`](RouteDef::permissions) or [`can`](RouteDef::can) fails
//!   the boot.
//! - [`build_router`] assembles the app. Request pipeline, outermost first:
//!   request id → tracing and request log → CORS → panic catcher → timeout →
//!   security headers → compression (not for `text/event-stream`) →
//!   problem rendering → maintenance mode → per-IP rate limit → body limit →
//!   [`LayerSlot::Session`] → [`LayerSlot::AuthContext`] (the registered
//!   [`Authenticator`] runs first) → [`LayerSlot::Uow`] → router → per-route:
//!   authentication → per-user throttle → [`Guard`]s → [`RouteAuthorizer`]
//!   → route timeout → handler → audit action.
//! - Every error is an [`AppError`] rendered as problem+json
//!   (`{"type","title","status","detail"?,"request_id","errors"?}`), including
//!   panics, timeouts, 404s and framework rejections, and every response
//!   carries `X-Request-Id` and, for allowed origins, the CORS headers.
//!
//! ```
//! use std::sync::Arc;
//!
//! use axum::extract::Path;
//! use axum::routing::get;
//! use http::Method;
//! use navree_http::{Controller, Json, ModuleBuilderHttpExt, RouteDef};
//! use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
//!
//! struct HelloController;
//!
//! impl Injectable for HelloController {
//!     fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
//!         Box::pin(async { Ok(HelloController) })
//!     }
//! }
//!
//! impl Controller for HelloController {
//!     fn prefix(&self) -> &'static str {
//!         "/hello"
//!     }
//!
//!     fn version(&self) -> Option<&'static str> {
//!         Some("1")
//!     }
//!
//!     fn routes(self: Arc<Self>) -> Vec<RouteDef> {
//!         vec![
//!             // GET /api/v1/hello/{name}
//!             RouteDef::new(
//!                 Method::GET,
//!                 "/{name}",
//!                 get(|Path(name): Path<String>| async move {
//!                     Json(serde_json::json!({ "hello": name }))
//!                 }),
//!             )
//!             .public(),
//!         ]
//!     }
//! }
//!
//! struct HelloModule;
//!
//! impl Module for HelloModule {
//!     fn register(&self, m: &mut ModuleBuilder) {
//!         m.controller::<HelloController>();
//!     }
//! }
//! ```
#![forbid(unsafe_code)]

mod auth;
mod controller;
mod error;
mod extract;
mod limiter;
mod maintenance;
mod module;
mod options;
mod pipeline;
mod route;
mod router;
mod signed;

pub use auth::{Authenticator, CurrentPrincipal, Guard, RouteAuthorizer};
pub use axum::Json;
pub use controller::{ApplyLayer, Controller, GlobalLayer, LayerSlot, ModuleBuilderHttpExt};
pub use error::{AppError, ErrorMapper, PROBLEM_JSON, Result};
pub use extract::{ClientIp, Created, NoContent};
pub use maintenance::{MaintenanceInfo, MaintenanceMode};
pub use module::{HttpModule, HttpServer};
pub use options::{CorsOptions, HttpOptions};
pub use route::{AccessSpec, ApiMeta, CanSpec, RouteDef, RouteMeta, SchemaFn, Throttle, schema_fn};
pub use router::{RouteErrors, build_router, route_table};
pub use signed::{UrlSigner, ValidSignature};
