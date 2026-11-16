//! [`CacheModule`], [`CacheOptions`] and [`CacheStoreKind`].

use std::sync::Arc;
use std::time::Duration;

use navree_config::__private::{
    build_driver, check_registrations, contribute_slot, selector_error, selector_name,
};
use navree_config::{ConfigSection, DriverName, DriverNameError, ModuleBuilderConfigExt};
use navree_contracts::{BoxFuture, HealthCheck, HealthStatus};
use navree_kernel::{
    CommandSpec, ConfiguredModule, Factory, Module, ModuleBuilder, ProviderKey, Resolver,
};
use navree_telemetry::Telemetry;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::cache::DEFAULT_TTL;
use crate::drivers::{CacheBackend, ChosenBackend, SLOT};
use crate::{Cache, CacheError, CacheStore, LockStore, Locks, MemoryLockStore, MemoryStore};

/// Which backend stores cache entries and locks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CacheStoreKind {
    /// In-process: [`MemoryStore`] and [`MemoryLockStore`] (`"memory"`).
    #[default]
    Memory,
    /// Redis (`"redis"`): the `dyn CacheStore` and `dyn LockStore`
    /// providers named `redis`, which navree-redis's `RedisModule`
    /// registers. Shared by every process using the same Redis server.
    Redis,
    /// A store a module registers under this name with
    /// [`m.cache_store`](crate::ModuleBuilderCacheExt::cache_store)
    /// (`"two_tier"`); its settings are `cache.{name}`. Holding a built-in
    /// name, it is that built-in store.
    Registered(DriverName),
}

impl CacheStoreKind {
    /// The configuration name (`memory`, `redis`, or the name a store is
    /// registered under), also the store's name in telemetry `CacheEvent`s.
    pub fn as_str(self) -> &'static str {
        match self {
            CacheStoreKind::Memory => "memory",
            CacheStoreKind::Redis => "redis",
            CacheStoreKind::Registered(name) => name.as_str(),
        }
    }

    /// The kind, with a `Registered` built-in name turned into that
    /// built-in (`Registered("redis")` is `Redis`).
    pub(crate) fn resolved(self) -> CacheStoreKind {
        match self {
            CacheStoreKind::Registered(name) => match name.as_str() {
                "memory" => CacheStoreKind::Memory,
                "redis" => CacheStoreKind::Redis,
                _ => self,
            },
            other => other,
        }
    }
}

/// The configuration name, as [`as_str`](CacheStoreKind::as_str) gives it.
impl Serialize for CacheStoreKind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// `memory` and `redis` are the built-in stores, also when written as a
/// table whose only key is the name (`{ memory = {} }`); any other
/// lower-case name (a letter, then letters, digits or `_`) is a store a
/// module registers, which the boot resolves. Anything else, a table of any
/// other key included, is an error that lists the names and never repeats
/// the value.
impl<'de> Deserialize<'de> for CacheStoreKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = selector_name(deserializer, "store", &SLOT)?;
        match name.as_str() {
            "memory" => Ok(CacheStoreKind::Memory),
            "redis" => Ok(CacheStoreKind::Redis),
            other => match DriverName::new(other) {
                Ok(name) => Ok(CacheStoreKind::Registered(name)),
                Err(DriverNameError::Invalid) => {
                    Err(serde::de::Error::custom(selector_error("store", &SLOT)))
                }
                Err(error) => Err(serde::de::Error::custom(error)),
            },
        }
    }
}

/// `CacheModule` settings: configuration section `cache`.
///
/// ```toml
/// [cache]
/// store = "memory"   # or "redis" (with navree-redis's RedisModule)
/// prefix = "crm:"
/// default_ttl_secs = 3600
/// ```
///
/// Every field has a default, so a missing section is valid.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ConfigSection)]
#[config(key = "cache", docs = "/docs/digging-deeper/cache/", validate = Self::check)]
#[serde(default)]
pub struct CacheOptions {
    /// The backend (default `memory`; `redis` needs navree-redis's
    /// `RedisModule`).
    #[config(driver = "memory, redis", register = "m.cache_store")]
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

    /// Checks the values (the boot calls this).
    fn check(&self) -> Result<(), String> {
        if self.default_ttl_secs == 0 {
            return Err("default_ttl_secs must be greater than 0".to_owned());
        }
        Ok(())
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
        ConfiguredModule::<CacheModule, _>::for_root(options, register)
    }

    /// The module configured from section `cache` of the app configuration.
    pub fn from_config() -> impl Module {
        ConfiguredModule::<CacheModule, CacheOptions>::from_config(register)
    }
}

impl Module for CacheModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

fn register(m: &mut ModuleBuilder, options: Option<CacheOptions>) {
    m.global();
    contribute_slot(m, SLOT);
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
    let chosen_key = ProviderKey::of::<ChosenBackend>();
    m.factory(
        // Every registration of a store is checked, whichever is selected;
        // a store a module registers is built here, once, for both halves.
        Factory::<ChosenBackend>::new(|r: Resolver| async move {
            let options = r.get::<CacheOptions>().await?;
            check_registrations(&r.app(), &SLOT, CacheOptions::schema())?;
            let backend = match options.store.resolved() {
                CacheStoreKind::Registered(name) => Some(
                    build_driver::<CacheBackend>(
                        &r,
                        &SLOT,
                        "cache.store",
                        name,
                        format!("cache.{name}"),
                        None,
                        None,
                    )
                    .await?,
                ),
                _ => None,
            };
            Ok(Arc::new(ChosenBackend(backend)))
        })
        .deps(vec![options_key.clone()]),
    )
    .factory(
        Factory::<dyn CacheStore>::new(|r: Resolver| async move {
            let options = r.get::<CacheOptions>().await?;
            let chosen = r.get::<ChosenBackend>().await?;
            match options.store.resolved() {
                CacheStoreKind::Memory => Ok(Arc::new(MemoryStore::new()) as Arc<dyn CacheStore>),
                CacheStoreKind::Registered(_) => match &chosen.0 {
                    Some(backend) => Ok(backend.store.clone()),
                    None => Err(unbuilt(options.store)),
                },
                kind => named_store::<dyn CacheStore>(&r, kind).await,
            }
        })
        .deps(vec![options_key.clone(), chosen_key.clone()]),
    )
    .factory(
        Factory::<dyn LockStore>::new(|r: Resolver| async move {
            let options = r.get::<CacheOptions>().await?;
            let chosen = r.get::<ChosenBackend>().await?;
            match options.store.resolved() {
                CacheStoreKind::Memory => {
                    Ok(Arc::new(MemoryLockStore::new()) as Arc<dyn LockStore>)
                }
                CacheStoreKind::Registered(_) => match &chosen.0 {
                    Some(backend) => Ok(backend.locks.clone()),
                    None => Err(unbuilt(options.store)),
                },
                kind => named_store::<dyn LockStore>(&r, kind).await,
            }
        })
        .deps(vec![options_key.clone(), chosen_key]),
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

/// The error of a half of a registered store whose backend is missing; the
/// boot builds the backend first, so it is there.
fn unbuilt(kind: CacheStoreKind) -> navree_kernel::BoxError {
    format!("the cache store `{}` was not built", kind.as_str()).into()
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

#[cfg(test)]
mod tests {
    use navree_config::FieldType;
    use serde_json::json;

    use super::*;

    #[test]
    fn cache_store_kind_keeps_copy_and_its_static_name() {
        fn copy<T: Copy>(_: T) {}
        let kind = CacheStoreKind::Registered(DriverName::from_static("two_tier"));
        copy(kind);
        let name: &'static str = kind.as_str();
        assert_eq!(name, "two_tier");
        assert_eq!(
            CacheStoreKind::Registered(DriverName::from_static("redis")).resolved(),
            CacheStoreKind::Redis
        );
    }

    #[test]
    fn cache_config_of_today_reads_as_before() {
        for (text, kind) in [
            ("memory", CacheStoreKind::Memory),
            ("redis", CacheStoreKind::Redis),
        ] {
            assert_eq!(
                serde_json::from_value::<CacheStoreKind>(json!(text)).unwrap(),
                kind
            );
            assert_eq!(serde_json::to_value(kind).unwrap(), json!(text));
            assert_eq!(kind.as_str(), text);
        }
        assert_eq!(
            serde_json::to_value(CacheOptions::default()).unwrap()["store"],
            "memory"
        );
        assert_eq!(CacheOptions::defaults().unwrap()["store"], "memory");
        let read: CacheStoreKind = serde_json::from_value(json!("kulfi_box")).unwrap();
        assert_eq!(read.as_str(), "kulfi_box");
        let error = serde_json::from_value::<CacheStoreKind>(json!("carrier-pigeon"))
            .unwrap_err()
            .to_string();
        assert_eq!(
            error,
            "the store must be `memory`, `redis` or the name of a cache store registered with m.cache_store (a lower-case letter, then lower-case letters, digits or _)"
        );
    }

    #[test]
    fn cache_store_written_as_a_one_key_table_reads_as_before() {
        for (table, kind, text) in [
            (json!({"memory": {}}), CacheStoreKind::Memory, "memory"),
            (json!({"redis": null}), CacheStoreKind::Redis, "redis"),
        ] {
            let read = serde_json::from_value::<CacheStoreKind>(table).unwrap();
            assert_eq!(read, kind);
            assert_eq!(serde_json::to_value(read).unwrap(), json!(text));
        }
        let options: CacheOptions =
            serde_json::from_value(json!({"store": {"memory": {}}, "prefix": "crm:"})).unwrap();
        assert_eq!(options.store, CacheStoreKind::Memory);

        // A table never names a store a module registers, known or not, and
        // reading one adds no name to the process's names.
        let refused = "the store must be `memory`, `redis` or the name of a cache store registered with m.cache_store (a lower-case letter, then lower-case letters, digits or _)";
        DriverName::register("ludhiana_vault").unwrap();
        for (table, name) in [
            (json!({"two_tier": {}}), "two_tier"),
            (json!({"ludhiana_vault": {}}), "ludhiana_vault"),
            (json!({"pune_vault": {"capacity": 500}}), "pune_vault"),
        ] {
            let error = serde_json::from_value::<CacheStoreKind>(table)
                .unwrap_err()
                .to_string();
            assert_eq!(error, refused);
            assert!(!error.contains(name), "{error}");
        }
        assert_eq!(DriverName::lookup("pune_vault"), None);
    }

    #[test]
    fn the_section_describes_itself() {
        let schema = CacheOptions::schema().expect("schema");
        assert_eq!(schema.key, "cache");
        assert_eq!(schema.docs.as_deref(), Some("/docs/digging-deeper/cache/"));
        assert!(
            schema.unmarked_secrets().is_empty(),
            "{:?}",
            schema.unmarked_secrets()
        );
        let field = |name: &str| {
            schema
                .fields
                .iter()
                .find(|field| field.name == name)
                .expect(name)
        };
        assert_eq!(
            field("store").ty,
            FieldType::Driver {
                built_in: vec!["memory".into(), "redis".into()],
                register: "m.cache_store".into(),
            }
        );
        assert_eq!(field("prefix").ty, FieldType::String);
        assert_eq!(field("default_ttl_secs").ty, FieldType::Integer);
        let defaults = CacheOptions::defaults().expect("defaults");
        assert_eq!(defaults["store"], "memory");
        assert_eq!(defaults["default_ttl_secs"], 3600);
    }

    #[test]
    fn a_zero_ttl_is_invalid() {
        let options = CacheOptions {
            default_ttl_secs: 0,
            ..CacheOptions::default()
        };
        assert!(options.validate().unwrap_err().contains("default_ttl_secs"));
        assert!(CacheOptions::default().validate().is_ok());
    }
}
