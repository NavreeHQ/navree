//! The root module: the framework modules, then the kit's feature modules.

use navree::config::ConfigSources;
use navree::kernel::{AppBuilder, Navree};

use crate::platform::platform;
use crate::{modules, seed};

/// The app, configured from `sources`.
pub fn app(sources: ConfigSources) -> AppBuilder {
    platform(Navree::app(), sources)
        // Profile, sessions and sign-in options on top of AuthModule.
        .module(modules::accounts::AccountsModule)
        // Workspace settings, notification preferences and billing.
        .module(modules::settings::SettingsModule)
        // The example feature: a workspace-scoped resource.
        .module(modules::projects::ProjectsModule)
        // Helm resources for users, workspaces and subscriptions.
        .module(modules::admin::AdminModule)
        // `db:seed` / `migrate:fresh --seed`: the demo workspace.
        .module(seed::SeedModule)
    // navree:modules
}
