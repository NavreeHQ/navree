//! Databases (with a log of the statements they run) and apps for the
//! relation tests.

#![allow(dead_code)]

use std::sync::{Arc, Mutex, Once};

use chrono::{DateTime, TimeZone, Utc};
use navree_contracts::{Clock, FixedClock};
use navree_db::{DatabaseModule, Db, DbOptions, Executor, ExecutorExt, Statement};
use navree_kernel::Module;
use navree_orm::relations::MorphMap;
use navree_testing::TestApp;
use tempfile::TempDir;

use crate::models::{Company, Deal, Event, SCHEMA, Ticket};

/// The SQL of the statements a database ran, in order.
#[derive(Clone, Default)]
pub struct QueryLog(Arc<Mutex<Vec<String>>>);

impl QueryLog {
    /// The statements since the last `take` (or `clear`), forgetting them.
    pub fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }

    /// Forgets the statements so far.
    pub fn clear(&self) {
        self.0.lock().unwrap().clear();
    }

    /// The `SELECT` statements since the last `take`, forgetting every
    /// statement.
    pub fn selects(&self) -> Vec<String> {
        self.take()
            .into_iter()
            .filter(|sql| sql.trim_start().starts_with("SELECT"))
            .collect()
    }
}

/// A fresh SQLite database with the test tables. Keep it alive for the
/// test: dropping it deletes the file.
pub struct TestDb {
    pub db: Db,
    pub log: QueryLog,
    _dir: TempDir,
}

/// Creates the test tables.
pub async fn create_tables(db: &Db) {
    for statement in SCHEMA.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        db.execute_script(statement)
            .await
            .unwrap_or_else(|error| panic!("{statement}: {error}"));
    }
}

/// The models the morph map of every test knows.
pub fn register_morphs() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        MorphMap::register::<Deal>();
        MorphMap::register::<Company>();
        MorphMap::register::<Ticket>();
        MorphMap::register::<Event>();
    });
}

/// A database of its own, logging the statements it runs.
pub async fn sqlite() -> TestDb {
    register_morphs();
    let dir = tempfile::tempdir().expect("a temporary directory");
    let url = format!(
        "sqlite://{}?mode=rwc",
        dir.path().join("relations.db").display()
    );
    let db = Db::connect(&DbOptions {
        url,
        max_connections: 4,
        ..DbOptions::default()
    })
    .await
    .expect("connects");
    let log = QueryLog::default();
    let sink = log.clone();
    db.on_query(move |query| sink.0.lock().unwrap().push(query.sql.clone()));
    create_tables(&db).await;
    log.clear();
    TestDb { db, log, _dir: dir }
}

/// The moment the fixed clock of [`app`] starts at.
pub fn epoch() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 27, 9, 30, 0).unwrap()
}

/// An app with `DatabaseModule` on a fresh SQLite file, a fixed clock and
/// `extra` modules; the tables are created.
pub async fn app(extra: Vec<Box<dyn Module>>) -> (TestApp, Arc<Db>, Arc<FixedClock>) {
    register_morphs();
    let clock = Arc::new(FixedClock::new(epoch()));
    let mut builder = TestApp::builder()
        .temp_database()
        .override_value::<dyn Clock>(clock.clone())
        .module(DatabaseModule::from_config());
    for module in extra {
        builder = builder.module(Boxed(module));
    }
    let app = builder.build().await;
    let db = app.get::<Db>().await;
    create_tables(&db).await;
    (app, db, clock)
}

/// A boxed module.
struct Boxed(Box<dyn Module>);

impl Module for Boxed {
    fn id(&self) -> navree_kernel::ModuleId {
        self.0.id()
    }

    fn name(&self) -> &'static str {
        self.0.name()
    }

    fn is_configured(&self) -> bool {
        self.0.is_configured()
    }

    fn register(&self, m: &mut navree_kernel::ModuleBuilder) {
        self.0.register(m);
    }
}

/// Counts the rows of `table` directly.
pub async fn count_rows(db: &Db, table: &str) -> i64 {
    db.fetch_scalar::<i64>(&Statement::text(format!("SELECT COUNT(*) AS n FROM {table}")))
        .await
        .unwrap()
}

/// Runs `sql` directly.
pub async fn exec(db: &Db, sql: &str) {
    db.execute_script(sql)
        .await
        .unwrap_or_else(|error| panic!("{sql}: {error}"));
}
