//! [`ContactsModule`]: `/api/v1/contacts`, including the CSV export.

mod controller;
mod dto;
mod export;
mod resource;
mod service;

use crm_core::CrmCoreModule;
use navree::api::{ModuleBuilderApiExt, RouteDoc};
use navree::prelude::*;

pub use controller::ContactsController;
pub use dto::{ContactList, CreateContact, UpdateContact};
pub use export::{CSV_CONTENT_TYPE, CSV_HEADER, contacts_csv};
pub use resource::{ContactPage, ContactResource, ContactType};
pub use service::ContactService;

/// Contacts; see the [crate docs](crate).
#[module(
    imports = [CrmCoreModule],
    providers = [ContactService],
    controllers = [ContactsController],
    exports = [ContactService],
    register = Self::wire,
)]
pub struct ContactsModule;

impl ContactsModule {
    /// The list query of `GET /api/v1/contacts` and of its CSV export, and the export's file, in
    /// the document.
    fn wire(&self, m: &mut ModuleBuilder) {
        m.api_route(RouteDoc::get("/api/v1/contacts").list::<ContactList>());
        m.api_route(
            RouteDoc::get("/api/v1/contacts/export")
                .filters::<ContactList>()
                .text_response(
                    200,
                    "text/csv",
                    "The contacts that match, one row each, after a header row.",
                ),
        );
    }
}
