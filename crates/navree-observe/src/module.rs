//! [`ObserveModule`].

use std::sync::{Arc, Mutex};
use std::time::Duration;

use navree_config::{Config, ModuleBuilderConfigExt};
use navree_contracts::HealthCheck;
use navree_http::ModuleBuilderHttpExt;
use navree_kernel::{
    AppContext, BoxError, Factory, Module, ModuleBuilder, ModuleId, ProviderKey, Resolver,
};
use navree_telemetry::{ModuleBuilderTelemetryExt, TelemetrySink};

use crate::controller::ObserveController;
use crate::export::{self, Fanout, NoExport, SpanExport, WriterExport};
use crate::metrics::MetricsRecorder;
use crate::{Health, ObserveOptions, TracingOptions};

/// The observability module. Global; provides and exports
/// [`ObserveOptions`] and [`Health`] (every `dyn HealthCheck` of the app),
/// mounts `GET /health/live`, `GET /health/ready` and (with metrics on)
/// `GET /metrics` when the app has `HttpModule`, and installs the
/// Prometheus `metrics` recorder at boot, once per process.
///
/// ```no_run
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_http::HttpModule;
/// use navree_kernel::Navree;
/// use navree_observe::ObserveModule;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
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
/// keep answering: they are in navree-http's default
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
        .controller::<ObserveController>()
        .factory(Factory::<TracingOptions>::new(|r: Resolver| async move {
            let options = match r.app().try_get::<Config>().await {
                Some(config) => TracingOptions::from_config(&config)?,
                None => TracingOptions::default(),
            };
            Ok(Arc::new(options))
        }))
        .exports::<TracingOptions>()
        .telemetry_sink_factory(
            Factory::<dyn TelemetrySink>::new(|r: Resolver| async move {
                let options = r.get::<TracingOptions>().await?;
                span_exporter(&r.app(), &options).await
            })
            .deps(vec![ProviderKey::of::<TracingOptions>()]),
        );
}

/// The telemetry sink that exports spans: a queue drained by a task on the
/// app's tracker (flushed at shutdown), to stdout and/or OTLP; a sink that
/// wants nothing when spans go nowhere.
async fn span_exporter(
    app: &AppContext,
    options: &TracingOptions,
) -> Result<Arc<dyn TelemetrySink>, BoxError> {
    if !options.exports() {
        return Ok(Arc::new(NoExport));
    }
    let mut exporters: Vec<Arc<dyn SpanExport>> = Vec::new();
    if options.stdout {
        exporters.push(Arc::new(WriterExport(Mutex::new(std::io::stdout()))));
    }
    if options.endpoint.is_some()
        && let Some(exporter) = otlp_exporter(app, options).await?
    {
        exporters.push(exporter);
    }
    if exporters.is_empty() {
        return Ok(Arc::new(NoExport));
    }
    let token = app.shutdown_token();
    let (sink, task) = export::start(
        Arc::new(Fanout(exporters)),
        options.max_queue_size,
        Duration::from_millis(options.timeout_ms),
        async move { token.cancelled().await },
    );
    app.tracker().spawn(task);
    Ok(Arc::new(sink))
}

/// The OTLP exporter for `observe.tracing.endpoint`.
#[cfg(feature = "otel")]
async fn otlp_exporter(
    app: &AppContext,
    options: &TracingOptions,
) -> Result<Option<Arc<dyn SpanExport>>, BoxError> {
    let version = match app.try_get::<Config>().await {
        Some(config) => config.get_string("app.version"),
        None => None,
    };
    let exporter = crate::otlp::OtlpExport::new(options, version)?;
    Ok(Some(Arc::new(exporter)))
}

/// Without the `otel` feature there is no OTLP exporter: an endpoint set in
/// the configuration fails the boot; one from `OTEL_EXPORTER_OTLP_ENDPOINT`
/// is ignored with a warning.
#[cfg(not(feature = "otel"))]
async fn otlp_exporter(
    _app: &AppContext,
    options: &TracingOptions,
) -> Result<Option<Arc<dyn SpanExport>>, BoxError> {
    if options.endpoint_from_env {
        tracing::warn!(
            target: "navree::tracing",
            "OTEL_EXPORTER_OTLP_ENDPOINT is set, but navree-observe was built without its `otel` feature: spans are not exported"
        );
        return Ok(None);
    }
    Err("observe.tracing.endpoint is set, but navree-observe was built without its `otel` feature: turn on the `otel` feature of the navree crate (or of navree-observe) to export spans over OTLP".into())
}
