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

use navree_db::{DatabaseModule, Db, PageQuery, Repo};
use navree_queue::{Queue, QueueModule};
use navree_search::{DatabaseEngine, Metric, PgvectorStore, PostgresOptions, Search, SearchModule};
use navree_testing::{TestApp, TestAppBuilder};
use sea_orm::{ActiveValue::Set, ConnectionTrait, Database, IntoActiveModel};
use uuid::Uuid;

use crate::conformance::{Caps, suite};
use crate::support::{self, Contact, CrmModule, insert_contacts, new_contact};
use crate::vector_stores::vector_suite;

/// A database of its own on the test server; dropped with [`Scratch::drop`].
struct Scratch {
    admin: String,
    name: String,
    url: String,
}

impl Scratch {
    async fn new(test: &str) -> Option<Scratch> {
        let admin = support::service("NAVREE_TEST_POSTGRES_URL", test)?;
        let name = format!("navree_search_{}", Uuid::now_v7().simple());
        let conn = Database::connect(&admin).await.unwrap();
        conn.execute_unprepared(&format!("CREATE DATABASE {name}"))
            .await
            .unwrap();
        conn.close().await.unwrap();
        // postgres://user:password@host:port/database?options
        let (base, rest) = admin.rsplit_once('/').unwrap();
        let options = rest.find('?').map_or("", |at| &rest[at..]);
        let url = format!("{base}/{name}{options}");
        Some(Scratch { admin, name, url })
    }

    async fn drop(self) {
        let conn = Database::connect(&self.admin).await.unwrap();
        conn.execute_unprepared(&format!(
            "DROP DATABASE IF EXISTS {} WITH (FORCE)",
            self.name
        ))
        .await
        .unwrap();
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
        grace.score = Set(7);
        grace.status = Set("customer".to_owned());
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

        // A delete through `Repo` removes the document once the job runs.
        support::in_uow(&db, async |uow| {
            Repo::<Contact>::delete(uow, grace.clone()).await.unwrap()
        })
        .await;
        queue.run_pending().await.unwrap();
        search.assert_not_indexed::<Contact>(grace.id).await;

        // A fresh import rebuilds the index from the table.
        let mut row = ada.clone().into_active_model();
        row.last_name = Set("Byron".to_owned());
        search
            .without_syncing(support::in_uow(&db, async |uow| {
                Repo::<Contact>::update(uow, row).await.unwrap()
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
        let available = db
            .conn()
            .query_one_raw(sea_orm::Statement::from_string(
                sea_orm::DbBackend::Postgres,
                "SELECT COUNT(*) AS n FROM pg_available_extensions WHERE name = 'vector'",
            ))
            .await
            .unwrap()
            .and_then(|row| row.try_get::<i64>("", "n").ok())
            .unwrap_or(0);
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
