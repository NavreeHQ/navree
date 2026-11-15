//! Session stores a module registers under a name: `session.store` selects
//! one, its factory reads its own settings and sessions are kept in it, and a
//! name that resolves to nothing, or to two things, fails the boot.

#[path = "session/support.rs"]
#[allow(dead_code)]
mod support;

use std::sync::{Arc, Mutex};

use navree_contracts::{BoxError, BoxFuture, SessionRecord, SessionStore};
use navree_kernel::{Module, ModuleBuilder};
use navree_session::{MemoryStore, ModuleBuilderSessionExt, SessionModule};
use serde::Deserialize;
use serde_json::json;

/// A session store over a `MemoryStore` the test keeps.
struct Tijori(Arc<MemoryStore>);

impl SessionStore for Tijori {
    fn load(&self, id: &str) -> BoxFuture<'_, Result<Option<SessionRecord>, BoxError>> {
        self.0.load(id)
    }

    fn save(&self, id: &str, record: SessionRecord) -> BoxFuture<'_, Result<(), BoxError>> {
        self.0.save(id, record)
    }

    fn delete(&self, id: &str) -> BoxFuture<'_, Result<(), BoxError>> {
        self.0.delete(id)
    }
}

/// What the `tijori` store reads: `[session.tijori]`.
#[derive(Default, Deserialize)]
#[serde(default)]
struct TijoriSettings {
    shelf: String,
}

/// Registers the session store `tijori` over a `MemoryStore` the test keeps,
/// and records the shelf and the settings key its factory read.
#[derive(Default)]
struct TijoriModule {
    store: Arc<MemoryStore>,
    seen: Arc<Mutex<Option<(String, String)>>>,
}

impl Module for TijoriModule {
    fn register(&self, m: &mut ModuleBuilder) {
        let (store, seen) = (self.store.clone(), self.seen.clone());
        m.session_store("tijori", move |ctx| {
            let (store, seen) = (store.clone(), seen.clone());
            async move {
                let settings: TijoriSettings = ctx.settings()?;
                *seen.lock().unwrap() = Some((settings.shelf, ctx.settings_key().to_owned()));
                Ok(Tijori(store))
            }
        });
    }
}

/// Registers `tijori` a second time.
struct SecondTijoriModule;

impl Module for SecondTijoriModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.session_store("tijori", |_| async {
            Ok(Tijori(Arc::new(MemoryStore::new())))
        });
    }
}

/// Registers a store under the built-in name `redis`.
struct RedisNamedModule;

impl Module for RedisNamedModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.session_store("redis", |_| async {
            Ok(Tijori(Arc::new(MemoryStore::new())))
        });
    }
}

/// Registers a store under the name of a key of the `session` section.
struct CookieModule;

impl Module for CookieModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.session_store("cookie", |_| async {
            Ok(Tijori(Arc::new(MemoryStore::new())))
        });
    }
}

/// Registers a store whose factory fails.
struct BrokenModule;

impl Module for BrokenModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.session_store("broken", |_| async {
            Err::<Tijori, BoxError>("the Amritsar vault is sealed".into())
        });
    }
}

#[tokio::test]
async fn a_registered_session_store_keeps_sessions() {
    let module = TijoriModule::default();
    let (store, seen) = (module.store.clone(), module.seen.clone());
    let app = support::base()
        .config("session.store", "tijori")
        .config("session.tijori.shelf", "Doaba")
        .module(module)
        .module(SessionModule::from_config())
        .build()
        .await;
    let client = app.client();

    let id = support::put(&client, "city", json!("Ludhiana")).await;
    let record = store
        .load(&id)
        .await
        .unwrap()
        .expect("saved in the registered store");
    assert_eq!(record.data["city"], json!("Ludhiana"));
    assert_eq!(support::get(&client, "city").await, (id, json!("Ludhiana")));
    assert_eq!(
        *seen.lock().unwrap(),
        Some(("Doaba".to_owned(), "session.tijori".to_owned()))
    );
}

#[tokio::test]
async fn an_unregistered_session_store_fails_the_boot_naming_m_session_store() {
    let error = support::base()
        .config("session.store", "almari")
        .module(TijoriModule::default())
        .module(SessionModule::from_config())
        .try_build()
        .await
        .expect_err("no store named almari");
    let message = error.to_string();
    assert!(
        message.contains("session.store is `almari`, but no module registers a session store under that name with m.session_store (known: memory, database, redis, tijori)"),
        "{message}"
    );
}

#[tokio::test]
async fn a_session_store_registration_that_cannot_work_fails_the_boot_naming_m_session_store() {
    let memory = || {
        support::base()
            .config("session.store", "memory")
            .module(SessionModule::from_config())
    };

    let twice = "session store `tijori` is registered by both `TijoriModule` and `SecondTijoriModule`; register it once";
    let message = memory()
        .module(TijoriModule::default())
        .module(SecondTijoriModule)
        .try_build()
        .await
        .expect_err("two stores named tijori")
        .to_string();
    assert!(
        message.contains(twice),
        "checked although memory is selected: {message}"
    );

    // The check does not depend on `dyn SessionStore` being built: a test
    // that replaces the store still has its registrations checked.
    let message = memory()
        .module(TijoriModule::default())
        .module(SecondTijoriModule)
        .override_value::<dyn SessionStore>(Arc::new(MemoryStore::new()))
        .try_build()
        .await
        .expect_err("two stores named tijori, with the store replaced")
        .to_string();
    assert!(message.contains(twice), "{message}");

    let built_in = "m.session_store(\"redis\") in `RedisNamedModule`: `redis` is a built-in session store; register yours under another name";
    let message = memory()
        .module(RedisNamedModule)
        .try_build()
        .await
        .expect_err("a built-in name")
        .to_string();
    assert!(message.contains(built_in), "{message}");

    let setting = "m.session_store(\"cookie\") in `CookieModule`: `session.cookie` is a setting of the session section; register the session store under another name";
    let message = memory()
        .module(CookieModule)
        .try_build()
        .await
        .expect_err("the name of a setting")
        .to_string();
    assert!(message.contains(setting), "{message}");

    let message = support::base()
        .config("session.store", "broken")
        .module(SessionModule::from_config())
        .module(BrokenModule)
        .try_build()
        .await
        .expect_err("a factory that fails")
        .to_string();
    assert!(
        message.contains("session store `broken` (registered with m.session_store in `BrokenModule`) failed to start: the Amritsar vault is sealed"),
        "{message}"
    );
}
