//! A plugin's global scopes and model hooks: a plugin registers them only for its own models,
//! and the app registers a plugin's scope or hook on any model from a module of its own.

#[path = "orm/models.rs"]
mod models;
#[path = "orm/support.rs"]
mod support;

use std::any::type_name;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use navree_db::DatabaseModule;
use navree_db::Executor as _;
use navree_kernel::{
    BoxError, BoxFuture, Factory, Injectable, KernelError, Module, ModuleBuilder, Navree, Resolver,
};
use navree_orm::events::ModelEventFake;
use navree_orm::hooks::{ModelEvent, ModelHook};
use navree_orm::strict;
use navree_orm::{Model, ModelLifecycle, ModuleBuilderOrmExt, Observer, OrmModule, OrmOptions};
use navree_plugin_ledger::{LedgerModule, Titled, entry, live, open, stamp};
use navree_testing::TestApp;

use crate::models::{Company, Deal};

/// The ledger plugin. A test binary is one crate, so the plugin's crate is this module, and the
/// app's modules and models at the top of the file (and in `models`) are in no crate it names.
mod navree_plugin_ledger {
    use navree_kernel::{BoxFuture, Module, ModuleBuilder, PluginInfo};
    use navree_orm::{Builder, ExecContext, Model, Record, Result};

    /// The plugin's info.
    const LEDGER: PluginInfo =
        PluginInfo::new("navree-plugin-ledger", module_path!(), "ledger", "1.0.0");

    /// The plugin's own model: an entry of the ledger.
    pub mod entry {
        use serde::Serialize;

        /// An entry of the ledger, settled or not.
        #[derive(Clone, Debug, Default, PartialEq, navree_orm::Model, Serialize)]
        #[model(table = "ledger_entries")]
        pub struct Model {
            pub id: i64,
            pub title: String,
            pub settled: bool,
        }
    }

    /// The table of the plugin's entries.
    pub const ENTRIES: &str = r#"
CREATE TABLE ledger_entries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    settled BOOLEAN NOT NULL
)"#;

    /// The plugin's scope for its own entries: the ones not settled yet.
    pub fn live(query: Builder<entry::Model>) -> Builder<entry::Model> {
        query.where_(entry::Model::SETTLED.eq(false))
    }

    /// A scope the plugin ships for any model: it leaves every row in.
    pub fn open<M: Model>(query: Builder<M>) -> Builder<M> {
        query
    }

    /// What has a title the plugin's listener tidies.
    pub trait Titled {
        /// The title.
        fn title_mut(&mut self) -> &mut String;
    }

    impl Titled for entry::Model {
        fn title_mut(&mut self) -> &mut String {
            &mut self.title
        }
    }

    /// The plugin's listener: it trims the title of what is saved.
    pub fn stamp<'a, M: Model + Titled>(
        record: &'a mut Record<M>,
        _ctx: &'a ExecContext,
    ) -> BoxFuture<'a, Result<()>> {
        let title = record.title_mut();
        *title = title.trim().to_owned();
        Box::pin(async { Ok(()) })
    }

    /// The plugin's module: it names the plugin, then registers what its function adds.
    #[derive(Clone, Copy)]
    pub struct LedgerModule(pub fn(&mut ModuleBuilder));

    impl Module for LedgerModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(LEDGER);
            (self.0)(m);
        }
    }
}

/// The refusal of the ledger plugin's module hooking `model`.
fn ledger_hooks(model: &str) -> String {
    format!(
        "plugin `navree-plugin-ledger` hooks the model `{model}` in `LedgerModule`, a model of another crate; only a module of the app hooks it: remove the plugin or update it to a version that leaves the hook to the app, and to use the plugin's hook, register it from a module of the app"
    )
}

/// The refusal of the ledger plugin's module registering the global scope `scope` on `model`.
fn ledger_scopes(scope: &str, model: &str) -> String {
    format!(
        "plugin `navree-plugin-ledger` registers the global scope `{scope}` on `{model}` in `LedgerModule`, a model of another crate; only a module of the app registers a global scope on it: remove the plugin or update it to a version that leaves the scope to the app, and to use the plugin's scope, register it from a module of the app"
    )
}

/// The boot error of an app with the database and `ledger` that must not boot.
async fn boot_error(ledger: LedgerModule) -> String {
    let built = TestApp::builder()
        .temp_database()
        .module(DatabaseModule::from_config())
        .module(ledger)
        .try_build()
        .await;
    match built {
        Ok(_) => panic!("the app booted"),
        Err(error) => error.to_string(),
    }
}

#[tokio::test]
async fn a_plugin_global_scope_on_another_crates_model_fails_the_boot() {
    // A scope that would turn off every other scope of the app's companies.
    let ledger = LedgerModule(|m| {
        m.global_scope::<Company>("open", |q| q.without_global_scopes());
    });
    let error = boot_error(ledger).await;
    let expected = ledger_scopes("open", type_name::<Company>());
    assert!(
        error.contains(&expected),
        "expected `{expected}` in:\n{error}"
    );
    // The scopes apply to the whole process once installed: the refused boot installed none.
    assert!(!Company::has_global_scope("open"));
    assert!(Company::global_scope_names().is_empty());
}

#[tokio::test]
async fn a_plugins_global_scope_on_another_crates_model_is_refused_before_strict_mode_is_installed()
{
    assert!(strict::current().prevents_missing_attributes());
    assert!(strict::current().prevents_discarding_attributes());
    assert!(!strict::current().auto_eager_loads());

    // The app configures lenient modes and auto-eager-loading on `OrmModule`, while the plugin
    // registers a global scope on the app's `Company`: the refusal runs before `strict::install`
    // changes the process-wide strictness.
    let ledger = LedgerModule(|m| {
        m.global_scope::<Company>("open", |q| q.without_global_scopes());
    });
    let mut options = OrmOptions::lenient();
    options.auto_eager_load = true;
    let built = TestApp::builder()
        .temp_database()
        .module(DatabaseModule::from_config())
        .module(OrmModule::for_root(options))
        .module(ledger)
        .try_build()
        .await;
    let error = match built {
        Ok(_) => panic!("the app booted"),
        Err(error) => error.to_string(),
    };
    let expected = ledger_scopes("open", type_name::<Company>());
    assert!(
        error.contains(&expected),
        "expected `{expected}` in:\n{error}"
    );
    assert!(strict::current().prevents_missing_attributes());
    assert!(strict::current().prevents_discarding_attributes());
    assert!(!strict::current().auto_eager_loads());
    assert!(!Company::has_global_scope("open"));
}

/// An observer of the app's companies that the container builds; it changes nothing.
struct AuditObserver;

impl Injectable for AuditObserver {
    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(AuditObserver) })
    }
}

impl Observer<Company> for AuditObserver {}

#[tokio::test]
async fn a_plugin_hook_on_another_crates_model_fails_the_boot() {
    /// Whether a hook's factory ran.
    static BUILT: AtomicBool = AtomicBool::new(false);

    // A listener that would rename every company it saves, an observer given as a value, one
    // the container builds, and a hook registered under its own key.
    let hooks: [fn(&mut ModuleBuilder); 4] = [
        |m| {
            m.on_model::<Company, _>(ModelEvent::Saving, |company, _ctx| {
                Box::pin(async move {
                    company.name = "Harpreet Traders".to_owned();
                    Ok(())
                })
            });
        },
        |m| {
            m.observe::<Company>(AuditObserver);
        },
        |m| {
            m.observer::<Company, AuditObserver>();
        },
        |m| {
            m.multi(Factory::<dyn ModelHook<Company>>::new(|_| async {
                BUILT.store(true, Ordering::SeqCst);
                Err::<Arc<dyn ModelHook<Company>>, BoxError>("never built".into())
            }));
        },
    ];
    let refused = ledger_hooks(type_name::<Company>());
    for hook in hooks {
        let validated = Navree::app()
            .module(LedgerModule(hook))
            .validate()
            .expect_err("the plugin hooks the app's companies")
            .to_string();
        assert_eq!(validated, refused);
        let error = boot_error(LedgerModule(hook)).await;
        assert!(
            error.contains(&refused),
            "expected `{refused}` in:\n{error}"
        );
    }
    assert!(
        !BUILT.load(Ordering::SeqCst),
        "the hook's factory never ran"
    );
}

#[tokio::test]
async fn a_plugin_scope_and_hook_on_its_own_model_apply() {
    let ledger = LedgerModule(|m| {
        m.global_scope::<entry::Model>("live", live)
            .on_model::<entry::Model, _>(ModelEvent::Saving, stamp::<entry::Model>);
    });
    let (app, db, _clock) = support::app(vec![Box::new(ledger)]).await;
    db.pool()
        .execute_script(navree_plugin_ledger::ENTRIES)
        .await
        .unwrap();

    // The plugin's listener trims what is saved.
    let rent = entry::Model::create(
        &*db,
        entry::Model {
            title: "  Rent, Ludhiana  ".to_owned(),
            ..entry::Model::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(rent.title, "Rent, Ludhiana");
    entry::Model::create(
        &*db,
        entry::Model {
            title: "Stock, Amritsar".to_owned(),
            settled: true,
            ..entry::Model::default()
        },
    )
    .await
    .unwrap();

    // The plugin's scope leaves out the settled entry.
    let unsettled = entry::Model::all(&*db)
        .await
        .unwrap()
        .map(|row| row.title.clone());
    assert_eq!(unsettled, ["Rent, Ludhiana"]);
    let every = entry::Model::query()
        .without_global_scope_named("live")
        .count(&*db)
        .await
        .unwrap();
    assert_eq!(every, 2);
    app.shutdown().await.unwrap();
}

impl Titled for Deal {
    fn title_mut(&mut self) -> &mut String {
        &mut self.title
    }
}

/// The app's module: it registers the ledger plugin's scope and listener on the app's deals.
struct DealsModule;

impl Module for DealsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.global_scope::<Deal>("ledger-open", open::<Deal>)
            .on_model::<Deal, _>(ModelEvent::Saving, stamp::<Deal>);
    }
}

#[tokio::test]
async fn the_app_may_register_a_plugins_scope_and_hook_on_another_crates_model() {
    // The plugin is in the app, so the attribution runs: the scope and the listener are the
    // app's.
    let plugin: Box<dyn Module> = Box::new(LedgerModule(|_| {}));
    let (app, db, _clock) = support::app(vec![plugin, Box::new(DealsModule)]).await;
    assert_eq!(app.ctx().plugins().plugins.len(), 1);
    assert!(Deal::has_global_scope("ledger-open"));
    let deal = Deal::create(&*db, Deal::new("  Jalandhar dairy  ", 1000))
        .await
        .unwrap();
    assert_eq!(deal.title, "Jalandhar dairy");
    app.shutdown().await.unwrap();
}

/// A module of the app: it puts a model event fake in the app, as a test does.
struct RecordsEvents;

impl Module for RecordsEvents {
    fn register(&self, m: &mut ModuleBuilder) {
        m.value(ModelEventFake::new());
    }
}

#[test]
fn a_plugin_may_not_silence_the_apps_model_listeners() {
    // A fake in a plugin's module would stop every listener and observer of the app's models,
    // on every model, and only record what they would have heard.
    let refused = Navree::app()
        .module(LedgerModule(|m| {
            m.value(ModelEventFake::new());
        }))
        .validate()
        .expect_err("the plugin silences the app's listeners")
        .to_string();
    let provider = type_name::<ModelEventFake>();
    assert_eq!(
        refused,
        format!(
            "plugin `navree-plugin-ledger` provides `{provider}` in `LedgerModule`, which silences the listeners of every model of the app; only a module of the app registers it: remove the plugin or update it to a version that leaves it to the app, and to use the plugin's, register it from a module of the app"
        )
    );
    // A module of the app puts one in the app beside the plugin, as before.
    Navree::app()
        .module(LedgerModule(|_| {}))
        .module(RecordsEvents)
        .validate()
        .expect("the app's own fake");
}
