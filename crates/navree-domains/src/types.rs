//! The public shapes: [`CustomDomain`], its statuses, the DNS records a
//! customer creates, [`NewDomain`] and the summaries of the commands.

use chrono::{DateTime, Utc};
use navree_openapi::ToSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Where a domain is in its verification.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DomainStatus {
    /// Waiting for its DNS records; checked every
    /// `domains.verification.interval_mins`.
    #[default]
    Pending,
    /// Its TXT record was found (and it points at the app, when
    /// required): it resolves workspaces and gets certificates.
    Verified,
    /// Still pending `domains.verification.give_up_after_hours` after it
    /// was added, or closed because another workspace proved control of
    /// the host first (`last_error` says which); no longer checked on a
    /// schedule, never in the way of another workspace, and deleted
    /// after `domains.verification.failed_retention_hours`.
    Failed,
}

impl DomainStatus {
    /// `pending`, `verified` or `failed`.
    pub fn as_str(self) -> &'static str {
        match self {
            DomainStatus::Pending => "pending",
            DomainStatus::Verified => "verified",
            DomainStatus::Failed => "failed",
        }
    }

    /// The status a column holds (`pending` for anything unknown).
    pub fn parse(value: &str) -> DomainStatus {
        match value {
            "verified" => DomainStatus::Verified,
            "failed" => DomainStatus::Failed,
            _ => DomainStatus::Pending,
        }
    }
}

/// Where a domain's certificate is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TlsStatus {
    /// Not verified yet, or (ACME) waiting for its certificate.
    #[default]
    Pending,
    /// Served over HTTPS: an issued certificate (ACME), or a verified
    /// domain the on-demand proxy gets a certificate for.
    Active,
    /// The last issuance (ACME) or probe failed (`tls_error` says why).
    Failed,
    /// `domains.tls = "off"`.
    Off,
}

impl TlsStatus {
    /// `pending`, `active`, `failed` or `off`.
    pub fn as_str(self) -> &'static str {
        match self {
            TlsStatus::Pending => "pending",
            TlsStatus::Active => "active",
            TlsStatus::Failed => "failed",
            TlsStatus::Off => "off",
        }
    }

    /// The status a column holds (`pending` for anything unknown).
    pub fn parse(value: &str) -> TlsStatus {
        match value {
            "active" => TlsStatus::Active,
            "failed" => TlsStatus::Failed,
            "off" => TlsStatus::Off,
            _ => TlsStatus::Pending,
        }
    }
}

/// A DNS record the domain's owner creates.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct DnsInstruction {
    /// `TXT`, `CNAME`, `A` or `AAAA`.
    #[serde(rename = "type")]
    pub kind: String,
    /// The record's full name.
    pub name: String,
    /// Its value.
    pub value: String,
    /// `verification` (the TXT record) or `routing` (where the domain
    /// points).
    pub purpose: String,
}

/// A custom domain, as the routes and the service return it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CustomDomain {
    /// The id.
    pub id: Uuid,
    /// The workspace it belongs to.
    pub workspace_id: Uuid,
    /// The host (`crm.acme-corp.com`), lower-case ASCII.
    pub host: String,
    /// What the workspace uses it for (`app` by default).
    pub purpose: String,
    /// Where it is in its verification.
    pub status: DomainStatus,
    /// The workspace's primary domain.
    pub primary: bool,
    /// Created and verified through the DNS provider.
    pub managed: bool,
    /// Whether it points at the app (`None`: not checked).
    pub points_to_app: Option<bool>,
    /// Where its certificate is.
    pub tls: TlsStatus,
    /// When its certificate expires, when known.
    pub certificate_expires_at: Option<DateTime<Utc>>,
    /// Why the last issuance or probe failed.
    pub tls_error: Option<String>,
    /// What the last check found missing.
    pub last_error: Option<String>,
    /// When DNS was last checked.
    pub last_checked_at: Option<DateTime<Utc>>,
    /// When the scheduler checks it next.
    pub next_check_at: Option<DateTime<Utc>>,
    /// When it was verified.
    pub verified_at: Option<DateTime<Utc>>,
    /// The records to create: the TXT record, then the CNAME (or A and
    /// AAAA records for an apex domain).
    pub dns_records: Vec<DnsInstruction>,
    /// Who added it.
    pub created_by: Option<Uuid>,
    /// When it was added.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

impl CustomDomain {
    /// Whether the domain is verified.
    pub fn is_verified(&self) -> bool {
        self.status == DomainStatus::Verified
    }
}

/// A domain to add (`POST /api/domains`, [`Domains::add`](crate::Domains::add)).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NewDomain {
    /// The domain (`crm.acme-corp.com`); normalized before it is stored.
    pub host: String,
    /// What the workspace uses it for (default `app`; lower-case letters,
    /// digits, `-` and `_`, at most 32).
    #[serde(default)]
    pub purpose: Option<String>,
    /// Who adds it (the routes set the signed-in user).
    #[serde(skip)]
    pub created_by: Option<Uuid>,
}

impl NewDomain {
    /// The domain `host` for the `app` purpose.
    pub fn new(host: impl Into<String>) -> NewDomain {
        NewDomain {
            host: host.into(),
            purpose: None,
            created_by: None,
        }
    }

    /// For `purpose` (`links`, `landing`).
    #[must_use]
    pub fn purpose(mut self, purpose: impl Into<String>) -> NewDomain {
        self.purpose = Some(purpose.into());
        self
    }

    /// Added by `user`.
    #[must_use]
    pub fn created_by(mut self, user: Uuid) -> NewDomain {
        self.created_by = Some(user);
        self
    }
}

/// What `Domains::check_certificates` did.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CertificateRun {
    /// Hosts that got their first certificate.
    pub issued: Vec<String>,
    /// Hosts whose certificate was renewed.
    pub renewed: Vec<String>,
    /// Hosts whose certificate was read over TLS (on-demand), with its
    /// expiry.
    pub probed: Vec<(String, DateTime<Utc>)>,
    /// Hosts whose issuance or probe failed, with the reason.
    pub failed: Vec<(String, String)>,
    /// Hosts whose certificate expires within
    /// `domains.certificates.warn_days`, with its expiry.
    pub expiring: Vec<(String, DateTime<Utc>)>,
}
