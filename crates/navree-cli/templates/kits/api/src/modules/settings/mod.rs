//! Settings: the workspace's name and slug, each member's notification
//! preferences, and billing. Workspaces are what the app bills:
//! `BillingModule::for_feature::<Workspace>()` makes the `/api/billing/*`
//! routes act on the request's workspace (plans, checkout, the customer
//! portal, invoices), for members with `billing.manage`.
//!
//! ```text
//! settings/
//! ├── mod.rs           # SettingsModule
//! ├── controller.rs    # /api/v1/settings/workspace, /notifications, the fake checkout
//! ├── service.rs       # SettingsService
//! ├── entity.rs        # the `notification_settings` table
//! ├── dto.rs           # request and response bodies
//! └── migrations/      # the table and its row-level security policy
//! (tests: tests/settings.rs and tests/billing.rs boot the whole app)
//! ```

mod controller;
pub mod dto;
pub mod entity;
mod migrations;
mod service;

use navree::prelude::*;
use navree::workspace::Workspace;

pub use controller::SettingsController;
pub use service::SettingsService;

/// Workspace settings, notification preferences and workspace billing.
#[module(
    imports = [BillingModule::for_feature::<Workspace>()],
    providers = [SettingsService],
    controllers = [SettingsController],
    exports = [SettingsService],
    register = Self::wire,
)]
pub struct SettingsModule;

impl SettingsModule {
    /// Registrations `#[module]` has no key for. `navree make:*` adds its
    /// lines above the `navree:wire` marker.
    fn wire(&self, m: &mut ModuleBuilder) {
        m.migrations(migrations::migrations());
        // navree:wire
    }
}
