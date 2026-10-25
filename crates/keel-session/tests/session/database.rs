//! The database store: the `sessions` table, owned by the `session`
//! migration set.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Json;
use axum::http::Method;
use chrono::{TimeDelta, Utc};
use keel_contracts::{SessionRecord, SessionStore};
use keel_db::Uow;
use keel_db::{DatabaseModule, Db, Migrator};
use keel_http::{Controller, ModuleBuilderHttpExt, RouteDef};
use keel_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use keel_session::Session;
use keel_session::{DatabaseStore, SessionModule, SessionOptions, SessionStoreKind};
use keel_testing::TestApp;
use sea_orm::{ConnectionTrait, Statement};
use serde_json::{Map, json};

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

async fn write_both(uow: Uow, session: Session) -> keel_http::Result<Json<serde_json::Value>> {
    let tx = uow.tx().await?;
    tx.execute_unprepared("CREATE TABLE IF NOT EXISTS notes (body TEXT)")
        .await?;
    tx.execute_unprepared("INSERT INTO notes (body) VALUES ('hello')")
        .await?;
    drop(tx);
    session.insert("noted", &true)?;
    Ok(Json(json!({ "ok": true })))
}

struct WritesBothModule;

impl Module for WritesBothModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<WritesBoth>();
    }
}

async fn app() -> TestApp {
    support::base()
        .temp_database()
        .module(DatabaseModule)
        .module(WritesBothModule)
        .module(SessionModule::for_root(SessionOptions {
            store: SessionStoreKind::Database,
            ..SessionOptions::default()
        }))
        .build()
        .await
}

async fn rows(db: &Db) -> i64 {
    let row = db
        .conn()
        .query_one_raw(Statement::from_string(
            db.backend(),
            "SELECT count(*) AS n FROM sessions",
        ))
        .await
        .expect("count sessions")
        .expect("a row");
    row.try_get::<i64>("", "n").expect("count")
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

#[tokio::test]
async fn database_store_roundtrip() {
    let app = app().await;
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

#[tokio::test]
async fn the_session_migration_set_creates_the_table() {
    let app = app().await;
    let status = Migrator::status(app.ctx()).await.unwrap();
    let session: Vec<_> = status.iter().filter(|m| m.module == "session").collect();
    assert_eq!(session.len(), 1, "{status:?}");
    assert!(session[0].applied);
}

#[tokio::test]
async fn database_store_from_config() {
    let app = support::base()
        .temp_database()
        .config("session.store", "database")
        .module(DatabaseModule)
        .module(SessionModule::from_config())
        .build()
        .await;
    let client = app.client();
    support::put(&client, "k", json!(1)).await;
    assert_eq!(rows(&*app.get::<Db>().await).await, 1);
}

#[tokio::test]
async fn regenerate_deletes_the_old_row() {
    let app = app().await;
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

#[tokio::test]
async fn prune_expired_deletes_old_rows() {
    let app = app().await;
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

#[tokio::test]
async fn the_session_is_saved_after_the_unit_of_work_commits() {
    let app = app().await;
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
    let notes = db
        .conn()
        .query_one_raw(Statement::from_string(
            db.backend(),
            "SELECT count(*) AS n FROM notes",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<i64>("", "n")
        .unwrap();
    assert_eq!(notes, 1, "the unit of work committed");
}
