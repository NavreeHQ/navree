//! # navree-contracts
//!
//! Small cross-cutting traits and types shared by every module, so modules can
//! cooperate without depending on each other: [`Secret`], [`RequestId`],
//! [`Principal`] and [`AuthState`], the audit, event, broadcast, health and
//! session sinks, the job contract ([`Job`], [`Dispatcher`],
//! [`JobRegistration`], with progress and cancellation through
//! [`JobMonitor`]), permission checks outside routes ([`PermissionCheck`],
//! [`permitted`]), [`Clock`], the zone sources ([`ZoneSource`]), and the
//! conformity checks behind `navree check` ([`ConformityCheck`],
//! [`ConformityModule`]), and ending a user's tokens and grants
//! ([`CredentialRevoker`]).
//!
//! A module that needs, say, auditing resolves `Arc<dyn AuditSink>` (usually
//! with `AppContext::try_get`) instead of depending on navree-audit.
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod audit;
mod broadcast;
mod clock;
mod conformity;
mod events;
mod health;
mod jobs;
mod permissions;
mod principal;
mod redactor;
mod request_id;
mod revocation;
mod secret;
mod session;
mod zone;

pub use audit::{AuditEntry, AuditKind, AuditSink};
pub use broadcast::Broadcaster;
pub use clock::{Clock, FixedClock, SystemClock};
pub use conformity::{
    CONFORMITY_COMMAND, CheckContext, CheckOutcome, CheckReport, CheckStatus, ConformityCheck,
    ConformityModule, ConformityOptions, ConformityReport, Finding, ModuleBuilderConformityExt,
    Severity,
};
pub use events::EventSink;
pub use health::{HealthCheck, HealthStatus};
pub use jobs::{
    Backoff, Dispatcher, Job, JobCancelled, JobContext, JobEnvelope, JobMonitor, JobOptions,
    JobProgress, JobRegistration, ModuleBuilderJobsExt,
};
pub use navree_kernel::{BoxError, BoxFuture};
pub use permissions::{PermissionCheck, permitted};
pub use principal::{AuthState, AuthVia, Impersonator, Principal};
pub use redactor::Redactor;
pub use request_id::RequestId;
pub use revocation::{CredentialRevoker, ModuleBuilderRevocationExt, RevokedByKind};
pub use secret::Secret;
pub use session::{SessionRecord, SessionStore};
pub use zone::{ZoneLevel, ZoneOwner, ZoneRequest, ZoneSource};
