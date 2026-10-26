//! [`app`]: the platform modules and the API's own modules.

use navree::prelude::*;

use crate::deals::DealsModule;
use crate::world::WorldModule;

/// The benchmark API configured from `sources`: the Navree modules it needs
/// (configuration, logging, crypto for auth, HTTP, database, validation,
/// auth), then `WorldModule` (`/json`, `/db`, `/queries`) and `DealsModule`
/// (`/api/deals`).
pub fn app(sources: ConfigSources) -> AppBuilder {
    Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(LoggerModule::from_config())
        .module(CryptoModule::from_config())
        .module(HttpModule::from_config())
        .module(DatabaseModule::from_config())
        .module(ValidationModule::from_config())
        .module(AuthModule::from_config())
        .module(WorldModule)
        .module(DealsModule)
}
