//! The root module: the framework modules, then the system domain.

use navree::prelude::*;

use crate::platform::platform;
use crate::{modules, seed};

/// The system app, configured from `sources`.
pub fn app(sources: ConfigSources) -> AppBuilder {
    platform(Navree::app(), sources)
        // Projects: a workspace-scoped model, searchable.
        .module(modules::projects::ProjectsModule)
        // Tasks: a workspace-scoped navree-orm model.
        .module(modules::tasks::TasksModule)
        // Reports: a queued job that writes its result.
        .module(modules::reports::ReportsModule)
        // Live: realtime publishes for the load profiles.
        .module(modules::live::LiveModule)
        // GraphQL fields and MCP tools over the system data.
        .module(modules::showcase::ShowcaseModule)
        // Helm resources: users, workspaces, projects, tasks, reports, activity.
        .module(modules::admin::AdminModule)
        // Failing, slow and outgoing routes, queue work, mail, scheduled tasks.
        .module(modules::demo::DemoModule)
        // `db:seed system`: accounts, workspaces, projects, tasks.
        .module(seed::SeedModule)
}
