//! Test databases on SQLite: template copies, in-memory databases (also
//! under an app), truncation and lazy databases. (PostgreSQL and MySQL
//! copies run in the `servers` suite.)

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use http::Method;
use navree_db::sea_orm::ConnectionTrait;
use navree_db::{DatabaseModule, Db};
use navree_http::{Controller, Json, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_orm::binding::Bound;
use navree_orm::testing::database::{DatabaseKind, LazyTestDb, Template, TestDb};
use navree_orm::{Model, Value, col, table};
use navree_testing::TestApp;
use serde_json::json;

use crate::models::{Company, Deal};
use crate::support::create_tables;

/// How many times [`NOTES`] was built.
static BUILDS: AtomicUsize = AtomicUsize::new(0);

/// Authors and notes, one author, and a migration history row.
static NOTES: Template = Template::new("integration notes", |template| {
    Box::pin(async move {
        BUILDS.fetch_add(1, Ordering::SeqCst);
        let conn = template.db().conn();
        for sql in [
            "CREATE TABLE authors (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL)",
            "CREATE TABLE notes (id INTEGER PRIMARY KEY AUTOINCREMENT, author_id INTEGER NOT NULL REFERENCES authors (id), body TEXT NOT NULL)",
            "CREATE TABLE navree_migrations (version TEXT PRIMARY KEY)",
            "INSERT INTO authors (name) VALUES ('Ada')",
            "INSERT INTO navree_migrations (version) VALUES ('m0001')",
        ] {
            conn.execute_unprepared(sql).await?;
        }
        Ok(())
    })
});

/// The integration schema, for ORM models.
static ORM: Template = Template::new("integration orm", |template| {
    Box::pin(async move {
        create_tables(template.db()).await;
        Ok(())
    })
});

async fn count(db: &Db, name: &str) -> u64 {
    table(name).count(db).await.unwrap()
}

#[tokio::test]
async fn every_copy_starts_from_the_template() {
    let first = NOTES.sqlite().await.unwrap();
    let second = NOTES.sqlite().await.unwrap();
    assert_eq!(first.kind(), DatabaseKind::Sqlite);
    assert_ne!(first.url(), second.url());
    first
        .db()
        .conn()
        .execute_unprepared("INSERT INTO notes (author_id, body) VALUES (1, 'hello')")
        .await
        .unwrap();
    assert_eq!(count(first.db(), "notes").await, 1);
    assert_eq!(count(second.db(), "notes").await, 0);
    assert_eq!(count(second.db(), "authors").await, 1, "the template's rows");
    assert_eq!(
        BUILDS.load(Ordering::SeqCst),
        1,
        "built once for the whole test binary"
    );
    first.close().await.unwrap();
}

#[tokio::test]
async fn truncation_empties_every_table_but_the_kept_ones() {
    let test = NOTES.sqlite().await.unwrap();
    test.db()
        .conn()
        .execute_unprepared("INSERT INTO notes (author_id, body) VALUES (1, 'a'), (1, 'b')")
        .await
        .unwrap();
    test.truncate(&["navree_migrations"]).await.unwrap();
    assert_eq!(count(test.db(), "notes").await, 0);
    assert_eq!(count(test.db(), "authors").await, 0);
    assert_eq!(count(test.db(), "navree_migrations").await, 1);
    // Counters start over.
    test.db()
        .conn()
        .execute_unprepared("INSERT INTO authors (name) VALUES ('Grace')")
        .await
        .unwrap();
    let first = table("authors")
        .where_(col("id").eq(Value::from(1_i64)))
        .count(test.db())
        .await
        .unwrap();
    assert_eq!(first, 1);
}

#[tokio::test]
async fn models_and_factories_run_on_copies_and_memory_databases() {
    let copy = ORM.sqlite().await.unwrap();
    let deal = Deal::factory().create_one(copy.db()).await.unwrap();
    assert_eq!(Company::query().count(copy.db()).await.unwrap(), 1);
    let found = Deal::find(copy.db(), deal.id).await.unwrap().unwrap();
    assert_eq!(found.slug, deal.slug);

    // One connection, a database of its own.
    let memory = TestDb::memory().await.unwrap();
    create_tables(memory.db()).await;
    Deal::factory().count(3).create(memory.db()).await.unwrap();
    assert_eq!(Deal::query().count(memory.db()).await.unwrap(), 3);
    let uow = memory.db().uow(None, None).await;
    Deal::factory().create_one(&uow).await.unwrap();
    uow.commit().await.unwrap();
    assert_eq!(Deal::query().count(memory.db()).await.unwrap(), 4);
    let other = TestDb::memory().await.unwrap();
    assert!(
        other
            .db()
            .conn()
            .execute_unprepared("SELECT 1 FROM deals")
            .await
            .is_err(),
        "memory databases are private"
    );
}

#[tokio::test]
async fn lazy_databases_are_created_on_first_use() {
    let lazy = LazyTestDb::new(|| Box::pin(NOTES.sqlite()));
    assert!(!lazy.is_created());
    let first = lazy.get().await.unwrap() as *const TestDb;
    assert!(lazy.is_created());
    let again = lazy.get().await.unwrap();
    assert!(std::ptr::eq(first, again), "the same database");
    assert_eq!(count(again.db(), "authors").await, 1);
}

struct Routes;

impl Injectable for Routes {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Routes) })
    }
}

impl Controller for Routes {
    fn prefix(&self) -> &'static str {
        "/"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "/deals/{deal}", |deal: Bound<Deal>| async move {
                Json(json!({ "title": deal.title }))
            })
            .public(),
        ]
    }
}

struct RoutesModule;

impl Module for RoutesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<Routes>();
    }
}

#[tokio::test]
async fn apps_boot_on_copies_and_in_memory_databases() {
    let copy = ORM.sqlite().await.unwrap();
    let deal = Deal::factory()
        .set(Deal::TITLE, "On a copy")
        .create_one(copy.db())
        .await
        .unwrap();
    let app = TestApp::builder()
        .config("database.url", copy.url())
        .module(DatabaseModule::from_config())
        .module(RoutesModule)
        .build()
        .await;
    app.client()
        .get(&format!("/api/deals/{}", deal.slug))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("title", json!("On a copy"));

    let app = TestApp::builder()
        .memory_database()
        .module(DatabaseModule::from_config())
        .module(RoutesModule)
        .build()
        .await;
    let db = app.get::<Db>().await;
    create_tables(&db).await;
    let deal = Deal::factory()
        .set(Deal::TITLE, "In memory")
        .create_one(&*db)
        .await
        .unwrap();
    for _ in 0..2 {
        app.client()
            .get(&format!("/api/deals/{}", deal.slug))
            .send()
            .await
            .assert_status(200)
            .assert_json_path("title", json!("In memory"));
    }
}
