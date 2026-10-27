//! # navree-auth
//!
//! `AuthModule`: authentication with stateless JWT access tokens,
//! rotating refresh tokens, pluggable strategies and user accounts.
//!
//! - [`AuthModule`] (global; `for_root(AuthOptions)` or `from_config()`,
//!   section `auth`) provides [`AuthService`] and [`JwtService`], registers
//!   navree-http's `dyn Authenticator` and mounts the routes under
//!   `{api_prefix}{routes_prefix}` (default `/api/auth`):
//!
//!   | Route | Access | Result |
//!   |---|---|---|
//!   | `POST /auth/register` `{name, email, password}` | public (404 when `auth.registration` is off) | 201 `{user, access_token, refresh_token, token_type, expires_in}` |
//!   | `POST /auth/login` `{email, password}` | public; 5 failures per minute per email + IP, then 429 | 200 `{user, access_token, refresh_token, token_type, expires_in}`, or [`TwoFactorChallenge`] `{two_factor_required: true, challenge_token, expires_in}` with two-factor authentication on; 401 "Invalid credentials" |
//!   | `POST /auth/refresh` `{refresh_token}` | public | 200 a new [`TokenPair`]; 401 "Invalid refresh token" |
//!   | `POST /auth/logout` `{refresh_token?, all?}` | authenticated | 204 |
//!   | `GET /auth/me` | authenticated | 200 [`UserResource`] |
//!   | `PATCH /auth/me` `{name?, email?, current_password?}` | authenticated | 200 [`UserResource`]; a new email needs `current_password` (400 `errors.current_password`), is unverified, gets a verification link, and the old address is notified |
//!   | `PATCH /auth/me/password` `{current_password, password}` | authenticated | 200 a new [`TokenPair`] (every other session is signed out); 400 `errors.current_password` |
//!   | `POST /auth/forgot-password` `{email}` | public | always 202 `{message}`; mails a reset token (SHA-256 at rest, 60 minutes) to registered addresses |
//!   | `POST /auth/reset-password` `{email, token, password}` | public | 204 (every session is signed out); 400 "Invalid or expired password reset token" |
//!   | `POST /auth/email/verification` | authenticated | 202 `{message}` (a signed link is mailed); 204 when already verified |
//!   | `GET /auth/email/verify/{id}/{hash}?expires&signature` | public, signed URL (`UrlSigner`) | 200 `{message}`; 401 bad or expired signature; 403 "Invalid verification link" |
//!   | `POST /auth/2fa/enable` | authenticated | 200 [`TwoFactorSetup`] `{secret, otpauth_url, qr_svg}`; 409 when already on |
//!   | `POST /auth/2fa/confirm` `{code}` | authenticated | 200 `{recovery_codes}` (two-factor is on from now); 400 `errors.code` |
//!   | `POST /auth/2fa/disable` `{password}` | authenticated | 204; 400 `errors.password` |
//!   | `POST /auth/2fa/recovery-codes` | authenticated | 200 `{recovery_codes}` (the old ones stop working) |
//!   | `POST /auth/2fa/verify` `{challenge_token, code \| recovery_code}` | public; 5 wrong codes per user per minute, then 429 | 200 like login; 401 "Invalid two-factor code" / "Invalid token" / "Token expired" |
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
//!   stay deny-by-default and opt out with `.public()` (`#[public]`): a
//!   global guard with a public opt-out.
//! - [`CurrentUser`] loads the signed-in [`User`] in handlers;
//!   [`ClientMeta`] gives the client's IP, user agent and
//!   request id.
//! - [`user::Entity`] is navree-access's subject `User` (permission prefix
//!   `users`): import `AccessModule::for_feature::<navree_auth::user::Entity>()`
//!   for `can(.., User, from = "id")` checks and `users.*` permissions.
//! - Passwords are hashed with Argon2id (navree-crypto) and rehashed at login
//!   when the parameters change. `users.password_hash` and the two-factor
//!   columns are never serialized.
//! - **Account mails** (password reset, email verification) go through the
//!   app's navree-mail `Mailer` when MailModule is in the app (queued when a
//!   job dispatcher is registered, which also keeps response times the same
//!   for registered and unknown addresses), after the request commits.
//!   Configure the links with `auth.password_reset.url` and
//!   `auth.verification.base_url`, and the bodies with navree-view templates
//!   (`auth.password_reset.mail_template`, `auth.verification.mail_template`);
//!   see [`AuthOptions`].
//! - **Two-factor authentication** is TOTP (totp-rs; SHA-1, 6 digits, 30 s):
//!   the secret is encrypted with navree-crypto's `Encrypter`, recovery codes
//!   are stored as encrypted SHA-256 digests and work once, a code is
//!   accepted once, and the login challenge is a 5-minute JWT of type
//!   [`TokenType::TwoFactorChallenge`] that is never an access token.
//! - Security events go to the `dyn AuditSink` when one is registered:
//!   `auth.login`, `auth.login_failed`, `auth.refresh`,
//!   `auth.refresh_reuse_detected`, `auth.logout`, `auth.register`,
//!   `auth.password_reset_requested`, `auth.password_reset`,
//!   `auth.password_changed`, `auth.password_confirmation_failed`,
//!   `auth.profile_updated`, `auth.email_verification_sent`,
//!   `auth.email_verified`, `auth.two_factor_enabled`,
//!   `auth.two_factor_confirmed`, `auth.two_factor_disabled`,
//!   `auth.two_factor_recovery_codes_regenerated`,
//!   `auth.two_factor_challenge`, `auth.two_factor_failed` and
//!   `auth.two_factor_recovery_code_used`. Writes are audited once their
//!   unit of work commits.
//!
//! ```no_run
//! use std::sync::Arc;
//!
//! use axum::http::Method;
//! use navree_auth::{AuthModule, CurrentUser};
//! use navree_config::{ConfigModule, ConfigSources};
//! use navree_crypto::CryptoModule;
//! use navree_db::DatabaseModule;
//! use navree_http::{Controller, HttpModule, Json, ModuleBuilderHttpExt, RouteDef};
//! use navree_kernel::{BoxFuture, Injectable, Navree, KernelError, Module, ModuleBuilder, Resolver};
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
//! # fn main() -> Result<(), navree_kernel::BoxError> {
//! Navree::app()
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

mod account;
mod account_controller;
mod controller;
mod disconnect;
mod entity;
mod error;
mod extract;
mod jwt;
mod mail;
mod migration;
mod module;
mod options;
mod policy;
mod resource;
mod service;
mod strategy;
mod throttle;
mod token;
mod two_factor;

pub use account::ProfileUpdate;
pub use account_controller::{
    AuthMessage, ChangePasswordInput, ConfirmTwoFactorInput, DisableTwoFactorInput,
    ForgotPasswordInput, RecoveryCodesResponse, ResetPasswordInput, TwoFactorVerifyInput,
    UpdateProfileInput,
};
pub use controller::{
    LoginInput, LoginResponse, LogoutInput, RefreshInput, RegisterInput, TwoFactorChallenge,
};
pub use entity::{User, password_reset_token, refresh_token, user};
pub use error::AuthError;
pub use extract::CurrentUser;
pub use jwt::{Claims, JwtService, LEEWAY_SECS, TokenType};
pub use migration::AUTH_MIGRATIONS;
pub use module::{AuthModule, ModuleBuilderAuthExt};
pub use options::{
    AuthOptions, JwtOptions, MAX_RECOVERY_CODES, MIN_SECRET_LEN, PasswordResetOptions,
    TwoFactorOptions, VerificationOptions,
};
pub use policy::PasswordSignInPolicy;
pub use resource::{USER_PERMISSION_PREFIX, USER_SUBJECT, UserResource, UserResourceExtension};
pub use service::{AuthService, ClientMeta, NewUser, TokenPair};
pub use strategy::{JwtStrategy, Strategy, StrategyAuthenticator};
pub use two_factor::{LoginOutcome, TwoFactorProof, TwoFactorSetup, two_factor_enabled};
