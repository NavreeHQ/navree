//! Invoices: the `invoices` table, the service that drafts and reads them,
//! and the message handlers other services reach them through.
//!
//! ```text
//! invoices/
//! ├── mod.rs           # InvoicesModule: what the module provides and registers
//! ├── handlers.rs      # InvoiceHandlers: #[message_pattern] and #[event_pattern]
//! ├── service.rs       # InvoiceService: the business logic
//! ├── entity.rs        # the `invoices` table (an ORM model)
//! └── migrations/      # migrations owned by this module
//! (tests: tests/invoices.rs calls the handlers through MicroFake)
//! ```

mod entity;
mod handlers;
mod migrations;
mod service;

use navree::db::ModuleBuilderDbExt;
use navree::kernel::ModuleBuilder;
use navree::micro::ModuleBuilderMicroExt;
use navree::module;
use navree::orm::ModuleBuilderOrmExt;

pub use handlers::InvoiceHandlers;
pub use service::InvoiceService;

/// Invoices, reached over messages: `invoices.get` and the `deal.won`
/// event.
#[module(
    providers = [InvoiceService],
    exports = [InvoiceService],
    register = Self::wire,
)]
pub struct InvoicesModule;

impl InvoicesModule {
    /// Registrations `#[module]` has no key for (migrations, message
    /// handlers, jobs, listeners, commands). `navree make:*` adds its lines
    /// above the `navree:wire` marker.
    fn wire(&self, m: &mut ModuleBuilder) {
        m.migrations(migrations::migrations())
            .model::<entity::StoredInvoice>()
            .message_handlers::<InvoiceHandlers>();
        // navree:wire
    }
}
