//! # crm-api
//!
//! The Keel CRM API: companies, contacts, deals, activities and a
//! dashboard for a small sales team, built on the Keel framework (spec §6
//! and §7).
//!
//! [`app`] assembles it: the platform modules
//! ([`crm_core::platform`]), `CrmCoreModule` (tables, abilities, roles),
//! the feature modules (users, companies, contacts, deals, activities,
//! dashboard) and the `crm-demo` seeder ([`seed`]). The binary and the
//! tests both start from it:
//!
//! ```no_run
//! use keel::prelude::*;
//!
//! # fn main() -> Result<(), BoxError> {
//! crm_api::app(ConfigSources::defaults()).run()
//! # }
//! ```
//!
//! The crates under `crates/`: `crm-core` (shared entities, enums,
//! migrations, abilities, platform), `crm-testkit` (`TestCrm`),
//! `crm-users`, `crm-companies`, `crm-deals`, `crm-activities`.
#![forbid(unsafe_code)]

mod app;
pub mod seed;

pub use app::app;
