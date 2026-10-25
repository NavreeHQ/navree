//! [`LoggerModule`].

use std::sync::Arc;

use keel_config::ModuleBuilderConfigExt;
use keel_kernel::{
    AppContext, BoxError, BoxFuture, Factory, Lifecycle, Module, ModuleBuilder, ModuleId,
    ProviderKey, Resolver,
};

use crate::{LogControl, LogOptions, install};

/// The global logging module: installs Keel's tracing subscriber and exports
/// [`LogControl`] and [`LogOptions`] to every module.
///
/// - [`LoggerModule::for_root`] takes the options directly;
/// - [`LoggerModule::from_config`], or a plain `LoggerModule`, reads the `log`
///   section of the app configuration at boot (so the app must include
///   `ConfigModule`); every key is optional.
///
/// ```no_run
/// use keel_config::{ConfigModule, ConfigSources};
/// use keel_kernel::Keel;
/// use keel_log::LoggerModule;
///
/// # fn main() -> Result<(), keel_kernel::BoxError> {
/// Keel::app()
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
        ConfiguredLoggerModule {
            options: Some(options),
        }
    }

    /// The module configured from the `log` section of the app's configuration.
    pub fn from_config() -> impl Module {
        ConfiguredLoggerModule { options: None }
    }
}

impl Module for LoggerModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

/// `for_root(...)` / `from_config()`: same module id, configured.
struct ConfiguredLoggerModule {
    /// `None`: read the `log` configuration section.
    options: Option<LogOptions>,
}

impl Module for ConfiguredLoggerModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<LoggerModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.options.clone());
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
            // Opening the log file touches the disk: keep it off async workers.
            let control = tokio::task::spawn_blocking(move || install(&options)).await??;
            Ok(Arc::new(control))
        })
        .deps(vec![ProviderKey::of::<LogOptions>()]),
    )
    .factory(
        Factory::<FlushOnShutdown>::new(|r: Resolver| async move {
            Ok(Arc::new(FlushOnShutdown(r.get::<LogControl>().await?)))
        })
        .deps(vec![ProviderKey::of::<LogControl>()]),
    )
    .lifecycle::<FlushOnShutdown>()
    .exports::<LogControl>()
    .exports::<LogOptions>();
}

/// Flushes the log file when the app shuts down.
struct FlushOnShutdown(Arc<LogControl>);

impl Lifecycle for FlushOnShutdown {
    fn on_shutdown(&self, _ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        let control = self.0.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || control.flush()).await??;
            Ok(())
        })
    }
}
