//! The admin panel: Helm resources for users, workspaces and subscriptions,
//! served at `/admin` for super admins (the global `admin` role, which
//! passes every check). Grant other roles `helm.view` plus the abilities on
//! each subject to let them in with less.
//!
//! ```text
//! admin/
//! ├── mod.rs           # AdminModule: registers the resources
//! └── resources.rs     # UserAdmin, WorkspaceAdmin, SubscriptionAdmin
//! ```
//!
//! `navree make:resource <module> <Entity>` adds a resource for one of
//! your entities.

mod resources;

use navree::prelude::*;

pub use resources::{SubscriptionAdmin, UserAdmin, WorkspaceAdmin};

/// The admin panel's resources.
#[module(register = Self::wire)]
pub struct AdminModule;

impl AdminModule {
    /// `m.resource::<R>()` registers each resource as a provider and adds
    /// it to the panel; `navree make:resource` adds its lines above the
    /// `navree:wire` marker.
    fn wire(&self, m: &mut ModuleBuilder) {
        m.resource::<UserAdmin>()
            .resource::<WorkspaceAdmin>()
            .resource::<SubscriptionAdmin>();
        // navree:wire
    }
}
