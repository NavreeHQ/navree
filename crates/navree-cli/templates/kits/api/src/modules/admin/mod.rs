//! The admin panel: a Helm resource for users,
//! served at `/admin` for super admins (the global `admin` role, which
//! passes every check). Grant other roles `helm.view` plus the abilities on
//! each subject to let them in with less.
//!
//! ```text
//! admin/
//! ├── mod.rs           # AdminModule: registers the resources
//! └── resources.rs     # UserAdmin
//! ```
//!
//! `navree make:resource <module> <Entity>` adds a resource for one of
//! your entities.

mod resources;

use navree::prelude::*;

pub use resources::UserAdmin;

/// The admin panel's resources.
#[module(register = Self::wire)]
pub struct AdminModule;

impl AdminModule {
    /// `m.resource::<R>()` registers each resource as a provider and adds
    /// it to the panel; `navree make:resource` adds its lines above the
    /// `navree:wire` marker.
    fn wire(&self, m: &mut ModuleBuilder) {
        m.resource::<UserAdmin>();
        // navree:wire
    }
}
