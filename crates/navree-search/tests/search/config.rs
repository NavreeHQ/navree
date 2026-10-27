//! Configuring `SearchModule`: `for_root`, `from_config`, environment
//! variables and boot errors.

use navree_config::ConfigSources;
use navree_db::DatabaseModule;
use navree_search::{Engine, Search, SearchModule, SearchOptions, SyncMode};
use navree_testing::TestApp;

use crate::support::CrmModule;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn for_root_takes_an_engine_or_options() {
    let app = TestApp::builder()
        .temp_database()
        .module(DatabaseModule::from_config())
        .module(SearchModule::for_root(Engine::memory()))
        .module(CrmModule)
        .build()
        .await;
    let search = app.get::<Search>().await;
    assert_eq!(search.engine().name(), "memory");
    assert_eq!(search.options().sync, SyncMode::Queued);

    let app = TestApp::builder()
        .temp_database()
        .module(DatabaseModule::from_config())
        .module(SearchModule::for_root(SearchOptions {
            engine: Engine::memory(),
            prefix: "crm_".to_owned(),
            sync: SyncMode::Off,
            ..SearchOptions::default()
        }))
        .module(CrmModule)
        .build()
        .await;
    let search = app.get::<Search>().await;
    assert_eq!(search.options().index_name("contacts"), "crm_contacts");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn from_config_reads_the_environment() {
    let sources = ConfigSources::new().env_vars(
        "APP_",
        [
            ("APP_SEARCH__ENGINE", "memory"),
            ("APP_SEARCH__PREFIX", "crm_"),
            ("APP_SEARCH__SYNC", "inline"),
            ("APP_SEARCH__CHUNK_SIZE", "50"),
        ],
    );
    let app = TestApp::builder()
        .sources(sources)
        .temp_database()
        .module(DatabaseModule::from_config())
        .module(SearchModule)
        .module(CrmModule)
        .build()
        .await;
    let options = app.get::<SearchOptions>().await;
    assert_eq!(options.engine.name(), "memory");
    assert_eq!(options.prefix, "crm_");
    assert_eq!(options.sync, SyncMode::Inline);
    assert_eq!(options.chunk_size, 50);
}

async fn boot_error(config: &[(&str, serde_json::Value)]) -> String {
    let mut builder = TestApp::builder()
        .temp_database()
        .module(DatabaseModule::from_config())
        .module(SearchModule::from_config())
        .module(CrmModule);
    for (key, value) in config {
        builder = builder.config(key, value.clone());
    }
    match builder.try_build().await {
        Ok(_) => panic!("the app booted with {config:?}"),
        Err(error) => error.to_string(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalid_configuration_fails_the_boot_naming_the_key() {
    use serde_json::json;
    let error = boot_error(&[("search.engine", json!("elastic"))]).await;
    assert!(error.contains("search.engine"), "{error}");
    let error = boot_error(&[("search.engine", json!("meilisearch"))]).await;
    assert!(error.contains("search.url"), "{error}");
    let error = boot_error(&[
        ("search.engine", json!("typesense")),
        ("search.url", json!("http://127.0.0.1:8108")),
    ])
    .await;
    assert!(error.contains("search.key"), "{error}");
    let error = boot_error(&[
        ("search.engine", json!("memory")),
        ("search.vectors.store", json!("memory")),
    ])
    .await;
    assert!(error.contains("embeddings"), "{error}");
    #[cfg(not(feature = "tantivy"))]
    {
        let error = boot_error(&[("search.engine", json!("tantivy"))]).await;
        assert!(error.contains("search-tantivy"), "{error}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_postgres_engine_needs_the_database() {
    let Err(error) = TestApp::builder()
        .module(SearchModule::for_root(Engine::postgres()))
        .try_build()
        .await
    else {
        panic!("the boot succeeded without a database");
    };
    assert!(error.to_string().contains("DatabaseModule"), "{error}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn registering_an_entity_twice_is_harmless() {
    use navree_kernel::{Module, ModuleBuilder};
    struct Twice;
    impl Module for Twice {
        fn register(&self, m: &mut ModuleBuilder) {
            m.import(SearchModule::for_feature::<crate::support::Contact>());
        }
    }
    TestApp::builder()
        .temp_database()
        .module(DatabaseModule::from_config())
        .module(SearchModule::for_root(Engine::memory()))
        .module(CrmModule)
        .module(Twice)
        .build()
        .await;
}

/// `people`, indexed under the contacts' index name.
mod person {
    use navree_search::Searchable;
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Searchable)]
    #[sea_orm(table_name = "people")]
    #[searchable(index = "contacts")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        #[searchable]
        pub name: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_entities_cannot_share_an_index() {
    use navree_kernel::{Module, ModuleBuilder};
    struct People;
    impl Module for People {
        fn register(&self, m: &mut ModuleBuilder) {
            m.import(SearchModule::for_feature::<person::Entity>());
        }
    }
    let Err(error) = TestApp::builder()
        .temp_database()
        .module(DatabaseModule::from_config())
        .module(SearchModule::for_root(Engine::memory()))
        .module(CrmModule)
        .module(People)
        .try_build()
        .await
    else {
        panic!("the boot succeeded with a shared index");
    };
    assert!(
        error
            .to_string()
            .contains("both use the search index `contacts`"),
        "{error}"
    );
}

#[cfg(feature = "tantivy")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_app_on_tantivy_syncs_and_searches() {
    let dir = tempfile::tempdir().unwrap();
    let app = crate::support::builder("tantivy", "inline")
        .config(
            "search.tantivy.path",
            dir.path().to_string_lossy().to_string(),
        )
        .config("search.tantivy.memory_budget_mb", 15)
        .build()
        .await;
    let db = app.get::<navree_db::Db>().await;
    let rows =
        crate::support::insert_contacts(&db, vec![crate::support::new_contact("Ada", "Lovelace")])
            .await;
    crate::support::settle(&app).await;
    let search = app.get::<Search>().await;
    assert_eq!(search.engine().name(), "tantivy");
    let found = search
        .query::<crate::support::Contact>("lovlace")
        .ids(10)
        .await
        .unwrap();
    assert_eq!(found, [rows[0].id.to_string()]);
    app.run_command("search:import", &["Contact", "--fresh"])
        .await
        .unwrap();
    search
        .assert_indexed::<crate::support::Contact>(rows[0].id)
        .await;
    app.shutdown().await.unwrap();
}
