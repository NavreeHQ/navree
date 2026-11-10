//! # navree-domains
//!
//! `DomainsModule`: workspaces on their own domains (`crm.acme-corp.com`
//! instead of `acme.app.example.com`), with automatic TLS.
//!
//! - **Adding a domain** ([`Domains::add`], `POST /api/domains`): the
//!   domain is checked ([`normalize_host`]), refused when it is reserved
//!   (your own domains), verified for another workspace already or over
//!   the workspace's limit, and stored as a pending claim with a
//!   verification token of its own. The answer lists the DNS records
//!   to create ([`DnsInstruction`]): a TXT record at
//!   `_domain-verification.<domain>` and a CNAME to `domains.target` (A and
//!   AAAA records for an apex domain).
//! - **Proof of control, not first come first served.** Any number of
//!   workspaces may hold a pending claim on one host; the claim whose TXT
//!   record is found first becomes verified, atomically (a unique index on
//!   the verified host), and the others are closed with a
//!   [`DomainClaimedElsewhere`] event. Failed claims never block anyone
//!   and are deleted after `domains.verification.failed_retention_hours`.
//!   A verified domain moves to another workspace only through
//!   [`Domains::transfer`] (`domains:transfer`), which needs a fresh TXT
//!   proof of the workspace that takes it.
//! - **Verification** through DNS (hickory-resolver, [`DnsLookup`]):
//!   `domains:verify --due` runs every minute and checks pending domains
//!   every `domains.verification.interval_mins` until their TXT record,
//!   and that they point at the app, are found, or gives up after
//!   `domains.verification.give_up_after_hours`.
//! - **Resolution**: with `workspace.resolvers = ["domain", ...]`, a
//!   request to a verified domain names its workspace for
//!   `CurrentWorkspace`; [`CurrentDomain`] gives public pages the domain
//!   and its workspace.
//! - **Automatic TLS** ([`TlsMode`]): `on_demand` (the default) answers an
//!   on-demand TLS proxy's ask endpoint (`GET /_domains/allow?domain=`),
//!   which Harbor's Caddy calls before it gets a certificate; `acme` gets
//!   certificates from an ACME CA in the app ([`InstantAcme`], HTTP-01
//!   challenges served from the database), stores them with their keys
//!   sealed, renews them, and serves them with a rustls certificate
//!   resolver ([`DomainCertificates`], [`HttpsServer`]).
//! - **Managed zones**: with a DNS provider ([`CloudflareDns`]), a domain
//!   added inside `domains.dns.zone` gets its record created and is
//!   verified at once.
//! - **Expiry reports**: `domains:certificates --due` renews (ACME) or
//!   reads over TLS ([`TlsProbe`], on-demand) each certificate's expiry and
//!   reports the ones close to it: a warning, a
//!   [`DomainCertificateExpiring`] event and, with the `lighthouse`
//!   feature, a Lighthouse issue.
//! - **Events** [`DomainAdded`], [`DomainVerified`],
//!   [`DomainVerificationFailed`], [`DomainClaimedElsewhere`],
//!   [`DomainRemoved`], [`DomainCertificateIssued`],
//!   [`DomainCertificateExpiring`]; commands `domains:list`,
//!   `domains:verify`, `domains:transfer` and `domains:certificates`.
//! - **Tests** use [`DnsFake`], [`AcmeFake`], [`ProbeFake`] and
//!   [`DnsProviderFake`].
//!
//! ```no_run
//! use navree_config::{ConfigModule, ConfigSources};
//! use navree_db::DatabaseModule;
//! use navree_domains::DomainsModule;
//! use navree_http::HttpModule;
//! use navree_kernel::Navree;
//!
//! # fn main() -> Result<(), navree_kernel::BoxError> {
//! Navree::app()
//!     .module(ConfigModule::for_root(ConfigSources::defaults()))
//!     .module(HttpModule::from_config())
//!     .module(DatabaseModule::from_config())
//!     .module(DomainsModule::from_config())
//!     .run()
//! # }
//! ```
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod events;
pub mod models;

mod acme;
mod certs;
mod commands;
mod controller;
mod current;
mod dns;
mod error;
mod host;
mod issue;
mod migration;
mod module;
mod options;
mod probe;
mod provider;
mod runtime;
mod savepoint;
mod service;
mod tls;
mod types;
mod verify;

pub use acme::{AcmeClient, AcmeFake, AcmeOrder, InstantAcme, IssuedCertificate};
pub use certs::DomainCertificates;
pub use controller::{AskQuery, DomainsController, DomainsPublicController, MANAGE_PERMISSION};
pub use current::CurrentDomain;
pub use dns::{DnsFake, DnsLookup, SystemDns};
pub use error::DomainsError;
pub use events::{
    DomainAdded, DomainCertificateExpiring, DomainCertificateIssued, DomainClaimedElsewhere,
    DomainRemoved, DomainVerificationFailed, DomainVerified,
};
pub use host::{normalize_host, request_host};
pub use migration::DOMAINS_MIGRATIONS;
pub use module::DomainsModule;
pub use navree_workspace::DomainWorkspace;
pub use options::{
    AcmeOptions, CertificateOptions, CloudflareOptions, DnsOptions, DomainsOptions, LETS_ENCRYPT,
    LETS_ENCRYPT_STAGING, TlsMode, VerificationOptions,
};
pub use probe::{CertificateProbe, ProbeFake, ProbedCertificate, TlsProbe};
pub use provider::{CloudflareDns, DnsProvider, DnsProviderFake, RecordKind};
pub use service::Domains;
pub use tls::HttpsServer;
pub use types::{CertificateRun, CustomDomain, DnsInstruction, DomainStatus, NewDomain, TlsStatus};
