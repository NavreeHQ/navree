//! [`TimeModule`] and [`ModuleBuilderTimeExt`].

use std::sync::Arc;

use navree_config::ModuleBuilderConfigExt;
use navree_contracts::{Clock, SystemClock, ZoneSource};
use navree_http::ModuleBuilderHttpExt;
use navree_kernel::{Factory, Module, ModuleBuilder, ModuleId, ProviderKey, Resolver};

use crate::commands::commands;
use crate::controller::TimeController;
use crate::{TimeOptions, Zones};

/// Time zones for the app: the [`Zones`] service, the
/// [`CurrentZone`](crate::CurrentZone) extractor and the zone list.
///
/// Global; provides and exports [`Zones`] and [`TimeOptions`], and
/// contributes:
///
/// - `GET {http.api_prefix}/time/zones` ([`TimeController`]), public
///   reference data for zone pickers, unless `time.routes` is false;
/// - the command `time:zones`.
///
/// Zone sources come from other modules as `dyn ZoneSource` multi-bindings
/// (navree-workspace registers one for workspaces; register your users'
/// with [`ModuleBuilderTimeExt::zone_source`]). `from_config` and the plain
/// module read the `time` section, so the app must include
/// `ConfigModule`.
///
/// ```no_run
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_http::HttpModule;
/// use navree_kernel::Navree;
/// use navree_time::TimeModule;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
///     .module(ConfigModule::for_root(ConfigSources::defaults()))
///     .module(HttpModule::from_config())
///     .module(TimeModule::from_config()) // section `time`
///     .run()
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct TimeModule;

impl TimeModule {
    /// The module with explicit options (configure it once, in the app
    /// root). Invalid options fail the boot.
    pub fn for_root(options: TimeOptions) -> impl Module {
        ConfiguredTimeModule {
            options: Some(options),
        }
    }

    /// The module configured from the `time` section of `Config` at boot.
    pub fn from_config() -> impl Module {
        ConfiguredTimeModule { options: None }
    }
}

impl Module for TimeModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

/// `TimeModule::for_root` / `from_config`: same module id, configured.
struct ConfiguredTimeModule {
    options: Option<TimeOptions>,
}

impl Module for ConfiguredTimeModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<TimeModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.options.clone());
    }
}

fn register(m: &mut ModuleBuilder, options: Option<TimeOptions>) {
    m.global();
    match options {
        Some(options) => m.factory(Factory::<TimeOptions>::new(move |_| {
            let options = options.clone();
            async move {
                options
                    .check()
                    .map_err(|error| format!("invalid time options: {error}"))?;
                Ok(Arc::new(options))
            }
        })),
        None => m.config::<TimeOptions>(),
    };
    m.exports::<TimeOptions>()
        .factory(
            Factory::<Zones>::new(|r: Resolver| async move {
                let options = r.get::<TimeOptions>().await?;
                let sources = r.get_all::<dyn ZoneSource>().await?;
                let clock: Arc<dyn Clock> = r
                    .app()
                    .try_get::<dyn Clock>()
                    .await
                    .unwrap_or_else(|| Arc::new(SystemClock));
                Ok(Arc::new(
                    Zones::new(options.zone(), sources)
                        .with_holidays(options.holidays())
                        .with_clock(clock),
                ))
            })
            .deps(vec![ProviderKey::of::<TimeOptions>()]),
        )
        .exports::<Zones>()
        .controller::<TimeController>();
    for command in commands() {
        m.command(command);
    }
}

/// Time registrations on a [`ModuleBuilder`].
pub trait ModuleBuilderTimeExt {
    /// Registers `source` as a `dyn ZoneSource` multi-binding: the zones of
    /// one level (your users' zones, say) for [`Zones`] and
    /// [`CurrentZone`](crate::CurrentZone). Sources that need providers
    /// (a database) register a factory instead:
    /// `m.multi(Factory::<dyn ZoneSource>::new(..))`.
    fn zone_source<S: ZoneSource>(&mut self, source: S) -> &mut Self;
}

impl ModuleBuilderTimeExt for ModuleBuilder {
    fn zone_source<S: ZoneSource>(&mut self, source: S) -> &mut Self {
        let source: Arc<dyn ZoneSource> = Arc::new(source);
        self.multi(Factory::<dyn ZoneSource>::new(move |_| {
            let source = source.clone();
            async move { Ok(source) }
        }))
    }
}
