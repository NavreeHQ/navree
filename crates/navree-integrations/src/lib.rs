//! # navree-integrations
//!
//! `IntegrationsModule`: connections from your app's workspaces and
//! organizations to third-party APIs: Microsoft 365, Google Workspace,
//! Slack, any OAuth 2.0 API, and API keys.
//!
//! - **Providers.** [`MicrosoftGraph`] (admin consent and app-only tokens,
//!   or delegated tokens), [`GoogleWorkspace`] (service-account keys with
//!   domain-wide delegation, signed RS256 through `ring`, or OAuth),
//!   [`Slack`] (OAuth v2 installs, token rotation), `type = "oauth2"` for
//!   any OAuth 2.0 API and `type = "api_key"` for keys. [`Provider`] names
//!   one as a type, for extractors and typed clients.
//! - **Connections** belong to a workspace, an organization or the app
//!   ([`Owner`]); a workspace without its own uses its organization's.
//!   Credentials are encrypted with the application key in
//!   `integration_connections` (the navree-orm model
//!   [`IntegrationConnection`]); [`ConnectionInfo`] never carries them.
//! - **Consent flows.** `GET /api/integrations/{provider}/connect` gives
//!   the consent URL and a sealed state (PKCE where the provider supports
//!   it); `GET .../callback` with the provider's parameters stores the
//!   connection. Service-account keys and API keys are posted.
//! - **Scope sets** ([`ScopeSet`]) keep least privilege per feature:
//!   directory read is connected apart from mail send.
//! - **Tokens** are cached encrypted and refreshed under a cache lock, so
//!   each connection refreshes once however many requests need it.
//! - **Health.** `integrations:check` (scheduled every five minutes for
//!   the due connections) mints a token and probes the API; a connection
//!   that stops working emits [`ConnectionUnhealthy`].
//! - **Typed clients** on the app's `HttpClient`: [`IntegrationClient`],
//!   [`GraphClient`], [`GoogleClient`], [`SlackClient`], and the
//!   [`Connection<P, S>`] extractor: `graph.get("/users").send().await?`.
//! - **Tests**: [`Integrations::fake`] gives an [`IntegrationsFake`] that
//!   answers codes and grants and stubs the APIs.
//! - **Notifications** (crate feature `notify`): the module registers
//!   navree-notify's `dyn ConnectionTokens` (`IntegrationTokens`), so Slack
//!   and Teams bot routes that name a connection send with its token.
//!
//! ```no_run
//! use navree_config::{ConfigModule, ConfigSources};
//! use navree_crypto::CryptoModule;
//! use navree_db::DatabaseModule;
//! use navree_http::HttpModule;
//! use navree_http_client::HttpClientModule;
//! use navree_integrations::IntegrationsModule;
//! use navree_kernel::Navree;
//!
//! # fn main() -> Result<(), navree_kernel::BoxError> {
//! // [integrations]
//! // redirect_base = "https://app.acme.test/settings/integrations/callback"
//! // [integrations.providers.microsoft]
//! // client_id = "0f6c1d2e-8a3b-4c5d-9e7f-1a2b3c4d5e6f"
//! Navree::app()
//!     .module(ConfigModule::for_root(ConfigSources::defaults()))
//!     .module(CryptoModule::from_config())
//!     .module(HttpModule::from_config())
//!     .module(HttpClientModule::from_config())
//!     .module(DatabaseModule::from_config())
//!     .module(IntegrationsModule::from_config())
//!     .run()
//! # }
//! ```
#![forbid(unsafe_code)]

pub mod events;

mod client;
mod commands;
mod controller;
mod entry;
mod error;
mod extract;
mod fake;
mod flow;
mod migration;
mod model;
mod module;
#[cfg(feature = "notify")]
mod notify;
mod oauth;
mod options;
mod provider;
mod runtime;
mod service;
mod state;
mod tokens;
mod types;

pub use client::{ApiRequest, GoogleClient, GraphClient, IntegrationClient, SlackClient};
pub use controller::{
    ApiKeyInput, ConnectQuery, IntegrationsController, MANAGE_PERMISSION, ServiceAccountInput,
};
pub use error::IntegrationsError;
pub use events::{
    ConnectionRestored, ConnectionUnhealthy, IntegrationConnected, IntegrationDisconnected,
};
pub use extract::{Connection, IntegrationOwner};
pub use fake::{FakeGrant, IntegrationsFake, Minted};
pub use flow::{
    CompletedConnection, ConnectCallback, ConnectOptions, ConnectRedirect, NewApiKey,
    NewServiceAccount,
};
pub use migration::INTEGRATIONS_MIGRATIONS;
pub use model::IntegrationConnection;
pub use module::{IntegrationsModule, ModuleBuilderIntegrationsExt};
#[cfg(feature = "notify")]
pub use notify::IntegrationTokens;
pub use options::{
    AppCredentials, ClientAuth, IntegrationsOptions, Level, MicrosoftMode, ProviderConfig,
    ProviderType,
};
pub use provider::{
    DEFAULT_SCOPE_SET, DefaultScopes, GoogleWorkspace, MicrosoftGraph, Provider, ProviderInfo,
    ScopeSet, ScopeSetInfo, Slack,
};
pub use service::Integrations;
pub use types::{
    ApiKeyCredentials, CheckSummary, ConnectionInfo, ConnectionStatus, CredentialKind, Health,
    Owner, OwnerType, ServiceAccountKey,
};
