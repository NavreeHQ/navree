//! [`EventsModule`]: provides the [`EventBus`] and the [`EventSink`] contract.

use std::sync::Arc;

use navree_contracts::EventSink;
use navree_kernel::{
    AppContext, BoxError, BoxFuture, Factory, Lifecycle, Module, ModuleBuilder, ModuleId,
    ProviderKey, Resolver,
};

use crate::EventBus;

/// The global module that provides the app's [`EventBus`], exported both as
/// `EventBus` and as `dyn EventSink` (the contract other framework crates
/// publish through without depending on navree-events).
///
/// Add it once, in the app root, with [`EventsModule::for_root`]; feature
/// modules subscribe listeners with
/// [`ModuleBuilderEventsExt`](crate::ModuleBuilderEventsExt) and inject the
/// bus without importing this module. It has no options, so the plain
/// `.module(EventsModule)` works too.
///
/// Queued listeners are dispatched through `Arc<dyn Dispatcher>` (provided by
/// the queue module); without one they run inline.
#[derive(Clone, Copy, Debug, Default)]
pub struct EventsModule;

impl EventsModule {
    /// The module, for the app root: `Navree::app().module(EventsModule::for_root())`.
    pub fn for_root() -> impl Module {
        ConfiguredEventsModule
    }
}

impl Module for EventsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m);
    }
}

/// `EventsModule::for_root()`: same module id, configured.
struct ConfiguredEventsModule;

impl Module for ConfiguredEventsModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<EventsModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m);
    }
}

/// Global; provides and exports `EventBus` and `dyn EventSink`, and detaches
/// the bus from the app at shutdown.
fn register(m: &mut ModuleBuilder) {
    m.global()
        .factory(Factory::<EventBus>::new(|r: Resolver| async move {
            Ok(Arc::new(EventBus::new(r.app())))
        }))
        .exports::<EventBus>()
        .factory(
            Factory::<dyn EventSink>::new(|r: Resolver| async move {
                let bus: Arc<dyn EventSink> = r.get::<EventBus>().await?;
                Ok(bus)
            })
            .deps(vec![ProviderKey::of::<EventBus>()]),
        )
        .exports::<dyn EventSink>()
        .factory(
            Factory::<DetachOnShutdown>::new(|r: Resolver| async move {
                Ok(Arc::new(DetachOnShutdown(r.get::<EventBus>().await?)))
            })
            .deps(vec![ProviderKey::of::<EventBus>()]),
        )
        .lifecycle::<DetachOnShutdown>();
}

/// Detaches the [`EventBus`] from the app at shutdown, so no event is
/// delivered after the app's shutdown hooks ran.
struct DetachOnShutdown(Arc<EventBus>);

impl Lifecycle for DetachOnShutdown {
    fn on_shutdown(&self, _ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        self.0.detach();
        Box::pin(async { Ok(()) })
    }
}
