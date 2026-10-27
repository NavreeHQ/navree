//! Test app, journal and job types shared by the navree-queue integration tests.

#![allow(dead_code)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use navree_contracts::{
    Backoff, BoxError, Clock, FixedClock, Job, JobContext, JobOptions, ModuleBuilderJobsExt,
};
use navree_db::DatabaseModule;
use navree_kernel::{Module, ModuleBuilder};
use navree_queue::{Queue, QueueDriver, QueueModule, QueueOptions, WorkOptions};
use navree_testing::{TestApp, TestAppBuilder};
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;

/// The fixed clock's starting time.
pub fn start() -> DateTime<Utc> {
    "2026-10-25T12:00:00Z".parse().expect("valid time")
}

/// A booted app with `QueueModule`, a fixed clock and a journal of job runs.
pub struct Harness {
    pub app: TestApp,
    pub queue: Arc<Queue>,
    pub clock: Arc<FixedClock>,
    pub journal: Arc<Journal>,
}

impl Harness {
    /// Runs every job available now, like a test worker.
    pub async fn drain(&self) -> navree_queue::WorkStats {
        self.queue
            .work(WorkOptions {
                stop_when_empty: true,
                ..WorkOptions::default()
            })
            .await
            .expect("worker runs")
    }

    pub fn advance(&self, secs: u64) {
        self.clock.advance(Duration::from_secs(secs));
    }
}

/// The options every harness starts from: `driver`, two workers, fast polling.
pub fn options(driver: QueueDriver) -> QueueOptions {
    QueueOptions {
        driver,
        workers: 2,
        poll_interval_ms: 20,
        ..QueueOptions::default()
    }
}

pub async fn harness(driver: QueueDriver) -> Harness {
    harness_with(options(driver)).await
}

pub async fn harness_with(options: QueueOptions) -> Harness {
    let clock = Arc::new(FixedClock::new(start()));
    let journal = Arc::new(Journal::default());
    let mut builder = TestApp::builder();
    if options.driver == QueueDriver::Redis {
        // The `redis` driver name resolves the reference store, which
        // implements only the public `QueueStore` trait.
        builder = builder.module(crate::reference_store::NamedStoreModule(
            crate::reference_store::ReferenceStore::default(),
        ));
    }
    let app = builder
        .temp_database()
        .module(DatabaseModule)
        .module(QueueModule::for_root(options))
        .module(JobsModule)
        .override_value(clock.clone() as Arc<dyn Clock>)
        .override_value(journal.clone())
        .build()
        .await;
    let queue = app.get::<Queue>().await;
    Harness {
        app,
        queue,
        clock,
        journal,
    }
}

/// An app on the database at `url` (migrated at boot) sharing `journal`
/// and `clock`: several of them act like several processes.
pub async fn app_on(
    url: &str,
    options: QueueOptions,
    journal: Arc<Journal>,
    clock: Arc<FixedClock>,
) -> TestApp {
    app_builder_on(url, options, journal, clock)
        .module(JobsModule)
        .build()
        .await
}

/// [`app_on`] without the test jobs: its workers have no handlers.
pub async fn app_without_jobs_on(
    url: &str,
    options: QueueOptions,
    journal: Arc<Journal>,
    clock: Arc<FixedClock>,
) -> TestApp {
    app_builder_on(url, options, journal, clock).build().await
}

fn app_builder_on(
    url: &str,
    options: QueueOptions,
    journal: Arc<Journal>,
    clock: Arc<FixedClock>,
) -> TestAppBuilder {
    TestApp::builder()
        .config("database.url", url)
        .config("database.migrate_on_boot", true)
        .module(DatabaseModule)
        .module(QueueModule::for_root(options))
        .override_value(clock as Arc<dyn Clock>)
        .override_value(journal)
}

/// One job run, as recorded by the test jobs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub job: &'static str,
    pub label: String,
    pub attempt: u32,
}

/// What the test jobs did.
#[derive(Default)]
pub struct Journal {
    runs: Mutex<Vec<Run>>,
    /// While set, [`Toggle`] fails.
    pub failing: AtomicBool,
    /// Notified when a [`Hang`] job starts hanging.
    pub hanging: Notify,
}

impl Journal {
    pub fn record(&self, job: &'static str, label: &str, attempt: u32) {
        self.runs.lock().unwrap().push(Run {
            job,
            label: label.to_owned(),
            attempt,
        });
    }

    pub fn runs(&self) -> Vec<Run> {
        self.runs.lock().unwrap().clone()
    }

    pub fn labels(&self) -> Vec<String> {
        self.runs().into_iter().map(|run| run.label).collect()
    }

    pub fn set_failing(&self, failing: bool) {
        self.failing.store(failing, Ordering::SeqCst);
    }
}

async fn journal(ctx: &JobContext) -> Result<Arc<Journal>, BoxError> {
    Ok(ctx.app.get::<Journal>().await?)
}

/// Records its label.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub label: String,
}

impl Record {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
        }
    }
}

impl Job for Record {
    const NAME: &'static str = "test.record";

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        journal(&ctx)
            .await?
            .record(Self::NAME, &self.label, ctx.attempt);
        Ok(())
    }
}

/// Records its label on the `high` queue.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Urgent {
    pub label: String,
}

impl Job for Urgent {
    const NAME: &'static str = "test.urgent";

    fn options() -> JobOptions {
        JobOptions {
            queue: "high".to_owned(),
            ..JobOptions::default()
        }
    }

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        journal(&ctx)
            .await?
            .record(Self::NAME, &self.label, ctx.attempt);
        Ok(())
    }
}

/// Records the attempt, then fails: three attempts, exponential backoff
/// from 10 s.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AlwaysFails {
    pub label: String,
}

impl Job for AlwaysFails {
    const NAME: &'static str = "test.always_fails";

    fn options() -> JobOptions {
        JobOptions {
            max_attempts: 3,
            backoff: Backoff::Exponential {
                base_secs: 10,
                max_secs: 300,
            },
            ..JobOptions::default()
        }
    }

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        journal(&ctx)
            .await?
            .record(Self::NAME, &self.label, ctx.attempt);
        Err(format!("boom on attempt {}", ctx.attempt).into())
    }
}

/// Sleeps far longer than its one-second timeout; one attempt.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sleepy {
    pub label: String,
}

impl Job for Sleepy {
    const NAME: &'static str = "test.sleepy";

    fn options() -> JobOptions {
        JobOptions {
            max_attempts: 1,
            timeout_secs: 1,
            ..JobOptions::default()
        }
    }

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        journal(&ctx)
            .await?
            .record(Self::NAME, &self.label, ctx.attempt);
        tokio::time::sleep(Duration::from_secs(30)).await;
        Ok(())
    }
}

/// Unique for 60 seconds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UniqueReport {
    pub account: String,
}

impl Job for UniqueReport {
    const NAME: &'static str = "test.unique_report";

    fn options() -> JobOptions {
        JobOptions {
            unique_for_secs: Some(60),
            ..JobOptions::default()
        }
    }

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        journal(&ctx)
            .await?
            .record(Self::NAME, &self.account, ctx.attempt);
        Ok(())
    }
}

/// Hangs forever on its first attempt (a worker that crashed mid-job);
/// later attempts record and succeed. Timeout 5 s.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hang {
    pub label: String,
}

impl Job for Hang {
    const NAME: &'static str = "test.hang";

    fn options() -> JobOptions {
        JobOptions {
            timeout_secs: 5,
            ..JobOptions::default()
        }
    }

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        let journal = journal(&ctx).await?;
        if ctx.attempt == 1 {
            journal.hanging.notify_one();
            std::future::pending::<()>().await;
        }
        journal.record(Self::NAME, &self.label, ctx.attempt);
        Ok(())
    }
}

/// Hangs forever; one attempt, timeout 5 s.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HangLast {
    pub label: String,
}

impl Job for HangLast {
    const NAME: &'static str = "test.hang_last";

    fn options() -> JobOptions {
        JobOptions {
            max_attempts: 1,
            timeout_secs: 5,
            ..JobOptions::default()
        }
    }

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        let journal = journal(&ctx).await?;
        journal.hanging.notify_one();
        std::future::pending::<()>().await;
        journal.record(Self::NAME, &self.label, ctx.attempt);
        Ok(())
    }
}

/// Fails while the journal is `failing`; one attempt.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Toggle {
    pub label: String,
}

impl Job for Toggle {
    const NAME: &'static str = "test.toggle";

    fn options() -> JobOptions {
        JobOptions {
            max_attempts: 1,
            ..JobOptions::default()
        }
    }

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        let journal = journal(&ctx).await?;
        if journal.failing.load(Ordering::SeqCst) {
            return Err(format!("toggle `{}` is failing", self.label).into());
        }
        journal.record(Self::NAME, &self.label, ctx.attempt);
        Ok(())
    }
}

/// Panics; one attempt.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Panics;

impl Job for Panics {
    const NAME: &'static str = "test.panics";

    fn options() -> JobOptions {
        JobOptions {
            max_attempts: 1,
            ..JobOptions::default()
        }
    }

    async fn handle(self, _ctx: JobContext) -> Result<(), BoxError> {
        panic!("the job panicked on purpose")
    }
}

/// Renames the `jobs` table to `jobs_away` while it runs (database
/// driver), so the worker cannot record the outcome.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HideJobsTable;

impl Job for HideJobsTable {
    const NAME: &'static str = "test.hide_jobs_table";

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        use sea_orm::ConnectionTrait;
        let db = ctx.app.get::<navree_db::Db>().await?;
        db.conn()
            .execute_unprepared("ALTER TABLE jobs RENAME TO jobs_away")
            .await?;
        journal(&ctx)
            .await?
            .record(Self::NAME, "hidden", ctx.attempt);
        Ok(())
    }
}

/// A job type no module registers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Unregistered;

impl Job for Unregistered {
    const NAME: &'static str = "test.unregistered";

    async fn handle(self, _ctx: JobContext) -> Result<(), BoxError> {
        Ok(())
    }
}

/// Registers the test jobs (except [`Unregistered`]).
pub struct JobsModule;

impl Module for JobsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.job::<Record>()
            .job::<Urgent>()
            .job::<AlwaysFails>()
            .job::<Sleepy>()
            .job::<UniqueReport>()
            .job::<Hang>()
            .job::<HangLast>()
            .job::<Toggle>()
            .job::<Panics>()
            .job::<HideJobsTable>();
    }
}
