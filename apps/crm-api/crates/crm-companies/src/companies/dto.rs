//! Company request bodies and the list query, with their validation rules.

use crm_core::CompanySize;
use garde::Validate;
use navis::validation::{AsyncRule, AsyncRules};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::common::{
    MAX_TEXT, blank_as_none, not_blank, nullable, nullable_text, optional_text, present,
    present_trimmed, trimmed,
};

/// The body of `POST /api/v1/companies`. Text fields are trimmed; blank
/// optional fields are stored as `null`.
#[derive(Debug, Deserialize, Validate, ToSchema)]
#[schema(as = CompanyInput)]
pub struct CreateCompany {
    /// The name (required).
    #[serde(default, deserialize_with = "trimmed")]
    #[garde(custom(not_blank), length(chars, max = MAX_TEXT))]
    pub name: String,
    /// The web domain, e.g. `acme.com`.
    #[serde(default, deserialize_with = "optional_text")]
    #[garde(length(chars, max = MAX_TEXT))]
    pub domain: Option<String>,
    /// The industry, free text.
    #[serde(default, deserialize_with = "optional_text")]
    #[garde(length(chars, max = MAX_TEXT))]
    pub industry: Option<String>,
    /// The head count bracket.
    #[serde(default)]
    #[garde(skip)]
    pub size: Option<CompanySize>,
    /// A phone number.
    #[serde(default, deserialize_with = "optional_text")]
    #[garde(length(chars, max = MAX_TEXT))]
    pub phone: Option<String>,
    /// The website URL.
    #[serde(default, deserialize_with = "optional_text")]
    #[garde(length(chars, max = MAX_TEXT))]
    pub website: Option<String>,
    /// The city.
    #[serde(default, deserialize_with = "optional_text")]
    #[garde(length(chars, max = MAX_TEXT))]
    pub city: Option<String>,
    /// The country.
    #[serde(default, deserialize_with = "optional_text")]
    #[garde(length(chars, max = MAX_TEXT))]
    pub country: Option<String>,
    /// The owner (default: the caller). Only users who can manage companies
    /// may name someone else (spec §9.3; 403 otherwise).
    #[serde(default)]
    #[garde(skip)]
    pub owner_id: Option<Uuid>,
}

impl AsyncRules for CreateCompany {
    fn async_rules(&self) -> Vec<AsyncRule> {
        vec![AsyncRule::exists("owner_id", "users", "id", self.owner_id)]
    }
}

/// The body of `PATCH /api/v1/companies/{id}`: only the fields sent
/// change; `null` (or a blank string) clears an optional field.
#[derive(Debug, Default, Deserialize, Validate, ToSchema)]
#[schema(as = CompanyUpdateInput)]
pub struct UpdateCompany {
    /// The name (cannot be cleared).
    #[serde(default, deserialize_with = "present_trimmed")]
    #[schema(nullable = false)]
    #[garde(inner(custom(not_blank)), length(chars, max = MAX_TEXT))]
    pub name: Option<String>,
    /// The web domain.
    #[serde(default, deserialize_with = "nullable_text")]
    #[garde(length(chars, max = MAX_TEXT))]
    #[schema(value_type = Option<String>)]
    pub domain: Option<Option<String>>,
    /// The industry.
    #[serde(default, deserialize_with = "nullable_text")]
    #[garde(length(chars, max = MAX_TEXT))]
    #[schema(value_type = Option<String>)]
    pub industry: Option<Option<String>>,
    /// The head count bracket.
    #[serde(default, deserialize_with = "nullable")]
    #[garde(skip)]
    #[schema(value_type = Option<CompanySize>)]
    pub size: Option<Option<CompanySize>>,
    /// A phone number.
    #[serde(default, deserialize_with = "nullable_text")]
    #[garde(length(chars, max = MAX_TEXT))]
    #[schema(value_type = Option<String>)]
    pub phone: Option<Option<String>>,
    /// The website URL.
    #[serde(default, deserialize_with = "nullable_text")]
    #[garde(length(chars, max = MAX_TEXT))]
    #[schema(value_type = Option<String>)]
    pub website: Option<Option<String>>,
    /// The city.
    #[serde(default, deserialize_with = "nullable_text")]
    #[garde(length(chars, max = MAX_TEXT))]
    #[schema(value_type = Option<String>)]
    pub city: Option<Option<String>>,
    /// The country.
    #[serde(default, deserialize_with = "nullable_text")]
    #[garde(length(chars, max = MAX_TEXT))]
    #[schema(value_type = Option<String>)]
    pub country: Option<Option<String>>,
    /// The new owner: managers and admins only (spec §9.3; 403 otherwise).
    #[serde(default, deserialize_with = "present")]
    #[garde(skip)]
    #[schema(nullable = false)]
    pub owner_id: Option<Uuid>,
}

impl AsyncRules for UpdateCompany {
    fn async_rules(&self) -> Vec<AsyncRule> {
        vec![AsyncRule::exists("owner_id", "users", "id", self.owner_id)]
    }
}

/// The order of `GET /api/v1/companies` (default `name`): by name
/// (case-insensitive) or creation time, `-` for descending.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
pub enum CompanySort {
    /// By name, A to Z.
    #[default]
    #[serde(rename = "name")]
    Name,
    /// By name, Z to A.
    #[serde(rename = "-name")]
    NameDesc,
    /// Oldest first.
    #[serde(rename = "created_at")]
    CreatedAt,
    /// Newest first.
    #[serde(rename = "-created_at")]
    CreatedAtDesc,
}

/// The query of `GET /api/v1/companies` (besides `page` and `per_page`).
/// Blank values count as left out.
#[derive(Debug, Default, Deserialize, Validate, ToSchema)]
pub struct CompanyFilters {
    /// Part of the name or domain (case-insensitive).
    #[serde(default, deserialize_with = "blank_as_none")]
    #[garde(skip)]
    pub search: Option<String>,
    /// Only companies this user owns.
    #[serde(default, deserialize_with = "blank_as_none")]
    #[garde(skip)]
    pub owner_id: Option<Uuid>,
    /// Only companies in this industry (case-insensitive).
    #[serde(default, deserialize_with = "blank_as_none")]
    #[garde(skip)]
    pub industry: Option<String>,
    /// The order.
    #[serde(default, deserialize_with = "blank_as_none")]
    #[garde(skip)]
    pub sort: Option<CompanySort>,
}
