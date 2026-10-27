//! [`Contact`]: people, optionally at a company (`contacts`).

use chrono::{DateTime, Utc};
use navree::orm::Expr;
use navree::orm::prelude::*;
use serde::Serialize;
use uuid::Uuid;

use super::{Activity, Company, Deal, User};
use crate::enums::ContactStatus;
use crate::factories::ContactFactory;

/// The most words of a search that are matched (the rest are ignored).
const MAX_SEARCH_TERMS: usize = 5;

/// A contact. Soft-deleted like companies: deleting one sets `deleted_at`,
/// and queries leave it out unless they ask `with_trashed()`.
#[derive(Model, Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[model(
    table = "contacts",
    timestamps,
    soft_deletes,
    subject,
    morph_name = "contact",
    factory = ContactFactory
)]
pub struct Contact {
    /// A UUID v7, generated when the contact is created.
    pub id: Uuid,
    /// The first name (required).
    pub first_name: String,
    /// The last name (required).
    pub last_name: String,
    /// The email address.
    pub email: Option<String>,
    /// A phone number.
    pub phone: Option<String>,
    /// The job title.
    pub title: Option<String>,
    /// The life-cycle status (default `lead`).
    #[model(cast = "string_enum")]
    pub status: ContactStatus,
    /// The company they work at.
    pub company_id: Option<Uuid>,
    /// The user who owns the contact.
    pub owner_id: Uuid,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
    /// When it was deleted; `None` while it exists.
    pub deleted_at: Option<DateTime<Utc>>,
}

impl Contact {
    /// `"<first> <last>"`: the API's `full_name`.
    pub fn full_name(&self) -> String {
        full_name(&self.first_name, &self.last_name)
    }
}

/// `"<first> <last>"`, trimmed (either part may be empty).
pub fn full_name(first_name: &str, last_name: &str) -> String {
    format!("{} {}", first_name.trim(), last_name.trim())
        .trim()
        .to_owned()
}

#[relations]
impl Contact {
    /// The company they work at, deleted or not: a contact keeps naming
    /// its company after the company is deleted.
    pub fn company(&self) -> BelongsTo<Contact, Company> {
        self.belongs_to(Contact::COMPANY_ID).with_trashed()
    }

    /// The user who owns the contact.
    pub fn owner(&self) -> BelongsTo<Contact, User> {
        self.belongs_to(Contact::OWNER_ID)
    }

    /// The deals where this is the main contact.
    pub fn deals(&self) -> HasMany<Contact, Deal> {
        self.has_many(Deal::CONTACT_ID)
    }

    /// The notes, calls, emails, meetings and tasks attached to them.
    pub fn activities(&self) -> HasMany<Contact, Activity> {
        self.morph_many("subject")
    }
}

#[scopes]
impl Contact {
    /// Contacts matching every word of `text` (the first five): each word
    /// must be in the first name, last name, email or title, ignoring
    /// case (non-ASCII letters too).
    #[scope]
    pub fn search(query: Builder<Self>, text: &str) -> Builder<Self> {
        text.split_whitespace()
            .take(MAX_SEARCH_TERMS)
            .fold(query, |query, word| {
                query.where_(
                    Contact::FIRST_NAME
                        .contains_ignore_case(word)
                        .or(Contact::LAST_NAME.contains_ignore_case(word))
                        .or(Contact::EMAIL.contains_ignore_case(word))
                        .or(Contact::TITLE.contains_ignore_case(word)),
                )
            })
    }

    /// Contacts at `company`.
    #[scope]
    pub fn at_company(query: Builder<Self>, company: Uuid) -> Builder<Self> {
        query.where_(Contact::COMPANY_ID.eq(company))
    }

    /// Contacts with `status`.
    #[scope]
    pub fn with_status(query: Builder<Self>, status: ContactStatus) -> Builder<Self> {
        query.where_(Contact::STATUS.eq(status))
    }

    /// Contacts `user` owns.
    #[scope]
    pub fn owned_by(query: Builder<Self>, user: Uuid) -> Builder<Self> {
        query.where_(Contact::OWNER_ID.eq(user))
    }

    /// By last name, then first name (ignoring ASCII case), then id.
    #[scope]
    pub fn alphabetical(query: Builder<Self>) -> Builder<Self> {
        query
            .order_by(Expr::lower(Contact::LAST_NAME))
            .order_by(Expr::lower(Contact::FIRST_NAME))
            .order_by(Contact::ID)
    }
}
