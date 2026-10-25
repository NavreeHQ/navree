//! The API's contact shapes (spec §7 `Contact`).

use chrono::{DateTime, Utc};
use crm_core::entities::contact;
use crm_core::{CompanyRef, ContactStatus, OwnerRef};
use keel::db::Page;
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::common::PageInfo;

/// The CASL subject type of a [`ContactResource`] (`"Contact"`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, ToSchema)]
pub enum ContactType {
    /// `"Contact"`.
    #[default]
    Contact,
}

/// A contact as the API returns it (schema `Contact`):
/// `{__type: "Contact", id, first_name, last_name, full_name, email, phone,
/// title, status, company_id, company: {id, name} | null, owner_id,
/// owner: {id, name}, created_at, updated_at}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[schema(as = Contact)]
pub struct ContactResource {
    // Always `"Contact"` (the CASL subject type). A plain comment: a doc
    // comment would wrap the inline enum schema in an `allOf`.
    #[serde(rename = "__type")]
    #[schema(inline)]
    pub resource_type: ContactType,
    /// The id (UUID v7).
    pub id: Uuid,
    /// The first name.
    pub first_name: String,
    /// The last name.
    pub last_name: String,
    /// `"<first> <last>"`.
    pub full_name: String,
    /// The email address.
    pub email: Option<String>,
    /// A phone number.
    pub phone: Option<String>,
    /// The job title.
    pub title: Option<String>,
    /// The life-cycle status.
    pub status: ContactStatus,
    /// The company's id.
    pub company_id: Option<Uuid>,
    /// The company they work at.
    pub company: Option<CompanyRef>,
    /// The owner's id.
    pub owner_id: Uuid,
    /// The owner.
    pub owner: OwnerRef,
    /// When it was created (UTC).
    pub created_at: DateTime<Utc>,
    /// When it last changed (UTC).
    pub updated_at: DateTime<Utc>,
}

impl ContactResource {
    /// `contact` with its `company` and `owner` embeds (an owner row that
    /// is gone gives an owner embed with an empty name).
    pub fn new(
        contact: contact::Model,
        company: Option<CompanyRef>,
        owner: Option<OwnerRef>,
    ) -> Self {
        let owner = owner.unwrap_or_else(|| OwnerRef {
            id: contact.owner_id,
            name: String::new(),
        });
        ContactResource {
            resource_type: ContactType::Contact,
            id: contact.id,
            full_name: contact.full_name(),
            first_name: contact.first_name,
            last_name: contact.last_name,
            email: contact.email,
            phone: contact.phone,
            title: contact.title,
            status: contact.status,
            company_id: contact.company_id,
            company,
            owner_id: contact.owner_id,
            owner,
            created_at: contact.created_at,
            updated_at: contact.updated_at,
        }
    }
}

/// A page of contacts: `{data: Contact[], meta}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct ContactPage {
    /// The contacts on this page.
    pub data: Vec<ContactResource>,
    // The pagination metadata (a plain comment, see `__type`).
    #[schema(inline)]
    pub meta: PageInfo,
}

impl From<Page<ContactResource>> for ContactPage {
    fn from(page: Page<ContactResource>) -> Self {
        ContactPage {
            data: page.data,
            meta: page.meta.into(),
        }
    }
}
