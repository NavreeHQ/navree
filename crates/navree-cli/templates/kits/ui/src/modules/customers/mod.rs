//! The customers module: a table, a form and a page for each customer of a
//! workspace, kept current through the app's live hub.
//!
//! ```text
//! customers/
//! ├── mod.rs        # CustomersModule: what the module provides
//! ├── entity.rs     # the `customers` table (an ORM model), Plan and Status
//! ├── form.rs       # the form that adds and edits a customer
//! ├── table.rs      # the table: columns, sort, search and filters
//! ├── service.rs    # the rules: unique emails, the figures, telling open pages
//! ├── pages.rs      # /customers, /customers/new, /customers/{id}, ...
//! ├── home.rs       # / and its live figures
//! ├── seed.rs       # two demo workspaces and their customers (never in production)
//! └── migrations/   # the customers table
//! ```
//!
//! Copy this module's shape for your own features: `navree make:module`
//! writes the JSON side, and the files here show the pages on top of it.

pub mod entity;
pub mod form;
mod home;
mod migrations;
mod pages;
pub mod seed;
pub mod service;
pub mod table;

use navree::prelude::*;

pub use entity::{Customer, Plan, Status};
pub use service::CustomerService;

/// Customers: the table, the pages and the demo data.
pub struct CustomersModule;

impl Module for CustomersModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(migrations::migrations());
        m.model::<Customer>();
        m.provider::<CustomerService>();
        m.provider::<seed::DemoData>().lifecycle::<seed::DemoData>();
        m.seeder::<seed::CustomerSeeder>();
        m.controller::<pages::CustomerPages>();
        m.controller::<home::HomePages>();
        // navree:wire
    }
}
