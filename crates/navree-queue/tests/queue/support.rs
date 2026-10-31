//! Test app, journal and job types shared by the navree-queue integration tests.

#![allow(dead_code)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use navree_contracts::{
    Backoff, BoxError, BoxFuture, Broadcaster, Clock, FixedClock, Job, JobContext, JobOptions,
    ModuleBuilderJobsExt, PermissionCheck, Principal,
};
use navree_db::DatabaseModule;
use navree_kernel::{Factory, Module, ModuleBuilder};
use navree_queue::{Operations, Queue, QueueDriver, QueueModule, QueueOptions, WorkOptions};
use navree_testing::{TestApp, TestAppBuilder};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::Notify;
use uuid::Uuid;

/// The fixed clock's starting time.
pub fn start() -> DateTime<Utc> {
    "2026-10-25T12:00:00Z".parse().expect("valid time")
}

/// A booted app with `QueueModule`, a fixed clock and a journal of job runs.
pub struct Harness {
    pub app: TestApp,
    pub queue: Arc<Queue>,
    pub operations: Arc<Operations>,
    pub clock: Arc<FixedClock>,
    pub journal: Arc<Journal>,
    pub broadcasts: Arc<Broadcasts>,
}

/// `builder` on a fresh database: the test's own database on the server
/// `NAVREE_TEST_DATABASE` names (see [`crate::servers`]), or a temporary
/// SQLite file.
fn with_database(builder: TestAppBuilder) -> TestAppBuilder {
    crate::servers::database(builder)
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

/// [`options`] with one worker (jobs run one at a time, in order) and
/// progress written at every report.
pub fn one_worker(driver: QueueDriver) -> QueueOptions {
    let mut options = options(driver);
    options.workers = 1;
    options.operations.progress_interval_ms = 0;
    options
}

pub async fn harness(driver: QueueDriver) -> Harness {
    harness_with(options(driver)).await
}

pub async fn harness_with(options: QueueOptions) -> Harness {
    harness_using(options, |builder| builder).await
}

/// [`harness_with`], with `extra` adding to the app (modules, `fake_auth`).
/// Broadcasts are recorded in [`Harness::broadcasts`].
pub async fn harness_using(
    options: QueueOptions,
    extra: impl FnOnce(TestAppBuilder) -> TestAppBuilder,
) -> Harness {
    let clock = Arc::new(FixedClock::new(start()));
    let journal = Arc::new(Journal::default());
    let broadcasts = Arc::new(Broadcasts::default());
    let mut builder = TestApp::builder();
    if options.driver == QueueDriver::Redis {
        // The `redis` driver name resolves the reference store, which
        // implements only the public `QueueStore` trait.
        builder = builder.module(crate::reference_store::NamedStoreModule(
            crate::reference_store::ReferenceStore::default(),
        ));
    }
    let builder = with_database(builder)
        .module(DatabaseModule)
        .module(QueueModule::for_root(options))
        .module(JobsModule)
        .module(BroadcastsModule(broadcasts.clone()))
        .override_value(clock.clone() as Arc<dyn Clock>)
        .override_value(journal.clone());
    let app = extra(builder).build().await;
    let queue = app.get::<Queue>().await;
    let operations = app.get::<Operations>().await;
    Harness {
        app,
        queue,
        operations,
        clock,
        journal,
        broadcasts,
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
    /// Notified when a [`Cooperative`] job starts.
    pub started: Notify,
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

/// Hangs forever on every attempt, recording each start: a job whose
/// worker dies every time it runs. Three attempts, timeout 5 s.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HangAlways {
    pub label: String,
}

impl Job for HangAlways {
    const NAME: &'static str = "test.hang_always";

    fn options() -> JobOptions {
        JobOptions {
            max_attempts: 3,
            timeout_secs: 5,
            ..JobOptions::default()
        }
    }

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        let journal = journal(&ctx).await?;
        journal.record(Self::NAME, &self.label, ctx.attempt);
        journal.hanging.notify_one();
        std::future::pending::<()>().await;
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
        use navree_db::Executor as _;
        let db = ctx.app.get::<navree_db::Db>().await?;
        db.execute_script("ALTER TABLE jobs RENAME TO jobs_away")
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

/// Reports progress every 10 ms until its batch or operation is cancelled
/// (then stops with `JobCancelled`); gives up with an error after 10 s.
/// Notifies the journal's `started` after its first report.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cooperative {
    pub label: String,
}

impl Job for Cooperative {
    const NAME: &'static str = "test.cooperative";

    fn options() -> JobOptions {
        JobOptions {
            max_attempts: 1,
            timeout_secs: 30,
            ..JobOptions::default()
        }
    }

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        let journal = journal(&ctx).await?;
        for step in 1..=1000 {
            ctx.progress(step, 1000, format!("step {step}")).await?;
            if step == 1 {
                journal.started.notify_one();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        journal.record(Self::NAME, &self.label, ctx.attempt);
        Err("never cancelled".into())
    }
}

/// Exports `rows` rows for operation `operation`: progress after each row,
/// then completes the operation with `{"rows": rows}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Export {
    pub operation: Uuid,
    pub rows: u64,
}

impl Job for Export {
    const NAME: &'static str = "test.export";

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        for row in 1..=self.rows {
            ctx.progress(row, self.rows, format!("Exported {row} of {}", self.rows))
                .await?;
        }
        let operations = ctx.app.get::<Operations>().await?;
        operations
            .complete(self.operation, json!({ "rows": self.rows }))
            .await?;
        journal(&ctx)
            .await?
            .record(Self::NAME, &self.operation.to_string(), ctx.attempt);
        Ok(())
    }
}

/// Completes operation `operation` with `result` (a batch's `then`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CompleteOperation {
    pub operation: Uuid,
    pub result: serde_json::Value,
}

impl Job for CompleteOperation {
    const NAME: &'static str = "test.complete_operation";

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        let operations = ctx.app.get::<Operations>().await?;
        operations.complete(self.operation, self.result).await?;
        journal(&ctx)
            .await?
            .record(Self::NAME, "complete", ctx.attempt);
        Ok(())
    }
}

/// Every broadcast, recorded (registered as the app's `dyn Broadcaster`).
#[derive(Default)]
pub struct Broadcasts(Mutex<Vec<(Vec<String>, String, serde_json::Value)>>);

impl Broadcasts {
    pub fn all(&self) -> Vec<(Vec<String>, String, serde_json::Value)> {
        self.0.lock().unwrap().clone()
    }
}

impl Broadcaster for Broadcasts {
    fn broadcast(
        &self,
        channels: Vec<String>,
        event: String,
        data: serde_json::Value,
    ) -> BoxFuture<'_, Result<(), BoxError>> {
        self.0.lock().unwrap().push((channels, event, data));
        Box::pin(async { Ok(()) })
    }
}

/// Registers [`Broadcasts`] as the app's `dyn Broadcaster`.
pub struct BroadcastsModule(pub Arc<Broadcasts>);

impl Module for BroadcastsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.global()
            .value_arc(self.0.clone() as Arc<dyn Broadcaster>)
            .exports::<dyn Broadcaster>();
    }
}

/// Grants `permission` to `user`, app-wide or in `workspace` (a
/// `dyn PermissionCheck` multi-binding).
pub struct Grant {
    pub user: Uuid,
    pub permission: &'static str,
    pub workspace: Option<Uuid>,
}

impl PermissionCheck for Grant {
    fn check<'a>(
        &'a self,
        principal: &'a Principal,
        permission: &'a str,
        workspace: Option<Uuid>,
    ) -> BoxFuture<'a, Result<Option<bool>, BoxError>> {
        Box::pin(async move {
            Ok(Some(
                principal.user_id == self.user
                    && permission == self.permission
                    && (self.workspace.is_none() || workspace == self.workspace),
            ))
        })
    }
}

/// Registers the grants as `dyn PermissionCheck` multi-bindings.
pub struct GrantsModule(pub Vec<Arc<Grant>>);

impl Module for GrantsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        for grant in &self.0 {
            let grant = grant.clone();
            m.multi(Factory::<dyn PermissionCheck>::new(move |_| {
                let grant = grant.clone();
                async move { Ok(grant as Arc<dyn PermissionCheck>) }
            }));
        }
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
            .job::<HangAlways>()
            .job::<Toggle>()
            .job::<Panics>()
            .job::<HideJobsTable>()
            .job::<Cooperative>()
            .job::<Export>()
            .job::<CompleteOperation>();
    }
}
