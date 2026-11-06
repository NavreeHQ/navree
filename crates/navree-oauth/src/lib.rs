//! # navree-oauth
//!
//! `OAuthServerModule`: an OAuth 2.1 authorization server and OpenID
//! Connect provider, so other software can act on your users' behalf, plus
//! personal access tokens for scripts.
//!
//! - **Grants.** Authorization code with PKCE (S256 required), client
//!   credentials (acting as the client's owner), the device authorization
//!   grant (RFC 8628) and refresh tokens, rotated on every use, where a
//!   used token that comes back revokes its whole family.
//! - **Scopes that map to permissions.** `oauth.scopes.<name>.permissions`
//!   are navree-access permission strings; a token's abilities are its
//!   user's narrowed to its scopes ([`OAuthStrategy`] sets
//!   `Principal::token_abilities`), so `#[can]`, `#[permissions]` and
//!   `Accessible` checks apply to OAuth tokens unchanged.
//! - **Consent** through your front end: `GET /oauth/authorize` redirects
//!   to `oauth.consent_url`, which reads and decides the request through
//!   [`OAuthApiController`]; approvals are remembered.
//! - **OpenID Connect:** `openid`, `profile`, `email` and
//!   `offline_access`; ID tokens signed with Ed25519 (EdDSA) or RSA (RS256,
//!   `oauth.id_token_signing_alg`), userinfo, the JWKS
//!   and the discovery documents.
//! - **Personal access tokens** ([`PersonalAccessTokens`]), revocation (RFC
//!   7009), introspection (RFC 7662) and, behind
//!   `oauth.dynamic_registration`, dynamic client registration (RFC 7591).
//! - **Rate limits** (`oauth.rate_limits`, [`RateLimitOptions`]): the token,
//!   introspection, revocation and device authorization endpoints are
//!   limited per client IP address (an IPv6 client counts as its network,
//!   `http.rate_limit_ipv6_prefix`) before anything is checked, and per
//!   client once it has authenticated; public clients are limited per IP
//!   only. A request over a limit is a 429 with `Retry-After`.
//! - **Offboarding:** the module registers a
//!   `navree_contracts::CredentialRevoker`, so navree-auth's
//!   `AuthService::revoke_credentials_of_domain` and `revoke_all_credentials`
//!   also end the users' personal access tokens, OAuth tokens, unexchanged
//!   codes and remembered consent.
//!
//! Tokens are opaque (`nva_` access, `nvr_` refresh, `nvp_` personal) and
//! stored as SHA-256 digests.
//!
//! ```no_run
//! use navree_access::{AccessModule, AccessOptions};
//! use navree_auth::AuthModule;
//! use navree_config::{ConfigModule, ConfigSources};
//! use navree_crypto::CryptoModule;
//! use navree_db::DatabaseModule;
//! use navree_http::HttpModule;
//! use navree_kernel::Navree;
//! use navree_oauth::OAuthServerModule;
//!
//! # fn main() -> Result<(), navree_kernel::BoxError> {
//! Navree::app()
//!     .module(ConfigModule::for_root(ConfigSources::defaults()))
//!     .module(CryptoModule::from_config())
//!     .module(HttpModule::from_config())
//!     .module(DatabaseModule::from_config())
//!     .module(AuthModule::from_config())
//!     .module(AccessModule::for_root(AccessOptions::default()))
//!     .module(OAuthServerModule::from_config()) // section `oauth`
//!     .run()
//! # }
//! ```
#![forbid(unsafe_code)]

pub mod entity;
#[cfg(feature = "testing")]
pub mod testing;

mod api;
mod clients;
mod commands;
mod config;
mod core;
mod error;
mod grants;
mod keys;
mod migration;
mod module;
mod offboarding;
mod pats;
mod protocol;
mod requests;
mod scope;
mod strategy;
mod throttle;
mod token;
#[cfg(feature = "ui")]
pub mod ui;

pub use api::{CreatePersonalAccessTokenInput, DataList, OAuthApiController};
pub use clients::{
    CreatedClient, GRANT_AUTHORIZATION_CODE, GRANT_CLIENT_CREDENTIALS, GRANT_DEVICE_CODE,
    GRANT_REFRESH_TOKEN, MAX_CLIENT_NAME_LEN, NewClient, OAuthClients,
};
pub use config::{
    EndpointLimit, OAuthOptions, PersonalAccessTokenOptions, RateLimitOptions, ScopeOptions,
};
pub use entity::{Authorization, Client, PersonalAccessToken};
pub use error::OAuthError;
pub use grants::{Grant, TokenResponse};
pub use keys::{
    IdTokenAlgorithm, IdTokenKey, KeyError, RSA_KEY_PREFIX, SIGNING_KEY_PREFIX, SigningKey25519,
    SigningKeys,
};
pub use migration::OAUTH_MIGRATIONS;
pub use module::OAuthServerModule;
pub use pats::{
    CreatedPersonalAccessToken, MAX_TOKEN_NAME_LEN, PersonalAccessTokenResource,
    PersonalAccessTokens,
};
pub use protocol::{OAuthProtocolController, RegistrationRequest};
pub use requests::{
    AuthorizationRequestView, AuthorizationView, ClientSummary, DEVICE_INTERVAL_SECS, Decision,
    DeviceAuthorization, DeviceRequestView,
};
pub use scope::{
    BUILT_IN_SCOPES, EMAIL, OFFLINE_ACCESS, OPENID, PROFILE, ScopeInfo, ScopeRegistry, is_built_in,
    join_scopes, parse_scope,
};
pub use strategy::{OAuthStrategy, OAuthToken};
pub use token::{
    ACCESS_TOKEN_PREFIX, CLIENT_SECRET_PREFIX, CODE_PREFIX, DEVICE_CODE_PREFIX,
    PERSONAL_ACCESS_TOKEN_PREFIX, REFRESH_TOKEN_PREFIX, TOKEN_LEN, USER_CODE_LEN, digest,
    display_user_code, normalize_user_code, pkce_challenge,
};
