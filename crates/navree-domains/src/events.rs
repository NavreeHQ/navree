//! The domains module's events, emitted after their change commits when
//! the app has `EventsModule`. They carry the domain, so a listener (a
//! mail to the workspace's admins, an audit trail) needs no query.

use chrono::{DateTime, Utc};
use navree_events::Event;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::CustomDomain;

/// A workspace added a domain (pending until its DNS records are found;
/// verified at once inside the managed zone).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainAdded {
    /// The domain.
    pub domain: CustomDomain,
}

impl Event for DomainAdded {
    const NAME: &'static str = "domains.added";
}

/// A domain was verified: it now resolves its workspace and gets a
/// certificate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainVerified {
    /// The domain.
    pub domain: CustomDomain,
}

impl Event for DomainVerified {
    const NAME: &'static str = "domains.verified";
}

/// A domain stayed pending for `domains.verification.give_up_after_hours`
/// and is no longer checked on a schedule.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainVerificationFailed {
    /// The domain.
    pub domain: CustomDomain,
    /// What the last check found missing.
    pub reason: String,
}

impl Event for DomainVerificationFailed {
    const NAME: &'static str = "domains.verification_failed";
}

/// Another workspace proved control of a host, so this workspace's claim
/// on it was closed: a pending claim becomes `failed` (with the reason in
/// `last_error`), and a verified domain that moved through
/// `Domains::transfer` is deleted (its snapshot is still `verified`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainClaimedElsewhere {
    /// The claim that was closed, as it was when it closed.
    pub domain: CustomDomain,
    /// The workspace that proved control and now owns the host.
    pub claimed_by: Uuid,
}

impl Event for DomainClaimedElsewhere {
    const NAME: &'static str = "domains.claimed_elsewhere";
}

/// A domain was removed (by its workspace, or with the workspace).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainRemoved {
    /// The domain's id.
    pub domain_id: Uuid,
    /// Its workspace.
    pub workspace_id: Uuid,
    /// Its host.
    pub host: String,
}

impl Event for DomainRemoved {
    const NAME: &'static str = "domains.removed";
}

/// A certificate was issued or renewed through ACME.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainCertificateIssued {
    /// The host.
    pub host: String,
    /// The custom domain (`None` for one of `domains.acme.hosts`).
    pub domain_id: Option<Uuid>,
    /// When the new certificate expires.
    pub expires_at: DateTime<Utc>,
    /// A renewal, not the first certificate.
    pub renewal: bool,
}

impl Event for DomainCertificateIssued {
    const NAME: &'static str = "domains.certificate_issued";
}

/// A certificate expires within `domains.certificates.warn_days` (ACME
/// renewals keep failing, or the on-demand proxy has not renewed it).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainCertificateExpiring {
    /// The host.
    pub host: String,
    /// The custom domain (`None` for one of `domains.acme.hosts`).
    pub domain_id: Option<Uuid>,
    /// When the certificate expires.
    pub expires_at: DateTime<Utc>,
    /// Whole days left (0 once expired).
    pub days_left: i64,
}

impl Event for DomainCertificateExpiring {
    const NAME: &'static str = "domains.certificate_expiring";
}
