//! [`LoggerModule`].

use std::sync::Arc;

use navree_config::ModuleBuilderConfigExt;
use navree_kernel::{
    AppContext, BoxError, BoxFuture, ConfiguredModule, Factory, Lifecycle, Module, ModuleBuilder,
    ProviderKey, Resolver,
};
use navree_telemetry::Telemetry;

use crate::telemetry::{clear_background, set_background};
use crate::{Console, LogControl, LogOptions, install};

/// The global logging module: installs Navree's tracing subscriber and exports
/// [`LogControl`] and [`LogOptions`] to every module.
///
/// - [`LoggerModule::for_root`] takes the options directly;
/// - [`LoggerModule::from_config`], or a plain `LoggerModule`, reads the `log`
///   section of the app configuration at boot (so the app must include
///   `ConfigModule`); every key is optional.
///
/// ```no_run
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_kernel::Navree;
/// use navree_log::LoggerModule;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
///     .module(ConfigModule::for_root(ConfigSources::defaults()))
///     .module(LoggerModule::from_config())
///     .run()
/// # }
/// ```
///
/// The subscriber is process-wide and installed once, by the first boot
/// (see [`init_for_tests`](crate::init_for_tests) for tests). Later boots in
/// the same process (tests, a rebuilt container) reuse it: their `level` and
/// `levels` replace the current filter, while `format`, `file` and `redact`
/// stay as first installed. If another global subscriber was installed
/// first, it is left in place and [`LogControl`] cannot change levels.
///
/// At shutdown the module waits for queued log-file lines to be written.
/// Shutdown hooks run in reverse module order, so list `LoggerModule` right
/// after `ConfigModule`: its flush then runs after every other module's
/// shutdown hook has logged.
#[derive(Clone, Copy, Debug, Default)]
pub struct LoggerModule;

impl LoggerModule {
    /// The module configured with `options` (configure it once, in the app root).
    pub fn for_root(options: LogOptions) -> impl Module {
        ConfiguredModule::<LoggerModule, _>::for_root(options, register)
    }

    /// The module configured from the `log` section of the app's configuration.
    pub fn from_config() -> impl Module {
        ConfiguredModule::<LoggerModule, LogOptions>::from_config(register)
    }
}

impl Module for LoggerModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

/// Global; provides `LogOptions` (given or from config), installs the
/// subscriber when `LogControl` is built, and flushes the file at shutdown.
fn register(m: &mut ModuleBuilder, options: Option<LogOptions>) {
    m.global();
    match options {
        Some(options) => m.value(options),
        None => m.config::<LogOptions>(),
    };
    m.factory(
        Factory::<LogControl>::new(|r: Resolver| async move {
            let options = r.get::<LogOptions>().await?;
            // A command that speaks a protocol on stdout gets the console
            // lines on stderr.
            let console = Console::for_command(r.app().command());
            // Opening the log file touches the disk: keep it off async workers.
            let control = tokio::task::spawn_blocking(move || install(&options, console)).await??;
            Ok(Arc::new(control))
        })
        .deps(vec![ProviderKey::of::<LogOptions>()]),
    )
    .factory(
        Factory::<FlushOnShutdown>::new(|r: Resolver| async move {
            let control = r.get::<LogControl>().await?;
            // Lines written outside a request or job go to this app's
            // telemetry sinks while it runs.
            let telemetry = r.app().try_get::<Telemetry>().await.map(|t| (*t).clone());
            if let Some(telemetry) = &telemetry {
                set_background(telemetry.clone());
            }
            Ok(Arc::new(FlushOnShutdown { control, telemetry }))
        })
        .deps(vec![ProviderKey::of::<LogControl>()]),
    )
    .lifecycle::<FlushOnShutdown>()
    .exports::<LogControl>()
    .exports::<LogOptions>();
}

/// Flushes the log file when the app shuts down, and stops sending
/// background log lines to the app's telemetry.
struct FlushOnShutdown {
    control: Arc<LogControl>,
    telemetry: Option<Telemetry>,
}

impl Lifecycle for FlushOnShutdown {
    fn on_shutdown(&self, _ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        if let Some(telemetry) = &self.telemetry {
            clear_background(telemetry);
        }
        let control = self.control.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || control.flush()).await??;
            Ok(())
        })
    }
}
