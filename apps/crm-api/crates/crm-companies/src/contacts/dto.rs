//! Contact request bodies and the list (and export) query, with their
//! validation rules.

use crm_core::ContactStatus;
use garde::Validate;
use keel::validation::{AsyncRule, AsyncRules};
use serde::Deserialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::common::{
    MAX_TEXT, blank_as_none, not_blank, nullable, nullable_text, optional_text, present,
    present_trimmed, trimmed,
};

/// The body of `POST /api/v1/contacts`. Text fields are trimmed; blank
/// optional fields are stored as `null`.
#[derive(Debug, Deserialize, Validate, ToSchema)]
#[schema(as = ContactInput)]
pub struct CreateContact {
    /// The first name (required).
    #[serde(default, deserialize_with = "trimmed")]
    #[garde(custom(not_blank), length(chars, max = MAX_TEXT))]
    pub first_name: String,
    /// The last name (required).
    #[serde(default, deserialize_with = "trimmed")]
    #[garde(custom(not_blank), length(chars, max = MAX_TEXT))]
    pub last_name: String,
    /// The email address.
    #[serde(default, deserialize_with = "optional_text")]
    #[garde(email, length(chars, max = MAX_TEXT))]
    pub email: Option<String>,
    /// A phone number.
    #[serde(default, deserialize_with = "optional_text")]
    #[garde(length(chars, max = MAX_TEXT))]
    pub phone: Option<String>,
    /// The job title.
    #[serde(default, deserialize_with = "optional_text")]
    #[garde(length(chars, max = MAX_TEXT))]
    pub title: Option<String>,
    /// The life-cycle status (default `lead`).
    #[serde(default)]
    #[garde(skip)]
    pub status: Option<ContactStatus>,
    /// The company they work at (must exist and not be deleted).
    #[serde(default)]
    #[garde(skip)]
    pub company_id: Option<Uuid>,
    /// The owner (default: the caller). Only users who can manage contacts
    /// may name someone else (spec §9.3; 403 otherwise).
    #[serde(default)]
    #[garde(skip)]
    pub owner_id: Option<Uuid>,
}

impl AsyncRules for CreateContact {
    fn async_rules(&self) -> Vec<AsyncRule> {
        vec![
            AsyncRule::exists("company_id", "companies", "id", self.company_id),
            AsyncRule::exists("owner_id", "users", "id", self.owner_id),
        ]
    }
}

/// The body of `PATCH /api/v1/contacts/{id}`: only the fields sent change;
/// `null` (or a blank string) clears an optional field.
#[derive(Debug, Default, Deserialize, Validate, ToSchema)]
#[schema(as = ContactUpdateInput)]
pub struct UpdateContact {
    /// The first name (cannot be cleared).
    #[serde(default, deserialize_with = "present_trimmed")]
    #[schema(nullable = false)]
    #[garde(inner(custom(not_blank)), length(chars, max = MAX_TEXT))]
    pub first_name: Option<String>,
    /// The last name (cannot be cleared).
    #[serde(default, deserialize_with = "present_trimmed")]
    #[schema(nullable = false)]
    #[garde(inner(custom(not_blank)), length(chars, max = MAX_TEXT))]
    pub last_name: Option<String>,
    /// The email address.
    #[serde(default, deserialize_with = "nullable_text")]
    #[garde(email, length(chars, max = MAX_TEXT))]
    #[schema(value_type = Option<String>)]
    pub email: Option<Option<String>>,
    /// A phone number.
    #[serde(default, deserialize_with = "nullable_text")]
    #[garde(length(chars, max = MAX_TEXT))]
    #[schema(value_type = Option<String>)]
    pub phone: Option<Option<String>>,
    /// The job title.
    #[serde(default, deserialize_with = "nullable_text")]
    #[garde(length(chars, max = MAX_TEXT))]
    #[schema(value_type = Option<String>)]
    pub title: Option<Option<String>>,
    /// The life-cycle status (cannot be cleared).
    #[serde(default, deserialize_with = "present")]
    #[garde(skip)]
    #[schema(nullable = false)]
    pub status: Option<ContactStatus>,
    /// The company (`null`: none).
    #[serde(default, deserialize_with = "nullable")]
    #[garde(skip)]
    #[schema(value_type = Option<Uuid>)]
    pub company_id: Option<Option<Uuid>>,
    /// The new owner: managers and admins only (spec §9.3; 403 otherwise).
    #[serde(default, deserialize_with = "present")]
    #[garde(skip)]
    #[schema(nullable = false)]
    pub owner_id: Option<Uuid>,
}

impl AsyncRules for UpdateContact {
    fn async_rules(&self) -> Vec<AsyncRule> {
        vec![
            AsyncRule::exists("company_id", "companies", "id", self.company_id.flatten()),
            AsyncRule::exists("owner_id", "users", "id", self.owner_id),
        ]
    }
}

/// The query of `GET /api/v1/contacts` (besides `page` and `per_page`) and
/// of `GET /api/v1/contacts/export`. Blank values count as left out.
#[derive(Debug, Default, Deserialize, Validate, ToSchema)]
pub struct ContactFilters {
    /// Words that must each appear in the first name, last name, email or
    /// title (case-insensitive).
    #[serde(default, deserialize_with = "blank_as_none")]
    #[garde(skip)]
    pub search: Option<String>,
    /// Only contacts at this company.
    #[serde(default, deserialize_with = "blank_as_none")]
    #[garde(skip)]
    pub company_id: Option<Uuid>,
    /// Only contacts with this status.
    #[serde(default, deserialize_with = "blank_as_none")]
    #[garde(skip)]
    pub status: Option<ContactStatus>,
    /// Only contacts this user owns.
    #[serde(default, deserialize_with = "blank_as_none")]
    #[garde(skip)]
    pub owner_id: Option<Uuid>,
}
