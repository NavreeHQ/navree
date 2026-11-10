//! The module's tables as navree-orm models.
//!
//! Read them freely (`models::Domain::query().where_(models::Domain::WORKSPACE_ID.eq(id))`);
//! change them through [`Domains`](crate::Domains), which keeps the host
//! cache, the certificates and the events in step.

use std::fmt;

use chrono::{DateTime, Utc};
use navree_orm::Model;
use serde::Serialize;
use uuid::Uuid;

/// A row of `custom_domains`: a domain of a workspace.
#[derive(Model, Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[model(table = "custom_domains")]
pub struct Domain {
    /// The id (a UUID v7).
    pub id: Uuid,
    /// The workspace.
    pub workspace_id: Uuid,
    /// The host, lower-case ASCII. Not unique: several workspaces may
    /// claim one host, each with a row (and a token) of its own; a
    /// workspace has one row per host.
    pub host: String,
    /// What the workspace uses it for (`app`, `links`, `landing`).
    pub purpose: String,
    /// `pending`, `verified` or `failed`.
    pub status: String,
    /// The host while the domain is verified, `None` otherwise. It is
    /// unique, so the database lets only one claim own a host: the first
    /// to prove control wins, atomically.
    pub verified_host: Option<String>,
    /// The value of the verification TXT record.
    pub verification_token: String,
    /// The claim was made by `Domains::transfer`: proving control moves
    /// the host from the workspace that holds it. Only `transfer` sets it.
    pub transfer: bool,
    /// Whether the DNS provider created its record (a host inside
    /// `domains.dns.zone`).
    pub managed: bool,
    /// The workspace's primary domain.
    pub is_primary: bool,
    /// Whether the domain points at the app; `None` until checked, or
    /// when neither `domains.target` nor `domains.addresses` is set.
    pub points_to_app: Option<bool>,
    /// Checks since it was added or last verified.
    pub check_attempts: i32,
    /// When DNS was last checked.
    pub last_checked_at: Option<DateTime<Utc>>,
    /// When the scheduler checks it next.
    pub next_check_at: Option<DateTime<Utc>>,
    /// When it was verified.
    pub verified_at: Option<DateTime<Utc>>,
    /// What the last check found missing.
    pub last_error: Option<String>,
    /// `pending`, `active`, `failed` or `off`.
    pub tls_status: String,
    /// When its certificate expires, when known.
    pub certificate_expires_at: Option<DateTime<Utc>>,
    /// When its certificate was last checked, issued or renewed.
    pub certificate_checked_at: Option<DateTime<Utc>>,
    /// Why the last certificate issuance or probe failed.
    pub tls_error: Option<String>,
    /// Who added it.
    pub created_by: Option<Uuid>,
    /// When it was added.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

/// A row of `custom_domain_certificates`: a certificate issued through
/// ACME (`domains.tls = "acme"`). The private key is sealed with the
/// application key and never serialized.
#[derive(Model, Clone, Default, PartialEq, Eq, Serialize)]
#[model(table = "custom_domain_certificates")]
pub struct Certificate {
    /// The id.
    pub id: Uuid,
    /// The host (unique).
    pub host: String,
    /// The custom domain, `None` for one of `domains.acme.hosts`.
    pub domain_id: Option<Uuid>,
    /// The certificate chain, PEM, leaf first.
    pub chain_pem: String,
    /// The private key, PEM, sealed. Never serialized.
    #[serde(skip_serializing)]
    #[model(hidden)]
    pub private_key: String,
    /// The start of the leaf's validity.
    pub not_before: DateTime<Utc>,
    /// The end of the leaf's validity.
    pub not_after: DateTime<Utc>,
    /// When it was first issued.
    pub created_at: DateTime<Utc>,
    /// When it was last renewed.
    pub updated_at: DateTime<Utc>,
}

impl fmt::Debug for Certificate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Certificate")
            .field("id", &self.id)
            .field("host", &self.host)
            .field("domain_id", &self.domain_id)
            .field("not_before", &self.not_before)
            .field("not_after", &self.not_after)
            .finish_non_exhaustive()
    }
}

/// A row of `custom_domain_challenges`: an HTTP-01 challenge answer the
/// app serves at `/.well-known/acme-challenge/{token}` while an order is
/// open, on whichever server the CA reaches.
#[derive(Model, Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[model(table = "custom_domain_challenges")]
pub struct Challenge {
    /// The id.
    pub id: Uuid,
    /// The challenge token (unique).
    pub token: String,
    /// The host being validated.
    pub host: String,
    /// The answer.
    pub key_authorization: String,
    /// When the answer is dropped.
    pub expires_at: DateTime<Utc>,
    /// When it was published.
    pub created_at: DateTime<Utc>,
}

/// A row of `custom_domain_acme_accounts`: the ACME account of one
/// directory. Its credentials are sealed and never serialized.
#[derive(Model, Clone, Default, PartialEq, Eq, Serialize)]
#[model(table = "custom_domain_acme_accounts")]
pub struct AcmeAccount {
    /// The id.
    pub id: Uuid,
    /// The ACME directory URL (unique).
    pub directory_url: String,
    /// The account credentials (JSON), sealed. Never serialized.
    #[serde(skip_serializing)]
    #[model(hidden)]
    pub credentials: String,
    /// When the account was created.
    pub created_at: DateTime<Utc>,
    /// When the credentials last changed.
    pub updated_at: DateTime<Utc>,
}

impl fmt::Debug for AcmeAccount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AcmeAccount")
            .field("id", &self.id)
            .field("directory_url", &self.directory_url)
            .finish_non_exhaustive()
    }
}
