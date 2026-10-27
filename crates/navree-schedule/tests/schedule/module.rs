//! `ScheduleModule` wiring: imports, the database requirement, the clock.

use std::sync::Arc;

use chrono::Utc;
use navree_db::{DatabaseModule, DbOptions, Migrator};
use navree_kernel::{Module, ModuleBuilder, Navree};
use navree_schedule::{
    ModuleBuilderScheduleExt, RUNS_TABLE, ScheduleError, ScheduleModule, Scheduler,
};

use crate::support::{Counter, Tasks, TestDb, at, clock, scheduler};

/// A feature module that imports `ScheduleModule` by type, as the PRD shows.
struct BillingModule(Counter);

impl Module for BillingModule {
    fn register(&self, m: &mut ModuleBuilder) {
        let counter = self.0.clone();
        m.import(ScheduleModule).schedule(|s| {
            s.task("billing.sync", counter.task()).hourly();
        });
    }
}

#[tokio::test]
async fn a_plain_import_reuses_the_configured_module() {
    let db = TestDb::new();
    let count = Counter::default();
    let app = db
        .app(&clock("2026-10-25T00:00:00Z"), BillingModule(count.clone()))
        .boot()
        .await
        .expect("boots");
    let outcomes = scheduler(&app)
        .await
        .tick(at("2026-10-25T01:00:00Z"))
        .await
        .unwrap();
    assert_eq!(outcomes.len(), 1);
    assert_eq!(count.get(), 1);
}

#[tokio::test]
async fn the_migration_creates_schedule_runs() {
    let db = TestDb::new();
    let app = db
        .boot(&clock("2026-10-25T00:00:00Z"), Tasks::new(|_| {}))
        .await;
    let status = Migrator::status(app.ctx()).await.unwrap();
    let ours: Vec<_> = status.iter().filter(|m| m.module == "schedule").collect();
    // The table, then its conversion to a TimescaleDB hypertable.
    assert_eq!(ours.len(), 2, "{status:?}");
    assert!(ours.iter().all(|m| m.applied));
    assert_eq!(RUNS_TABLE, "schedule_runs");
}

#[tokio::test]
async fn the_database_module_is_required() {
    let error = Navree::app()
        .module(ScheduleModule::for_root())
        .boot()
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("Db"), "{error}");
}

#[tokio::test]
async fn without_a_clock_the_system_clock_is_used() {
    let db = TestDb::new();
    let app = Navree::app()
        .module(DatabaseModule::for_root(DbOptions {
            url: db.url.clone(),
            migrate_on_boot: true,
            ..DbOptions::default()
        }))
        .module(ScheduleModule::for_root())
        .boot()
        .await
        .expect("boots");
    let scheduler = app.ctx().get::<Scheduler>().await.unwrap();
    let drift = (Utc::now() - scheduler.now()).num_seconds().abs();
    assert!(drift < 5, "{drift}");
}

#[tokio::test]
async fn ticking_after_the_app_is_dropped_fails() {
    let db = TestDb::new();
    let app = db
        .boot(&clock("2026-10-25T00:00:00Z"), Tasks::new(|_| {}))
        .await;
    let scheduler: Arc<Scheduler> = scheduler(&app).await;
    drop(app);
    let error = scheduler
        .tick(at("2026-10-25T00:00:00Z"))
        .await
        .unwrap_err();
    assert!(matches!(error, ScheduleError::AppDropped), "{error}");
}
