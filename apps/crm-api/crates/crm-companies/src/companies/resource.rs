//! The API's company shapes.

use chrono::{DateTime, Utc};
use crm_core::models::Company;
use crm_core::subjects::COMPANY;
use crm_core::{CompanySize, OwnerRef};
use navree::api::ApiResource;
use navree::db::Page;
use navree::openapi::ToSchema;
use navree::orm::attributes::Resource;
use navree::orm::{OrmError, Record};
use serde::Serialize;
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
    /// The web domain, e.g. `agra.com`.
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
    /// The relations a company's resource embeds: load them with the
    /// records (`Company::query().with_all(CompanyResource::EMBEDS)`).
    pub const EMBEDS: [&'static str; 1] = ["owner"];
}

impl ApiResource for CompanyResource {
    const TYPE: &'static str = COMPANY;
}

impl Resource<Company> for CompanyResource {
    /// `company` with its loaded `owner`.
    fn from_record(company: &Record<Company>) -> Self {
        CompanyResource {
            resource_type: CompanyType::Company,
            id: company.id,
            name: company.name.clone(),
            domain: company.domain.clone(),
            industry: company.industry.clone(),
            size: company.size,
            phone: company.phone.clone(),
            website: company.website.clone(),
            city: company.city.clone(),
            country: company.country.clone(),
            owner_id: company.owner_id,
            owner: OwnerRef::loaded(company.loaded(Company::owner), company.owner_id),
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

impl CompanyDetail {
    /// The detail of `company`, loaded with its `owner`, the counts of its
    /// `contacts` and `open_deals` (`load_count`) and the sum of the open
    /// deals' `amount_cents` (`load_sum`).
    pub fn from_record(company: &Record<Company>) -> Result<Self, OrmError> {
        let count = |relation: &str| -> Result<u64, OrmError> {
            let count = company.extra::<i64>(&format!("{relation}_count"))?;
            Ok(count.map_or(0, |count| u64::try_from(count).unwrap_or_default()))
        };
        Ok(CompanyDetail {
            company: CompanyResource::from_record(company),
            contacts_count: count("contacts")?,
            open_deals_count: count("open_deals")?,
            // No open deals: the sum is `NULL`, shown as zero.
            open_deals_amount_cents: company
                .extra::<Option<i64>>("open_deals_sum_amount_cents")?
                .flatten()
                .unwrap_or(0),
        })
    }
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
