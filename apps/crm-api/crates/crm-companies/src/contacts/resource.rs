//! The API's contact shapes.

use chrono::{DateTime, Utc};
use crm_core::models::Contact;
use crm_core::subjects::CONTACT;
use crm_core::{CompanyRef, ContactStatus, OwnerRef};
use navree::api::ApiResource;
use navree::db::Page;
use navree::openapi::ToSchema;
use navree::orm::Record;
use navree::orm::attributes::Resource;
use serde::Serialize;
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
    /// The relations a contact's resource embeds: load them with the
    /// records (`Contact::query().with_all(ContactResource::EMBEDS)`).
    pub const EMBEDS: [&'static str; 2] = ["company", "owner"];
}

impl ApiResource for ContactResource {
    const TYPE: &'static str = CONTACT;
}

impl Resource<Contact> for ContactResource {
    /// `contact` with its loaded `company` (deleted or not) and `owner`.
    fn from_record(contact: &Record<Contact>) -> Self {
        ContactResource {
            resource_type: ContactType::Contact,
            id: contact.id,
            first_name: contact.first_name.clone(),
            last_name: contact.last_name.clone(),
            full_name: contact.full_name(),
            email: contact.email.clone(),
            phone: contact.phone.clone(),
            title: contact.title.clone(),
            status: contact.status,
            company_id: contact.company_id,
            company: contact
                .loaded(Contact::company)
                .and_then(Option::as_ref)
                .map(CompanyRef::from),
            owner_id: contact.owner_id,
            owner: OwnerRef::loaded(contact.loaded(Contact::owner), contact.owner_id),
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
