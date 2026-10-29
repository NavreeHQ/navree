//! PostgreSQL: the `postgres` engine on PostgreSQL (the weighted
//! `tsvector`, trigram typo tolerance), the `pgvector` store, and an app
//! that syncs through the database queue.
//!
//! Opt-in: build with `--features postgres` and set
//! `NAVREE_TEST_POSTGRES_URL` to a superuser URL of a throwaway server, for
//! example `postgres://postgres:postgres@127.0.0.1:55432/postgres` (the
//! pgvector image, for the vector test). Each test creates its own database
//! and drops it at the end.

use std::sync::Arc;

use navree_db::{DatabaseModule, Db, Executor, ExecutorExt, PageQuery, Statement};
use navree_queue::{Queue, QueueModule};
use navree_search::{DatabaseEngine, Metric, PgvectorStore, PostgresOptions, Search, SearchModule};
use navree_testing::{TestApp, TestAppBuilder};

use crate::conformance::{Caps, suite};
use crate::servers::{Server, ServerDatabase};
use crate::support::{self, Contact, CrmModule, insert_contacts, new_contact};
use crate::vector_stores::vector_suite;

/// A database of its own on the test server, dropped at the end.
struct Scratch {
    database: ServerDatabase,
    url: String,
}

impl Scratch {
    async fn new(test: &str) -> Option<Scratch> {
        support::service("NAVREE_TEST_POSTGRES_URL", test)?;
        let database = ServerDatabase::create(Server::Postgres);
        let url = database.url.clone();
        Some(Scratch { database, url })
    }

    async fn drop(self) {
        drop(self.database);
    }
}

/// An app on the scratch database: the database queue, the `postgres`
/// engine with trigrams, and the test entities.
fn builder(scratch: &Scratch, sync: &str) -> TestAppBuilder {
    TestApp::builder()
        .config("database.url", scratch.url.clone())
        .config("database.migrate_on_boot", true)
        .config("queue.driver", "database")
        .config("search.engine", "postgres")
        .config("search.sync", sync)
        .config("search.postgres.trigram", true)
        .module(DatabaseModule::from_config())
        .module(QueueModule::from_config())
        .module(SearchModule::from_config())
        .module(CrmModule)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn postgres_engine_conforms_with_and_without_trigrams() {
    let Some(scratch) = Scratch::new("postgres_engine_conforms_with_and_without_trigrams").await
    else {
        return;
    };
    {
        let app = builder(&scratch, "off").build().await;
        let search = app.get::<Search>().await;
        assert_eq!(search.engine().name(), "postgres");
        suite(
            search.engine().clone(),
            Caps {
                typos: true,
                nulls: true,
            },
        )
        .await;

        let db = app.get::<Db>().await;
        let plain = DatabaseEngine::new((*db).clone(), PostgresOptions::default());
        suite(
            Arc::new(plain),
            Caps {
                typos: false,
                nulls: true,
            },
        )
        .await;
    }
    scratch.drop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_typo_threshold_applies_per_query_and_the_trigram_index_exists() {
    use navree_search::{Document, EngineQuery, Filter, IndexSettings, SearchEngine};

    let Some(scratch) =
        Scratch::new("the_typo_threshold_applies_per_query_and_the_trigram_index_exists").await
    else {
        return;
    };
    {
        let app = builder(&scratch, "off").build().await;
        let db = app.get::<Db>().await;
        let settings = IndexSettings::new("Contact", "contacts").text("name", 1);
        let found = async |engine: &DatabaseEngine, text: &str| {
            let filter = Filter::Const(true);
            engine
                .search(
                    "contacts",
                    EngineQuery {
                        settings: &settings,
                        text,
                        filter: &filter,
                        sort: &[],
                        offset: 0,
                        limit: 10,
                        highlight: None,
                        with_title: false,
                    },
                )
                .await
                .unwrap()
                .total
        };
        let engine = |typo_threshold| {
            DatabaseEngine::new(
                (*db).clone(),
                PostgresOptions {
                    trigram: true,
                    typo_threshold,
                    ..PostgresOptions::default()
                },
            )
        };
        let loose = engine(0.4);
        loose.prepare("contacts", &settings).await.unwrap();
        loose
            .upsert(
                "contacts",
                &settings,
                vec![
                    Document::new("1").field("name", "Ada Lovelace"),
                    Document::new("2").field("name", "Grace Hopper"),
                ],
            )
            .await
            .unwrap();

        // "lovlace" is 0.55 similar to "Lovelace": above 0.4, below the
        // PostgreSQL default of 0.6.
        assert_eq!(found(&loose, "lovlace").await, 1);
        assert_eq!(found(&loose, "ada lovlace").await, 1);
        assert_eq!(found(&loose, "zzzzzz").await, 0);
        // A stricter threshold is honoured, so it is set on every query.
        assert_eq!(found(&engine(0.9), "lovlace").await, 0);
        assert_eq!(found(&loose, "lovlace").await, 1);

        // The setting stayed local to the queries.
        let setting = Statement::text(
            "SELECT current_setting('pg_trgm.word_similarity_threshold') AS value",
        );
        let shown: String = db.fetch_scalar(&setting).await.unwrap();
        assert_eq!(shown, "0.6");

        // And inside a unit of work's open transaction, which gets it back.
        let uow = db.uow(None, None).await;
        uow.scope(async {
            uow.execute(&Statement::text("SELECT 1")).await.unwrap();
            assert_eq!(found(&loose, "lovlace").await, 1);
            let shown: String = uow.fetch_scalar(&setting).await.unwrap();
            assert_eq!(shown, "0.6");
        })
        .await;
        uow.rollback().await.unwrap();

        let defs: Vec<String> = db
            .fetch_column(&Statement::text(
                "SELECT indexdef FROM pg_indexes WHERE tablename = 'search_documents'",
            ))
            .await
            .unwrap();
        assert!(
            defs.iter()
                .any(|d| d.contains("USING gin") && d.contains("gin_trgm_ops")),
            "{defs:?}"
        );
    }
    scratch.drop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_app_on_postgres_syncs_through_the_database_queue() {
    let Some(scratch) = Scratch::new("an_app_on_postgres_syncs_through_the_database_queue").await
    else {
        return;
    };
    {
        let app = builder(&scratch, "queued").build().await;
        let db = app.get::<Db>().await;
        let search = app.get::<Search>().await;
        let queue = app.get::<Queue>().await;

        let mut grace = new_contact("Grace", "Hopper");
        grace.score = 7;
        grace.status = "customer".to_owned();
        let [ada, grace] = insert_contacts(&db, vec![new_contact("Ada", "Lovelace"), grace])
            .await
            .try_into()
            .unwrap();
        // The sync job was written in the same transaction as the rows.
        assert_eq!(queue.pending_count("default").await.unwrap(), 1);
        search.assert_not_indexed::<Contact>(ada.id).await;
        queue.run_pending().await.unwrap();

        // Typos, filters and sorts on PostgreSQL.
        let page = search
            .query::<Contact>("lovlace")
            .paginate(PageQuery::default())
            .await
            .unwrap();
        assert_eq!(page.meta.total, 1);
        assert_eq!(page.data[0].id, ada.id);
        let customers = search
            .query::<Contact>("")
            .filter("status", "customer")
            .ids(10)
            .await
            .unwrap();
        assert_eq!(customers, [grace.id.to_string()]);
        let by_score = search
            .query::<Contact>("")
            .sort_desc("score")
            .ids(10)
            .await
            .unwrap();
        assert_eq!(by_score, [grace.id.to_string(), ada.id.to_string()]);
        let hits = search
            .query::<Contact>("hopper")
            .hits(PageQuery::default())
            .await
            .unwrap();
        assert_eq!(hits.data[0].record.id, grace.id);
        assert!(hits.data[0].score > 0.0 && hits.data[0].score <= 1.0);

        // A delete through the model API removes the document once the job
        // runs.
        support::delete(&db, &grace).await;
        queue.run_pending().await.unwrap();
        search.assert_not_indexed::<Contact>(grace.id).await;

        // A fresh import rebuilds the index from the table.
        search
            .without_syncing(support::update(&db, &ada, |c| {
                c.last_name = "Byron".to_owned();
            }))
            .await;
        let report = search.import_fresh::<Contact>().await.unwrap();
        assert_eq!((report.rows, report.documents), (1, 1));
        let found = search.query::<Contact>("byron").ids(10).await.unwrap();
        assert_eq!(found, [ada.id.to_string()]);
    }
    scratch.drop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pgvector_store_conforms() {
    let Some(scratch) = Scratch::new("pgvector_store_conforms").await else {
        return;
    };
    {
        let app = builder(&scratch, "off").build().await;
        let db = app.get::<Db>().await;
        let available: i64 = db
            .fetch_scalar(&Statement::text(
                "SELECT COUNT(*) AS n FROM pg_available_extensions WHERE name = 'vector'",
            ))
            .await
            .unwrap();
        if available == 0 {
            assert!(
                std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() != Ok("1"),
                "pgvector_store_conforms needs the pgvector extension on the server"
            );
            println!("SKIPPED pgvector_store_conforms: the server has no `vector` extension");
        } else {
            let store = PgvectorStore::new((*db).clone(), 3, Metric::Cosine);
            vector_suite(Arc::new(store)).await;
        }
    }
    scratch.drop().await;
}
