//! [`ContactsModule`]: `/api/v1/contacts`, including the CSV export.

mod controller;
mod dto;
mod export;
mod resource;
mod service;

use crm_core::CrmCoreModule;
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
)]
pub struct ContactsModule;
