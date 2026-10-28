//! The root module: the framework modules, then the evidence domain.

use navree::prelude::*;

use crate::platform::platform;
use crate::{modules, seed};

/// The evidence app, configured from `sources`.
pub fn app(sources: ConfigSources) -> AppBuilder {
    platform(Navree::app(), sources)
        // Projects: a workspace-scoped SeaORM entity, searchable.
        .module(modules::projects::ProjectsModule)
        // Tasks: a workspace-scoped navree-orm model.
        .module(modules::tasks::TasksModule)
        // Reports: a queued job that writes its result.
        .module(modules::reports::ReportsModule)
        // Live: realtime publishes for the load profiles.
        .module(modules::live::LiveModule)
        // GraphQL fields and MCP tools over the evidence data.
        .module(modules::showcase::ShowcaseModule)
        // Helm resources: users, workspaces, projects.
        .module(modules::admin::AdminModule)
        // `db:seed evidence`: accounts, workspaces, projects, tasks.
        .module(seed::SeedModule)
}
