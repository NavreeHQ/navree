//! # crm-core
//!
//! What every Navree CRM crate shares, so the feature crates (companies and
//! contacts, deals, activities and the dashboard) build in parallel without
//! depending on each other:
//!
//! - [`entities`]: the SeaORM entities of every CRM table (`companies`,
//!   `contacts`, `deals`, `activities`), each an access subject, and the
//!   subject aliases [`Company`], [`Contact`], [`Deal`], [`Activity`],
//!   [`User`] for `#[can(..)]` and the access extractors;
//! - [`enums`]: [`CompanySize`], [`ContactStatus`], [`Stage`] (with
//!   `probability()` and `label()`), [`ActivityKind`], [`SubjectType`];
//! - [`migrations`]: the one `crm` migration set (after `auth` and
//!   `access`);
//! - [`abilities`]: who may do what (spec §6 and §9), [`roles`],
//!   [`permissions`] and the custom [`actions`] [`Move`], [`Export`] and
//!   [`Complete`];
//! - [`embeds`]: [`OwnerRef`], [`CompanyRef`], [`ContactRef`] and their
//!   batch loaders;
//! - [`ownership`]: [`owner_for`](ownership::owner_for), who may set a
//!   record's `owner_id` (spec §9.3);
//! - [`notify`]: [`NotifiableUser`](notify::NotifiableUser) and the
//!   notification [`NotificationData`](notify::NotificationData) shape
//!   (spec §9.7);
//! - [`seed`]: the roles seeder and the demo accounts;
//! - [`CrmCoreModule`] and [`platform`], the framework modules configured
//!   for the CRM.
#![forbid(unsafe_code)]

pub mod abilities;
pub mod actions;
pub mod embeds;
pub mod entities;
pub mod enums;
pub mod migrations;
pub mod notify;
pub mod ownership;
pub mod permissions;
pub mod roles;
pub mod seed;
pub mod subjects;

mod module;
mod platform;

pub use actions::{Complete, Export, Move};
pub use embeds::{CompanyRef, ContactRef, OwnerRef};
pub use entities::{Activity, Company, Contact, Deal, User};
pub use enums::{ActivityKind, CompanySize, ContactStatus, Stage, SubjectType};
pub use module::CrmCoreModule;
pub use notify::{NotifiableUser, NotificationData};
pub use ownership::owner_for;
pub use platform::{
    DEAL_UPDATED, DEALS_CHANNEL, MCP_INSTRUCTIONS, REALTIME_KEY, TRUSTED_PROXIES, platform,
};
pub use roles::UserRole;
