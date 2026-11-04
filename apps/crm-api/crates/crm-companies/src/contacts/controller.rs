//! `/api/v1/contacts`.

use crm_core::{Contact, Export};
use navree::http::response::Response;
use navree::orm::list::List;
use navree::prelude::*;

use super::dto::{ContactList, CreateContact, UpdateContact};
use super::export::csv_response;
use super::resource::{ContactPage, ContactResource};
use super::service::ContactService;

/// `/api/v1/contacts`: everyone signed in with a CRM role reads and creates
/// contacts; owners (and managers, admins) edit and delete them; admins and
/// managers export them. `from = "id"` routes load the contact first (404
/// when there is none, 403 when the rules say no).
#[injectable]
pub struct ContactsController {
    contacts: Arc<ContactService>,
}

#[controller("/contacts", version = "1")]
impl ContactsController {
    /// A page of contacts by last name; `search` matches names, email and
    /// title.
    #[get("/")]
    #[can(Read, Contact)]
    #[api(
        summary = "List contacts",
        description = "Query: `search` (every word must match the first name, last name, email or title; case-insensitive), `filter[company_id]`, `filter[status]`, `filter[owner_id]`, `page`, `per_page` (default 15, at most 100). Ordered by last name, then first name.",
        tag = "Contacts",
        response = ContactPage,
        responses(400, 401, 403)
    )]
    async fn index(
        &self,
        uow: Uow,
        scope: Accessible<Contact, Read>,
        list: List<ContactList>,
    ) -> Result<Json<ContactPage>> {
        Ok(Json(self.contacts.list(&uow, scope, &list).await?))
    }

    /// Every contact matching the list's filters as a CSV download
    /// (admins and managers).
    #[get("/export")]
    #[can(Export, Contact)]
    #[audit(action = "contacts.exported")]
    #[api(
        summary = "Export contacts as CSV",
        description = "`text/csv; charset=utf-8` attachment `contacts.csv` with the columns id, first_name, last_name, email, phone, title, status, company, owner, created_at, one row per contact matching the list's `filter[...]` parameters and `search`, in list order.",
        tag = "Contacts",
        responses(400, 401, 403)
    )]
    async fn export(
        &self,
        uow: Uow,
        scope: Accessible<Contact, Read>,
        list: List<ContactList>,
    ) -> Result<Response> {
        let contacts = self.contacts.all(&uow, scope, &list).await?;
        csv_response(&contacts)
    }

    /// Creates a contact, owned by the caller unless a manager names the
    /// owner.
    #[post("/")]
    #[can(Create, Contact)]
    #[api(
        summary = "Create a contact",
        tag = "Contacts",
        request = CreateContact,
        response = ContactResource,
        status = 201,
        responses(400, 401, 403)
    )]
    async fn store(
        &self,
        uow: Uow,
        CurrentUser(me): CurrentUser,
        Abilities(ability): Abilities,
        ValidAsync(Json(input)): ValidAsync<Json<CreateContact>>,
    ) -> Result<Created<ContactResource>> {
        let contact = self.contacts.create(&uow, &ability, me.id, input).await?;
        Ok(Created(format!("/api/v1/contacts/{}", contact.id), contact))
    }

    /// One contact.
    #[get("/{id}")]
    #[can(Read, Contact, from = "id")]
    #[api(
        summary = "Show a contact",
        tag = "Contacts",
        response = ContactResource,
        responses(401, 403, 404)
    )]
    async fn show(
        &self,
        uow: Uow,
        contact: Authorized<Contact, Read>,
    ) -> Result<Json<ContactResource>> {
        Ok(Json(self.contacts.show(&uow, contact.into_inner()).await?))
    }

    /// Changes the fields sent; `null` clears an optional field.
    #[patch("/{id}")]
    #[can(Update, Contact, from = "id")]
    #[api(
        summary = "Update a contact",
        tag = "Contacts",
        request = UpdateContact,
        response = ContactResource,
        responses(400, 401, 403, 404)
    )]
    async fn update(
        &self,
        uow: Uow,
        CurrentUser(me): CurrentUser,
        contact: Authorized<Contact, Update>,
        ValidAsync(Json(input)): ValidAsync<Json<UpdateContact>>,
    ) -> Result<Json<ContactResource>> {
        let ability = contact.ability().clone();
        let contact = self
            .contacts
            .update(&uow, &ability, me.id, contact.into_inner(), input)
            .await?;
        Ok(Json(contact))
    }

    /// Soft-deletes a contact: it disappears from the API, its row stays.
    #[delete("/{id}")]
    #[can(Delete, Contact, from = "id")]
    #[api(
        summary = "Delete a contact",
        tag = "Contacts",
        status = 204,
        responses(401, 403, 404)
    )]
    async fn destroy(&self, uow: Uow, contact: Authorized<Contact, Delete>) -> Result<NoContent> {
        self.contacts.delete(&uow, contact.into_inner()).await?;
        Ok(NoContent)
    }
}
