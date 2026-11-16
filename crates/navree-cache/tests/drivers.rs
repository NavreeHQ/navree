//! Cache stores a module registers under a name: `cache.store` selects
//! one, its factory reads its own settings, and a name that resolves to
//! nothing, or to two things, fails the boot and `config:check` alike.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use navree_cache::{
    Cache, CacheBackend, CacheModule, CacheOptions, CacheStore, CacheStoreKind, LockStore, Locks,
    MemoryLockStore, MemoryStore, ModuleBuilderCacheExt,
};
use navree_config::check::CheckOptions;
use navree_config::{ConfigModule, ConfigSources, DriverName};
use navree_kernel::{AppBuilder, CommandSpec, Module, ModuleBuilder, Navree};
use serde::Deserialize;
use serde_json::json;

const MINUTE: Duration = Duration::from_secs(60);

/// What the `two_tier` store reads: `[cache.two_tier]`.
#[derive(Default, Deserialize)]
#[serde(default)]
struct TwoTierSettings {
    capacity: usize,
}

/// Registers the cache store `two_tier` over stores the test keeps, and
/// records the capacity and the settings key its factory read.
#[derive(Default)]
struct TwoTierModule {
    store: Arc<MemoryStore>,
    locks: Arc<MemoryLockStore>,
    seen: Arc<Mutex<Option<(usize, String)>>>,
}

impl Module for TwoTierModule {
    fn register(&self, m: &mut ModuleBuilder) {
        let (store, locks, seen) = (self.store.clone(), self.locks.clone(), self.seen.clone());
        m.cache_store("two_tier", move |ctx| {
            let (store, locks, seen) = (store.clone(), locks.clone(), seen.clone());
            async move {
                let settings: TwoTierSettings = ctx.settings()?;
                *seen.lock().unwrap() = Some((settings.capacity, ctx.settings_key().to_owned()));
                Ok(CacheBackend::new(store, locks))
            }
        });
    }
}

/// A backend over fresh memory stores.
fn memory_backend() -> CacheBackend {
    CacheBackend::new(
        Arc::new(MemoryStore::new()),
        Arc::new(MemoryLockStore::new()),
    )
}

/// Registers `two_tier` a second time.
struct SecondTwoTierModule;

impl Module for SecondTwoTierModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.cache_store("two_tier", |_| async { Ok(memory_backend()) });
    }
}

/// Registers a store under the built-in name `redis`.
struct RedisLikeModule;

impl Module for RedisLikeModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.cache_store("redis", |_| async { Ok(memory_backend()) });
    }
}

/// Registers a store under a name with a capital and a `-`.
struct MisnamedModule;

impl Module for MisnamedModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.cache_store("Two-Tier", |_| async { Ok(memory_backend()) });
    }
}

/// Registers a store under the name of a key of the `cache` section.
struct PrefixModule;

impl Module for PrefixModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.cache_store("prefix", |_| async { Ok(memory_backend()) });
    }
}

/// Registers a store whose factory fails.
struct FailingModule;

impl Module for FailingModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.cache_store("failing", |_| async {
            Err("the Amritsar node is down".into())
        });
    }
}

/// A module that registers one command.
struct CommandModule(CommandSpec);

impl Module for CommandModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.command(self.0.clone());
    }
}

/// What `config:check` prints for an app built by `add`, without booting it.
async fn check_text(sources: ConfigSources, add: impl FnOnce(AppBuilder) -> AppBuilder) -> String {
    let out = Arc::new(Mutex::new(String::new()));
    let sink = out.clone();
    let probe = CommandModule(
        CommandSpec::new("probe:check", "config:check's report", move |ctx, _| {
            let sink = sink.clone();
            async move {
                let report =
                    navree_config::__private::check_report(&ctx, &CheckOptions::default()).await?;
                *sink.lock().unwrap() = report.render();
                Ok(())
            }
        })
        .offline(),
    );
    add(Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(probe))
    .run_with_args(vec!["probe:check".to_owned()])
    .await
    .unwrap();
    out.lock().unwrap().clone()
}

/// The boot errors of an app built by `add` over `ConfigModule::for_root(sources)`.
async fn boot(sources: ConfigSources, add: impl FnOnce(AppBuilder) -> AppBuilder) -> String {
    match add(Navree::app().module(ConfigModule::for_root(sources)))
        .boot()
        .await
    {
        Ok(_) => panic!("the app booted"),
        Err(errors) => errors.to_string(),
    }
}

#[tokio::test]
async fn a_registered_cache_store_is_selected_by_name() {
    let sources = || {
        ConfigSources::new().set(
            "cache",
            json!({"store": "two_tier", "prefix": "crm:", "two_tier": {"capacity": 500}}),
        )
    };
    let module = TwoTierModule::default();
    let (store, locks, seen) = (
        module.store.clone(),
        module.locks.clone(),
        module.seen.clone(),
    );
    let app = Navree::app()
        .module(ConfigModule::for_root(sources()))
        .module(CacheModule::from_config())
        .module(module)
        .boot()
        .await
        .unwrap();

    let options = app.ctx().get::<CacheOptions>().await.unwrap();
    assert_eq!(options.store.as_str(), "two_tier");
    let cache = app.ctx().get::<Cache>().await.unwrap();
    cache.put("deal:ludhiana", &100_000, MINUTE).await.unwrap();
    assert!(
        store.get("crm:deal:ludhiana").await.unwrap().is_some(),
        "the registered store holds the entry"
    );
    let held = app
        .ctx()
        .get::<Locks>()
        .await
        .unwrap()
        .lock("deal:ludhiana", MINUTE)
        .acquire()
        .await
        .unwrap();
    assert!(held.is_some());
    assert!(
        !locks
            .acquire("crm:lock:deal:ludhiana", "someone", MINUTE)
            .await
            .unwrap(),
        "the registered lock store holds the lock"
    );
    assert_eq!(
        *seen.lock().unwrap(),
        Some((500, "cache.two_tier".to_owned()))
    );

    // Given in code, the name selects the store too.
    let module = TwoTierModule::default();
    let (store, seen) = (module.store.clone(), module.seen.clone());
    let options = CacheOptions {
        store: CacheStoreKind::Registered(DriverName::from_static("two_tier")),
        prefix: "crm:".into(),
        ..CacheOptions::default()
    };
    let app = Navree::app()
        .module(ConfigModule::for_root(ConfigSources::new()))
        .module(CacheModule::for_root(options))
        .module(module)
        .boot()
        .await
        .unwrap();
    let cache = app.ctx().get::<Cache>().await.unwrap();
    cache.put("deal:amritsar", &50_000, MINUTE).await.unwrap();
    assert!(store.get("crm:deal:amritsar").await.unwrap().is_some());
    assert_eq!(
        *seen.lock().unwrap(),
        Some((0, "cache.two_tier".to_owned()))
    );

    let text = check_text(sources(), |app| {
        app.module(CacheModule::from_config())
            .module(TwoTierModule::default())
    })
    .await;
    assert!(text.contains("config:check: ok ("), "{text}");
}

#[tokio::test]
async fn an_unregistered_or_doubly_registered_or_built_in_cache_store_fails_the_boot_naming_m_cache_store()
 {
    let unregistered = "cache.store does not name a built-in cache store or one a module registers with m.cache_store (known: memory, redis, two_tier)";
    let sources = || ConfigSources::new().set("cache", json!({"store": "kulfi"}));
    let add = |app: AppBuilder| {
        app.module(CacheModule::from_config())
            .module(TwoTierModule::default())
    };
    assert!(boot(sources(), add).await.contains(unregistered));
    assert!(
        check_text(sources(), add)
            .await
            .contains(&format!("error    {unregistered}"))
    );

    let twice = "cache store `two_tier` is registered by both `TwoTierModule` and `SecondTwoTierModule`; register it once";
    let sources = || ConfigSources::new().set("cache", json!({"store": "memory"}));
    let add = |app: AppBuilder| {
        app.module(CacheModule::from_config())
            .module(TwoTierModule::default())
            .module(SecondTwoTierModule)
    };
    assert!(
        boot(sources(), add).await.contains(twice),
        "checked although memory is selected"
    );
    assert!(
        check_text(sources(), add)
            .await
            .contains(&format!("error    {twice}"))
    );

    let built_in = "m.cache_store(\"redis\") in `RedisLikeModule`: `redis` is a built-in cache store; register yours under another name";
    let add = |app: AppBuilder| {
        app.module(CacheModule::from_config())
            .module(RedisLikeModule)
    };
    assert!(boot(sources(), add).await.contains(built_in));
    assert!(
        check_text(sources(), add)
            .await
            .contains(&format!("error    {built_in}"))
    );

    // An app that registers nothing sees nothing new.
    let add = |app: AppBuilder| app.module(CacheModule::from_config());
    assert!(
        check_text(sources(), add)
            .await
            .contains("config:check: ok (")
    );
}

#[tokio::test]
async fn a_failing_or_misnamed_cache_store_registration_fails_the_boot() {
    let memory = || ConfigSources::new().set("cache", json!({"store": "memory"}));

    let text = boot(memory(), |app| {
        app.module(CacheModule::from_config())
            .module(MisnamedModule)
    })
    .await;
    assert!(
        text.contains("m.cache_store(\"Two-Tier\") in `MisnamedModule`: a cache store name is a lower-case letter followed by up to 63 lower-case letters, digits or _"),
        "{text}"
    );

    let text = boot(memory(), |app| {
        app.module(CacheModule::from_config()).module(PrefixModule)
    })
    .await;
    assert!(
        text.contains("m.cache_store(\"prefix\") in `PrefixModule`: `cache.prefix` is a setting of the cache section; register the cache store under another name"),
        "{text}"
    );

    let failing = ConfigSources::new().set("cache", json!({"store": "failing"}));
    let text = boot(failing, |app| {
        app.module(CacheModule::from_config()).module(FailingModule)
    })
    .await;
    assert!(
        text.contains("cache store `failing` (registered with m.cache_store in `FailingModule`) failed to start: the Amritsar node is down"),
        "{text}"
    );

    // A setting that does not read names the key, never the value.
    let unreadable = ConfigSources::new().set(
        "cache",
        json!({"store": "two_tier", "two_tier": {"capacity": "secret-500"}}),
    );
    let text = boot(unreadable, |app| {
        app.module(CacheModule::from_config())
            .module(TwoTierModule::default())
    })
    .await;
    assert!(text.contains("cache.two_tier"), "{text}");
    assert!(!text.contains("secret-500"), "{text}");
}
