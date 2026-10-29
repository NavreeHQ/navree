//! # navree-audit
//!
//! `AuditModule`: an audit trail of data changes, sign-ins and permission
//! changes, in the table `audit_logs`.
//!
//! - **Data changes** made through a `navree_db::Uow` (navree-orm models
//!   saved in it, navree-db's recorded record writes, or `navree_db::Repo`)
//!   are written as `data` entries by a `dyn ChangeObserver`
//!   (`observe`) inside the unit of work's transaction, so a
//!   rolled-back change leaves no audit row. Entries carry the unit of
//!   work's actor and request id, the subject type and id (`Deal`,
//!   `0199...`), the action (`deal.created`, `deal.updated`,
//!   `deal.deleted`) and the old and new values (only the changed fields
//!   for updates).
//! - **Security, sign-in and permission events** come in through
//!   `dyn AuditSink` (navree-access, navree-auth, navree-http's route `audit`
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
//! - **Entries are models:** [`AuditRecord`] is a navree-orm model, so code
//!   reads the trail with the query builder (`AuditRecord::query()`, or an
//!   [`AuditFilter`]'s [`query`](AuditFilter::query)).
//!
//! ```no_run
//! use navree_audit::AuditModule;
//! use navree_config::{ConfigModule, ConfigSources};
//! use navree_db::DatabaseModule;
//! use navree_kernel::Navree;
//!
//! # fn main() -> Result<(), navree_kernel::BoxError> {
//! Navree::app()
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
//! use navree_contracts::{AuditEntry, AuditKind};
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

/// One stored audit entry (a row of `audit_logs`): a navree-orm model
/// (`AuditRecord::query()`, typed columns such as `AuditRecord::ACTION`),
/// written as a navree-db record (see [`audit_log`]).
pub type AuditRecord = audit_log::Model;
