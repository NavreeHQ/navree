//! # keel-session
//!
//! `SessionModule`: encrypted session cookies, memory and database stores,
//! and CSRF protection, compatible with Laravel Sanctum's SPA protocol.
//!
//! - [`SessionModule`] is global. Configure it with
//!   [`SessionModule::for_root`] or [`SessionModule::from_config`] (section
//!   `session`, [`SessionOptions`]); it needs `CryptoModule` (the cookie is
//!   encrypted with the app key) and, for the database store,
//!   `DatabaseModule`.
//! - Its layer runs in [`LayerSlot::Session`](keel_http::LayerSlot::Session),
//!   before authentication: it decrypts the session cookie, loads the
//!   record, puts the [`Session`] into the request (an extractor, and what
//!   an `Authenticator` reads), enforces CSRF, and after the response saves
//!   the session when it changed and sets the cookies (`HttpOnly`,
//!   `SameSite` and `Secure` per the options). Idle sessions expire after
//!   `lifetime_minutes`; activity extends them.
//! - Stores implement `keel_contracts::SessionStore`: [`MemoryStore`] and
//!   [`DatabaseStore`] (table [`SESSIONS_TABLE`], created by the `session`
//!   migration set the module contributes). Override the store with
//!   `AppBuilder::override_value::<dyn SessionStore>(..)`.
//! - `GET /sanctum/csrf-cookie` (public) answers 204 and sets the
//!   [`XSRF_COOKIE`] cookie (readable by JavaScript).
//!
//! **CSRF rule.** A request with a state-changing method (anything but
//! `GET`, `HEAD`, `OPTIONS` and `TRACE`) that carries the session cookie and
//! no `Authorization: Bearer` token must send the session's CSRF token in
//! `X-XSRF-TOKEN` (the URL-decoded `XSRF-TOKEN` cookie, as Axios and
//! Laravel Echo send it) or `X-CSRF-TOKEN`; otherwise it fails with a 403
//! `"Invalid CSRF token"` error before the handler runs, is logged on
//! target `keel::security` and, when the app has a `dyn AuditSink`, written
//! to the audit trail (`csrf.mismatch`). Such a request *without* the
//! session cookie has no token to send and passes, unless the browser marks
//! it cross-site (`Sec-Fetch-Site: cross-site`) and its `Origin` is not in
//! `http.cors.allowed_origins` (a forged cross-site form, e.g. login CSRF).
//! Paths matching [`SessionOptions::csrf_except`] are exempt from both
//! checks. Tokens are compared in constant time and never logged.
//!
//! ```no_run
//! use std::sync::Arc;
//!
//! use axum::http::Method;
//! use keel_config::{ConfigModule, ConfigSources};
//! use keel_crypto::CryptoModule;
//! use keel_http::{Controller, HttpModule, Json, ModuleBuilderHttpExt, RouteDef};
//! use keel_kernel::{BoxFuture, Injectable, Keel, KernelError, Module, ModuleBuilder, Resolver};
//! use keel_session::{Session, SessionModule};
//!
//! struct CartController;
//!
//! impl Injectable for CartController {
//!     fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
//!         Box::pin(async { Ok(CartController) })
//!     }
//! }
//!
//! async fn add(session: Session, Json(item): Json<String>) -> keel_http::Result<Json<Vec<String>>> {
//!     let mut cart: Vec<String> = session.get("cart").unwrap_or_default();
//!     cart.push(item);
//!     session.insert("cart", &cart)?;
//!     Ok(Json(cart))
//! }
//!
//! impl Controller for CartController {
//!     fn prefix(&self) -> &'static str {
//!         "/cart"
//!     }
//!
//!     fn routes(self: Arc<Self>) -> Vec<RouteDef> {
//!         // POST /api/cart: needs X-XSRF-TOKEN once the browser has a session.
//!         vec![RouteDef::on(Method::POST, "/", add).public()]
//!     }
//! }
//!
//! struct ShopModule;
//!
//! impl Module for ShopModule {
//!     fn register(&self, m: &mut ModuleBuilder) {
//!         m.controller::<CartController>();
//!     }
//! }
//!
//! fn main() -> Result<(), keel_kernel::BoxError> {
//!     Keel::app()
//!         .module(ConfigModule::for_root(ConfigSources::defaults()))
//!         .module(CryptoModule::from_config())
//!         .module(HttpModule::from_config())
//!         .module(SessionModule::from_config())
//!         .module(ShopModule)
//!         .run()
//! }
//! ```
#![forbid(unsafe_code)]

mod controller;
mod database;
mod layer;
mod memory;
mod module;
mod options;
mod session;

pub use database::{DatabaseStore, SESSIONS_TABLE};
pub use memory::MemoryStore;
pub use module::SessionModule;
pub use options::{SameSite, SessionOptions, SessionStoreKind};
pub use session::{Session, SessionError};

/// Stores delete expired records on every this many saves.
const PRUNE_EVERY: u64 = 100;

/// The cookie holding the CSRF token for JavaScript clients (not
/// `HttpOnly`); send its URL-decoded value back as `X-XSRF-TOKEN`.
pub const XSRF_COOKIE: &str = "XSRF-TOKEN";
