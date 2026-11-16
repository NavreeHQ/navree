//! Cache stores a module registers under a name: [`ModuleBuilderCacheExt`]
//! and [`CacheBackend`].

use std::future::Future;
use std::sync::Arc;

use navree_config::__private::register_driver;
use navree_config::{DriverContext, DriverSlot};
use navree_kernel::{BoxError, ModuleBuilder};

use crate::{CacheStore, LockStore};

/// The selector `cache.store`, its built-in stores and the method that
/// registers more.
pub(crate) const SLOT: DriverSlot = DriverSlot::new(
    "cache.store",
    &["memory", "redis"],
    "m.cache_store",
    "cache store",
);

/// The two halves of a cache store a module registers: where entries live
/// and where locks are taken. A registration's factory returns one.
///
/// ```
/// use std::sync::Arc;
///
/// use navree_cache::{CacheBackend, MemoryLockStore, MemoryStore};
///
/// let backend = CacheBackend::new(
///     Arc::new(MemoryStore::with_capacity(50_000)),
///     Arc::new(MemoryLockStore::new()),
/// );
/// # drop(backend);
/// ```
#[derive(Clone)]
#[non_exhaustive]
pub struct CacheBackend {
    pub(crate) store: Arc<dyn CacheStore>,
    pub(crate) locks: Arc<dyn LockStore>,
}

impl CacheBackend {
    /// The backend over `store` (entries) and `locks`.
    pub fn new(store: Arc<dyn CacheStore>, locks: Arc<dyn LockStore>) -> CacheBackend {
        CacheBackend { store, locks }
    }
}

/// The backend of the store `cache.store` names when a module registered
/// it; `None` for a built-in store.
pub(crate) struct ChosenBackend(pub(crate) Option<Arc<CacheBackend>>);

/// Registers cache stores on a [`ModuleBuilder`]: `use
/// navree::plugin::prelude::*;` (or this trait) brings the method.
///
/// ```no_run
/// use std::sync::Arc;
///
/// use navree_cache::{CacheBackend, CacheModule, MemoryLockStore, MemoryStore, ModuleBuilderCacheExt};
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_kernel::{Module, ModuleBuilder, Navree};
/// use serde::Deserialize;
///
/// /// The settings of the store: `[cache.two_tier]`.
/// #[derive(Default, Deserialize)]
/// #[serde(default)]
/// struct TwoTierSettings {
///     capacity: usize,
/// }
///
/// struct TwoTierModule;
///
/// impl Module for TwoTierModule {
///     fn register(&self, m: &mut ModuleBuilder) {
///         // Selected with `cache.store = "two_tier"`.
///         m.cache_store("two_tier", |ctx| async move {
///             let settings: TwoTierSettings = ctx.settings()?;
///             let store = Arc::new(MemoryStore::with_capacity(settings.capacity.max(1)));
///             Ok(CacheBackend::new(store, Arc::new(MemoryLockStore::new())))
///         });
///     }
/// }
///
/// fn main() -> Result<(), navree_kernel::BoxError> {
///     Navree::app()
///         .module(ConfigModule::for_root(ConfigSources::defaults()))
///         .module(CacheModule::from_config())
///         .module(TwoTierModule)
///         .run()
/// }
/// ```
pub trait ModuleBuilderCacheExt {
    /// Registers a cache store under `name` (a lower-case letter followed
    /// by up to 63 lower-case letters, digits or `_`): `cache.store = "{name}"`
    /// selects it, and `factory` builds it at boot from its
    /// [`DriverContext`], whose settings are `cache.{name}`.
    ///
    /// A name that is not such a word, a built-in name (`memory`, `redis`),
    /// a key of the `cache` section (`prefix`) and a name two modules
    /// register fail the boot, naming `m.cache_store` and the module, also
    /// when another store is selected; `config:check` says the same. A
    /// factory that fails fails the boot, naming the store and the module. A
    /// factory's error is shown in the boot error as written, so it must not
    /// contain a secret from the driver's settings.
    fn cache_store<F, Fut>(&mut self, name: &'static str, factory: F) -> &mut Self
    where
        F: Fn(DriverContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<CacheBackend, BoxError>> + Send + 'static;
}

impl ModuleBuilderCacheExt for ModuleBuilder {
    fn cache_store<F, Fut>(&mut self, name: &'static str, factory: F) -> &mut Self
    where
        F: Fn(DriverContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<CacheBackend, BoxError>> + Send + 'static,
    {
        register_driver::<CacheBackend, _, _>(self, &SLOT, Some("cache"), name, move |ctx| {
            let built = factory(ctx);
            async move { built.await.map(Arc::new) }
        })
    }
}
