//! Apps, jobs and helpers shared by the navree-fleet integration tests.

#![allow(dead_code)]

use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use navree_contracts::{
    Backoff, BoxError, Clock, FixedClock, Job, JobContext, JobOptions, ModuleBuilderJobsExt,
};
use navree_db::DatabaseModule;
use navree_fleet::{Fleet, FleetModule, MasterSettings};
use navree_kernel::{Module, ModuleBuilder};
use navree_queue::{Queue, QueueModule};
use navree_testing::{TestApp, TestAppBuilder};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A fixed clock's starting time.
pub fn start() -> DateTime<Utc> {
    "2026-10-26T09:00:00Z".parse().expect("valid time")
}

/// What the test jobs did.
#[derive(Debug, Default)]
pub struct Journal {
    runs: Mutex<Vec<String>>,
}

impl Journal {
    pub fn record(&self, label: &str) {
        self.runs.lock().unwrap().push(label.to_owned());
    }

    pub fn labels(&self) -> Vec<String> {
        self.runs.lock().unwrap().clone()
    }

    pub fn count(&self) -> usize {
        self.runs.lock().unwrap().len()
    }
}

async fn journal(ctx: &JobContext) -> Result<Arc<Journal>, BoxError> {
    Ok(ctx.app.get::<Journal>().await?)
}

/// Records its label; on queue `default`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Record {
    pub label: String,
}

impl Record {
    pub fn new(label: &str) -> Self {
        Self {
            label: label.to_owned(),
        }
    }
}

impl Job for Record {
    const NAME: &'static str = "test.record";

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        journal(&ctx).await?.record(&self.label);
        Ok(())
    }
}

/// Records its label; on queue `mail` (as the docs' `SendDigest`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SendDigest {
    pub user: String,
    pub api_token: String,
}

impl Job for SendDigest {
    const NAME: &'static str = "mail.digest";

    fn options() -> JobOptions {
        JobOptions {
            queue: "mail".into(),
            ..JobOptions::default()
        }
    }

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        journal(&ctx).await?.record(&self.user);
        Ok(())
    }
}

/// Always fails, once (no retries); on queue `exports`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Export {
    pub contact: String,
    pub password: String,
}

impl Job for Export {
    const NAME: &'static str = "contacts.export";

    fn options() -> JobOptions {
        JobOptions {
            queue: "exports".into(),
            max_attempts: 1,
            backoff: Backoff::Fixed { secs: 0 },
            ..JobOptions::default()
        }
    }

    async fn handle(self, _ctx: JobContext) -> Result<(), BoxError> {
        Err("the mail server is down".into())
    }
}

/// Sleeps `ms`, then records; on queue `default`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Slow {
    pub label: String,
    pub ms: u64,
}

impl Job for Slow {
    const NAME: &'static str = "test.slow";

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        tokio::time::sleep(Duration::from_millis(self.ms)).await;
        journal(&ctx).await?.record(&self.label);
        Ok(())
    }
}

/// Registers the test jobs.
pub struct JobsModule;

impl Module for JobsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.job::<Record>()
            .job::<SendDigest>()
            .job::<Export>()
            .job::<Slow>();
    }
}

/// A test app with a database, the queue, Fleet (no dashboard) and the
/// test jobs; workers poll every 20 ms.
pub fn builder() -> TestAppBuilder {
    crate::database::database(TestApp::builder())
        .config("queue.poll_interval_ms", 20)
        .config("fleet.dashboard", false)
        .module(DatabaseModule::from_config())
        .module(QueueModule::from_config())
        .module(FleetModule::from_config())
        .module(JobsModule)
        .override_value(Arc::new(Journal::default()))
}

/// A booted app and its services.
pub struct Harness {
    pub app: TestApp,
    pub fleet: Arc<Fleet>,
    pub queue: Arc<Queue>,
    pub journal: Arc<Journal>,
}

impl Harness {
    pub async fn new(builder: TestAppBuilder) -> Self {
        let app = builder.build().await;
        Self {
            fleet: app.get::<Fleet>().await,
            queue: app.get::<Queue>().await,
            journal: app.get::<Journal>().await,
            app,
        }
    }

    /// With the clock fixed at [`start`].
    pub async fn with_clock(builder: TestAppBuilder) -> (Self, Arc<FixedClock>) {
        let clock = Arc::new(FixedClock::new(start()));
        let harness = Self::new(builder.override_value(clock.clone() as Arc<dyn Clock>)).await;
        (harness, clock)
    }
}

/// Fast master pacing for tests.
pub fn fast() -> MasterSettings {
    MasterSettings {
        name: "worker-1".into(),
        tick: Duration::from_millis(20),
        heartbeat: Duration::from_millis(40),
        worker_heartbeat: Duration::from_millis(20),
        restart_delay: Duration::from_millis(100),
        max_restart_delay: Duration::from_millis(400),
    }
}

/// Polls `check` every 10 ms until it returns true, for at most 10 s.
pub async fn eventually<F, Fut>(what: &str, mut check: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    let deadline = Instant::now() + Duration::from_secs(10);
    while !check().await {
        assert!(Instant::now() < deadline, "timed out waiting for: {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// `value` with key `key` removed, for comparing records with ids.
pub fn without(mut value: Value, key: &str) -> Value {
    if let Some(map) = value.as_object_mut() {
        map.remove(key);
    }
    value
}
