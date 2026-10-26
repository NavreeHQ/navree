//! # navree-testing
//!
//! The testing kit for Navree apps, in the spirit of Laravel's HTTP tests:
//!
//! - [`TestApp`] boots an app for one test. [`TestApp::builder`] assembles
//!   it from modules, with hermetic configuration
//!   ([`TestAppBuilder::config`]), a private SQLite database
//!   ([`TestAppBuilder::temp_database`]) and provider overrides;
//!   [`TestApp::from_builder`] boots an app root built elsewhere (for
//!   example `crm_api::app(sources)`).
//! - [`TestClient`] sends requests in-process through the app's full router
//!   (request id, CORS, session, authentication, unit of work, ...), with a
//!   cookie jar, `Accept: application/json`, the app's first CORS origin as
//!   `Origin`, bearer tokens and the SPA CSRF handshake
//!   ([`TestClient::csrf_cookie`], [`TestRequest::xsrf`]).
//! - [`TestResponse`] holds the collected response; its assertions show the
//!   status and body when they fail.
//! - [`TestAuthenticator`] and [`TestApp::acting_as`] authenticate requests
//!   as any `Principal` ([`principal`] builds a plain test user).
//! - [`temp_sqlite_url`], [`TempDirGuard`] and [`FixedClock`] provide
//!   databases and controllable time.
//!
//! ```
//! use std::sync::Arc;
//!
//! use http::Method;
//! use navree_http::{Controller, CurrentPrincipal, Json, ModuleBuilderHttpExt, RouteDef};
//! use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
//! use navree_testing::{TestApp, principal};
//! use serde_json::json;
//!
//! struct MeController;
//!
//! impl Injectable for MeController {
//!     fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
//!         Box::pin(async { Ok(MeController) })
//!     }
//! }
//!
//! impl Controller for MeController {
//!     fn prefix(&self) -> &'static str {
//!         "/me"
//!     }
//!
//!     fn routes(self: Arc<Self>) -> Vec<RouteDef> {
//!         vec![
//!             RouteDef::on(Method::GET, "/", |CurrentPrincipal(me): CurrentPrincipal| async move {
//!                 Json(json!({ "id": me.user_id }))
//!             })
//!             .authenticated(),
//!         ]
//!     }
//! }
//!
//! struct MeModule;
//!
//! impl Module for MeModule {
//!     fn register(&self, m: &mut ModuleBuilder) {
//!         m.controller::<MeController>();
//!     }
//! }
//!
//! # #[tokio::main(flavor = "current_thread")]
//! # async fn main() {
//! let app = TestApp::builder().module(MeModule).build().await;
//! let alice = uuid::Uuid::now_v7();
//!
//! app.acting_as(principal(alice))
//!     .get("/api/me")
//!     .send()
//!     .await
//!     .assert_status(200)
//!     .assert_json_path("id", json!(alice));
//!
//! app.client()
//!     .get("/api/me")
//!     .send()
//!     .await
//!     .assert_error(401, "Unauthorized");
//! # }
//! ```
#![forbid(unsafe_code)]

mod app;
mod auth;
mod client;
mod cookies;
mod response;
mod temp;

pub use app::{TestApp, TestAppBuilder};
pub use auth::{TestAuthenticator, principal};
pub use client::{TestClient, TestRequest};
pub use response::TestResponse;
pub use temp::{TempDirGuard, temp_sqlite_url};

/// A clock that stands still until moved; register it with
/// `override_value::<dyn Clock>(Arc::new(FixedClock::new(now)))`.
pub use navree_contracts::FixedClock;

/// The `cookie` crate, for [`TestResponse::set_cookie`].
pub use cookie;
