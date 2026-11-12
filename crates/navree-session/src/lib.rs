//! # navree-session
//!
//! `SessionModule`: encrypted session cookies, memory and database stores (a
//! Redis one with navree-redis), and CSRF protection for single-page apps (a CSRF cookie echoed in a header).
//!
//! - [`SessionModule`] is global. Configure it with
//!   [`SessionModule::for_root`] or [`SessionModule::from_config`] (section
//!   `session`, [`SessionOptions`]); it needs `CryptoModule` (the cookie is
//!   encrypted with the app key) and, for the database store,
//!   `DatabaseModule`.
//! - Its layer runs in [`LayerSlot::Session`](navree_http::LayerSlot::Session),
//!   before authentication: it decrypts the session cookie, loads the
//!   record, puts the [`Session`] into the request (an extractor, and what
//!   an `Authenticator` reads), enforces CSRF, and after the response saves
//!   the session when it changed and sets the cookies (`HttpOnly`,
//!   `SameSite` and `Secure` per the options). The session cookie is named
//!   `__Host-navree_session` whenever browsers allow the prefix
//!   ([`SessionOptions::cookie_name`]). Idle sessions expire after
//!   `lifetime_minutes`, or the lifetime a session set itself
//!   ([`Session::set_lifetime`], "remember me"); activity extends them.
//! - navree-auth signs browsers in on this session (its `session` feature):
//!   the sign-in regenerates the session, and its strategy reads the user
//!   back on every request.
//! - Stores implement `navree_contracts::SessionStore`: [`MemoryStore`] and
//!   [`DatabaseStore`] (table [`SESSIONS_TABLE`], created by the `session`
//!   migration set the module contributes); `store = "redis"` uses the
//!   `dyn SessionStore` provider named `redis` (navree-redis's
//!   `RedisModule`). Override the store with
//!   `AppBuilder::override_value::<dyn SessionStore>(..)`.
//! - `GET /csrf-cookie` (public; [`CSRF_COOKIE_PATH`]) answers 204 and sets the
//!   [`XSRF_COOKIE`] cookie (readable by JavaScript).
//!
//! **CSRF rule.** A request with a state-changing method (anything but
//! `GET`, `HEAD`, `OPTIONS` and `TRACE`) that carries the session cookie and
//! no `Authorization: Bearer` token must send the session's CSRF token in
//! `X-XSRF-TOKEN` (the URL-decoded `XSRF-TOKEN` cookie, as Axios sends
//! it) or `X-CSRF-TOKEN`; otherwise it fails with a 403
//! `"Invalid CSRF token"` error before the handler runs, is logged on
//! target `navree::security` and, when the app has a `dyn AuditSink`, written
//! to the audit trail (`csrf.mismatch`). Such a request *without* the
//! session cookie has no token to send and passes, unless the browser marks
//! it cross-site (`Sec-Fetch-Site: cross-site`) and its `Origin` is not in
//! `http.cors.allowed_origins` (a forged cross-site form, e.g. login CSRF).
//! A plain HTML form has no script to set a header, so a request whose body is
//! `application/x-www-form-urlencoded` and sends neither header may carry the
//! token once, in the field [`CSRF_FIELD`] (`_csrf`) of its body. The field is
//! taken out before the handler reads the body; two fields of that name, one
//! in the URL, a body past 256 KiB and any other body type do not count; and a
//! header that is wrong is not rescued by the field. A `multipart/form-data`
//! body (a form with a file field) may carry it too, as its first part: the
//! field `_csrf`, which navree-ui's forms send first. Only that part is read,
//! and it must end within the body's first 8 KiB; the body then reaches the
//! handler as it came, the field included, so nothing of a file is read or
//! held to find the token. A first part that is another field or a file,
//! anything before the first delimiter, and a part's head that could be read
//! two ways do not count. Paths matching
//! [`SessionOptions::csrf_except`] are exempt from both
//! checks. A path a module names with
//! [`SameOriginOnly`](navree_http::csrf::SameOriginOnly) (navree-ui's appearance route,
//! which only sets the reader's theme cookie) is checked by its origin in place of the
//! token: `Sec-Fetch-Site: same-origin`, else an `Origin` naming this host, else no word
//! of either. Tokens are compared in constant time and never logged.
//!
//! ```no_run
//! use std::sync::Arc;
//!
//! use http::Method;
//! use navree_config::{ConfigModule, ConfigSources};
//! use navree_crypto::CryptoModule;
//! use navree_http::{Controller, HttpModule, Json, ModuleBuilderHttpExt, RouteDef};
//! use navree_kernel::{BoxFuture, Injectable, Navree, KernelError, Module, ModuleBuilder, Resolver};
//! use navree_session::{Session, SessionModule};
//!
//! struct CartController;
//!
//! impl Injectable for CartController {
//!     fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
//!         Box::pin(async { Ok(CartController) })
//!     }
//! }
//!
//! async fn add(session: Session, Json(item): Json<String>) -> navree_http::Result<Json<Vec<String>>> {
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
//! fn main() -> Result<(), navree_kernel::BoxError> {
//!     Navree::app()
//!         .module(ConfigModule::for_root(ConfigSources::defaults()))
//!         .module(CryptoModule::from_config())
//!         .module(HttpModule::from_config())
//!         .module(SessionModule::from_config())
//!         .module(ShopModule)
//!         .run()
//! }
//! ```
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod controller;
mod database;
mod layer;
mod memory;
mod module;
mod multipart;
mod options;
mod session;

pub use controller::{CSRF_COOKIE_PATH, CSRF_COOKIE_ROUTE};
pub use database::{DatabaseStore, SESSIONS_TABLE};
pub use layer::CSRF_FIELD;
pub use memory::MemoryStore;
pub use module::SessionModule;
pub use options::{SameSite, SessionOptions, SessionStoreKind};
pub use session::{Session, SessionError};

/// Stores delete expired records on every this many saves.
const PRUNE_EVERY: u64 = 100;

/// The cookie holding the CSRF token for JavaScript clients (not
/// `HttpOnly`); send its URL-decoded value back as `X-XSRF-TOKEN`. The name
/// is navree-http's ([`navree_http::csrf`]), which pages and forms use too.
pub const XSRF_COOKIE: &str = navree_http::csrf::XSRF_COOKIE;
