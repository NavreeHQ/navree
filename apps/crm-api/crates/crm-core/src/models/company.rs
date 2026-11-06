//! [`Company`]: the organisations the team sells to (`companies`).

use chrono::{DateTime, Utc};
use navree::orm::prelude::*;
use serde::Serialize;
use uuid::Uuid;

use super::{Activity, Contact, Deal, DealScopes, User};
use crate::enums::CompanySize;
use crate::factories::CompanyFactory;

/// A company. Soft-deleted: deleting one sets `deleted_at`, and every query
/// leaves it out unless it asks `with_trashed()` (the embeds of its
/// contacts and deals still name it).
#[derive(Model, Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[model(
    table = "companies",
    timestamps,
    soft_deletes,
    subject,
    morph_name = "company",
    factory = CompanyFactory
)]
pub struct Company {
    /// A UUID v7, generated when the company is created.
    pub id: Uuid,
    /// The name (required).
    pub name: String,
    /// The web domain, e.g. `agra.com`.
    pub domain: Option<String>,
    /// The industry, free text.
    pub industry: Option<String>,
    /// The head count bracket.
    #[model(cast = "string_enum")]
    pub size: Option<CompanySize>,
    /// A phone number.
    pub phone: Option<String>,
    /// The website URL.
    pub website: Option<String>,
    /// The city.
    pub city: Option<String>,
    /// The country.
    pub country: Option<String>,
    /// The user who owns the account.
    pub owner_id: Uuid,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
    /// When it was deleted; `None` while it exists.
    pub deleted_at: Option<DateTime<Utc>>,
}

#[relations]
impl Company {
    /// The user who owns the account.
    pub fn owner(&self) -> BelongsTo<Company, User> {
        self.belongs_to(Company::OWNER_ID)
    }

    /// The people working there (deleted ones left out).
    pub fn contacts(&self) -> HasMany<Company, Contact> {
        self.has_many(Contact::COMPANY_ID)
    }

    /// Its deals.
    pub fn deals(&self) -> HasMany<Company, Deal> {
        self.has_many(Deal::COMPANY_ID)
    }

    /// Its deals still open: neither won nor lost.
    pub fn open_deals(&self) -> HasMany<Company, Deal> {
        self.has_many(Deal::COMPANY_ID).open()
    }

    /// The notes, calls, emails, meetings and tasks attached to it.
    pub fn activities(&self) -> HasMany<Company, Activity> {
        self.morph_many("subject")
    }
}

#[scopes]
impl Company {
    /// Companies whose name or domain contains `term`, ignoring case
    /// (non-ASCII letters too, on every database).
    #[scope]
    pub fn search(query: Builder<Self>, term: &str) -> Builder<Self> {
        query.where_(
            Company::NAME
                .contains_ignore_case(term)
                .or(Company::DOMAIN.contains_ignore_case(term)),
        )
    }

    /// Companies in `industry`, ignoring case.
    #[scope]
    pub fn in_industry(query: Builder<Self>, industry: &str) -> Builder<Self> {
        query.where_(Company::INDUSTRY.eq_ignore_case(industry))
    }

    /// Companies `user` owns.
    #[scope]
    pub fn owned_by(query: Builder<Self>, user: Uuid) -> Builder<Self> {
        query.where_(Company::OWNER_ID.eq(user))
    }
}
