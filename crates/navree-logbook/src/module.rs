//! [`LogbookModule`] and the `logbook:clear` command.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use navree_config::{Config, ModuleBuilderConfigExt};
use navree_db::ChangeObserver;
use navree_http::{HttpOptions, ModuleBuilderHttpExt};
use navree_kernel::{
    AppContext, BoxError, BoxFuture, CommandSpec, Factory, Lifecycle, Module, ModuleBuilder,
    ModuleId, ProviderKey, Resolver,
};
use navree_telemetry::{ModuleBuilderTelemetryExt, TelemetrySink};

use crate::capture;
use crate::redact::Redactor;
use crate::store::{LogbookStore, MemoryStore};
#[cfg(feature = "sqlite")]
use crate::store::SqliteStore;
use crate::ui::LogbookController;
use crate::{Activation, Logbook, LogbookOptions, StoreKind};

/// How often the store is pruned while the app runs.
pub const PRUNE_EVERY: Duration = Duration::from_secs(5 * 60);

/// The Logbook module: a local debugging timeline of what the app does.
///
/// Global; provides and exports [`LogbookOptions`] and [`Logbook`]. When
/// Logbook is on (`logbook.enabled`, by default in debug builds only), it
/// registers the watchers (a telemetry sink and a data-change observer),
/// an HTTP layer that captures requests and responses, the UI and its API
/// at `logbook.path` (`/_logbook`), and prunes its store every five
/// minutes. Off, it adds no route, no layer and no watcher.
///
/// It needs `HttpModule` to serve the UI and record requests; every other
/// module it watches is optional. The command `logbook:clear` deletes every
/// entry.
///
/// ```no_run
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_http::HttpModule;
/// use navree_kernel::Navree;
/// use navree_logbook::LogbookModule;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
///     .module(ConfigModule::for_root(ConfigSources::defaults()))
///     .module(HttpModule::from_config())
///     .module(LogbookModule::from_config())
///     .run()
/// # }
/// ```
///
/// Used plainly (`.module(LogbookModule)`) or through
/// [`from_config`](LogbookModule::from_config), it reads the `logbook`
/// section, so the app must include `ConfigModule`;
/// [`for_root`](LogbookModule::for_root) takes the options directly.
#[derive(Clone, Copy, Debug, Default)]
pub struct LogbookModule;

impl LogbookModule {
    /// The module with explicit options.
    pub fn for_root(options: LogbookOptions) -> impl Module {
        ConfiguredLogbookModule {
            options: Some(options),
        }
    }

    /// The module configured from the `logbook` section at boot.
    pub fn from_config() -> impl Module {
        ConfiguredLogbookModule { options: None }
    }
}

impl Module for LogbookModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

/// `LogbookModule::for_root` / `from_config`: same module id, configured.
struct ConfiguredLogbookModule {
    options: Option<LogbookOptions>,
}

impl Module for ConfiguredLogbookModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<LogbookModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.options.clone());
    }
}

fn register(m: &mut ModuleBuilder, options: Option<LogbookOptions>) {
    m.global();
    match options {
        Some(options) => m.factory(Factory::<LogbookOptions>::new(move |_| {
            let options = options.clone();
            async move {
                options.check()?;
                Ok(Arc::new(options))
            }
        })),
        None => m.config::<LogbookOptions>(),
    };
    // The HTTP layer is applied after boot; it reads the built Logbook here.
    let built: Arc<OnceLock<Arc<Logbook>>> = Arc::new(OnceLock::new());
    let slot = built.clone();
    m.exports::<LogbookOptions>()
        .factory(
            Factory::<Logbook>::new(move |r: Resolver| {
                let slot = slot.clone();
                async move {
                    let options = r.get::<LogbookOptions>().await?;
                    let logbook = Arc::new(build(&r, (*options).clone()).await?);
                    let _ = slot.set(logbook.clone());
                    Ok(logbook)
                }
            })
            .deps(vec![ProviderKey::of::<LogbookOptions>()]),
        )
        .exports::<Logbook>()
        .telemetry_sink_factory(
            Factory::<dyn TelemetrySink>::new(|r: Resolver| async move {
                Ok(r.get::<Logbook>().await? as Arc<dyn TelemetrySink>)
            })
            .deps(vec![ProviderKey::of::<Logbook>()]),
        )
        .multi(
            Factory::<dyn ChangeObserver>::new(|r: Resolver| async move {
                Ok(r.get::<Logbook>().await? as Arc<dyn ChangeObserver>)
            })
            .deps(vec![ProviderKey::of::<Logbook>()]),
        )
        .http_layer(capture::layer(built))
        .controller::<LogbookController>()
        .factory(
            Factory::<Announce>::new(|r: Resolver| async move {
                Ok(Arc::new(Announce(r.get::<Logbook>().await?)))
            })
            .deps(vec![ProviderKey::of::<Logbook>()]),
        )
        .lifecycle::<Announce>()
        .command(CommandSpec::new(
            "logbook:clear",
            "Delete every Logbook entry",
            clear_command,
        ));
}

/// Builds the Logbook the options ask for: off, or recording into its
/// store with the writer and the pruner on the app's task tracker.
async fn build(r: &Resolver, options: LogbookOptions) -> Result<Logbook, BoxError> {
    if !options.is_enabled() {
        return Ok(Logbook::disabled(options));
    }
    let ctx = r.app();
    let store = open_store(&options).await?;
    let config = ctx.try_get::<Config>().await;
    let redactor = Redactor::from_config(config.as_deref());
    let shutdown = ctx.shutdown_token();
    let tracker = ctx.tracker();
    let logbook = Logbook::start_with(
        options,
        store,
        redactor,
        async move { shutdown.cancelled().await },
        |writer| {
            tracker.spawn(writer);
        },
    );
    if let Some(active) = logbook.active() {
        let _ = active.app.set(ctx.downgrade());
        if let Some(http) = ctx.try_get::<HttpOptions>().await {
            let _ = active.base_url.set(base_url(&http.addr));
        }
    }
    Ok(logbook)
}

/// The store of `options`.
async fn open_store(options: &LogbookOptions) -> Result<Arc<dyn LogbookStore>, BoxError> {
    Ok(match options.store {
        StoreKind::Memory => Arc::new(MemoryStore::new()),
        #[cfg(feature = "sqlite")]
        StoreKind::File => Arc::new(SqliteStore::open(&options.file).await.map_err(|error| {
            format!("logbook.file `{}` cannot be opened: {error}", options.file)
        })?),
        #[cfg(not(feature = "sqlite"))]
        StoreKind::File => return Err(NO_FILE_STORE.into()),
    })
}

/// Why `logbook.store = "file"` fails in a build without SQLite.
#[cfg(not(feature = "sqlite"))]
const NO_FILE_STORE: &str = "logbook.store = \"file\" keeps entries in a SQLite file, and this \
     build compiles no SQLite: enable the `sqlite` feature (navree's, or navree-logbook's), or \
     set logbook.store = \"memory\"";

/// `http://<addr>`, with a wildcard address read as loopback.
pub(crate) fn base_url(addr: &str) -> String {
    let addr = addr
        .replace("0.0.0.0", "127.0.0.1")
        .replace("[::]", "[::1]");
    format!("http://{addr}")
}

/// Says where Logbook records and serves at boot (and warns in a release
/// build), and runs the pruner.
struct Announce(Arc<Logbook>);

impl Lifecycle for Announce {
    fn on_bootstrap(&self, ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move {
            let Some(active) = self.0.active() else {
                return Ok(());
            };
            let options = self.0.options();
            if options.activation() == Activation::Forced {
                tracing::warn!(
                    target: "navree::logbook",
                    "Logbook is recording in a release build"
                );
            }
            let target = match options.store {
                StoreKind::File => options.file.clone(),
                StoreKind::Memory => "memory".to_owned(),
            };
            let url = active.base_url.get().map_or_else(
                || options.path.clone(),
                |base| format!("{base}{}", options.path),
            );
            tracing::info!(
                target: "navree::logbook",
                "recording to {target}, open {url}"
            );
            // Prune once now, before the app serves, then every five minutes.
            prune(&self.0).await;
            let logbook = self.0.clone();
            let shutdown = ctx.shutdown_token();
            ctx.tracker().spawn(async move {
                let start = tokio::time::Instant::now() + PRUNE_EVERY;
                let mut every = tokio::time::interval_at(start, PRUNE_EVERY);
                every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                loop {
                    tokio::select! {
                        () = shutdown.cancelled() => return,
                        _ = every.tick() => prune(&logbook).await,
                    }
                }
            });
            Ok(())
        })
    }
}

/// Prunes the store as Logbook's own work (nothing it emits is recorded).
async fn prune(logbook: &Logbook) {
    if let Err(error) = crate::logbook::own_work(logbook.prune()).await {
        tracing::warn!(target: "navree::logbook", error = %error, "cannot prune the Logbook store");
    }
}

/// `logbook:clear`: deletes every entry (also the file store's when
/// Logbook is off in this build).
async fn clear_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    if let Some(arg) = args.first() {
        return Err(format!("logbook:clear takes no arguments (got `{arg}`)").into());
    }
    let logbook = ctx.get::<Logbook>().await?;
    let deleted = if logbook.is_enabled() {
        logbook.clear().await?
    } else {
        let options = logbook.options();
        match options.store {
            #[cfg(feature = "sqlite")]
            StoreKind::File if std::path::Path::new(&options.file).exists() => {
                SqliteStore::open(&options.file).await?.clear().await?
            }
            _ => 0,
        }
    };
    println!("Deleted {deleted} Logbook entries.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_addresses_link_to_loopback() {
        assert_eq!(base_url("0.0.0.0:8080"), "http://127.0.0.1:8080");
        assert_eq!(base_url("127.0.0.1:3000"), "http://127.0.0.1:3000");
        assert_eq!(base_url("[::]:8080"), "http://[::1]:8080");
    }
}
