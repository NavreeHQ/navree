//! [`ContactService`]: listing, showing, creating, editing, soft-deleting
//! and exporting contacts.

use std::sync::Arc;

use crm_core::models::{Company, Contact, ContactScopes};
use crm_core::owner_for;
use crm_core::subjects::CONTACT;
use navree::access::{Ability, Accessible, Read};
use navree::contracts::Clock;
use navree::orm::access::AccessibleQuery;
use navree::orm::list::{ListQuery, ListQueryExt, ListResource};
use navree::orm::{Builder, Record};
use navree::prelude::*;
use navree::validation::{Messages, ValidationErrors};
use uuid::Uuid;

use super::dto::{ContactList, CreateContact, UpdateContact};
use super::resource::{ContactPage, ContactResource};
use crate::common::search_text;

/// Contacts, as the API sees them: soft-deleted ones do not exist (the
/// model's soft deletes leave them out of every query, the route's record
/// included). Writes run in the caller's unit of work, so the audit trail
/// records them with the request.
#[injectable]
pub struct ContactService {
    clock: Arc<dyn Clock>,
    messages: Arc<Messages>,
}

impl ContactService {
    /// One page of the contacts `scope` allows that match `list` (read
    /// against [`ContactList`]), by last then first name.
    pub async fn list(
        &self,
        uow: &Uow,
        scope: Accessible<Contact, Read>,
        list: &ListQuery,
    ) -> Result<ContactPage> {
        let Page { data, meta } = filtered(&scope, list)?
            .paginate(uow, list.page_request(&ContactList::SPEC))
            .await?;
        Ok(ContactPage {
            data: data.iter().map(Record::to_resource).collect(),
            meta: meta.into(),
        })
    }

    /// Every contact `scope` allows that matches `list`, in list order,
    /// with their embeds (for the CSV export).
    pub async fn all(
        &self,
        uow: &Uow,
        scope: Accessible<Contact, Read>,
        list: &ListQuery,
    ) -> Result<Vec<ContactResource>> {
        let contacts = filtered(&scope, list)?.get(uow).await?;
        Ok(contacts.to_resource_collection())
    }

    /// `contact` with its embeds.
    pub async fn show(&self, uow: &Uow, mut contact: Record<Contact>) -> Result<ContactResource> {
        contact.load_all(uow, ContactResource::EMBEDS).await?;
        Ok(contact.to_resource())
    }

    /// Creates a contact owned by `me`, or by `input.owner_id` when
    /// `ability` may manage contacts (else a 403).
    pub async fn create(
        &self,
        uow: &Uow,
        ability: &Ability,
        me: Uuid,
        input: CreateContact,
    ) -> Result<ContactResource> {
        let owner_id = owner_for(ability, CONTACT, me, None, input.owner_id)?;
        self.check_company(uow, input.company_id).await?;
        let mut contact = Contact::create(uow, input.into_contact(owner_id)).await?;
        contact.load_all(uow, ContactResource::EMBEDS).await?;
        Ok(contact.to_resource())
    }

    /// Changes the fields `input` sends; a new owner needs `manage` on
    /// contacts (else a 403). A request that changes nothing writes
    /// nothing.
    pub async fn update(
        &self,
        uow: &Uow,
        ability: &Ability,
        me: Uuid,
        mut contact: Record<Contact>,
        input: UpdateContact,
    ) -> Result<ContactResource> {
        let owner_id = owner_for(ability, CONTACT, me, Some(contact.owner_id), input.owner_id)?;
        // Only a new company is checked: an edit form sending back a
        // company deleted since stays valid.
        if let Some(company_id) = input
            .company_id
            .filter(|company_id| *company_id != contact.company_id)
        {
            self.check_company(uow, company_id).await?;
        }
        input.apply_to(&mut contact);
        contact.owner_id = owner_id;
        contact.save(uow).await?;
        contact.load_all(uow, ContactResource::EMBEDS).await?;
        Ok(contact.to_resource())
    }

    /// Soft-deletes `contact`: sets `deleted_at`, written as an update, so
    /// the audit trail records it as `contact.updated` setting `deleted_at`
    /// (which the web apps show as a deletion).
    pub async fn delete(&self, uow: &Uow, mut contact: Record<Contact>) -> Result<()> {
        let now = self.clock.now();
        contact
            .update(uow, |contact| contact.deleted_at = Some(now))
            .await?;
        Ok(())
    }

    /// A 400 on `company_id` unless the company exists and is not deleted
    /// (the `exists` rule already rejected unknown ids).
    async fn check_company(&self, uow: &Uow, company_id: Option<Uuid>) -> Result<()> {
        let Some(company_id) = company_id else {
            return Ok(());
        };
        if Company::query().where_key(company_id).exists(uow).await? {
            return Ok(());
        }
        let mut errors = ValidationErrors::new();
        errors.add(
            "company_id",
            self.messages.render("company_id", "exists", &[]),
        );
        Err(errors.into_app_error())
    }
}

/// The contacts `scope` allows that match `list`, by last name, first name
/// (ignoring ASCII case) and id, with their embeds. A list has no `sort`, so
/// the order is always this one.
fn filtered(scope: &Accessible<Contact, Read>, list: &ListQuery) -> Result<Builder<Contact>> {
    Ok(scope
        .apply(Contact::query())
        .list(list, &ContactList::SPEC)?
        // Every word of the search must match, which the model's scope does.
        .when_some(search_text(list.search.as_deref()), |query, text| {
            query.search(text)
        })
        .alphabetical()
        .with_all(ContactResource::EMBEDS))
}
