//! Helpers shared by the navree-schedule integration tests.

#![allow(dead_code)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::{DateTime, Utc};
use navree_contracts::{Clock, FixedClock};
use navree_db::{DatabaseModule, Db, DbOptions};
use navree_kernel::{App, AppBuilder, AppContext, BoxError, Module, ModuleBuilder, Navree};
use navree_schedule::{ModuleBuilderScheduleExt, Schedule, ScheduleModule, Scheduler};
use navree_testing::TempDirGuard;
use sea_orm::{ConnectionTrait, Statement};

/// Fails the test instead of hanging when `f` takes longer than 20 seconds.
pub async fn within<T>(f: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(std::time::Duration::from_secs(20), f)
        .await
        .expect("timed out after 20 s")
}

/// `2026-10-25T02:30:00Z`-style UTC time.
pub fn at(time: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(time)
        .expect("an RFC 3339 time")
        .with_timezone(&Utc)
}

/// A module that schedules whatever `f` declares.
pub struct Tasks(Box<dyn Fn(&mut Schedule) + Send + Sync>);

impl Tasks {
    pub fn new(f: impl Fn(&mut Schedule) + Send + Sync + 'static) -> Self {
        Self(Box::new(f))
    }
}

impl Module for Tasks {
    fn register(&self, m: &mut ModuleBuilder) {
        m.schedule(|s| (self.0)(s));
    }
}

/// Counts task runs.
#[derive(Clone, Default)]
pub struct Counter(Arc<AtomicUsize>);

impl Counter {
    pub fn get(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }

    /// A task body that counts its runs.
    pub fn task(
        &self,
    ) -> impl Fn(AppContext) -> std::future::Ready<Result<(), BoxError>> + Send + Sync + 'static
    {
        let counter = self.0.clone();
        move |_ctx| {
            counter.fetch_add(1, Ordering::SeqCst);
            std::future::ready(Ok(()))
        }
    }
}

/// A temporary SQLite database shared by the apps of one test.
pub struct TestDb {
    pub url: String,
    _guard: TempDirGuard,
}

impl TestDb {
    pub fn new() -> Self {
        let (url, guard) = navree_testing::temp_sqlite_url();
        Self { url, _guard: guard }
    }

    /// An app on this database with `ScheduleModule`, the `tasks` module and
    /// `clock` as `dyn Clock`.
    pub fn app(&self, clock: &Arc<FixedClock>, tasks: impl Module) -> AppBuilder {
        Navree::app()
            .module(DatabaseModule::for_root(DbOptions {
                url: self.url.clone(),
                migrate_on_boot: true,
                ..DbOptions::default()
            }))
            .module(ScheduleModule::for_root())
            .module(tasks)
            .override_value::<dyn Clock>(clock.clone())
    }

    /// Boots [`app`](TestDb::app).
    pub async fn boot(&self, clock: &Arc<FixedClock>, tasks: impl Module) -> App {
        self.app(clock, tasks).boot().await.expect("the app boots")
    }
}

/// A clock standing at `time`.
pub fn clock(time: &str) -> Arc<FixedClock> {
    Arc::new(FixedClock::new(at(time)))
}

pub async fn scheduler(app: &App) -> Arc<Scheduler> {
    app.ctx().get::<Scheduler>().await.expect("a Scheduler")
}

/// One `schedule_runs` row.
#[derive(Debug, Clone, PartialEq)]
pub struct RunRow {
    pub task: String,
    pub tick: DateTime<Utc>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub status: String,
    pub error: Option<String>,
}

/// Every `schedule_runs` row, oldest tick first.
pub async fn runs(app: &App) -> Vec<RunRow> {
    let db = app.ctx().get::<Db>().await.expect("Db");
    let rows = db
        .conn()
        .query_all_raw(Statement::from_string(
            db.backend(),
            "SELECT task, tick, started_at, finished_at, status, error FROM schedule_runs ORDER BY tick, task",
        ))
        .await
        .expect("select schedule_runs");
    rows.iter()
        .map(|row| RunRow {
            task: row.try_get("", "task").expect("task"),
            tick: row.try_get("", "tick").expect("tick"),
            started_at: row.try_get("", "started_at").expect("started_at"),
            finished_at: row.try_get("", "finished_at").expect("finished_at"),
            status: row.try_get("", "status").expect("status"),
            error: row.try_get("", "error").expect("error"),
        })
        .collect()
}
