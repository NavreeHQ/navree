//! Projects: the kit's example feature, a resource owned by a workspace.
//! Copy its shape for your own features.
//!
//! ```text
//! projects/
//! ├── mod.rs           # ProjectsModule: what the module provides
//! ├── controller.rs    # ProjectsController: /api/v1/projects, behind InWorkspace
//! ├── service.rs       # ProjectService: WorkspaceRepo reads and writes, broadcasts
//! ├── entity.rs        # the `projects` table: Subject + WorkspaceScoped
//! ├── dto.rs           # request and response bodies
//! └── migrations/      # the table and its row-level security policy
//! (tests: tests/projects.rs boots the whole app)
//! ```

mod controller;
pub mod dto;
pub mod entity;
mod migrations;
mod service;

use navree::prelude::*;
// COMPAT (data-layer port): `sea_workspace_scoped` for the SeaORM entity.
use navree::workspace::sea_compat::ModuleBuilderSeaWorkspaceExt as _;

pub use controller::ProjectsController;
pub use entity::Project;
pub use service::{PROJECT_CHANGED, ProjectService};

/// Projects, scoped to the request's workspace.
#[module(
    imports = [AccessModule::for_feature::<Project>()],
    providers = [ProjectService],
    controllers = [ProjectsController],
    exports = [ProjectService],
    register = Self::wire,
)]
pub struct ProjectsModule;

impl ProjectsModule {
    /// Registrations `#[module]` has no key for. `navree make:*` adds its
    /// lines above the `navree:wire` marker.
    fn wire(&self, m: &mut ModuleBuilder) {
        m.migrations(migrations::migrations())
            // `#[can(.., from = "id")]` answers 404 for another workspace's project.
            .sea_workspace_scoped::<Project>();
        // navree:wire
    }
}
