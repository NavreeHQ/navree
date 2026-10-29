//! The database store: the `sessions` table, owned by the `session`
//! migration set, on SQLite, PostgreSQL and MySQL (each test on a database
//! of its own; see `databases.rs` for the servers).

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Json;
use axum::http::Method;
use chrono::{TimeDelta, Utc};
use navree_contracts::{SessionRecord, SessionStore};
use navree_db::{DatabaseModule, Db, Executor, ExecutorExt, Migrator, Statement, Uow};
use navree_http::{Controller, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_session::Session;
use navree_session::{DatabaseStore, SessionModule, SessionOptions, SessionStoreKind};
use navree_testing::{TestApp, TestAppBuilder};
use serde_json::{Map, json};

use crate::databases::Scratch;
use crate::on_every_server;
use crate::support;

/// A route that writes in the request's unit of work and in the session.
struct WritesBoth;

impl Injectable for WritesBoth {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(WritesBoth) })
    }
}

impl Controller for WritesBoth {
    fn prefix(&self) -> &'static str {
        "/both"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![RouteDef::on(Method::POST, "/", write_both).public()]
    }
}

async fn write_both(uow: Uow, session: Session) -> navree_http::Result<Json<serde_json::Value>> {
    uow.execute(&Statement::text(
        "INSERT INTO notes (body) VALUES ('hello')",
    ))
    .await?;
    session.insert("noted", &true)?;
    Ok(Json(json!({ "ok": true })))
}

struct WritesBothModule;

impl Module for WritesBothModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<WritesBoth>();
    }
}

/// An app on the scratch database, migrated on boot.
fn builder(scratch: &Scratch) -> TestAppBuilder {
    support::base()
        .config("database.url", scratch.url.as_str())
        .config("database.migrate_on_boot", true)
}

async fn app(scratch: &Scratch) -> TestApp {
    let app = builder(scratch)
        .module(DatabaseModule)
        .module(WritesBothModule)
        .module(SessionModule::for_root(SessionOptions {
            store: SessionStoreKind::Database,
            ..SessionOptions::default()
        }))
        .build()
        .await;
    app.get::<Db>()
        .await
        .execute_script("CREATE TABLE notes (body TEXT)")
        .await
        .expect("creates notes");
    app
}

async fn rows(db: &Db) -> i64 {
    db.fetch_scalar(&Statement::text("SELECT count(*) FROM sessions"))
        .await
        .expect("count sessions")
}

fn record(data: serde_json::Value, expires_in: TimeDelta) -> SessionRecord {
    let serde_json::Value::Object(data) = data else {
        panic!("session data is an object");
    };
    SessionRecord {
        data,
        expires_at: Some(Utc::now() + expires_in),
    }
}

async fn database_store_roundtrip(scratch: Scratch) {
    let app = app(&scratch).await;
    let db = app.get::<Db>().await;
    let store = app.get::<dyn SessionStore>().await;

    // Through HTTP: the session lives in the `sessions` table.
    let client = app.client();
    let id = support::put(&client, "cart", json!({"items": [1, 2]})).await;
    assert_eq!(rows(&db).await, 1);
    let (same, value) = support::get(&client, "cart").await;
    assert_eq!(same, id);
    assert_eq!(value, json!({"items": [1, 2]}));
    let stored = store.load(&id).await.unwrap().expect("the record");
    assert_eq!(stored.data["cart"], json!({"items": [1, 2]}));
    assert!(stored.expires_at.unwrap() > Utc::now() + TimeDelta::minutes(119));

    // Directly: save, replace, load, delete.
    let saved = record(
        json!({"a": 1, "nested": {"b": [true, null]}}),
        TimeDelta::minutes(5),
    );
    store.save("direct", saved.clone()).await.unwrap();
    let loaded = store.load("direct").await.unwrap().expect("saved record");
    assert_eq!(loaded.data, saved.data);
    let drift = loaded.expires_at.unwrap() - saved.expires_at.unwrap();
    assert!(drift.abs() < TimeDelta::seconds(1), "{drift}");

    store
        .save("direct", record(json!({"a": 2}), TimeDelta::minutes(5)))
        .await
        .unwrap();
    assert_eq!(
        store.load("direct").await.unwrap().unwrap().data["a"],
        json!(2)
    );
    assert_eq!(rows(&db).await, 2, "saving again replaces the row");

    store.delete("direct").await.unwrap();
    assert!(store.load("direct").await.unwrap().is_none());
    store.delete("never-existed").await.unwrap();
    assert!(store.load("never-existed").await.unwrap().is_none());
}

async fn the_session_migration_set_creates_the_table(scratch: Scratch) {
    let app = app(&scratch).await;
    let status = Migrator::status(app.ctx()).await.unwrap();
    let session: Vec<_> = status.iter().filter(|m| m.module == "session").collect();
    assert_eq!(session.len(), 1, "{status:?}");
    assert_eq!(session[0].name, "m20261025_000001_create_sessions_table");
    assert!(session[0].applied);
}

async fn database_store_from_config(scratch: Scratch) {
    let app = builder(&scratch)
        .config("session.store", "database")
        .module(DatabaseModule)
        .module(SessionModule::from_config())
        .build()
        .await;
    let client = app.client();
    support::put(&client, "k", json!(1)).await;
    assert_eq!(rows(&*app.get::<Db>().await).await, 1);
}

async fn regenerate_deletes_the_old_row(scratch: Scratch) {
    let app = app(&scratch).await;
    let db = app.get::<Db>().await;
    let client = app.client();
    support::put(&client, "k", json!(1)).await;
    client
        .post("/api/probe/regenerate")
        .xsrf()
        .send()
        .await
        .assert_status(200);
    assert_eq!(rows(&db).await, 1, "only the new row is left");
}

async fn prune_expired_deletes_old_rows(scratch: Scratch) {
    let app = app(&scratch).await;
    let db = app.get::<Db>().await;
    let store = DatabaseStore::new((*db).clone());
    store
        .save("old", record(json!({}), TimeDelta::minutes(-1)))
        .await
        .unwrap();
    store
        .save("live", record(json!({}), TimeDelta::minutes(10)))
        .await
        .unwrap();
    store
        .save(
            "forever",
            SessionRecord {
                data: Map::new(),
                expires_at: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(store.prune_expired().await.unwrap(), 1);
    assert!(store.load("old").await.unwrap().is_none());
    assert!(store.load("live").await.unwrap().is_some());
    assert!(store.load("forever").await.unwrap().is_some());
}

async fn the_session_is_saved_after_the_unit_of_work_commits(scratch: Scratch) {
    let app = app(&scratch).await;
    let db = app.get::<Db>().await;
    let client = app.client();
    client.csrf_cookie().await;

    let started = Instant::now();
    client
        .post("/api/both")
        .xsrf()
        .send()
        .await
        .assert_status(200);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "the session write does not wait for the request's transaction"
    );
    assert_eq!(support::get(&client, "noted").await.1, json!(true));
    let notes: i64 = db
        .fetch_scalar(&Statement::text("SELECT count(*) FROM notes"))
        .await
        .unwrap();
    assert_eq!(notes, 1, "the unit of work committed");
}

/// A failed write is a `DbError` of the engine, classified like any other.
async fn a_store_without_the_table_reports_a_database_error(scratch: Scratch) {
    let db = scratch.db().await;
    let store = DatabaseStore::new(db);
    let error = store.prune_expired().await.expect_err("no sessions table");
    assert!(!error.is_unique_violation(), "{error}");
    assert!(
        store.load("anything").await.is_err(),
        "loading without the table fails too"
    );
}

on_every_server!(
    "navree_session";
    database_store_roundtrip,
    the_session_migration_set_creates_the_table,
    database_store_from_config,
    regenerate_deletes_the_old_row,
    prune_expired_deletes_old_rows,
    the_session_is_saved_after_the_unit_of_work_commits,
    a_store_without_the_table_reports_a_database_error,
);

#[tokio::test]
async fn the_database_store_needs_the_database_module() {
    let error = support::base()
        .module(SessionModule::for_root(SessionOptions {
            store: SessionStoreKind::Database,
            ..SessionOptions::default()
        }))
        .try_build()
        .await
        .expect_err("no DatabaseModule");
    let message = error.to_string();
    assert!(message.contains("DatabaseModule"), "{message}");
}
