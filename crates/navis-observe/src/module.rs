//! [`ObserveModule`].

use std::sync::Arc;

use navis_config::ModuleBuilderConfigExt;
use navis_contracts::HealthCheck;
use navis_http::ModuleBuilderHttpExt;
use navis_kernel::{Factory, Module, ModuleBuilder, ModuleId, Resolver};

use crate::controller::ObserveController;
use crate::metrics::MetricsRecorder;
use crate::{Health, ObserveOptions};

/// The observability module. Global; provides and exports
/// [`ObserveOptions`] and [`Health`] (every `dyn HealthCheck` of the app),
/// mounts `GET /health/live`, `GET /health/ready` and (with metrics on)
/// `GET /metrics` when the app has `HttpModule`, and installs the
/// Prometheus `metrics` recorder at boot, once per process.
///
/// ```no_run
/// use navis_config::{ConfigModule, ConfigSources};
/// use navis_http::HttpModule;
/// use navis_kernel::Navis;
/// use navis_observe::ObserveModule;
///
/// # fn main() -> Result<(), navis_kernel::BoxError> {
/// Navis::app()
///     .module(ConfigModule::for_root(ConfigSources::defaults()))
///     .module(HttpModule::from_config())
///     .module(ObserveModule::from_config()) // section `observe`
///     .run()
/// # }
/// ```
///
/// Used plainly (`.module(ObserveModule)`) or through
/// [`from_config`](ObserveModule::from_config), it reads the `observe`
/// configuration section, so the app must include `ConfigModule`;
/// [`for_root`](ObserveModule::for_root) needs no configuration. An empty
/// `metrics_token` fails the boot, and so does a `metrics` recorder that
/// something else already installed in the process (set
/// `observe.metrics = false` to keep it).
///
/// Because the recorder is process-wide, apps booted in the same process
/// (tests) share their metrics. In maintenance mode (`down`) these routes
/// keep answering: they are in navis-http's default
/// `http.maintenance_except` list.
#[derive(Clone, Copy, Debug, Default)]
pub struct ObserveModule;

impl ObserveModule {
    /// The module with explicit options (configure it once, in the app root).
    pub fn for_root(options: ObserveOptions) -> impl Module {
        ConfiguredObserveModule {
            options: Some(options),
        }
    }

    /// The module configured from the `observe` section of `Config` at boot.
    pub fn from_config() -> impl Module {
        ConfiguredObserveModule { options: None }
    }
}

impl Module for ObserveModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

/// `ObserveModule::for_root` / `from_config`: same module id, configured.
struct ConfiguredObserveModule {
    /// `None`: read from configuration.
    options: Option<ObserveOptions>,
}

impl Module for ConfiguredObserveModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<ObserveModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.options.clone());
    }
}

fn register(m: &mut ModuleBuilder, options: Option<ObserveOptions>) {
    m.global();
    match options {
        Some(options) => m.factory(Factory::<ObserveOptions>::new(move |_| {
            let options = options.clone();
            async move {
                options.check()?;
                Ok(Arc::new(options))
            }
        })),
        None => m.config::<ObserveOptions>(),
    };
    m.exports::<ObserveOptions>()
        .factory(Factory::<Health>::new(|r: Resolver| async move {
            let checks = r.get_all::<dyn HealthCheck>().await?;
            Ok(Arc::new(Health::new(checks)))
        }))
        .exports::<Health>()
        .provider::<MetricsRecorder>()
        .lifecycle::<MetricsRecorder>()
        .controller::<ObserveController>();
}
