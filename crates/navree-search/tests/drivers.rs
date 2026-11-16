//! Search engines a module registers under a name: `search.engine` selects
//! one, read trimmed and in lower case as the built-in names are, and a
//! name no module registers fails the boot and `config:check` alike.

#[path = "search/servers.rs"]
mod servers;
#[path = "search/support.rs"]
mod support;

use std::sync::{Arc, Mutex};

use navree_config::check::CheckOptions;
use navree_config::{ConfigModule, ConfigSources};
use navree_db::{DatabaseModule, Db};
use navree_kernel::{AppBuilder, BoxError, BoxFuture, CommandSpec, Module, ModuleBuilder, Navree};
use navree_search::{
    Document, EngineQuery, EngineResults, Filter, IndexSettings, MemoryEngine,
    ModuleBuilderSearchExt, Search, SearchEngine, SearchError, SearchModule,
};
use navree_testing::TestApp;
use serde_json::json;

use crate::support::Contact;

/// The `memory` engine under the name `kosh`.
struct KoshEngine(MemoryEngine);

impl SearchEngine for KoshEngine {
    fn name(&self) -> &'static str {
        "kosh"
    }

    fn prepare<'a>(
        &'a self,
        index: &'a str,
        settings: &'a IndexSettings,
    ) -> BoxFuture<'a, Result<(), SearchError>> {
        self.0.prepare(index, settings)
    }

    fn upsert<'a>(
        &'a self,
        index: &'a str,
        settings: &'a IndexSettings,
        documents: Vec<Document>,
    ) -> BoxFuture<'a, Result<(), SearchError>> {
        self.0.upsert(index, settings, documents)
    }

    fn delete<'a>(
        &'a self,
        index: &'a str,
        ids: &'a [String],
    ) -> BoxFuture<'a, Result<(), SearchError>> {
        self.0.delete(index, ids)
    }

    fn flush<'a>(&'a self, index: &'a str) -> BoxFuture<'a, Result<(), SearchError>> {
        self.0.flush(index)
    }

    fn drop_index<'a>(&'a self, index: &'a str) -> BoxFuture<'a, Result<(), SearchError>> {
        self.0.drop_index(index)
    }

    fn swap<'a>(
        &'a self,
        index: &'a str,
        staged: &'a str,
    ) -> BoxFuture<'a, Result<(), SearchError>> {
        self.0.swap(index, staged)
    }

    fn search<'a>(
        &'a self,
        index: &'a str,
        query: EngineQuery<'a>,
    ) -> BoxFuture<'a, Result<EngineResults, SearchError>> {
        self.0.search(index, query)
    }

    fn count<'a>(&'a self, index: &'a str) -> BoxFuture<'a, Result<u64, SearchError>> {
        self.0.count(index)
    }

    fn contains<'a>(
        &'a self,
        index: &'a str,
        id: &'a str,
    ) -> BoxFuture<'a, Result<bool, SearchError>> {
        self.0.contains(index, id)
    }

    fn filters_exactly(&self, filter: &Filter) -> bool {
        self.0.filters_exactly(filter)
    }
}

/// Registers the search engine `kosh`.
struct KoshModule;

impl Module for KoshModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.search_engine("kosh", |_| async {
            Ok::<_, BoxError>(KoshEngine(MemoryEngine::new()))
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

/// The boot error of a test app on its own database whose `search.engine`
/// is `engine`, with no module that registers an engine.
async fn boot_error(engine: &str) -> String {
    let builder = servers::database(TestApp::builder())
        .config("search.engine", engine)
        .module(DatabaseModule::from_config())
        .module(SearchModule::from_config());
    match builder.try_build().await {
        Ok(_) => panic!("the app booted with search.engine = {engine:?}"),
        Err(error) => error.to_string(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_registered_search_engine_indexes_and_searches() {
    let app = support::builder("kosh", "inline")
        .module(KoshModule)
        .build()
        .await;
    let search = app.get::<Search>().await;
    assert_eq!(search.engine().name(), "kosh");
    assert_eq!(search.options().engine.name(), "kosh");

    let db = app.get::<Db>().await;
    let mut harpreet = support::new_contact("Harpreet", "Sandhu");
    harpreet.title = Some("Ludhiana".to_owned());
    let mut meera = support::new_contact("Meera", "Sharma");
    meera.title = Some("Amritsar".to_owned());
    let rows = support::insert_contacts(&db, vec![harpreet, meera]).await;
    support::settle(&app).await;

    let found = search.query::<Contact>("Ludhiana").ids(10).await.unwrap();
    assert_eq!(found, [rows[0].id.to_string()]);
    search.assert_indexed::<Contact>(rows[1].id).await;
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn config_check_reads_the_search_engine_as_the_boot_does() {
    let sources = || ConfigSources::new().set("search", json!({"engine": " Kosh "}));

    // Registered: the app boots on it, and config:check agrees.
    let app = support::builder(" Kosh ", "inline")
        .module(KoshModule)
        .build()
        .await;
    assert_eq!(app.get::<Search>().await.engine().name(), "kosh");
    app.shutdown().await.unwrap();
    let text = check_text(sources(), |app| {
        app.module(SearchModule::from_config()).module(KoshModule)
    })
    .await;
    assert!(text.contains("config:check: ok ("), "{text}");

    // Not registered: both refuse it, read as the boot reads it (trimmed,
    // in lower case), and neither says the value back.
    let unregistered = "search.engine does not name a built-in search engine or one a module registers with m.search_engine (known: postgres, database, meilisearch, typesense, tantivy, memory)";
    let error = boot_error(" Kosh ").await;
    assert!(error.contains(unregistered), "{error}");
    assert!(!error.to_lowercase().contains("kosh"), "{error}");
    let text = check_text(sources(), |app| app.module(SearchModule::from_config())).await;
    assert!(text.contains(&format!("error    {unregistered}")), "{text}");
    assert!(!text.to_lowercase().contains("kosh"), "{text}");
}
