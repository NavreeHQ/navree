//! The root module: the framework modules the service uses, each configured
//! once, then its feature modules.

use navree::auth::AuthModule;
use navree::config::{ConfigModule, ConfigSources};
use navree::crypto::CryptoModule;
use navree::db::DatabaseModule;
use navree::events::EventsModule;
use navree::http::HttpModule;
use navree::kernel::{AppBuilder, Navree};
use navree::log::LoggerModule;
use navree::micro::MicroModule;
use navree::observe::ObserveModule;
use navree::validation::ValidationModule;

use crate::modules;

/// The service, configured from `sources`. Each `from_config()` module reads
/// its section (`http`, `database`, `micro`, ...) at boot.
pub fn app(sources: ConfigSources) -> AppBuilder {
    Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(LoggerModule::from_config())
        .module(CryptoModule::from_config())
        // HTTP for the orchestrator: /health/live, /health/ready (with a
        // `micro` check that fails while the broker is unreachable) and
        // /metrics, from ObserveModule.
        .module(HttpModule::from_config())
        .module(ObserveModule::from_config())
        .module(DatabaseModule::from_config())
        .module(EventsModule::for_root())
        .module(ValidationModule::from_config())
        // Verifies the bearer tokens other services forward with their
        // messages (the same `auth.jwt.secret` everywhere), so
        // `#[authenticated]` handlers see the caller's user.
        .module(AuthModule::from_config())
        // Messages and events over `micro.transport` (NATS). `serve` starts
        // the listener next to the HTTP server; `micro:listen` runs it alone.
        .module(MicroModule::from_config())
        // Feature modules (`navree make:module` adds new ones above the marker).
        .module(modules::invoices::InvoicesModule)
    // navree:modules
}
