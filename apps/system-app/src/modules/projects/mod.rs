//! Projects: a resource owned by a workspace (a navree-orm model behind
//! `WorkspaceRepo`), searchable through `SearchModule`, broadcast to the
//! workspace's members on every change.
//!
//! ```text
//! projects/
//! ├── mod.rs           # ProjectsModule: what the module provides
//! ├── controller.rs    # ProjectsController: /api/v1/projects, behind InWorkspace
//! ├── service.rs       # ProjectService: WorkspaceRepo reads and writes, broadcasts
//! ├── entity.rs        # the `projects` model: subject, WorkspaceScoped, Searchable
//! ├── dto.rs           # request and response bodies
//! └── migrations/      # the table and its row-level security policy
//! ```

mod controller;
pub mod dto;
pub mod entity;
mod migrations;
mod service;

use navree::prelude::*;

pub use controller::ProjectsController;
pub use entity::Project;
pub use service::{PROJECT_CHANGED, ProjectService};

/// Projects, scoped to the request's workspace.
#[module(
    imports = [
        AccessModule::for_feature::<Project>(),
        SearchModule::for_feature::<Project>(),
    ],
    providers = [ProjectService],
    controllers = [ProjectsController],
    exports = [ProjectService],
    register = Self::wire,
)]
pub struct ProjectsModule;

impl ProjectsModule {
    /// Registrations `#[module]` has no key for.
    fn wire(&self, m: &mut ModuleBuilder) {
        m.migrations(migrations::migrations())
            // `#[can(.., from = "id")]` answers 404 for another workspace's project.
            .workspace_scoped::<Project>();
        m.model::<Project>();
    }
}
