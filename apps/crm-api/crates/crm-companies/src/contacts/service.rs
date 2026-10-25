//! [`ContactService`]: listing, showing, creating, editing, soft-deleting
//! and exporting contacts.

use std::sync::Arc;

use crm_core::entities::{company, contact};
use crm_core::subjects::CONTACT;
use crm_core::{CompanyRef, OwnerRef, owner_for};
use navree::access::{Ability, Accessible, Read};
use navree::contracts::Clock;
use navree::db::paginate;
use navree::prelude::*;
use navree::validation::{Messages, ValidationErrors};
use sea_orm::ActiveValue::Set;
use sea_orm::sea_query::{Expr, Func};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DbBackend, EntityTrait, IntoActiveModel, Order, QueryFilter,
    QueryOrder, Select,
};
use uuid::Uuid;

use super::dto::{ContactFilters, CreateContact, UpdateContact};
use super::resource::{ContactPage, ContactResource};
use crate::common::{any_contains, search_text, set_some};

/// The most words of a search that are matched (the rest are ignored).
const MAX_SEARCH_TERMS: usize = 5;

/// Contacts, as the API sees them: soft-deleted ones do not exist. Writes
/// run in the caller's unit of work (`Repo`, so the audit trail records
/// them).
#[injectable]
pub struct ContactService {
    clock: Arc<dyn Clock>,
    messages: Arc<Messages>,
}

impl ContactService {
    /// One page of the contacts `scope` allows, matching `filters`, by last
    /// then first name.
    pub async fn list(
        &self,
        uow: &Uow,
        scope: Accessible<contact::Entity, Read>,
        filters: ContactFilters,
        page: PageQuery,
    ) -> Result<ContactPage> {
        let select = filtered(scope, &filters, uow.db().backend());
        let tx = uow.tx().await?;
        let Page { data, meta } = paginate(select, &*tx, page).await?;
        Ok(ContactPage {
            data: present_all(&*tx, data).await?,
            meta: meta.into(),
        })
    }

    /// Every contact `scope` allows that matches `filters`, in list order,
    /// with their embeds (for the CSV export).
    pub async fn all(
        &self,
        uow: &Uow,
        scope: Accessible<contact::Entity, Read>,
        filters: ContactFilters,
    ) -> Result<Vec<ContactResource>> {
        let tx = uow.tx().await?;
        let contacts = filtered(scope, &filters, uow.db().backend())
            .all(&*tx)
            .await?;
        present_all(&*tx, contacts).await
    }

    /// `contact` with its embeds; a 404 for a deleted contact.
    pub async fn show(&self, uow: &Uow, contact: contact::Model) -> Result<ContactResource> {
        let contact = live(contact)?;
        present(&*uow.tx().await?, contact).await
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
        let now = self.clock.now();
        let contact = Repo::<contact::Entity>::insert(
            uow,
            contact::ActiveModel {
                id: Set(Uuid::now_v7()),
                first_name: Set(input.first_name),
                last_name: Set(input.last_name),
                email: Set(input.email),
                phone: Set(input.phone),
                title: Set(input.title),
                status: Set(input.status.unwrap_or_default()),
                company_id: Set(input.company_id),
                owner_id: Set(owner_id),
                created_at: Set(now),
                updated_at: Set(now),
                deleted_at: Set(None),
            },
        )
        .await?;
        present(&*uow.tx().await?, contact).await
    }

    /// Changes the fields `input` sends; a new owner needs `manage` on
    /// contacts (else a 403). A 404 for a deleted contact.
    pub async fn update(
        &self,
        uow: &Uow,
        ability: &Ability,
        me: Uuid,
        contact: contact::Model,
        input: UpdateContact,
    ) -> Result<ContactResource> {
        let contact = live(contact)?;
        let owner_id = owner_for(ability, CONTACT, me, Some(contact.owner_id), input.owner_id)?;
        // Only a new company is checked: an edit form sending back a
        // company deleted since stays valid.
        if let Some(company_id) = input
            .company_id
            .filter(|company_id| *company_id != contact.company_id)
        {
            self.check_company(uow, company_id).await?;
        }
        let new_owner = (owner_id != contact.owner_id).then_some(owner_id);
        let mut active = contact.into_active_model();
        set_some(&mut active.first_name, input.first_name);
        set_some(&mut active.last_name, input.last_name);
        set_some(&mut active.email, input.email);
        set_some(&mut active.phone, input.phone);
        set_some(&mut active.title, input.title);
        set_some(&mut active.status, input.status);
        set_some(&mut active.company_id, input.company_id);
        set_some(&mut active.owner_id, new_owner);
        active.updated_at = Set(self.clock.now());
        let contact = Repo::<contact::Entity>::update(uow, active).await?;
        present(&*uow.tx().await?, contact).await
    }

    /// Soft-deletes `contact` (sets `deleted_at`); a 404 when it already is.
    pub async fn delete(&self, uow: &Uow, contact: contact::Model) -> Result<()> {
        let mut active = live(contact)?.into_active_model();
        active.deleted_at = Set(Some(self.clock.now()));
        Repo::<contact::Entity>::update(uow, active).await?;
        Ok(())
    }

    /// A 400 on `company_id` unless the company exists and is not deleted
    /// (the `exists` rule already rejected unknown ids).
    async fn check_company(&self, uow: &Uow, company_id: Option<Uuid>) -> Result<()> {
        let Some(company_id) = company_id else {
            return Ok(());
        };
        let live = company::Entity::find_by_id(company_id)
            .filter(company::Column::DeletedAt.is_null())
            .one(&*uow.tx().await?)
            .await?
            .is_some();
        if live {
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

/// The live contacts `scope` allows, matching `filters`, by last name,
/// first name (case-insensitive) and id.
fn filtered(
    scope: Accessible<contact::Entity, Read>,
    filters: &ContactFilters,
    backend: DbBackend,
) -> Select<contact::Entity> {
    let mut select = scope
        .apply(contact::Entity::find())
        .filter(contact::Column::DeletedAt.is_null());
    if let Some(search) = search_text(filters.search.as_deref()) {
        for term in search.split_whitespace().take(MAX_SEARCH_TERMS) {
            select = select.filter(any_contains(
                backend,
                &[
                    contact::Column::FirstName,
                    contact::Column::LastName,
                    contact::Column::Email,
                    contact::Column::Title,
                ],
                term,
            ));
        }
    }
    if let Some(company_id) = filters.company_id {
        select = select.filter(contact::Column::CompanyId.eq(company_id));
    }
    if let Some(status) = filters.status {
        select = select.filter(contact::Column::Status.eq(status));
    }
    if let Some(owner_id) = filters.owner_id {
        select = select.filter(contact::Column::OwnerId.eq(owner_id));
    }
    let lower = |column: contact::Column| Expr::expr(Func::lower(Expr::col(column)));
    select
        .order_by(lower(contact::Column::LastName), Order::Asc)
        .order_by(lower(contact::Column::FirstName), Order::Asc)
        .order_by_asc(contact::Column::Id)
}

/// `contact`, or a 404 when it is soft-deleted.
fn live(contact: contact::Model) -> Result<contact::Model> {
    if contact.deleted_at.is_some() {
        Err(AppError::not_found())
    } else {
        Ok(contact)
    }
}

/// `contact` with its embeds.
async fn present<C: ConnectionTrait>(conn: &C, contact: contact::Model) -> Result<ContactResource> {
    let mut resources = present_all(conn, vec![contact]).await?;
    resources
        .pop()
        .ok_or_else(|| AppError::internal("a presented contact went missing"))
}

/// `contacts` with their company and owner embeds (one query per embed).
async fn present_all<C: ConnectionTrait>(
    conn: &C,
    contacts: Vec<contact::Model>,
) -> Result<Vec<ContactResource>> {
    let companies = CompanyRef::load(conn, contacts.iter().filter_map(|c| c.company_id)).await?;
    let owners = OwnerRef::load(conn, contacts.iter().map(|c| c.owner_id)).await?;
    Ok(contacts
        .into_iter()
        .map(|contact| {
            let company = contact
                .company_id
                .and_then(|id| companies.get(&id).cloned());
            let owner = owners.get(&contact.owner_id).cloned();
            ContactResource::new(contact, company, owner)
        })
        .collect())
}
