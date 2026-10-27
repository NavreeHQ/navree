//! A small Navree app for Navigator's end-to-end test: configuration,
//! logging, the database, HTTP and one feature module with a route and a
//! command.

use navree::config::{ConfigModule, ConfigSources};
use navree::db::DatabaseModule;
use navree::http::HttpModule;
use navree::kernel::{CommandSpec, ModuleBuilder, Navree};
use navree::log::LoggerModule;
use navree::prelude::*;

/// Answers `GET /api/notes`.
#[injectable]
pub struct NotesController;

#[controller("/notes")]
impl NotesController {
    #[get("/")]
    #[public]
    async fn index(&self) -> &'static str {
        "[]"
    }
}

/// The notes feature.
#[module(controllers = [NotesController], register = Self::wire)]
pub struct NotesModule;

impl NotesModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.command(CommandSpec::new(
            "notes:prune",
            "Delete notes older than a year",
            |_ctx, _args| async { Ok(()) },
        ));
    }
}

fn main() -> Result<(), navree::kernel::BoxError> {
    Navree::app()
        .module(ConfigModule::for_root(ConfigSources::defaults()))
        .module(LoggerModule::from_config())
        .module(HttpModule::from_config())
        .module(DatabaseModule::from_config())
        .module(NotesModule)
        .run()
}
