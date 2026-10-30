//! # navree-http
//!
//! `HttpModule`: Navree's router and extractors, the default Tower layer
//! stack, JSON errors, controllers and route metadata.
//!
//! - Modules contribute routes with [`Controller`]s
//!   ([`ModuleBuilderHttpExt::controller`]); each [`RouteDef`] carries a
//!   [`RouteMeta`] with its access declaration, guards, throttle, timeout,
//!   body limit, audit action and OpenAPI metadata. **Deny by default:** a
//!   route without [`public`](RouteDef::public),
//!   [`authenticated`](RouteDef::authenticated), [`roles`](RouteDef::roles),
//!   [`permissions`](RouteDef::permissions) or [`can`](RouteDef::can) fails
//!   the boot.
//! - Routes are matched by Navree's [`routing::Router`]: no path
//!   normalization (dot segments and backslashes are refused), strict
//!   percent-decoding of parameters, static before parameter before
//!   catch-all, `HEAD`, `OPTIONS` and 405 with `Allow`. Handlers are async
//!   functions whose arguments are [`extract`]ors ([`Path`], [`Query`],
//!   [`Json`], [`Form`], [`Extension`], [`Scoped`], ...).
//! - [`build_router`] assembles the app. Request pipeline, outermost first:
//!   [`LayerSlot::Edge`] (protocols that share the port but not the
//!   pipeline, such as gRPC) → request id → tracing and request log → CORS
//!   → panic catcher → timeout →
//!   security headers → compression (not for `text/event-stream`) →
//!   error rendering → maintenance mode → per-IP rate limit → body limit →
//!   [`LayerSlot::Session`] → [`LayerSlot::AuthContext`] (the registered
//!   [`Authenticator`] runs first) → [`LayerSlot::Uow`] →
//!   [`LayerSlot::Context`] → router → per-route:
//!   authentication → per-user throttle → [`Guard`]s → [`RouteAuthorizer`]
//!   → route timeout → handler → audit action.
//! - Every error is an [`AppError`] rendered in one JSON shape, as
//!   `application/json`: `{"statusCode": 404, "message": "Not Found",
//!   "error": "Not Found", "code": "not_found"}`; `code` is one of the
//!   stable [`codes`] (or an app's own) for front ends to translate, and
//!   validation errors are 400s whose `message` lists every field message
//!   and whose `errors` maps fields to their messages.
//!   This covers panics (`"Internal server error"`, nothing leaked),
//!   timeouts, 404s and 405s, and extractor rejections (a body of the wrong
//!   shape is a 400), and every response carries `X-Request-Id` and, for
//!   allowed origins, the CORS headers.
//!
//! ```
//! use std::sync::Arc;
//!
//! use http::Method;
//! use navree_http::{Controller, Json, ModuleBuilderHttpExt, Path, RouteDef};
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
//!             RouteDef::on(Method::GET, "/{name}", |Path(name): Path<String>| async move {
//!                 Json(serde_json::json!({ "hello": name }))
//!             })
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
pub mod body;
pub mod codes;
mod controller;
mod cors;
mod error;
pub mod extract;
pub mod handler;
mod limiter;
mod maintenance;
mod module;
mod network;
mod options;
mod pipeline;
pub mod response;
mod route;
mod router;
pub mod routing;
mod signed;

pub use auth::{Authenticator, CurrentPrincipal, Guard, RouteAuthorizer};
pub use controller::{ApplyLayer, Controller, GlobalLayer, LayerSlot, ModuleBuilderHttpExt};
pub use cors::{Cors, CorsLayer};
pub use error::{AppError, ERROR_CONTENT_TYPE, ErrorMapper, Result};
pub use extract::{ClientIp, Extension, Form, Json, Path, Query, RawQuery, Scoped};
pub use handler::Handler;
pub use limiter::{KeyedRateLimiter, Limited};
pub use maintenance::{MaintenanceInfo, MaintenanceMode};
pub use module::{HttpModule, HttpServer};
pub use network::{DEFAULT_IPV6_PREFIX, rate_limit_key};
pub use options::{CorsOptions, ErrorOptions, HttpOptions};
pub use response::{Created, IntoResponse, NoContent, Response};
pub use route::{AccessSpec, ApiMeta, CanSpec, RouteDef, RouteMeta, SchemaFn, Throttle, schema_fn};
pub use router::{RouteErrors, build_router, route_table};
pub use routing::PathParams;
pub use signed::{UrlSigner, ValidSignature};
