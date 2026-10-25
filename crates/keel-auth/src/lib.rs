//! # keel-auth
//!
//! `AuthModule`: NestJS-style authentication with stateless JWT access
//! tokens, rotating refresh tokens, Passport-style strategies and user
//! accounts.
//!
//! - [`AuthModule`] (global; `for_root(AuthOptions)` or `from_config()`,
//!   section `auth`) provides [`AuthService`] and [`JwtService`], registers
//!   keel-http's `dyn Authenticator` and mounts the routes under
//!   `{api_prefix}{routes_prefix}` (default `/api/auth`):
//!
//!   | Route | Access | Result |
//!   |---|---|---|
//!   | `POST /auth/register` `{name, email, password}` | public (404 when `auth.registration` is off) | 201 `{user, access_token, refresh_token, token_type, expires_in}` |
//!   | `POST /auth/login` `{email, password}` | public; 5 failures per minute per email + IP, then 429 | 200 `{user, access_token, refresh_token, token_type, expires_in}`; 401 "Invalid credentials" |
//!   | `POST /auth/refresh` `{refresh_token}` | public | 200 a new [`TokenPair`]; 401 "Invalid refresh token" |
//!   | `POST /auth/logout` `{refresh_token?, all?}` | authenticated | 204 |
//!   | `GET /auth/me` | authenticated | 200 [`UserResource`] |
//!
//! - **Access tokens** are HS256 JWTs ([`Claims`]: `sub`, `iat`, `exp`,
//!   `iss`, `aud`, `jti`, `typ`), 15 minutes by default, signed with
//!   `auth.jwt.secret` or a key derived from the application key. They are
//!   stateless: logout does not end them before they expire.
//! - **Refresh tokens** are opaque `<id>.<48 base62>` strings stored as
//!   SHA-256 digests in `refresh_tokens`, valid 30 days and single-use:
//!   every refresh revokes the presented token and issues the next one of
//!   its family (login session). Presenting an already-exchanged token
//!   revokes the whole family (reuse detection).
//! - **Strategies** ([`Strategy`], multi-binding `dyn Strategy`) run in
//!   order; the built-in [`JwtStrategy`] reads JWTs from `Authorization:
//!   Bearer` (other bearer values, such as opaque API tokens, are left to
//!   other strategies and route guards). A present but invalid credential
//!   is a 401 on every route, public ones included (never a silent guest),
//!   so clients must not send stale tokens to `login` and `refresh`; routes
//!   stay deny-by-default and opt out with `.public()` (`#[public]`), like
//!   NestJS's global guard with `@Public()`.
//! - [`CurrentUser`] loads the signed-in [`User`] in handlers (NestJS
//!   `@CurrentUser()`); [`ClientMeta`] gives the client's IP, user agent and
//!   request id.
//! - [`user::Entity`] is keel-access's subject `User` (permission prefix
//!   `users`): import `AccessModule::for_feature::<keel_auth::user::Entity>()`
//!   for `can(.., User, from = "id")` checks and `users.*` permissions.
//! - Passwords are hashed with Argon2id (keel-crypto) and rehashed at login
//!   when the parameters change. `users.password_hash` and the two-factor
//!   columns are never serialized.
//! - Security events go to the `dyn AuditSink` when one is registered:
//!   `auth.login`, `auth.login_failed`, `auth.refresh`,
//!   `auth.refresh_reuse_detected`, `auth.logout`, `auth.register`.
//!
//! ```no_run
//! use std::sync::Arc;
//!
//! use axum::http::Method;
//! use keel_auth::{AuthModule, CurrentUser};
//! use keel_config::{ConfigModule, ConfigSources};
//! use keel_crypto::CryptoModule;
//! use keel_db::DatabaseModule;
//! use keel_http::{Controller, HttpModule, Json, ModuleBuilderHttpExt, RouteDef};
//! use keel_kernel::{BoxFuture, Injectable, Keel, KernelError, Module, ModuleBuilder, Resolver};
//!
//! struct GreetingController;
//!
//! impl Injectable for GreetingController {
//!     fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
//!         Box::pin(async { Ok(GreetingController) })
//!     }
//! }
//!
//! impl Controller for GreetingController {
//!     fn prefix(&self) -> &'static str {
//!         "/greeting"
//!     }
//!
//!     fn routes(self: Arc<Self>) -> Vec<RouteDef> {
//!         vec![
//!             // GET /api/greeting with `Authorization: Bearer <access token>`.
//!             RouteDef::on(Method::GET, "/", |CurrentUser(user): CurrentUser| async move {
//!                 Json(format!("Hello, {}!", user.name))
//!             })
//!             .authenticated(),
//!         ]
//!     }
//! }
//!
//! struct GreetingModule;
//!
//! impl Module for GreetingModule {
//!     fn register(&self, m: &mut ModuleBuilder) {
//!         m.controller::<GreetingController>();
//!     }
//! }
//!
//! # fn main() -> Result<(), keel_kernel::BoxError> {
//! Keel::app()
//!     .module(ConfigModule::for_root(ConfigSources::defaults()))
//!     .module(CryptoModule::from_config())
//!     .module(HttpModule::from_config())
//!     .module(DatabaseModule::from_config())
//!     .module(AuthModule::from_config())
//!     .module(GreetingModule)
//!     .run()
//! # }
//! ```
#![forbid(unsafe_code)]

mod controller;
mod entity;
mod error;
mod extract;
mod jwt;
mod migration;
mod module;
mod options;
mod resource;
mod service;
mod strategy;
mod throttle;
mod token;

pub use controller::{LoginInput, LoginResponse, LogoutInput, RefreshInput, RegisterInput};
pub use entity::{User, password_reset_token, refresh_token, user};
pub use error::AuthError;
pub use extract::CurrentUser;
pub use jwt::{Claims, JwtService, LEEWAY_SECS, TokenType};
pub use migration::AUTH_MIGRATIONS;
pub use module::{AuthModule, ModuleBuilderAuthExt};
pub use options::{AuthOptions, JwtOptions, MIN_SECRET_LEN};
pub use resource::{USER_PERMISSION_PREFIX, USER_SUBJECT, UserResource, UserResourceExtension};
pub use service::{AuthService, ClientMeta, NewUser, TokenPair};
pub use strategy::{JwtStrategy, Strategy, StrategyAuthenticator};
