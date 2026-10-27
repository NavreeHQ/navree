//! [`LighthouseModule`] and the access subject [`LighthouseSubject`].

use std::sync::Arc;

use navree_access::{AccessModule, Subject};
use navree_config::{Config, ModuleBuilderConfigExt};
use navree_contracts::ModuleBuilderJobsExt;
use navree_db::navree_schema::{Schema, SchemaOptions};
use navree_db::{Db, DbOptions, ModuleBuilderDbExt};
use navree_http::{AppError, ModuleBuilderHttpExt};
use navree_kernel::{
    AppContext, BoxError, BoxFuture, Factory, Lifecycle, Module, ModuleBuilder, ModuleId,
    ProviderKey, Resolver,
};
use navree_schedule::ModuleBuilderScheduleExt;
use navree_telemetry::{ModuleBuilderTelemetryExt, TelemetrySink};
use sea_orm_migration::SchemaManager;

use crate::alert::DeliverAlert;
use crate::api::LighthouseApiController;
use crate::commands::commands;
use crate::dashboard::LighthouseDashboardController;
use crate::dispatch::Alerter;
use crate::engine::{EngineHandle, Labels, Processor};
use crate::migration::{convert_time_series_at_boot, create_tables, migrations};
use crate::options::{LighthouseOptions, StoreKind, parse_hh_mm};
use crate::redact::{DEFAULT_FRAGMENTS, Redactor};
use crate::service::{Lighthouse, LighthouseSink};
use crate::store::{LighthouseError, LighthouseStore};

/// The access subject of the dashboard, its JSON API and the MCP tools:
/// `Lighthouse`, with the permission prefix `lighthouse`, so the
/// permission `lighthouse.view` grants `can("view", "Lighthouse")` and
/// `lighthouse.resolve` grants `can("resolve", "Lighthouse")`.
#[derive(Clone, Copy, Debug, Default)]
pub struct LighthouseSubject;

impl Subject for LighthouseSubject {
    const SUBJECT: &'static str = "Lighthouse";
    const PERMISSION_PREFIX: &'static str = "lighthouse";
}

/// The ability to open the dashboard and read everything on it.
pub const VIEW: &str = "view";
/// The ability to resolve, ignore and reopen issues.
pub const RESOLVE: &str = "resolve";

/// Application monitoring inside the app.
///
/// Global; provides and exports [`Lighthouse`] and [`LighthouseOptions`],
/// and contributes:
///
/// - a telemetry sink receiving requests, queries, jobs, scheduled tasks,
///   mail, cache and outgoing calls and exceptions from every module, and
///   an HTTP layer keeping the masked headers of requests it stores;
/// - the `lighthouse` migration set (`lighthouse_issues`,
///   `lighthouse_occurrences`, `lighthouse_samples`,
///   `lighthouse_aggregates`, `lighthouse_alerts`);
/// - the dashboard at `lighthouse.path` (`/_lighthouse`) and its JSON API
///   under `{path}/api`, behind the `view` and `resolve` abilities on the
///   subject `Lighthouse` (registered with the permission prefix
///   `lighthouse`);
/// - the `lighthouse.alert` job, the `lighthouse:prune` and
///   `lighthouse:test-alerts` commands, and the daily `lighthouse:prune`
///   task for the scheduler;
/// - with the `mcp` feature, `LighthouseMcp`: tools, a resource and a
///   prompt on the app's MCP server when it has `McpModule`.
///
/// It needs `DatabaseModule` (unless `lighthouse.store = "off"`); the
/// dashboard needs `AccessModule` (and `AuthModule` and `CryptoModule` for
/// its sign-in); email alerts need `MailModule`.
#[derive(Clone, Copy, Debug, Default)]
pub struct LighthouseModule;

impl LighthouseModule {
    /// The module with explicit options. Invalid options fail the boot.
    pub fn for_root(options: LighthouseOptions) -> impl Module {
        ConfiguredLighthouseModule {
            options: Some(options),
        }
    }

    /// The module configured from the `lighthouse` section at boot.
    pub fn from_config() -> impl Module {
        ConfiguredLighthouseModule { options: None }
    }
}

impl Module for LighthouseModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

/// `LighthouseModule::for_root` / `from_config`: same module id.
struct ConfiguredLighthouseModule {
    options: Option<LighthouseOptions>,
}

impl Module for ConfiguredLighthouseModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<LighthouseModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.options.clone());
    }
}

fn register(m: &mut ModuleBuilder, options: Option<LighthouseOptions>) {
    m.global();
    let prune_at = options.as_ref().map(|options| options.prune_at.clone());
    match options {
        Some(options) => m.factory(Factory::<LighthouseOptions>::new(move |_| {
            let options = options.clone();
            async move {
                options.check().map_err(|errors| {
                    format!("invalid lighthouse options: {}", errors.join("; "))
                })?;
                Ok(Arc::new(options))
            }
        })),
        None => m.config::<LighthouseOptions>(),
    };
    m.exports::<LighthouseOptions>()
        .factory(
            Factory::<Lighthouse>::new(|r: Resolver| async move {
                let options = r.get::<LighthouseOptions>().await?;
                Ok(Arc::new(build(&r.app(), options).await?))
            })
            .deps(vec![ProviderKey::of::<LighthouseOptions>()]),
        )
        .exports::<Lighthouse>()
        .telemetry_sink_factory(
            Factory::<dyn TelemetrySink>::new(|r: Resolver| async move {
                let lighthouse = r.get::<Lighthouse>().await?;
                Ok(Arc::new(LighthouseSink { lighthouse }) as Arc<dyn TelemetrySink>)
            })
            .deps(vec![ProviderKey::of::<Lighthouse>()]),
        )
        .factory(
            Factory::<Runtime>::new(|r: Resolver| async move {
                Ok(Arc::new(Runtime(r.get::<Lighthouse>().await?)))
            })
            .deps(vec![ProviderKey::of::<Lighthouse>()]),
        )
        .lifecycle::<Runtime>()
        .import(AccessModule::for_subject::<LighthouseSubject>())
        .migrations(migrations())
        .job::<DeliverAlert>()
        .controller::<LighthouseDashboardController>()
        .controller::<LighthouseApiController>()
        .http_layer(crate::http::layer())
        .map_error::<LighthouseError>(map_lighthouse_error)
        .schedule(|s| match prune_at {
            // Known now: run the command daily at that time.
            Some(at) => {
                if let Some((hour, minute)) = parse_hh_mm(at.trim()) {
                    s.command("lighthouse:prune", &[])
                        .daily_at(&format!("{hour:02}:{minute:02}"))
                        .without_overlapping();
                }
            }
            // Read at boot: check every minute whether it is time.
            None => {
                s.task("lighthouse:prune", prune_if_due)
                    .every_minute()
                    .without_overlapping();
            }
        });
    for command in commands() {
        m.command(command);
    }
    #[cfg(feature = "mcp")]
    {
        use navree_mcp::ModuleBuilderMcpExt as _;
        m.mcp_server::<crate::mcp::LighthouseMcp>();
    }
}

/// The scheduled prune of an app configured from `lighthouse`: prunes when
/// the current UTC time is `lighthouse.prune_at`.
async fn prune_if_due(ctx: AppContext) -> Result<(), BoxError> {
    let lighthouse = ctx.get::<Lighthouse>().await?;
    let options = lighthouse.options();
    let Some((hour, minute)) = parse_hh_mm(options.prune_at.trim()) else {
        return Ok(());
    };
    // The scheduler's clock: the app's `dyn Clock`, else the system's.
    let now = match ctx.try_get::<dyn navree_contracts::Clock>().await {
        Some(clock) => clock.now(),
        None => chrono::Utc::now(),
    };
    use chrono::Timelike as _;
    if now.hour() != hour || now.minute() != minute {
        return Ok(());
    }
    if let Some(store) = lighthouse.store() {
        let summary = store.prune(options.retention_days).await?;
        tracing::info!(target: "navree::lighthouse", ?summary, "pruned");
    }
    Ok(())
}

/// Builds the service: the store (the app's database, or
/// `database_url`'s), the labels, the redactor, the engine and the
/// alerter.
async fn build(app: &AppContext, options: Arc<LighthouseOptions>) -> Result<Lighthouse, BoxError> {
    if options.store == StoreKind::Off {
        return Ok(Lighthouse::live(options, None, None, None));
    }
    let config = app.try_get::<Config>().await;
    let db = match &options.database_url {
        Some(url) => {
            let db = Db::connect(&DbOptions {
                url: url.expose().clone(),
                ..DbOptions::default()
            })
            .await
            .map_err(|error| format!("lighthouse.database_url: cannot connect: {error}"))?;
            create_tables(&SchemaManager::new(db.conn())).await?;
            // The app's `schema` section (TimescaleDB mode, extensions)
            // applies to this database too.
            let schema_options = match &config {
                Some(config) => config.typed::<SchemaOptions>()?,
                None => SchemaOptions::default(),
            };
            convert_time_series_at_boot(&Schema::new(db.conn()).with_options(schema_options))
                .await
                .map_err(|error| format!("lighthouse.database_url: {error}"))?;
            db
        }
        None => Db::clone(&*app.try_get::<Db>().await.ok_or(
            "LighthouseModule needs DatabaseModule in the app (or lighthouse.store = \"off\")",
        )?),
    };
    let release = options
        .release
        .clone()
        .filter(|release| !release.trim().is_empty())
        .or_else(|| config.as_ref().and_then(|c| c.get_string("app.version")))
        .unwrap_or_else(|| "unknown".to_owned());
    let base_url = match &options.url {
        Some(url) => url.trim_end_matches('/').to_owned(),
        None => {
            let app_url = config
                .as_ref()
                .and_then(|c| c.get_string("app.url"))
                .unwrap_or_default();
            format!("{}{}", app_url.trim_end_matches('/'), options.mount_path())
        }
    };
    let store = LighthouseStore::new(db, base_url, options.clone(), release.clone());
    let environment = options.environment_label();
    let alerter = Arc::new(Alerter::new(
        store.clone(),
        options.clone(),
        environment.clone(),
        release.clone(),
    ));
    let recorder = if options.enabled {
        let server = options.server_label();
        let process = process_name();
        let suffix = uuid::Uuid::now_v7().simple().to_string();
        let labels = Labels {
            environment,
            release,
            process_key: format!("{server}/{process}/{}", &suffix[24..]),
            server,
            process,
        };
        let mut fragments = log_redact(config.as_deref());
        fragments.extend(options.redact.iter().cloned());
        let (handle, rx) = EngineHandle::new(options.buffer);
        let processor = Processor::new(
            options.clone(),
            labels,
            Redactor::new(&fragments),
            handle.clone(),
        );
        Some((handle, processor, rx))
    } else {
        None
    };
    Ok(Lighthouse::live(
        options,
        Some(store),
        recorder,
        Some(alerter),
    ))
}

/// `log.redact` (a list or a comma-separated string), or its defaults.
fn log_redact(config: Option<&Config>) -> Vec<String> {
    let value = config.and_then(|config| config.section::<serde_json::Value>("log.redact").ok());
    match value {
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_owned))
            .collect(),
        Some(serde_json::Value::String(text)) => text
            .split(',')
            .map(|item| item.trim().to_owned())
            .filter(|item| !item.is_empty())
            .collect(),
        _ => DEFAULT_FRAGMENTS.iter().map(|s| (*s).to_owned()).collect(),
    }
}

/// The process label: the command the binary runs (`serve`, `queue:work`,
/// `schedule:work`, ...), `serve` without one.
fn process_name() -> String {
    std::env::args()
        .nth(1)
        .filter(|arg| !arg.starts_with('-') && !arg.is_empty())
        .unwrap_or_else(|| "serve".to_owned())
}

/// Starts the background writer when the app boots and flushes it at
/// shutdown.
struct Runtime(Arc<Lighthouse>);

impl Lifecycle for Runtime {
    fn on_init(&self, ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move { self.0.start(&ctx).await })
    }

    fn on_shutdown(&self, _ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move {
            self.0.stop().await;
            Ok(())
        })
    }
}

/// [`LighthouseError`]s returned from handlers: 404 for a missing issue or
/// trace, 400 for a bad request, 500 otherwise.
fn map_lighthouse_error(error: &LighthouseError) -> Option<AppError> {
    match error {
        LighthouseError::NotFound(what) => {
            Some(AppError::not_found().with_detail(format!("{what} not found")))
        }
        LighthouseError::Other(message) => Some(AppError::bad_request(message.clone())),
        LighthouseError::Db(_) => None,
    }
}
