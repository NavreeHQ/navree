//! The API's company shapes (spec §7 `Company`).

use chrono::{DateTime, Utc};
use crm_core::entities::company;
use crm_core::{CompanySize, OwnerRef};
use keel::db::Page;
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::common::PageInfo;

/// The CASL subject type of a [`CompanyResource`] (`"Company"`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, ToSchema)]
pub enum CompanyType {
    /// `"Company"`.
    #[default]
    Company,
}

/// A company as the API returns it (schema `Company`):
/// `{__type: "Company", id, name, domain, industry, size, phone, website,
/// city, country, owner_id, owner: {id, name}, created_at, updated_at}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[schema(as = Company)]
pub struct CompanyResource {
    // Always `"Company"` (the CASL subject type). A plain comment: a doc
    // comment would wrap the inline enum schema in an `allOf`.
    #[serde(rename = "__type")]
    #[schema(inline)]
    pub resource_type: CompanyType,
    /// The id (UUID v7).
    pub id: Uuid,
    /// The name.
    pub name: String,
    /// The web domain, e.g. `acme.com`.
    pub domain: Option<String>,
    /// The industry.
    pub industry: Option<String>,
    /// The head count bracket.
    pub size: Option<CompanySize>,
    /// A phone number.
    pub phone: Option<String>,
    /// The website URL.
    pub website: Option<String>,
    /// The city.
    pub city: Option<String>,
    /// The country.
    pub country: Option<String>,
    /// The owner's id.
    pub owner_id: Uuid,
    /// The owner.
    pub owner: OwnerRef,
    /// When it was created (UTC).
    pub created_at: DateTime<Utc>,
    /// When it last changed (UTC).
    pub updated_at: DateTime<Utc>,
}

impl CompanyResource {
    /// `company` with its `owner` (`None` only if the owner row is gone:
    /// then the owner embed has an empty name).
    pub fn new(company: company::Model, owner: Option<OwnerRef>) -> Self {
        let owner = owner.unwrap_or_else(|| OwnerRef {
            id: company.owner_id,
            name: String::new(),
        });
        CompanyResource {
            resource_type: CompanyType::Company,
            id: company.id,
            name: company.name,
            domain: company.domain,
            industry: company.industry,
            size: company.size,
            phone: company.phone,
            website: company.website,
            city: company.city,
            country: company.country,
            owner_id: company.owner_id,
            owner,
            created_at: company.created_at,
            updated_at: company.updated_at,
        }
    }
}

/// `GET /api/v1/companies/{id}`: the company plus its live contacts and
/// open (not won or lost) deals.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct CompanyDetail {
    /// The company.
    #[serde(flatten)]
    pub company: CompanyResource,
    /// Contacts at the company (deleted ones left out).
    pub contacts_count: u64,
    /// Deals still open (stage neither `won` nor `lost`).
    pub open_deals_count: u64,
    /// The sum of the open deals' `amount_cents`, across currencies.
    pub open_deals_amount_cents: i64,
}

/// A page of companies: `{data: Company[], meta}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct CompanyPage {
    /// The companies on this page.
    pub data: Vec<CompanyResource>,
    // The pagination metadata (a plain comment, see `__type`).
    #[schema(inline)]
    pub meta: PageInfo,
}

impl From<Page<CompanyResource>> for CompanyPage {
    fn from(page: Page<CompanyResource>) -> Self {
        CompanyPage {
            data: page.data,
            meta: page.meta.into(),
        }
    }
}
