//! The admin panel's resources (Helm at `/admin`), written against the
//! system data so every screen type has a real instance:
//!
//! ```text
//! admin/
//! ├── mod.rs         AdminModule: registers the resources, the page and the settings
//! ├── accounts.rs    users and workspaces: filters, a bulk action, metrics, relationships
//! ├── work.rs        projects, tasks and reports: filters, row and bulk actions,
//! │                  metrics, a create form, edit forms with validation
//! ├── activity.rs    the audit log: read-only, filters and counts
//! ├── pages.rs       the operations overview, a custom page
//! ├── settings.rs    settings groups, edited under System, Settings
//! └── form.rs        reading submitted forms, announcing changes to webhooks
//! ```

mod accounts;
mod activity;
mod form;
mod pages;
mod settings;
mod work;

use navree::prelude::*;

pub use accounts::{UserAdmin, WorkspaceAdmin};
pub use activity::{ActivityAdmin, AuditEntry};
pub use settings::{Branding, TaskDefaults};
pub use work::{ProjectAdmin, ReportAdmin, TaskAdmin};

/// The admin panel's resources, page and settings.
#[module(register = Self::wire)]
pub struct AdminModule;

impl AdminModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.resource::<UserAdmin>()
            .resource::<WorkspaceAdmin>()
            .resource::<ProjectAdmin>()
            .resource::<TaskAdmin>()
            .resource::<ReportAdmin>()
            .resource::<ActivityAdmin>();
        m.templates(&[pages::TEMPLATE]);
        m.helm_page(pages::operations_overview());
        m.settings::<TaskDefaults>().settings::<Branding>();
    }
}
