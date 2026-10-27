//! The CRM's records as ORM models, each an access subject.
//!
//! | Model | Table | Subject | Permission prefix | Relations |
//! |---|---|---|---|---|
//! | [`Company`] | `companies` (soft deletes) | `Company` | `companies` | `owner`, `contacts`, `deals`, `open_deals`, `activities` |
//! | [`Contact`] | `contacts` (soft deletes) | `Contact` | `contacts` | `company`, `owner`, `deals`, `activities` |
//! | [`Deal`] | `deals` | `Deal` | `deals` | `company`, `contact`, `owner`, `activities` |
//! | [`Activity`] | `activities` (a time series) | `Activity` | `activities` | `subject` (company, contact or deal), `owner`, `assignee` |
//! | [`User`] | `users` (navree-auth's) | `User` | `users` | `roles` |
//! | [`Role`] | `roles` (navree-access's) | | | |
//!
//! Ids are UUID v7 (generated when a record is created, so they sort by
//! creation time), timestamps are UTC and kept by the ORM (`created_at`,
//! `updated_at`; `deleted_at` for soft deletes). Enums are stored as their
//! API strings (`#[model(cast = "string_enum")]`). Rule conditions name
//! these columns (`owner_id`, `assignee_id`, `stage`).
//!
//! Every model is an access subject (`#[model(subject)]`):
//! [`CrmCoreModule`](crate::CrmCoreModule) registers them, so
//! `#[can(Update, Deal, from = "id")]` routes load the record through the
//! model and hand it over as `Authorized<Deal, Update>`, and
//! `Accessible<Deal, Read>` filters a model query
//! (`deals.apply(Deal::query())`).
//!
//! Relations are eager loaded with the records that need them
//! (`Deal::query().with_all(["company", "contact", "owner"])`), one query
//! per relation for a whole page. Local scopes (`CompanyScopes`,
//! `ContactScopes`, `DealScopes`, `ActivityScopes`, `UserScopes`) name the
//! queries the CRM asks: `Deal::query().open().owned_by(user)`.
//!
//! Users and roles belong to navree-auth and navree-access: the CRM reads
//! them through [`User`] and [`Role`] (and renames users), while accounts
//! and role assignments are made through `AuthService` and
//! `AccessService`.

mod activity;
mod company;
mod contact;
mod deal;
mod role;
mod user;

pub use activity::{Activity, ActivityScopes};
pub use company::{Company, CompanyScopes};
pub use contact::{Contact, ContactScopes, full_name};
pub use deal::{Deal, DealScopes};
pub use role::Role;
pub use user::{User, UserScopes};
