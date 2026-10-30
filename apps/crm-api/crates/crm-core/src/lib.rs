//! # crm-core
//!
//! What every Navree CRM crate shares, so the feature crates (companies and
//! contacts, deals, activities and the dashboard) need not depend on each
//! other:
//!
//! - [`models`]: the ORM models of every CRM table ([`Company`],
//!   [`Contact`], [`Deal`], [`Activity`]) and of the users and roles the
//!   CRM reads ([`User`], [`models::Role`]), with their relations and
//!   scopes. Each is an access subject, named as such in `#[can(..)]` and
//!   the access extractors (`Authorized<Deal, Update>`,
//!   `Accessible<Company, Read>`);
//! - [`factories`]: their model factories, for tests and the demo seeder;
//! - [`migrations`]: the one `crm` migration set, written with the schema
//!   builder (after `auth` and `access`): the tables, and the activity
//!   timeline as a time series (a TimescaleDB hypertable);
//! - [`enums`]: [`CompanySize`], [`ContactStatus`], [`Stage`] (with
//!   `probability()` and `label()`), [`ActivityKind`], [`SubjectType`];
//! - [`abilities`]: who may do what, [`roles`],
//!   [`permissions`] and the custom [`actions`] [`Move`], [`Export`] and
//!   [`Complete`];
//! - [`embeds`]: [`OwnerRef`], [`CompanyRef`], [`ContactRef`], built from
//!   eager loaded relations;
//! - [`ownership`]: [`owner_for`], who may set a record's `owner_id`;
//! - [`notify`]: [`NotifiableUser`] and the notification
//!   [`NotificationData`] shape;
//! - [`seed`]: the roles seeder and the demo accounts;
//! - [`CrmCoreModule`] and [`platform`], the framework modules configured
//!   for the CRM.
#![forbid(unsafe_code)]

pub mod abilities;
pub mod actions;
pub mod embeds;
pub mod enums;
pub mod factories;
pub mod migrations;
pub mod models;
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
pub use enums::{ActivityKind, CompanySize, ContactStatus, Stage, SubjectType};
pub use factories::{ActivityFactory, CompanyFactory, ContactFactory, DealFactory};
pub use models::{Activity, Company, Contact, Deal, User};
pub use module::CrmCoreModule;
pub use notify::{NotifiableUser, NotificationData};
pub use ownership::owner_for;
pub use platform::{
    DEAL_UPDATED, DEALS_CHANNEL, MCP_INSTRUCTIONS, REALTIME_KEY, TRUSTED_PROXIES, platform,
};
pub use roles::UserRole;
