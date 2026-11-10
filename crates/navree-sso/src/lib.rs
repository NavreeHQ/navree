//! # navree-sso
//!
//! `SsoModule`: SAML 2.0 and OpenID Connect single sign-on for the
//! organizations and workspaces of an app, with verified email domains and
//! just-in-time provisioning.
//!
//! Items hidden from this documentation are not public API: Navree's own
//! macros and crates use them, and they can change in any release.
//!
//! - **Connections** ([`SsoConnection`]) belong to the app, an organization
//!   or a workspace ([`SsoOwner`]) and talk SAML 2.0 (identity provider
//!   metadata by URL or pasted) or OpenID Connect (issuer discovery, a
//!   client id and secret). [`Sso`] creates, changes and deletes them.
//! - **Verified domains** ([`SsoDomain`]): a TXT record proves a domain
//!   belongs to a connection. `POST /api/sso/discover {email}` then routes
//!   its emails to the connection, and an enforcing connection makes it
//!   their only way in: passwords, magic links, emailed codes, passkeys,
//!   social sign-in, token exchange and other connections are refused (a
//!   navree-auth `SignInPolicy`, code `sso_required`).
//! - **Sign-in**: `GET /api/sso/{connection}/login` sends the browser to
//!   the identity provider; `POST /api/sso/saml/{connection}/acs` or
//!   `GET /api/sso/oidc/{connection}/callback` checks the answer, finds,
//!   links or creates the user (just-in-time, with group to role mapping
//!   for the owner's membership, [`SsoUserProvisioned`]), and issues
//!   navree-auth tokens, through a one-time code for the front end
//!   (`sso.complete_url`, `POST /api/sso/exchange`).
//! - **SAML signatures** are verified in the crate: XML read by quick-xml
//!   into a strict tree (no document types, no entities), exclusive
//!   canonicalization, RSA-SHA256/384/512 and ECDSA on `ring` through
//!   `navree_crypto::jose` against the metadata's certificates. SHA-1 is
//!   refused; what a signature covers is the only thing read (signature
//!   wrapping cannot move it); assertions work once. [`saml`] has the
//!   building blocks.
//! - **OpenID Connect**: authorization code with PKCE (S256) and a nonce;
//!   the ID token is verified against the provider's key set (issuer,
//!   audience, expiry, nonce).
//! - **Tests**: [`SsoFake`] is a SAML identity provider whose responses
//!   pass the same checks; [`StaticTxtResolver`] answers DNS.
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod connection;
mod controller;
mod dns;
mod dsig;
mod entity;
mod error;
mod events;
mod fake;
#[cfg(feature = "fuzzing")]
#[doc(hidden)]
pub mod fuzzing;
mod migration;
mod module;
mod oidc;
mod options;
mod policy;
mod profile;
pub mod saml;
mod service;
mod store;
mod xml;

pub use connection::{
    AttributeMapping, ConnectionSettings, ConnectionUpdate, NewConnection, OidcSettings, Protocol,
    Provisioning, RoleRule, SamlSettings, ServiceProviderInfo, SsoConnection, SsoDiscovery,
    SsoDomain, SsoOwner,
};
pub use controller::{AddDomainInput, DiscoverInput, ExchangeInput, MANAGE_PERMISSION};
pub use dns::{DohResolver, StaticTxtResolver, TxtResolver};
pub use dsig::SignatureError;
pub use entity::{sso_connection, sso_domain, sso_identity};
pub use error::SsoError;
pub use events::{SsoDomainVerified, SsoUserProvisioned};
pub use fake::{FakeResponse, SsoFake};
pub use migration::SSO_MIGRATIONS;
pub use module::SsoModule;
pub use options::{DomainOptions, OidcOptions, SamlOptions, SsoOptions};
pub use profile::SsoProfile;
pub use service::{OidcCallback, Sso, SsoRedirect, SsoSignIn};
pub use xml::XmlError;
