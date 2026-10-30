//! Databases and apps for the integration tests.

#![allow(dead_code)]

use std::sync::Arc;

use chrono::{DateTime, TimeZone, Utc};
use navree_contracts::{Clock, FixedClock};
use navree_db::Executor as _;
use navree_db::{DatabaseModule, Db, DbOptions};
use navree_kernel::{Module, ModuleBuilder, ModuleId};
use navree_testing::{TempDirGuard, TestApp, TestAppBuilder};

use crate::models::SCHEMA;

/// A database of its own (no app). Keep it alive for the test.
pub struct TestDb {
    pub db: Db,
    _dir: TempDirGuard,
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

/// A fresh SQLite database with the tables, without an app.
pub async fn sqlite() -> TestDb {
    let dir = TempDirGuard::new();
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

/// When the fixed clock of [`app`] stands.
pub fn epoch() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 27, 9, 30, 0).unwrap()
}

/// A test app builder with `DatabaseModule` on a fresh SQLite file and a
/// fixed clock.
pub fn builder(clock: Arc<FixedClock>) -> TestAppBuilder {
    TestApp::builder()
        .temp_database()
        .override_value::<dyn Clock>(clock)
        .module(DatabaseModule::from_config())
}

/// An app with `DatabaseModule`, a fixed clock and `extra` modules; the
/// tables are created.
pub async fn app(extra: Vec<Box<dyn Module>>) -> (TestApp, Arc<Db>, Arc<FixedClock>) {
    let clock = Arc::new(FixedClock::new(epoch()));
    let mut builder = builder(clock.clone());
    for module in extra {
        builder = builder.module(Boxed(module));
    }
    let app = builder.build().await;
    let db = app.get::<Db>().await;
    create_tables(&db).await;
    (app, db, clock)
}

/// A boxed module.
pub struct Boxed(pub Box<dyn Module>);

impl Module for Boxed {
    fn id(&self) -> ModuleId {
        self.0.id()
    }

    fn name(&self) -> &'static str {
        self.0.name()
    }

    fn is_configured(&self) -> bool {
        self.0.is_configured()
    }

    fn register(&self, m: &mut ModuleBuilder) {
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
