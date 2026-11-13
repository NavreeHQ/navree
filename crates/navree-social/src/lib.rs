//! # navree-social
//!
//! `SocialModule`: sign in with Google, GitHub, Apple, Microsoft, GitLab,
//! Slack, LinkedIn, Discord, X or any OpenID Connect provider, link the
//! accounts to navree-auth users and issue normal Navree tokens.
//!
//! - [`SocialModule`] (global; `for_root(SocialOptions)` or
//!   `from_config()`, section `social`) provides the [`Social`] service and
//!   mounts the routes under `{api_prefix}{auth.routes_prefix}/social`
//!   (default `/api/auth/social`):
//!
//!   | Route | Access | Result |
//!   |---|---|---|
//!   | `GET {provider}/redirect?return_to&intent` | public (`intent=link`: signed in) | 200 [`SocialRedirect`] `{url, state}` |
//!   | `POST {provider}/callback` `{code, state}` | public; counted by the login throttle | 200 like `POST /auth/login`, plus `return_to` |
//!   | `GET accounts` | signed in | 200 `[`[`LinkedAccount`]`]` |
//!   | `POST {provider}/link` `{code, state}` | signed in | 201 [`LinkedAccount`] |
//!   | `DELETE accounts/{provider}` | signed in | 204 |
//!   | `POST {provider}/native/nonce` | public (`apple`, `google`) | 200 [`NativeNonce`] `{nonce, state, expires_in}` |
//!   | `POST {provider}/native` [`NativeSignIn`] | public; counted by the login throttle | 200 like `POST /auth/login` |
//!   | `POST {provider}/native/session` [`NativeSignIn`] | public, on the session cookie (feature `session`) | 200 the user, or `{"two_factor_required": true}` |
//!   | `POST apple/notifications` `{payload}` | public; Apple's signature checked | 200 |
//!
//! - **Native apps** sign in with the identity token the system's sheet
//!   gives them (Sign in with Apple, Android's Credential Manager), bound
//!   to a one-time nonce the server issued, for one of the site's app ids
//!   (`auth.apps.apple` bundle ids; the Google client id, offered only
//!   with an Android app in `auth.apps.android`). Apple's
//!   authorization code is exchanged and its tokens kept, so that
//!   disconnecting Apple, [`Social::revoke_tokens`] and navree-auth's
//!   offboarding revoke them at Apple ([`Social::native_user`]).
//!
//! - **The state** is sealed with the application key (AES-256-GCM): it
//!   carries the PKCE verifier (S256), the OpenID Connect nonce, the
//!   intent, the user of a link flow, the return path and the expiry
//!   (`social.state_ttl_secs`, 10 minutes), and works once.
//! - **ID tokens** (Google, Apple, Microsoft, `oidc`) are verified against
//!   the provider's JSON Web Key Set (RS256, PS256, ES256, EdDSA on
//!   `ring`): issuer, audience, expiry and nonce. GitLab, Slack and
//!   LinkedIn are read from their userinfo endpoints; GitHub, Discord and X
//!   from their APIs. Apple's client secret is an ES256 JWT signed with the
//!   `.p8` key.
//! - **Linking**: a linked account signs in its user; otherwise a user
//!   with the same **verified** email is linked (`link_by_email`);
//!   otherwise a user is created when registration is on; otherwise 409.
//! - **Sign-in policies**: [`Social::sign_in`] asks navree-auth's
//!   `SignInPolicy`s (`SignInMethod::Social { provider }`) about the
//!   provider's email before any account is created or linked, and about the
//!   account before the session, so single sign-on enforcement
//!   (`sso_required`) holds for social sign-ins too.
//! - **Tokens**: [`Social::sign_in`] returns navree-auth's
//!   [`LoginOutcome`](navree_auth::LoginOutcome), two-factor challenge
//!   included. Provider tokens are stored encrypted with
//!   `social.store_tokens`, and [`Social::token`] refreshes them.
//! - **Throttle**: callbacks count against `auth.login_throttle_per_minute`
//!   per client address (an IPv6 client counts as its network,
//!   `http.rate_limit_ipv6_prefix`).
//! - **Tests**: [`Social::fake`] never calls a provider; [`SocialFake`]
//!   decides what each code returns.
//!
//! ```no_run
//! use navree_auth::AuthModule;
//! use navree_config::{ConfigModule, ConfigSources};
//! use navree_crypto::CryptoModule;
//! use navree_db::DatabaseModule;
//! use navree_http::HttpModule;
//! use navree_http_client::HttpClientModule;
//! use navree_kernel::Navree;
//! use navree_social::SocialModule;
//!
//! # fn main() -> Result<(), navree_kernel::BoxError> {
//! // [social]
//! // redirect_base = "https://app.acme.test/auth/callback"
//! // [social.providers.github]
//! // client_id = "Iv1.8a61f9b3a7aba766"   # secret: APP_SOCIAL__PROVIDERS__GITHUB__CLIENT_SECRET
//! Navree::app()
//!     .module(ConfigModule::for_root(ConfigSources::defaults()))
//!     .module(CryptoModule::from_config())
//!     .module(HttpModule::from_config())
//!     .module(HttpClientModule::from_config())
//!     .module(DatabaseModule::from_config())
//!     .module(AuthModule::from_config())
//!     .module(SocialModule::from_config())
//!     .run()
//! # }
//! ```
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod authorize;
mod controller;
mod entity;
mod error;
mod events;
mod fake;
mod migration;
mod module;
mod native;
mod oauth;
mod options;
mod provider;
mod runtime;
mod service;
mod state;
mod user;

pub use authorize::AuthorizeUrl;
pub use entity::social_account;
pub use error::SocialError;
pub use events::{SocialAccountLinked, SocialAccountUnlinked, SocialProviderNotice};
pub use fake::SocialFake;
pub use migration::SOCIAL_MIGRATIONS;
pub use module::SocialModule;
pub use options::{LinkByEmail, ProviderConfig, SocialOptions, TwoFactorPolicy};
pub use provider::SocialProvider;
pub use service::Social;
pub use user::{
    Intent, LinkedAccount, NativeNonce, NativeSignIn, ProviderToken, RedirectOptions,
    SocialCallback, SocialRedirect, SocialUser,
};
