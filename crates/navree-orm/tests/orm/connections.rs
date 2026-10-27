//! Connections: models and queries on named connections, the default
//! connection replaced inside a scope, and read replicas, with reads from
//! the writer on demand and after a unit of work writes.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use futures::TryStreamExt;
use navree_db::sea_orm::ConnectionTrait;
use navree_db::{DatabaseModule, Db, DbOptions, QueryLog};
use navree_kernel::{App, Navree};
use navree_orm::{
    Model, OrmError, Record, Value, col, raw, select, table, using_connection, using_writer,
};
use serde::{Deserialize, Serialize};
use tempfile::TempDir;

use crate::models::Company;
use crate::support::create_tables;

/// A model living on the `analytics` connection.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[model(table = "metrics", connection = "analytics")]
pub struct Metric {
    pub id: i64,
    pub name: String,
    pub value: i64,
}

const METRICS: &str = "CREATE TABLE metrics (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, value INTEGER NOT NULL)";

/// Options for SQLite file `name` in `dir`.
fn file(dir: &TempDir, name: &str) -> DbOptions {
    DbOptions {
        url: format!("sqlite://{}?mode=rwc", dir.path().join(name).display()),
        ..DbOptions::default()
    }
}

/// An app whose default connection is `main.db`, with the named
/// connections `analytics` and `archive`, each holding a `metrics` table.
async fn app_with_connections(dir: &TempDir) -> (App, Arc<Db>) {
    let mut connections = BTreeMap::new();
    connections.insert("analytics".to_owned(), file(dir, "analytics.db"));
    connections.insert("archive".to_owned(), file(dir, "archive.db"));
    let app = Navree::app()
        .module(DatabaseModule::for_root(DbOptions {
            connections,
            ..file(dir, "main.db")
        }))
        .boot()
        .await
        .unwrap();
    let db = app.ctx().get::<Db>().await.unwrap();
    for name in ["analytics", "archive"] {
        let named = db.connection(name).await.unwrap();
        named.conn().execute_unprepared(METRICS).await.unwrap();
    }
    (app, db)
}

fn metric(name: &str, value: i64) -> Metric {
    Metric {
        name: name.into(),
        value,
        ..Metric::default()
    }
}

#[tokio::test]
async fn models_run_on_their_connection_given_the_default_db() {
    let dir = tempfile::tempdir().unwrap();
    let (_app, db) = app_with_connections(&dir).await;
    assert_eq!(Metric::connection_name(), Some("analytics"));
    assert_eq!(Company::connection_name(), None);

    let mut visits: Record<Metric> = Metric::create(&*db, metric("visits", 1)).await.unwrap();
    visits.value = 2;
    visits.save(&*db).await.unwrap();
    visits.refresh(&*db).await.unwrap();
    assert_eq!(visits.value, 2);
    Metric::insert(&*db, [metric("signups", 3)]).await.unwrap();
    assert_eq!(Metric::query().count(&*db).await.unwrap(), 2);
    assert_eq!(
        Metric::query().sum::<i64>(&*db, Metric::VALUE).await.unwrap(),
        5
    );
    let analytics = db.connection("analytics").await.unwrap();
    assert_eq!(
        table("metrics").count(&analytics).await.unwrap(),
        2,
        "the rows are on the analytics database"
    );
    let missing = table("metrics").count(&*db).await.unwrap_err();
    assert!(
        missing.to_string().contains("no such table: metrics"),
        "table queries name no model, so they stay on the default connection: {missing}"
    );
    // Chunks and streams follow the model too.
    let streamed: Vec<Record<Metric>> = Metric::query()
        .order_by(Metric::ID)
        .lazy_by_id(&*db, 1)
        .try_collect()
        .await
        .unwrap();
    assert_eq!(streamed.len(), 2);
    let cursor: Vec<Record<Metric>> = Metric::query().cursor(&*db).try_collect().await.unwrap();
    assert_eq!(cursor.len(), 2);
    let page = Metric::query().order_by(Metric::ID).paginate(&*db, 1).await.unwrap();
    assert_eq!(page.meta.total, 2);
    assert!(visits.delete(&*db).await.unwrap());
    assert_eq!(Metric::query().pluck(&*db, Metric::NAME).await.unwrap(), ["signups"]);
}

#[tokio::test]
async fn a_named_connection_runs_what_it_is_given_and_queries_can_name_one() {
    let dir = tempfile::tempdir().unwrap();
    let (app, db) = app_with_connections(&dir).await;
    Metric::create(&*db, metric("visits", 1)).await.unwrap();
    // A named connection's `Db` runs the model's operations itself.
    let archive = app.ctx().get_named::<Db>("archive").await.unwrap();
    assert_eq!(archive.name(), "archive");
    Metric::create(&archive, metric("old", 9)).await.unwrap();
    assert_eq!(Metric::query().pluck(&archive, Metric::NAME).await.unwrap(), ["old"]);
    assert_eq!(Metric::query().pluck(&*db, Metric::NAME).await.unwrap(), ["visits"]);
    // A query names its connection, whatever the executor's or the model's.
    assert_eq!(
        Metric::on_connection("archive").pluck(&*db, Metric::NAME).await.unwrap(),
        ["old"]
    );
    assert_eq!(
        Metric::query()
            .on_connection("analytics")
            .pluck(&archive, Metric::NAME)
            .await
            .unwrap(),
        ["visits"]
    );
    assert_eq!(
        table("metrics").on_connection("archive").count(&*db).await.unwrap(),
        1
    );
    table("metrics")
        .on_connection("archive")
        .where_(col("name").eq("old"))
        .update(&*db, [col("value").set(10)])
        .await
        .unwrap();
    assert_eq!(
        Metric::query().value(&archive, Metric::VALUE).await.unwrap(),
        Some(10)
    );
    // An unknown connection.
    let error = table("metrics")
        .on_connection("reports")
        .count(&*db)
        .await
        .unwrap_err();
    assert!(
        matches!(&error, OrmError::Connection { connection, .. } if connection == "reports"),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "connection `reports`: no database connection named `reports` (configured: analytics, archive, default)"
    );
}

#[tokio::test]
async fn a_unit_of_work_spans_one_connection() {
    let dir = tempfile::tempdir().unwrap();
    let (_app, db) = app_with_connections(&dir).await;
    let uow = db.uow(None, None).await;
    let error = Metric::query().count(&uow).await.unwrap_err();
    assert!(
        matches!(&error, OrmError::Connection { connection, .. } if connection == "analytics"),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "connection `analytics`: the unit of work runs on connection `default`, and a unit of work spans one connection; pass a `Db` (it runs the operation on `analytics`) or a unit of work of `analytics`"
    );
    let named = table("metrics").on_connection("archive").count(&uow).await;
    assert!(matches!(named, Err(OrmError::Connection { .. })));
    uow.rollback().await.unwrap();

    // A unit of work of the model's connection.
    let analytics = db.connection("analytics").await.unwrap();
    let work = analytics.uow(None, None).await;
    Metric::create(&work, metric("signups", 3)).await.unwrap();
    assert_eq!(work.changes().len(), 1, "change events recorded");
    work.commit().await.unwrap();
    assert_eq!(Metric::query().count(&*db).await.unwrap(), 1);
    // `&Db` joins that unit of work while it is current on its connection.
    let joined = analytics.uow(None, None).await;
    joined
        .scope(async {
            Metric::create(&*db, metric("visits", 1)).await.unwrap();
        })
        .await;
    assert_eq!(joined.changes().len(), 1, "written in the unit of work");
    joined.rollback().await.unwrap();
    assert_eq!(Metric::query().count(&*db).await.unwrap(), 1, "rolled back");
}

#[tokio::test]
async fn the_default_connection_can_be_replaced_inside_a_scope() {
    let dir = tempfile::tempdir().unwrap();
    let (_app, db) = app_with_connections(&dir).await;
    let archive = db.connection("archive").await.unwrap();
    Metric::create(&archive, metric("old", 9)).await.unwrap();
    Metric::create(&*db, metric("visits", 1)).await.unwrap();
    Metric::create(&*db, metric("signups", 2)).await.unwrap();

    let archived = using_connection("archive", table("metrics").count(&*db))
        .await
        .unwrap();
    assert_eq!(archived, 1);
    let rows = using_connection(
        "archive",
        select(&*db, raw("SELECT name FROM metrics", Vec::<Value>::new())),
    )
    .await
    .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(navree_orm::scoped_connection(), None);
    // A model's own connection, and a query's, stay.
    assert_eq!(
        using_connection("archive", Metric::query().count(&*db))
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        using_connection(
            "archive",
            table("metrics").on_connection("analytics").count(&*db)
        )
        .await
        .unwrap(),
        2
    );
}

#[tokio::test]
async fn a_db_outside_an_app_cannot_reach_other_connections() {
    let dir = tempfile::tempdir().unwrap();
    let alone = Db::connect(&file(&dir, "alone.db")).await.unwrap();
    let error = Metric::query().count(&alone).await.unwrap_err();
    assert!(
        matches!(&error, OrmError::Connection { connection, .. } if connection == "analytics"),
        "{error:?}"
    );
    assert!(error.to_string().contains("belongs to no app"), "{error}");
    // Connected under the model's connection name, it runs the model.
    let analytics = Db::connect_named(
        "analytics",
        &file(&dir, "analytics.db"),
        navree_telemetry::Telemetry::disabled(),
    )
    .await
    .unwrap();
    analytics.conn().execute_unprepared(METRICS).await.unwrap();
    Metric::create(&analytics, metric("visits", 1)).await.unwrap();
    assert_eq!(Metric::query().count(&analytics).await.unwrap(), 1);
}

#[tokio::test]
async fn orm_statements_reach_query_logs_listeners_and_hooks() {
    let test = crate::support::sqlite().await;
    let db = &test.db;
    let executing = Arc::new(Mutex::new(Vec::<String>::new()));
    let hook = executing.clone();
    db.before_executing(move |sql| hook.lock().unwrap().push(sql.to_owned()));
    let transactions = Arc::new(AtomicUsize::new(0));
    let counter = transactions.clone();
    db.before_starting_transaction(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    let heard = Arc::new(AtomicUsize::new(0));
    let listener = heard.clone();
    db.on_query(move |_| {
        listener.fetch_add(1, Ordering::SeqCst);
    });

    Company::create(
        db,
        Company {
            name: "Acme".into(),
            active: true,
            ..Company::default()
        },
    )
    .await
    .unwrap();
    let (count, queries) =
        QueryLog::capture(Company::query().where_(Company::ACTIVE.eq(true)).count(db)).await;
    assert_eq!(count.unwrap(), 1);
    assert_eq!(queries.len(), 1);
    assert!(queries[0].sql.starts_with("SELECT COUNT(*)"), "{}", queries[0].sql);
    assert_eq!(queries[0].values, [navree_db::sea_orm::Value::from(true)]);
    assert!(!queries[0].raw_sql().contains('?'), "{}", queries[0].raw_sql());
    assert_eq!(executing.lock().unwrap().last(), Some(&queries[0].sql));
    assert!(heard.load(Ordering::SeqCst) >= 2);

    // A unit of work counts the ORM's statements.
    let uow = db.uow(None, None).await;
    uow.scope(async {
        for _ in 0..3 {
            Company::query().count(db).await.unwrap();
        }
    })
    .await;
    assert_eq!(uow.query_stats().repeated[0].count, 3);
    uow.commit().await.unwrap();

    // The ORM announces the transactions it begins itself.
    let before = transactions.load(Ordering::SeqCst);
    Company::query().truncate(db).await.unwrap();
    assert_eq!(transactions.load(Ordering::SeqCst), before + 1);
}

// ---------------------------------------------------------------------------
// Read replicas
// ---------------------------------------------------------------------------

/// A writer and a replica whose companies tell them apart.
async fn replicated(dir: &TempDir) -> Db {
    let writer = file(dir, "writer.db");
    let replica = file(dir, "replica.db");
    for (options, name) in [(&writer, "writer"), (&replica, "replica")] {
        let db = Db::connect(options).await.unwrap();
        create_tables(&db).await;
        db.conn()
            .execute_unprepared(&format!(
                "INSERT INTO companies (name, created_at, updated_at) VALUES ('{name}', '2026-10-27 09:30:00', '2026-10-27 09:30:00')"
            ))
            .await
            .unwrap();
        db.close().await.unwrap();
    }
    Db::connect(&DbOptions {
        read_urls: vec![replica.url],
        ..writer
    })
    .await
    .unwrap()
}

async fn company_names(db: &Db) -> Vec<String> {
    Company::query()
        .order_by(Company::ID)
        .pluck(db, Company::NAME)
        .await
        .unwrap()
}

#[tokio::test]
async fn reads_go_to_the_writer_on_demand() {
    let dir = tempfile::tempdir().unwrap();
    let db = replicated(&dir).await;
    assert_eq!(company_names(&db).await, ["replica"]);
    assert_eq!(
        Company::query().use_write().pluck(&db, Company::NAME).await.unwrap(),
        ["writer"]
    );
    assert_eq!(
        Company::on_write_connection()
            .pluck(&db, Company::NAME)
            .await
            .unwrap(),
        ["writer"]
    );
    assert!(
        table("companies")
            .use_write()
            .where_(col("name").eq("writer"))
            .exists(&db)
            .await
            .unwrap()
    );
    assert!(
        !table("companies")
            .where_(col("name").eq("writer"))
            .exists(&db)
            .await
            .unwrap()
    );
    assert_eq!(using_writer(company_names(&db)).await, ["writer"]);
    let streamed: Vec<Record<Company>> = Company::query()
        .use_write()
        .cursor(&db)
        .try_collect()
        .await
        .unwrap();
    assert_eq!(streamed[0].name, "writer");
    // Writes go to the writer.
    Company::create(
        &db,
        Company {
            name: "new".into(),
            ..Company::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(company_names(&db).await, ["replica"]);
    assert_eq!(using_writer(company_names(&db)).await, ["writer", "new"]);
}

#[tokio::test]
async fn reads_stay_on_the_writer_once_a_unit_of_work_wrote() {
    let dir = tempfile::tempdir().unwrap();
    let db = replicated(&dir).await;
    let uow = db.uow(None, None).await;
    assert_eq!(uow.scope(company_names(&db)).await, ["replica"]);
    // A mass update records no change events; it still marks the unit of
    // work as having written.
    uow.scope(Company::query().update(&db, [Company::EMPLOYEES.set(5)]))
        .await
        .unwrap();
    assert!(uow.has_modified_records());
    assert!(uow.changes().is_empty());
    assert_eq!(
        uow.scope(company_names(&db)).await,
        ["writer"],
        "through its transaction"
    );
    uow.commit().await.unwrap();
    assert_eq!(
        uow.scope(company_names(&db)).await,
        ["writer"],
        "on the writer after the commit"
    );
    assert_eq!(company_names(&db).await, ["replica"], "other tasks read replicas");

    // A write through `&Db` after the unit of work finished.
    let finished = db.uow(None, None).await;
    finished.commit().await.unwrap();
    finished
        .scope(
            table("companies")
                .where_(col("name").eq("writer"))
                .update(&db, [col("employees").set(6)]),
        )
        .await
        .unwrap();
    assert!(finished.has_modified_records());
    assert_eq!(finished.scope(company_names(&db)).await, ["writer"]);

    // Writes through the unit of work itself.
    let direct = db.uow(None, None).await;
    table("companies")
        .where_(col("name").eq("writer"))
        .update(&direct, [col("employees").set(7)])
        .await
        .unwrap();
    assert!(direct.has_modified_records());
    direct.rollback().await.unwrap();

    // A read alone leaves the unit of work clean.
    let reading = db.uow(None, None).await;
    reading.scope(company_names(&db)).await;
    assert!(!reading.has_modified_records());
}
