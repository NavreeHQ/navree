//! # crm-companies
//!
//! [`CompaniesModule`] and [`ContactsModule`] (plan Task 33; spec §7, §9.3).
//!
//! | Route | Access | Result |
//! |---|---|---|
//! | `GET /api/v1/companies?search=&owner_id=&industry=&sort=&page=&per_page=` | `read Company` | page of [`CompanyResource`] |
//! | `POST /api/v1/companies` | `create Company` | 201 [`CompanyResource`] |
//! | `GET /api/v1/companies/{id}` | `read` it | [`CompanyDetail`] (counts of contacts and open deals) |
//! | `PATCH /api/v1/companies/{id}` | `update` it (owner, managers) | [`CompanyResource`] |
//! | `DELETE /api/v1/companies/{id}` | `delete` it (owner, managers) | 204 (soft delete) |
//! | `GET /api/v1/contacts?search=&company_id=&status=&owner_id=&page=&per_page=` | `read Contact` | page of [`ContactResource`] |
//! | `POST /api/v1/contacts` | `create Contact` | 201 [`ContactResource`] |
//! | `GET /api/v1/contacts/{id}` | `read` it | [`ContactResource`] |
//! | `PATCH /api/v1/contacts/{id}` | `update` it (owner, managers) | [`ContactResource`] |
//! | `DELETE /api/v1/contacts/{id}` | `delete` it (owner, managers) | 204 (soft delete) |
//! | `GET /api/v1/contacts/export?search=&company_id=&status=&owner_id=` | `export Contact` (admins, managers) | `text/csv` ([`CSV_HEADER`] columns) |
//!
//! Every resource carries `__type` (the CASL subject type) and an `owner`
//! embed; deleted (`deleted_at`) records are 404s and never listed.
//! Setting another user as `owner_id` needs `manage` on the record type
//! (spec §9.3; [`crm_core::owner_for`]): reps get a 403.
#![forbid(unsafe_code)]

mod common;
mod companies;
mod contacts;

pub use common::PageInfo;
pub use companies::{
    CompaniesController, CompaniesModule, CompanyDetail, CompanyFilters, CompanyPage,
    CompanyResource, CompanyService, CompanySort, CompanyType, CreateCompany, UpdateCompany,
};
pub use contacts::{
    CSV_CONTENT_TYPE, CSV_HEADER, ContactFilters, ContactPage, ContactResource, ContactService,
    ContactType, ContactsController, ContactsModule, CreateContact, UpdateContact, contacts_csv,
};
