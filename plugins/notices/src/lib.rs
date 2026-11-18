//! The `navree-plugin-notices` community plugin: a notice board for a housing
//! society or office that exercises every plugin extension point on the public
//! `navree` facade.

use navree::helm::HelmLink;
use navree::plugin::Permission;
use navree::plugin::prelude::*;
use navree::prelude::*;
use navree::ui::{AppSheet, ShellNavItem};

pub mod controller;
pub mod jobs;
pub mod migration;
pub mod model;
pub mod options;
pub mod pages;

pub use controller::{NewNotice, NoticesController};
pub use jobs::{ExpireNotices, expire_command, expire_notices};
pub use migration::{NOTICES_MIGRATIONS, migrations};
pub use model::notice;
pub use options::NoticesOptions;
pub use pages::board_page;

/// Permission required to view the notice board (`notices.read`).
pub const READ: Permission = Permission::new("notices.read", "Read the society's notices");

/// Permission required to post or delete a notice (`notices.publish`).
pub const PUBLISH: Permission = Permission::new("notices.publish", "Post a notice to the society");

/// Stylesheet served for the notice board under the `notices` namespace (`notices.board`).
pub const BOARD_SHEET: AppSheet =
    AppSheet::new("board", include_bytes!("../assets/board.css"), 4_000).namespace("notices");

/// Registers the `notices` plugin: its `[notices]` settings, `notices_notices`
/// migration, permissions, templates, translations, stylesheet, routes, sidebar
/// link, Helm panel link, CLI command, job, and daily IST schedule.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoticesModule;

impl NoticesModule {
    /// Builds the module reading its `[notices]` settings from the app configuration.
    pub fn from_config() -> NoticesModule {
        NoticesModule
    }
}

impl Module for NoticesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.plugin(navree::plugin_info!(title = "Notice board"));
        m.config_section::<NoticesOptions>();
        m.migrations(migration::migrations());
        m.permissions(&[READ, PUBLISH]);
        m.locales(navree::embed_locales!("locales"));
        m.plugin_templates(navree::embed_templates!("views"));
        m.contribute(BOARD_SHEET);
        m.controller::<NoticesController>();
        m.nav_item(
            ShellNavItem::new("Notices", "/notices")
                .group("Society")
                .permission("notices.read"),
        );
        m.helm_nav(HelmLink::new("Notices", "/admin/notices").can("manage", "notices.Notice"));
        m.command(expire_command());
        m.job::<ExpireNotices>();
        m.schedule(|s| {
            s.job(ExpireNotices)
                .daily_at("02:30")
                .in_zone("Asia/Kolkata");
        });
        // navree:wire
    }
}
