//! `CacheModule`: `for_root`, `from_config`, global exports, the health
//! check, the `cache:clear` command and background refreshes on shutdown.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use navree_cache::{
    Cache, CacheModule, CacheOptions, CacheStore, CacheStoreKind, LockStore, Locks, MemoryStore,
    Ttl,
};
use navree_config::{ConfigModule, ConfigSources};
use navree_contracts::{HealthCheck, HealthStatus};
use navree_kernel::{App, BootErrors, Factory, Module, ModuleBuilder, Navree, ProviderKey};
use serde_json::json;

const MINUTE: Duration = Duration::from_secs(60);

/// Uses the cache without importing `CacheModule` (it is global).
struct ReportsModule;

/// A provider of `ReportsModule` that depends on `Cache` and `Locks`.
struct Reports {
    cache: Arc<Cache>,
    locks: Arc<Locks>,
}

impl Module for ReportsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.factory(
            Factory::<Reports>::new(|r| async move {
                Ok(Arc::new(Reports {
                    cache: r.get::<Cache>().await?,
                    locks: r.get::<Locks>().await?,
                }))
            })
            .deps(vec![ProviderKey::of::<Cache>(), ProviderKey::of::<Locks>()]),
        )
        .exports::<Reports>();
    }
}

async fn boot_with_config(sources: ConfigSources, cache: impl Module) -> Result<App, BootErrors> {
    Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(cache)
        .boot()
        .await
}

#[tokio::test]
async fn for_root_exports_cache_and_locks_globally() {
    let options = CacheOptions {
        store: CacheStoreKind::Memory,
        prefix: "app:".into(),
        default_ttl_secs: 120,
    };
    let app = Navree::app()
        .module(CacheModule::for_root(options))
        .module(ReportsModule)
        .boot()
        .await
        .unwrap();

    let reports = app.ctx().get::<Reports>().await.unwrap();
    assert_eq!(reports.cache.prefix(), "app:");
    assert_eq!(reports.cache.default_ttl(), Duration::from_secs(120));
    reports.cache.put("k", &1, MINUTE).await.unwrap();
    let held = reports.locks.lock("job", MINUTE).acquire().await.unwrap();
    assert!(held.is_some());

    let cache = app.ctx().get::<Cache>().await.unwrap();
    assert_eq!(
        cache.get::<i32>("k").await.unwrap(),
        Some(1),
        "one shared cache"
    );
    let store = app.ctx().get::<dyn CacheStore>().await.unwrap();
    assert!(store.get("app:k").await.unwrap().is_some());
    let lock_store = app.ctx().get::<dyn LockStore>().await.unwrap();
    assert!(
        !lock_store
            .acquire("app:lock:job", "someone", MINUTE)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn from_config_reads_the_cache_section() {
    let sources =
        ConfigSources::new().set("cache", json!({"prefix": "crm:", "default_ttl_secs": 60}));
    let app = boot_with_config(sources, CacheModule::from_config())
        .await
        .unwrap();

    let cache = app.ctx().get::<Cache>().await.unwrap();
    assert_eq!(cache.prefix(), "crm:");
    assert_eq!(cache.default_ttl(), MINUTE);
}

#[tokio::test]
async fn plain_module_reads_config_and_defaults_apply() {
    let app = boot_with_config(ConfigSources::new(), CacheModule)
        .await
        .unwrap();

    let cache = app.ctx().get::<Cache>().await.unwrap();
    assert_eq!(cache.prefix(), "");
    assert_eq!(cache.default_ttl(), Duration::from_secs(3600));
    assert_eq!(
        CacheOptions::default().default_ttl(),
        Duration::from_secs(3600)
    );
}

#[tokio::test]
async fn environment_variables_override_cache_settings() {
    let sources = ConfigSources::new().env_vars(
        "APP_",
        [
            ("APP_CACHE__DEFAULT_TTL_SECS", "90"),
            ("APP_CACHE__STORE", "memory"),
        ],
    );
    let app = boot_with_config(sources, CacheModule::from_config())
        .await
        .unwrap();
    let cache = app.ctx().get::<Cache>().await.unwrap();
    assert_eq!(cache.default_ttl(), Duration::from_secs(90));
}

#[tokio::test]
async fn unknown_store_fails_the_boot() {
    let sources = ConfigSources::new().set("cache", json!({"store": "carrier-pigeon"}));
    let errors = boot_with_config(sources, CacheModule::from_config())
        .await
        .unwrap_err();
    let message = errors.to_string();
    assert!(message.contains("cache"), "{message}");
}

#[tokio::test]
async fn zero_default_ttl_fails_the_boot() {
    let sources = ConfigSources::new().set("cache", json!({"default_ttl_secs": 0}));
    let errors = boot_with_config(sources, CacheModule::from_config())
        .await
        .unwrap_err();
    assert!(errors.to_string().contains("default_ttl_secs"), "{errors}");

    let options = CacheOptions {
        default_ttl_secs: 0,
        ..CacheOptions::default()
    };
    let errors = Navree::app()
        .module(CacheModule::for_root(options))
        .boot()
        .await
        .unwrap_err();
    assert!(errors.to_string().contains("default_ttl_secs"), "{errors}");
}

#[tokio::test]
async fn overriding_the_store_replaces_the_backend() {
    let store = Arc::new(MemoryStore::new());
    let app = Navree::app()
        .module(CacheModule::for_root(CacheOptions::default()))
        .override_value::<dyn CacheStore>(store.clone())
        .boot()
        .await
        .unwrap();

    let cache = app.ctx().get::<Cache>().await.unwrap();
    cache.put("k", "v", Ttl::Default).await.unwrap();
    assert!(store.get("k").await.unwrap().is_some());
}

#[tokio::test]
async fn health_check_reports_up() {
    let app = Navree::app()
        .module(CacheModule::for_root(CacheOptions::default()))
        .boot()
        .await
        .unwrap();

    let checks = app.ctx().get_all::<dyn HealthCheck>().await.unwrap();
    let check = checks
        .iter()
        .find(|c| c.name() == "cache")
        .expect("the cache health check is registered");
    assert_eq!(check.check().await, HealthStatus::Up);
}

#[tokio::test]
async fn cache_clear_command_flushes_the_store() {
    let app = Navree::app()
        .module(CacheModule::for_root(CacheOptions::default()))
        .boot()
        .await
        .unwrap();
    let cache = app.ctx().get::<Cache>().await.unwrap();
    cache.forever("k", &1).await.unwrap();

    app.run_command("cache:clear", Vec::new()).await.unwrap();
    assert!(!cache.has("k").await.unwrap());
}

#[tokio::test(start_paused = true)]
async fn graceful_shutdown_waits_for_background_refreshes() {
    let app = Navree::app()
        .module(CacheModule::for_root(CacheOptions::default()))
        .boot()
        .await
        .unwrap();
    let cache = app.ctx().get::<Cache>().await.unwrap();

    // A `fresh` window of zero: every read after the first is stale.
    let (fresh, stale) = (Duration::ZERO, MINUTE);
    cache
        .flexible("k", fresh, stale, || async { Ok(1u32) })
        .await
        .unwrap();

    let refreshed = Arc::new(AtomicBool::new(false));
    let flag = refreshed.clone();
    let served = cache
        .flexible("k", fresh, stale, move || async move {
            tokio::time::sleep(Duration::from_secs(1)).await;
            flag.store(true, Ordering::SeqCst);
            Ok(2u32)
        })
        .await
        .unwrap();
    assert_eq!(served, 1);
    assert!(!refreshed.load(Ordering::SeqCst));

    app.shutdown().await.unwrap();
    assert!(
        refreshed.load(Ordering::SeqCst),
        "shutdown waited for the refresh"
    );
}
