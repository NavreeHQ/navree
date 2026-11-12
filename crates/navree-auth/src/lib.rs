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
//!   | `GET /auth/sessions` | authenticated | 200 `{data: [SessionResource]}` (the user's live sessions) |
//!   | `DELETE /auth/sessions/{id}` | authenticated, not while impersonating | 204; 404 for a session that is not the user's |
//!   | `POST /auth/confirm` `{password \| code}` | authenticated, not while impersonating | 200 [`ConfirmedToken`] (a new access token with `auth_time` now) |
//!   | `POST /auth/impersonate/{user}` `{reason}` | permission `users.impersonate`; when `auth.impersonation.enabled` | 200 like login plus `impersonator_id` |
//!   | `DELETE /auth/impersonate` | an impersonation session | 204 |
//!   | `POST /auth/magic-link` `{email}`, `POST /auth/magic-link/verify` `{token}` | public; when `auth.magic_link.enabled` | 202 `{message}`; 200 like login |
//!   | `POST /auth/otp` `{email}`, `POST /auth/otp/verify` `{email, code}` | public; when `auth.otp.enabled` | 202 `{message}`; 200 like login |
//!   | `/auth/passkeys/{register/options, register, login/options, login}`, `GET /auth/passkeys`, `DELETE /auth/passkeys/{id}` | when `auth.passkeys.enabled` | WebAuthn ceremonies (ES256, EdDSA) |
//!   | `POST /auth/session/login` `{email, password, remember?}`, `POST /auth/session/2fa/verify` `{code \| recovery_code}`, and when on `POST /auth/session/magic-link/verify`, `POST /auth/session/otp/verify` (feature `session`) | public; the session's CSRF token | 200 the user, or `{two_factor_required: true}`; the browser keeps the session cookie and gets no token |
//!   | `POST /auth/platform/login`, `POST /auth/platform/2fa/verify` | public; when `auth.platform.enabled` | a challenge; 200 a platform session |
//!   | `POST /auth/exchange` `{token}` | public; when `auth.trusted_issuers` is set | 200 like login for a trusted issuer's JWT |
//!
//! - **Access tokens** are HS256 JWTs on `navree_crypto::jose` ([`Claims`]: `sub`, `iat`, `exp`,
//!   `iss`, `aud`, `jti`, `typ`, and the session claims `sid`, `auth_time`
//!   and, for impersonation, `act`), 15 minutes by default, signed with
//!   `auth.jwt.secret` or a key derived from the application key. They are
//!   stateless: logout does not end them before they expire.
//! - **Sessions** are refresh-token families ([`AuthService::sessions`]);
//!   `#[recent_auth(minutes = N)]` ([`recent_auth`]) needs a sign-in or a
//!   confirmation (`POST /auth/confirm`) within N minutes, and
//!   `#[not_while_impersonating]` ([`not_while_impersonating`]) refuses
//!   impersonation sessions. [`AccessClaims`] and [`Actor`] read the token.
//! - **Two-factor policy**: `auth.two_factor.required_for` roles and
//!   [`TwoFactorPolicy`]s limit users without two-factor authentication to
//!   the setup routes (403 `two_factor_required` elsewhere).
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
//! - **Browser sign-in** (feature `session`, with navree-session's
//!   `SessionModule` in the app): `AuthService::sign_in_session` and
//!   `AuthService::verify_two_factor_session` sign a browser in on its
//!   session cookie through the same steps as a token client (sign-in
//!   policies, the login and two-factor throttles, two-factor
//!   authentication, the audit trail), regenerating the session against
//!   fixation. `SessionStrategy` (`"session"`, after the JWT strategy) reads
//!   the user back on every request once it has checked on the server that
//!   the sign-in is live, so signing out, ending the session from another
//!   device, changing or resetting the password and offboarding end it at
//!   once, for a copied cookie too. It sets the same [`AccessClaims`] as a
//!   token, so `.authenticated()`, `#[recent_auth]`,
//!   `#[not_while_impersonating]` and the two-factor setup rule hold for
//!   both. "Remember me" keeps a browser signed in for
//!   `auth.session.remember_days`. `login_with_magic_link_session`,
//!   `login_with_code_session` and `login_with_passkey_session` sign a
//!   browser in with a mailed link, a mailed code and a passkey, and
//!   `impersonate_session` / `stop_impersonating_session` let an
//!   administrator act as a user in the browser and come back. `AuthPages`
//!   (feature `ui`) draws the sign-in, two-factor, forgotten and reset
//!   password and sign-out pages, the two-factor setup page (a user a
//!   [`TwoFactorPolicy`] requires it of is sent there), and the pages of the
//!   mailed link, the mailed code and the passkey when those are on, with
//!   navree-ui. The `/auth/session/*` routes are the same sign-in as JSON for
//!   a single-page app.
//! - [`CurrentUser`] loads the signed-in [`User`] in handlers;
//!   [`ClientMeta`] gives the client's IP, user agent and
//!   request id.
//! - **Sign-in policies**: every way to a session (passwords, magic links,
//!   emailed codes, passkeys, trusted token exchange, the second step of a
//!   two-factor sign-in, social sign-in, single sign-on and the platform
//!   audience) asks the registered [`SignInPolicy`]s before it issues
//!   anything ([`AuthService::sign_in`], [`AuthService::check_sign_in`]); a
//!   refusal is a 403 with the policy's code, such as `sso_required`.
//!   [`email_domain`] gives the one spelling of an email's domain that
//!   policies compare.
//! - **Offboarding**: [`AuthService::revoke_credentials_of_domain`] and
//!   [`AuthService::revoke_all_credentials`] end every credential of a
//!   domain's users or of one user (sessions here; personal access tokens
//!   and OAuth grants through each module's `CredentialRevoker`), and report
//!   what they ended as [`CredentialsRevoked`].
//! - **Throttles** count failed logins per email and client network (an IPv4
//!   address, or an IPv6 /64: `http.rate_limit_ipv6_prefix`).
//! - [`User`] is navree-access's subject `User` (permission prefix
//!   `users`, `#[model(subject)]`): import
//!   `AccessModule::for_feature::<navree_auth::User>()` for
//!   `can(.., User, from = "id")` checks, which load users through the ORM
//!   in the request's unit of work, and for `users.*` permissions.
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
//! - **Two-factor authentication** is TOTP (`navree_crypto::totp`; SHA-1,
//!   6 digits, 30 s, one step of drift; the setup QR code is
//!   `navree_crypto::qr`):
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
//!   `auth.two_factor_challenge`, `auth.two_factor_failed`,
//!   `auth.two_factor_recovery_code_used`, `auth.throttled`,
//!   `auth.login_refused` (a [`SignInPolicy`] refused a sign-in),
//!   `auth.break_glass` and `auth.credentials_revoked`. Writes are audited
//!   once their unit of work commits.
//!
//! ```no_run
//! use std::sync::Arc;
//!
//! use http::Method;
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
#![cfg_attr(docsrs, feature(doc_cfg))]

mod account;
mod account_controller;
mod apps;
mod controller;
mod disconnect;
mod entity;
mod error;
mod exchange;
mod extract;
mod guards;
mod impersonation;
mod jwt;
mod mail;
mod migration;
mod module;
mod offboarding;
mod options;
#[cfg(feature = "ui")]
mod pages;
mod passkeys;
mod passwordless;
mod platform;
mod policy;
#[cfg(feature = "notify")]
mod push;
mod resource;
mod service;
#[cfg(feature = "session")]
mod session_auth;
mod session_controller;
#[cfg(feature = "session")]
mod session_json;
mod sessions;
mod sign_in_policy;
mod strategy;
mod telemetry;
pub mod testing;
mod throttle;
mod tickets;
mod token;
mod two_factor;

pub use account::ProfileUpdate;
pub use account_controller::{
    AuthMessage, ChangePasswordInput, ConfirmTwoFactorInput, DisableTwoFactorInput,
    ForgotPasswordInput, RecoveryCodesResponse, ResetPasswordInput, TwoFactorVerifyInput,
    UpdateProfileInput,
};
pub use apps::{
    ANDROID_ASSOCIATION_PATH, ANDROID_ORIGIN_PREFIX, APPLE_ASSOCIATION_PATH, AndroidApp, AppleApp,
    AppsOptions,
};
pub use controller::{
    LoginInput, LoginResponse, LogoutInput, RefreshInput, RegisterInput, TwoFactorChallenge,
};
pub use entity::{Passkey, User, login_token, passkey, password_reset_token, refresh_token, user};
pub use error::AuthError;
pub use extract::CurrentUser;
pub use guards::{AccessClaims, Actor, not_while_impersonating, platform_session, recent_auth};
pub use impersonation::MAX_IMPERSONATION_REASON;
pub use jwt::{
    ActorClaim, Claims, JwtService, LEEWAY_SECS, MAX_TOKEN_LEN, SessionClaims, TokenType,
};
pub use migration::AUTH_MIGRATIONS;
pub use module::{AuthModule, ModuleBuilderAuthExt};
pub use offboarding::CredentialsRevoked;
pub use options::{
    AuthOptions, ImpersonationOptions, JwtOptions, MAX_RECOVERY_CODES, MIN_SECRET_LEN,
    MagicLinkOptions, OtpOptions, PasskeyOptions, PasswordResetOptions, PlatformOptions,
    SessionAuthOptions, TrustedIssuerOptions, TwoFactorOptions, VerificationOptions,
};
#[cfg(feature = "ui")]
pub use pages::AuthPages;
pub use passkeys::confirm::{CONFIRM_TTL_SECS, ConfirmAction};
pub use passkeys::controller::{ConfirmOptionsInput, ConfirmPasskeyInput};
pub use passkeys::controller::{
    PasskeyList, PasskeyLoginInput, PasskeyLoginOptionsInput, PasskeyOptionsResponse,
    PasskeyRegisterInput,
};
pub use passkeys::{MAX_PASSKEY_NAME, PasskeyResource};
pub use platform::PlatformAdmin;
pub use policy::TwoFactorPolicy;
#[cfg(feature = "notify")]
pub use push::AuthLiveSessions;
pub use resource::{USER_PERMISSION_PREFIX, USER_SUBJECT, UserResource, UserResourceExtension};
pub use service::{AuthService, ClientMeta, NewUser, TokenPair};
#[cfg(feature = "session")]
pub use session_auth::{
    SESSION_CHALLENGE_KEY, SESSION_USER_KEY, SIGN_OUT_PATH, STOP_IMPERSONATING_PATH, SessionSignIn,
    SessionStrategy, SessionUser, TWO_FACTOR_SETUP_PATH,
};
pub use session_controller::{
    ConfirmInput, ExchangeInput, ImpersonateInput, MagicLinkVerifyInput, OtpVerifyInput,
    PasswordlessInput, SessionList,
};
#[cfg(feature = "session")]
pub use session_json::{
    SessionLoginInput, SessionMagicLinkInput, SessionOtpInput, SessionTwoFactorInput,
};
pub use sessions::{ConfirmProof, ConfirmedToken, SessionResource};
pub use sign_in_policy::{SignInAttempt, SignInAudience, SignInMethod, SignInPolicy, email_domain};
pub use strategy::{JwtStrategy, Strategy, StrategyAuthenticator};
pub use tickets::SealedTickets;
pub use two_factor::{LoginOutcome, TwoFactorProof, TwoFactorSetup, two_factor_enabled};
