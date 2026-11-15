//! Backplanes a module registers under a name: [`ModuleBuilderBackplaneExt`].

use std::future::Future;
use std::sync::Arc;

use navree_config::__private::register_driver;
use navree_config::{DriverContext, DriverSlot};
use navree_kernel::{BoxError, ModuleBuilder};

use crate::Backplane;

/// The selector `realtime.backplane`, its built-in backplanes and the method
/// that registers more.
pub(crate) const SLOT: DriverSlot = DriverSlot::new(
    "realtime.backplane",
    &["none", "redis"],
    "m.backplane",
    "backplane",
);

/// What the boot builds before `Realtime`: it holds that every registration
/// of a backplane was checked. A provider of its own, so an app that
/// replaces `Realtime` still has its registrations checked.
pub(crate) struct CheckedBackplanes;

/// Registers backplanes on a [`ModuleBuilder`]: `use
/// navree::plugin::prelude::*;` (or this trait) brings the method.
///
/// ```no_run
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_http::HttpModule;
/// use navree_kernel::{BoxError, Module, ModuleBuilder, Navree};
/// use navree_realtime::{MemoryBackplane, ModuleBuilderBackplaneExt, RealtimeModule};
///
/// struct KabootarModule {
///     // Every app given a clone of this backplane shares its broadcasts.
///     backplane: MemoryBackplane,
/// }
///
/// impl Module for KabootarModule {
///     fn register(&self, m: &mut ModuleBuilder) {
///         let backplane = self.backplane.clone();
///         // Selected with `realtime.backplane = "kabootar"`.
///         m.backplane("kabootar", move |_ctx| {
///             let backplane = backplane.clone();
///             async move { Ok(backplane) }
///         });
///     }
/// }
///
/// fn main() -> Result<(), BoxError> {
///     Navree::app()
///         .module(ConfigModule::for_root(ConfigSources::defaults()))
///         .module(HttpModule::from_config())
///         .module(RealtimeModule::from_config())
///         .module(KabootarModule { backplane: MemoryBackplane::new() })
///         .run()
/// }
/// ```
pub trait ModuleBuilderBackplaneExt {
    /// Registers a backplane under `name` (a lower-case letter followed by up
    /// to 63 lower-case letters, digits or `_`). Then
    /// `realtime.backplane = "{name}"` selects it, and `factory` builds it at
    /// boot from its [`DriverContext`], whose settings are `realtime.{name}`.
    /// Like the Redis backplane it needs `realtime.secret`.
    ///
    /// A name that is not such a word, a built-in name (`none`, `redis`), a
    /// key of the `realtime` section (`secret`) and a name two modules
    /// register fail the boot, naming `m.backplane` and the module, also when
    /// another backplane is selected; `config:check` says the same. A factory
    /// that fails fails the boot, naming the backplane and the module. A
    /// factory's error is shown in the boot error as written, so it must not
    /// contain a secret from the driver's settings.
    fn backplane<B, F, Fut>(&mut self, name: &'static str, factory: F) -> &mut Self
    where
        B: Backplane,
        F: Fn(DriverContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<B, BoxError>> + Send + 'static;
}

impl ModuleBuilderBackplaneExt for ModuleBuilder {
    fn backplane<B, F, Fut>(&mut self, name: &'static str, factory: F) -> &mut Self
    where
        B: Backplane,
        F: Fn(DriverContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<B, BoxError>> + Send + 'static,
    {
        register_driver::<dyn Backplane, _, _>(self, &SLOT, Some("realtime"), name, move |ctx| {
            let built = factory(ctx);
            async move {
                built
                    .await
                    .map(|backplane| Arc::new(backplane) as Arc<dyn Backplane>)
            }
        })
    }
}
