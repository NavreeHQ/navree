//! # navis-audit
//!
//! `AuditModule`: an audit trail of data changes, sign-ins and permission
//! changes, in the table `audit_logs`.
//!
//! - **Data changes** made through a `navis_db::Uow` (with `navis_db::Repo`)
//!   are written as `data` entries by a `dyn ChangeObserver` inside the
//!   unit of work's transaction, so a rolled-back change leaves no audit
//!   row. Entries carry the unit of work's actor and request id, the
//!   subject type and id (`Deal`, `0199...`), the action
//!   (`deal.created`, `deal.updated`, `deal.deleted`) and the old and new
//!   values (only the changed fields for updates).
//! - **Security, sign-in and permission events** come in through
//!   `dyn AuditSink` (navis-access, navis-auth, navis-http's route `audit`
//!   actions) and are written on the module's own connection, outside the
//!   request transaction, so they survive its rollback.
//! - **Secrets never reach the table:** passwords, tokens, secrets and
//!   two-factor codes ([`DEFAULT_SKIP_FIELDS`]) plus configured fields are
//!   dropped from every entry; whole tables can be skipped.
//! - **Reading and retention:** `GET {api_prefix}/audit` lists entries
//!   newest first, filtered by `actor_id`, `subject_type`, `subject_id`,
//!   `action` and `kind`, paginated (permission `audit.read`); `audit:prune
//!   [--days N]` deletes entries older than the retention period (365 days
//!   by default).
//!
//! ```no_run
//! use navis_audit::AuditModule;
//! use navis_config::{ConfigModule, ConfigSources};
//! use navis_db::DatabaseModule;
//! use navis_kernel::Navis;
//!
//! # fn main() -> Result<(), navis_kernel::BoxError> {
//! Navis::app()
//!     .module(ConfigModule::for_root(ConfigSources::defaults()))
//!     .module(DatabaseModule::from_config())
//!     .module(AuditModule::from_config()) // section `audit`
//!     .run()
//! # }
//! ```
//!
//! A custom entry from a handler or service:
//!
//! ```ignore
//! use navis_contracts::{AuditEntry, AuditKind};
//!
//! // Outside the transaction (survives a rollback):
//! audit.record(AuditEntry::new(AuditKind::Custom, "deal.exported").actor(user_id)).await?;
//! // Inside the unit of work (commits or rolls back with it):
//! audit.record_in(&uow, AuditEntry::new(AuditKind::Custom, "deal.won").subject("Deal", id)).await?;
//! ```
#![forbid(unsafe_code)]

pub mod audit_log;
mod controller;
mod filter;
mod log;
mod migration;
mod module;
mod naming;
mod options;
mod policy;
mod schema;

pub use controller::AUDIT_READ_PERMISSION;
pub use filter::AuditFilter;
pub use log::{AuditError, AuditLog};
pub use module::AuditModule;
pub use naming::subject_type_of;
pub use options::{AuditOptions, DEFAULT_SKIP_FIELDS};
pub use policy::AUDIT_TABLE;

/// One stored audit entry (a row of `audit_logs`).
pub type AuditRecord = audit_log::Model;
