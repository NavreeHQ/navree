//! The small records the API embeds in its resources:
//! `owner: {id, name}`, `company: {id, name}`, `contact: {id, full_name}`.
//!
//! They come from relations eager loaded with the records, one query per
//! relation for a whole page, never one per row:
//!
//! ```no_run
//! # use crm_core::OwnerRef;
//! # use crm_core::models::Deal;
//! # use navree::db::Uow;
//! # use navree::orm::Model;
//! # async fn demo(uow: Uow) -> navree::orm::Result<()> {
//! let deals = Deal::query().with_all(["company", "contact", "owner"]).get(&uow).await?;
//! let owner = OwnerRef::loaded(deals[0].loaded(Deal::owner), deals[0].owner_id);
//! # let _ = owner;
//! # Ok(()) }
//! ```

use navree::openapi::ToSchema;
use navree::orm::Record;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::models::{Company, Contact, User};

/// A user as embedded in a record (`owner`, `assignee`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub struct OwnerRef {
    /// The user's id.
    pub id: Uuid,
    /// The user's name.
    pub name: String,
}

/// A company as embedded in a contact or deal.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub struct CompanyRef {
    /// The company's id.
    pub id: Uuid,
    /// The company's name.
    pub name: String,
}

/// A contact as embedded in a deal.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub struct ContactRef {
    /// The contact's id.
    pub id: Uuid,
    /// `"<first> <last>"`.
    pub full_name: String,
}

impl OwnerRef {
    /// The embed of user `id` from a record's loaded relation
    /// (`deal.loaded(Deal::owner)`). Foreign keys keep owners, so a user
    /// is never missing; if one were (or the relation was not loaded), the
    /// embed has an empty name rather than failing the page.
    pub fn loaded(user: Option<&Option<Record<User>>>, id: Uuid) -> Self {
        user.and_then(Option::as_ref).map_or_else(
            || OwnerRef {
                id,
                name: String::new(),
            },
            OwnerRef::from,
        )
    }
}

impl CompanyRef {
    /// The embed of company `id` from a record's loaded relation
    /// (`deal.loaded(Deal::company)`); an empty name if it is missing.
    pub fn loaded(company: Option<&Option<Record<Company>>>, id: Uuid) -> Self {
        company.and_then(Option::as_ref).map_or_else(
            || CompanyRef {
                id,
                name: String::new(),
            },
            CompanyRef::from,
        )
    }
}

impl From<&Record<User>> for OwnerRef {
    fn from(user: &Record<User>) -> Self {
        OwnerRef {
            id: user.id,
            name: user.name.clone(),
        }
    }
}

impl From<&Record<Company>> for CompanyRef {
    fn from(company: &Record<Company>) -> Self {
        CompanyRef {
            id: company.id,
            name: company.name.clone(),
        }
    }
}

impl From<&Record<Contact>> for ContactRef {
    fn from(contact: &Record<Contact>) -> Self {
        ContactRef {
            id: contact.id,
            full_name: contact.full_name(),
        }
    }
}
