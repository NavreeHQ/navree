//! Databases and apps for the tests.

#![allow(dead_code)]

use std::sync::Arc;

use chrono::{DateTime, TimeZone, Utc};
use navree_contracts::{Clock, FixedClock};
use navree_db::Executor as _;
use navree_db::{DatabaseModule, Db, DbOptions};
use navree_kernel::Module;
use navree_testing::TestApp;
use tempfile::TempDir;

use crate::models::SCHEMA;

/// A fresh SQLite database with the test tables. Keep it alive for the
/// test: dropping it deletes the file.
pub struct TestDb {
    pub db: Db,
    _dir: TempDir,
}

/// Creates the test tables.
pub async fn create_tables(db: &Db) {
    for statement in SCHEMA.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        db.pool()
            .execute_script(statement)
            .await
            .unwrap_or_else(|error| panic!("{statement}: {error}"));
    }
}

/// A database of its own, without an app.
pub async fn sqlite() -> TestDb {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let url = format!("sqlite://{}?mode=rwc", dir.path().join("orm.db").display());
    let db = Db::connect(&DbOptions {
        url,
        ..DbOptions::default()
    })
    .await
    .expect("connects");
    create_tables(&db).await;
    TestDb { db, _dir: dir }
}

/// The moment the fixed clock of [`app`] starts at.
pub fn epoch() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 27, 9, 30, 0).unwrap()
}

/// An app with `DatabaseModule` on a fresh SQLite file, a fixed clock and
/// `extra` modules; the tables are created.
pub async fn app(extra: Vec<Box<dyn Module>>) -> (TestApp, Arc<Db>, Arc<FixedClock>) {
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
    navree_db::ExecutorExt::fetch_scalar(
        db.pool(),
        &navree_db::Statement::text(format!("SELECT COUNT(*) AS n FROM {table}")),
    )
    .await
    .unwrap()
}
