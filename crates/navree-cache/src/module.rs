//! [`CacheModule`], [`CacheOptions`] and [`CacheStoreKind`].

use std::sync::Arc;
use std::time::Duration;

use navree_config::{ConfigSection, ModuleBuilderConfigExt};
use navree_contracts::{BoxFuture, HealthCheck, HealthStatus};
use navree_kernel::{CommandSpec, Factory, Module, ModuleBuilder, ModuleId, ProviderKey, Resolver};
use navree_telemetry::Telemetry;
use serde::{Deserialize, Serialize};

use crate::cache::DEFAULT_TTL;
use crate::{Cache, CacheError, CacheStore, LockStore, Locks, MemoryLockStore, MemoryStore};

/// Which backend stores cache entries and locks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CacheStoreKind {
    /// In-process: [`MemoryStore`] and [`MemoryLockStore`] (`"memory"`).
    #[default]
    Memory,
    /// Redis (`"redis"`): the `dyn CacheStore` and `dyn LockStore`
    /// providers named `redis`, which navree-redis's `RedisModule`
    /// registers. Shared by every process using the same Redis server.
    Redis,
}

impl CacheStoreKind {
    /// The configuration name (`memory`, `redis`), also the store's name in
    /// telemetry `CacheEvent`s.
    pub fn as_str(self) -> &'static str {
        match self {
            CacheStoreKind::Memory => "memory",
            CacheStoreKind::Redis => "redis",
        }
    }
}

/// `CacheModule` settings: configuration section `cache`.
///
/// ```toml
/// [cache]
/// store = "memory"
/// prefix = "crm:"
/// default_ttl_secs = 3600
/// ```
///
/// Every field has a default, so a missing section is valid.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CacheOptions {
    /// The backend (default `memory`).
    pub store: CacheStoreKind,
    /// Prepended to every cache key and lock name (default empty).
    pub prefix: String,
    /// The TTL of [`Ttl::Default`](crate::Ttl::Default), in seconds; must be
    /// greater than 0 (default 3600).
    pub default_ttl_secs: u64,
}

impl CacheOptions {
    /// [`default_ttl_secs`](CacheOptions::default_ttl_secs) as a `Duration`.
    pub fn default_ttl(&self) -> Duration {
        Duration::from_secs(self.default_ttl_secs)
    }
}

impl Default for CacheOptions {
    fn default() -> Self {
        Self {
            store: CacheStoreKind::Memory,
            prefix: String::new(),
            default_ttl_secs: DEFAULT_TTL.as_secs(),
        }
    }
}

impl ConfigSection for CacheOptions {
    const KEY: &'static str = "cache";

    fn validate(&self) -> Result<(), String> {
        if self.default_ttl_secs == 0 {
            return Err("default_ttl_secs must be greater than 0".to_owned());
        }
        Ok(())
    }
}

/// The global module providing [`Cache`] and [`Locks`] (plus the
/// `dyn CacheStore` and `dyn LockStore` behind them), a `cache` health check
/// and the `cache:clear` command.
///
/// ```no_run
/// use navree_cache::{CacheModule, CacheOptions};
/// use navree_kernel::Navree;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
///     .module(CacheModule::for_root(CacheOptions {
///         prefix: "crm:".into(),
///         ..CacheOptions::default()
///     }))
///     .run()
/// # }
/// ```
///
/// [`from_config`](CacheModule::from_config), and the plain `CacheModule`
/// when no configured instance exists, read section `cache` at boot, so the
/// app must include `navree_config::ConfigModule`. Modules use the cache
/// without importing this module.
///
/// Tests can swap the backend with
/// `AppBuilder::override_value::<dyn CacheStore>(...)`.
#[derive(Clone, Copy, Debug, Default)]
pub struct CacheModule;

impl CacheModule {
    /// The module configured with `options` (configure it once, in the app root).
    pub fn for_root(options: CacheOptions) -> impl Module {
        ConfiguredCacheModule {
            options: Some(options),
        }
    }

    /// The module configured from section `cache` of the app configuration.
    pub fn from_config() -> impl Module {
        ConfiguredCacheModule { options: None }
    }
}

impl Module for CacheModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

/// `CacheModule::for_root(...)` / `from_config()`: same module id, configured.
struct ConfiguredCacheModule {
    /// `None`: read from configuration.
    options: Option<CacheOptions>,
}

impl Module for ConfiguredCacheModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<CacheModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.options.clone());
    }
}

fn register(m: &mut ModuleBuilder, options: Option<CacheOptions>) {
    m.global();
    match options {
        Some(options) => {
            let options = Arc::new(options);
            m.factory(Factory::<CacheOptions>::new(move |_| {
                let options = options.clone();
                async move {
                    options
                        .validate()
                        .map_err(|message| format!("invalid cache options: {message}"))?;
                    Ok(options)
                }
            }));
        }
        None => {
            m.config::<CacheOptions>();
        }
    }

    let options_key = ProviderKey::of::<CacheOptions>();
    m.factory(
        Factory::<dyn CacheStore>::new(|r: Resolver| async move {
            let options = r.get::<CacheOptions>().await?;
            match options.store {
                CacheStoreKind::Memory => Ok(Arc::new(MemoryStore::new()) as Arc<dyn CacheStore>),
                kind => named_store::<dyn CacheStore>(&r, kind).await,
            }
        })
        .deps(vec![options_key.clone()]),
    )
    .factory(
        Factory::<dyn LockStore>::new(|r: Resolver| async move {
            let options = r.get::<CacheOptions>().await?;
            match options.store {
                CacheStoreKind::Memory => {
                    Ok(Arc::new(MemoryLockStore::new()) as Arc<dyn LockStore>)
                }
                kind => named_store::<dyn LockStore>(&r, kind).await,
            }
        })
        .deps(vec![options_key.clone()]),
    )
    .factory(
        Factory::<Cache>::new(|r: Resolver| async move {
            let options = r.get::<CacheOptions>().await?;
            let store = r.get::<dyn CacheStore>().await?;
            let telemetry = Telemetry::from_app(&r.app()).await;
            Ok(Arc::new(
                Cache::with_tracker(
                    store,
                    options.prefix.clone(),
                    options.default_ttl(),
                    Some(r.app().tracker()),
                )
                .with_telemetry(telemetry, options.store.as_str()),
            ))
        })
        .deps(vec![
            options_key.clone(),
            ProviderKey::of::<dyn CacheStore>(),
        ]),
    )
    .factory(
        Factory::<Locks>::new(|r: Resolver| async move {
            let options = r.get::<CacheOptions>().await?;
            let store = r.get::<dyn LockStore>().await?;
            Ok(Arc::new(Locks::new(store, options.prefix.clone())))
        })
        .deps(vec![options_key, ProviderKey::of::<dyn LockStore>()]),
    )
    .multi(
        Factory::<dyn HealthCheck>::new(|r: Resolver| async move {
            let cache = r.get::<Cache>().await?;
            Ok(Arc::new(CacheHealth { cache }) as Arc<dyn HealthCheck>)
        })
        .deps(vec![ProviderKey::of::<Cache>()]),
    )
    .command(CommandSpec::new(
        "cache:clear",
        "Remove every entry from the cache store",
        |ctx, _args| async move {
            ctx.get::<Cache>().await?.flush().await?;
            println!("Cache cleared.");
            Ok(())
        },
    ))
    .exports::<Cache>()
    .exports::<Locks>()
    .exports::<dyn CacheStore>()
    .exports::<dyn LockStore>();
}

/// The store a module other than this one provides for `kind`: the
/// provider of `T` named after it (`redis`: navree-redis's `RedisModule`).
async fn named_store<T: ?Sized + Send + Sync + 'static>(
    r: &Resolver,
    kind: CacheStoreKind,
) -> Result<Arc<T>, navree_kernel::BoxError> {
    let name = kind.as_str();
    r.app().get_named::<T>(name).await.map_err(|error| {
        let module = match kind {
            CacheStoreKind::Redis => "RedisModule (navree-redis)",
            _ => "the module that provides it",
        };
        format!("cache.store = \"{name}\" needs {module} in the app: {error}").into()
    })
}

/// Readiness: a value written to the cache can be read back.
struct CacheHealth {
    cache: Arc<Cache>,
}

impl CacheHealth {
    async fn probe(&self) -> Result<bool, CacheError> {
        let key = format!("navree:health:{}", uuid::Uuid::new_v4().simple());
        self.cache.put(&key, &true, Duration::from_secs(10)).await?;
        let read = self.cache.get::<bool>(&key).await?;
        self.cache.forget(&key).await?;
        Ok(read == Some(true))
    }
}

impl HealthCheck for CacheHealth {
    fn name(&self) -> &str {
        "cache"
    }

    fn check(&self) -> BoxFuture<'_, HealthStatus> {
        Box::pin(async move {
            match self.probe().await {
                Ok(true) => HealthStatus::Up,
                Ok(false) => {
                    HealthStatus::Down("the cache did not return a value just written".into())
                }
                Err(error) => HealthStatus::Down(error.to_string()),
            }
        })
    }
}
