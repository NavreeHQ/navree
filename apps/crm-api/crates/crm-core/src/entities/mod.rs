//! The CRM's tables as SeaORM entities, each an access subject.
//!
//! | Entity | Table | Subject | Permission prefix |
//! |---|---|---|---|
//! | [`company`] | `companies` (soft deletes) | `Company` | `companies` |
//! | [`contact`] | `contacts` (soft deletes) | `Contact` | `contacts` |
//! | [`deal`] | `deals` | `Deal` | `deals` |
//! | [`activity`] | `activities` | `Activity` | `activities` |
//!
//! Ids are UUID v7 (`Uuid::now_v7()`), `owner_id` points at navree-auth's
//! `users`, timestamps are UTC. Rule conditions name these columns
//! (`owner_id`, `assignee_id`, `stage`). The type aliases ([`Company`],
//! [`Contact`], [`Deal`], [`Activity`], [`User`]) are the subjects in route
//! attributes and extractors: `#[can(Update, Deal, from = "id")]`,
//! `Authorized<Deal, Update>`, `Accessible<Company, Read>`.

pub mod activity;
pub mod company;
pub mod contact;
pub mod deal;

use navree::access::Subject;

use crate::subjects;

/// The `Company` subject (`companies`).
pub type Company = company::Entity;
/// The `Contact` subject (`contacts`).
pub type Contact = contact::Entity;
/// The `Deal` subject (`deals`).
pub type Deal = deal::Entity;
/// The `Activity` subject (`activities`).
pub type Activity = activity::Entity;
/// The `User` subject: navree-auth's `users` (permission prefix `users`).
pub type User = navree::auth::user::Entity;

impl Subject for company::Entity {
    const SUBJECT: &'static str = subjects::COMPANY;
    const PERMISSION_PREFIX: &'static str = "companies";
}

impl Subject for contact::Entity {
    const SUBJECT: &'static str = subjects::CONTACT;
    const PERMISSION_PREFIX: &'static str = "contacts";
}

impl Subject for deal::Entity {
    const SUBJECT: &'static str = subjects::DEAL;
    const PERMISSION_PREFIX: &'static str = "deals";
}

impl Subject for activity::Entity {
    const SUBJECT: &'static str = subjects::ACTIVITY;
    const PERMISSION_PREFIX: &'static str = "activities";
}
