//! Contact request bodies and the list (and export) query, with their
//! validation rules.

use crm_core::ContactStatus;
use crm_core::models::{Company, Contact, User};
use navree::openapi::ToSchema;
use navree::orm::list::{Field, ListResource, ListSpec};
use navree::orm::validation::exists;
use navree::validation::Validate;
use navree::validation::{AsyncRule, AsyncRules};
use serde::Deserialize;
use uuid::Uuid;

use crate::common::{
    MAX_TEXT, not_blank, nullable, nullable_text, optional_text, present, present_trimmed,
    set_some, trimmed,
};

/// The body of `POST /api/v1/contacts`. Text fields are trimmed; blank
/// optional fields are stored as `null`.
#[derive(Debug, Deserialize, Validate, ToSchema)]
#[schema(as = ContactInput)]
pub struct CreateContact {
    /// The first name (required).
    #[serde(default, deserialize_with = "trimmed")]
    #[validate(custom(not_blank), length(max = MAX_TEXT))]
    pub first_name: String,
    /// The last name (required).
    #[serde(default, deserialize_with = "trimmed")]
    #[validate(custom(not_blank), length(max = MAX_TEXT))]
    pub last_name: String,
    /// The email address.
    #[serde(default, deserialize_with = "optional_text")]
    #[validate(email, length(max = MAX_TEXT))]
    pub email: Option<String>,
    /// A phone number.
    #[serde(default, deserialize_with = "optional_text")]
    #[validate(length(max = MAX_TEXT))]
    pub phone: Option<String>,
    /// The job title.
    #[serde(default, deserialize_with = "optional_text")]
    #[validate(length(max = MAX_TEXT))]
    pub title: Option<String>,
    /// The life-cycle status (default `lead`).
    #[serde(default)]
    #[validate(skip)]
    pub status: Option<ContactStatus>,
    /// The company they work at (must exist and not be deleted).
    #[serde(default)]
    #[validate(skip)]
    pub company_id: Option<Uuid>,
    /// The owner (default: the caller). Only users who can manage contacts
    /// may name someone else (403 otherwise).
    #[serde(default)]
    #[validate(skip)]
    pub owner_id: Option<Uuid>,
}

impl CreateContact {
    /// The new contact, owned by `owner_id`.
    pub fn into_contact(self, owner_id: Uuid) -> Contact {
        Contact {
            first_name: self.first_name,
            last_name: self.last_name,
            email: self.email,
            phone: self.phone,
            title: self.title,
            status: self.status.unwrap_or_default(),
            company_id: self.company_id,
            owner_id,
            ..Contact::default()
        }
    }
}

/// `company_id` must name a company; a deleted one passes here and gets its
/// 400 from the service, which tells a deleted company from a new one.
fn company_exists(company_id: Option<Uuid>) -> AsyncRule {
    exists(Company::ID, company_id)
        .field("company_id")
        .with_trashed()
        .into()
}

impl AsyncRules for CreateContact {
    fn async_rules(&self) -> Vec<AsyncRule> {
        vec![
            company_exists(self.company_id),
            exists(User::ID, self.owner_id).field("owner_id").into(),
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
    #[validate(custom(not_blank), length(max = MAX_TEXT))]
    pub first_name: Option<String>,
    /// The last name (cannot be cleared).
    #[serde(default, deserialize_with = "present_trimmed")]
    #[schema(nullable = false)]
    #[validate(custom(not_blank), length(max = MAX_TEXT))]
    pub last_name: Option<String>,
    /// The email address.
    #[serde(default, deserialize_with = "nullable_text")]
    #[validate(email, length(max = MAX_TEXT))]
    #[schema(value_type = Option<String>)]
    pub email: Option<Option<String>>,
    /// A phone number.
    #[serde(default, deserialize_with = "nullable_text")]
    #[validate(length(max = MAX_TEXT))]
    #[schema(value_type = Option<String>)]
    pub phone: Option<Option<String>>,
    /// The job title.
    #[serde(default, deserialize_with = "nullable_text")]
    #[validate(length(max = MAX_TEXT))]
    #[schema(value_type = Option<String>)]
    pub title: Option<Option<String>>,
    /// The life-cycle status (cannot be cleared).
    #[serde(default, deserialize_with = "present")]
    #[validate(skip)]
    #[schema(nullable = false)]
    pub status: Option<ContactStatus>,
    /// The company (`null`: none).
    #[serde(default, deserialize_with = "nullable")]
    #[validate(skip)]
    #[schema(value_type = Option<Uuid>)]
    pub company_id: Option<Option<Uuid>>,
    /// The new owner: managers and admins only (403 otherwise).
    #[serde(default, deserialize_with = "present")]
    #[validate(skip)]
    #[schema(nullable = false)]
    pub owner_id: Option<Uuid>,
}

impl UpdateContact {
    /// Changes the fields sent (not the owner: who may change it is the
    /// service's call).
    pub fn apply_to(self, contact: &mut Contact) {
        set_some(&mut contact.first_name, self.first_name);
        set_some(&mut contact.last_name, self.last_name);
        set_some(&mut contact.email, self.email);
        set_some(&mut contact.phone, self.phone);
        set_some(&mut contact.title, self.title);
        set_some(&mut contact.status, self.status);
        set_some(&mut contact.company_id, self.company_id);
    }
}

impl AsyncRules for UpdateContact {
    fn async_rules(&self) -> Vec<AsyncRule> {
        vec![
            company_exists(self.company_id.flatten()),
            exists(User::ID, self.owner_id).field("owner_id").into(),
        ]
    }
}

/// The strings of [`ContactStatus`], in life-cycle order (`filter[status]`
/// takes these).
const STATUSES: &[&str] = &["lead", "prospect", "customer", "churned"];

/// The list query of `GET /api/v1/contacts` and of
/// `GET /api/v1/contacts/export`, in the list-query grammar:
///
/// - `filter[company_id]` and `filter[owner_id]`: a company's or a user's id.
/// - `filter[status]`: one status, or several with `filter[status][in]=lead,prospect`.
/// - `search`: words that must each appear in the first name, last name,
///   email or title, ignoring case.
/// - `page` and `per_page` (default 15, at most 100) for the list.
///
/// Contacts come by last name, then first name.
pub struct ContactList;

impl ListResource for ContactList {
    const SPEC: ListSpec = ListSpec::new(&[
        Field::uuid("company_id"),
        Field::one_of("status", STATUSES),
        Field::uuid("owner_id"),
    ])
    // Every word must match, which the model's own scope does.
    .searchable_by_app();
}

#[cfg(test)]
mod tests {
    use navree::orm::list::Kind;

    use super::*;

    #[test]
    fn the_status_filter_takes_exactly_the_statuses() {
        let field = ContactList::SPEC.field("status").expect("a status filter");
        let words: Vec<&str> = ContactStatus::ALL
            .iter()
            .map(|status| status.as_str())
            .collect();
        assert_eq!(field.kind, Kind::OneOf(STATUSES));
        assert_eq!(STATUSES, words);
    }
}
