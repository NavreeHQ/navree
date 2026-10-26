//! [`TelemetryModule`] and [`ModuleBuilderTelemetryExt`].

use std::sync::Arc;

use navree_kernel::{
    AppContext, BoxError, BoxFuture, Factory, Injectable, Lifecycle, Module, ModuleBuilder,
    ProviderKey, Resolver,
};

use crate::{Telemetry, TelemetrySink};

/// The telemetry registry module. Global; provides and exports the app's
/// [`Telemetry`]: every `dyn TelemetrySink` multi-binding, installed when
/// the app boots (in `on_init`, after every provider is built, so a sink
/// may depend on a provider that emits) and turned off at shutdown.
///
/// You rarely add it yourself: registering a sink with
/// [`ModuleBuilderTelemetryExt`] imports it. An app without any sink has no
/// `TelemetryModule`, and its emitters hold a
/// [`disabled`](Telemetry::disabled) registry.
#[derive(Clone, Copy, Debug, Default)]
pub struct TelemetryModule;

impl Module for TelemetryModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.global()
            .factory(Factory::<Telemetry>::new(|_| async {
                Ok(Arc::new(Telemetry::pending()))
            }))
            .exports::<Telemetry>()
            .factory(
                Factory::<Activator>::new(|r: Resolver| async move {
                    Ok(Arc::new(Activator(Telemetry::clone(
                        &*r.get::<Telemetry>().await?,
                    ))))
                })
                .deps(vec![ProviderKey::of::<Telemetry>()]),
            )
            .lifecycle::<Activator>();
    }
}

/// Installs the sinks at boot and turns delivery off at shutdown.
struct Activator(Telemetry);

impl Lifecycle for Activator {
    fn on_init(&self, ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move {
            let sinks = ctx.get_all::<dyn TelemetrySink>().await?;
            self.0.activate(sinks);
            Ok(())
        })
    }

    fn on_shutdown(&self, _ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        self.0.deactivate();
        Box::pin(async { Ok(()) })
    }
}

/// Registers telemetry sinks on a [`ModuleBuilder`]. Each method imports
/// [`TelemetryModule`] and adds a `dyn TelemetrySink` multi-binding.
///
/// ```
/// use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
/// use navree_telemetry::{EventKind, Interest, ModuleBuilderTelemetryExt, TelemetryEvent, TelemetryRecord, TelemetrySink};
///
/// /// Counts failed jobs.
/// #[derive(Default)]
/// struct FailedJobs(std::sync::atomic::AtomicU64);
///
/// impl TelemetrySink for FailedJobs {
///     fn interest(&self) -> Interest {
///         Interest::only([EventKind::JobFailed])
///     }
///
///     fn record(&self, record: &TelemetryRecord) {
///         if let TelemetryEvent::JobFailed(_) = &record.event {
///             self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
///         }
///     }
/// }
///
/// impl Injectable for FailedJobs {
///     fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
///         Box::pin(async { Ok(FailedJobs::default()) })
///     }
/// }
///
/// struct MonitoringModule;
///
/// impl Module for MonitoringModule {
///     fn register(&self, m: &mut ModuleBuilder) {
///         m.telemetry_sink::<FailedJobs>();
///     }
/// }
/// ```
pub trait ModuleBuilderTelemetryExt {
    /// Registers provider `S` (built by the container) and makes it a sink.
    fn telemetry_sink<S: TelemetrySink + Injectable>(&mut self) -> &mut Self;

    /// Registers an already-built sink.
    fn telemetry_sink_value<S: TelemetrySink>(&mut self, sink: S) -> &mut Self;

    /// Registers a sink built by `factory` (for sinks that need setup, or a
    /// provider the module already registers).
    fn telemetry_sink_factory(&mut self, factory: Factory<dyn TelemetrySink>) -> &mut Self;
}

impl ModuleBuilderTelemetryExt for ModuleBuilder {
    fn telemetry_sink<S: TelemetrySink + Injectable>(&mut self) -> &mut Self {
        self.provider::<S>();
        self.telemetry_sink_factory(
            Factory::<dyn TelemetrySink>::new(|r: Resolver| async move {
                Ok(r.get::<S>().await? as Arc<dyn TelemetrySink>)
            })
            .deps(vec![ProviderKey::of::<S>()]),
        )
    }

    fn telemetry_sink_value<S: TelemetrySink>(&mut self, sink: S) -> &mut Self {
        let sink: Arc<dyn TelemetrySink> = Arc::new(sink);
        self.telemetry_sink_factory(Factory::<dyn TelemetrySink>::new(move |_| {
            let sink = sink.clone();
            async move { Ok(sink) }
        }))
    }

    fn telemetry_sink_factory(&mut self, factory: Factory<dyn TelemetrySink>) -> &mut Self {
        self.import(TelemetryModule).multi(factory)
    }
}
