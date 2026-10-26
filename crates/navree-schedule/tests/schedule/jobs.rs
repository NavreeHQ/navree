//! `Schedule::job`: due ticks dispatch the job (or run it inline).

use std::sync::{Arc, Mutex};

use navree_contracts::{BoxFuture, Dispatcher, Job, JobContext, JobEnvelope};
use navree_kernel::BoxError;
use navree_schedule::RunStatus;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::support::{Counter, Tasks, TestDb, at, clock, scheduler};

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SyncInvoices {
    account: String,
}

impl Job for SyncInvoices {
    const NAME: &'static str = "billing.sync_invoices";

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        let seen = ctx.app.get::<InlineRuns>().await?;
        seen.0
            .lock()
            .unwrap()
            .push((self.account, ctx.job_id, ctx.attempt));
        Ok(())
    }
}

/// What inline runs of `SyncInvoices` saw: account, job id, attempt.
#[derive(Default)]
struct InlineRuns(Mutex<Vec<(String, String, u32)>>);

/// A dispatcher that records envelopes.
#[derive(Default)]
struct RecordingDispatcher(Mutex<Vec<JobEnvelope>>);

impl Dispatcher for RecordingDispatcher {
    fn dispatch_envelope(&self, env: JobEnvelope) -> BoxFuture<'_, Result<String, BoxError>> {
        Box::pin(async move {
            let mut sent = self.0.lock().unwrap();
            sent.push(env);
            Ok(format!("job-{}", sent.len()))
        })
    }
}

fn sync_acme() -> Tasks {
    Tasks::new(|s| {
        s.job(SyncInvoices {
            account: "acme".to_owned(),
        })
        .hourly();
    })
}

#[tokio::test]
async fn job_task_dispatches_job() {
    let db = TestDb::new();
    let dispatcher = Arc::new(RecordingDispatcher::default());
    let inline = Arc::new(InlineRuns::default());
    let app = db
        .app(&clock("2026-10-25T05:00:00Z"), sync_acme())
        .override_value::<dyn Dispatcher>(dispatcher.clone())
        .override_value(inline.clone())
        .boot()
        .await
        .expect("boots");

    let outcomes = scheduler(&app)
        .await
        .tick(at("2026-10-25T05:00:00Z"))
        .await
        .unwrap();
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].task, "billing.sync_invoices");
    assert_eq!(outcomes[0].status, RunStatus::Succeeded);

    let sent = dispatcher.0.lock().unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].job_type, "billing.sync_invoices");
    assert_eq!(sent[0].payload, json!({ "account": "acme" }));
    assert!(
        inline.0.lock().unwrap().is_empty(),
        "dispatched, not run inline"
    );
}

#[tokio::test]
async fn job_task_runs_inline_without_a_dispatcher() {
    let db = TestDb::new();
    let inline = Arc::new(InlineRuns::default());
    let app = db
        .app(&clock("2026-10-25T05:00:00Z"), sync_acme())
        .override_value(inline.clone())
        .boot()
        .await
        .expect("boots");

    let outcomes = scheduler(&app)
        .await
        .tick(at("2026-10-25T05:00:00Z"))
        .await
        .unwrap();
    assert_eq!(outcomes[0].status, RunStatus::Succeeded);
    let runs = inline.0.lock().unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].0, "acme");
    assert!(
        runs[0]
            .1
            .starts_with("billing.sync_invoices@2026-10-25T05:00:00"),
        "{}",
        runs[0].1
    );
    assert_eq!(runs[0].2, 1);
}

#[tokio::test]
async fn the_same_job_twice_needs_distinct_names() {
    let db = TestDb::new();
    let count = Counter::default();
    let error = db
        .app(
            &clock("2026-10-25T05:00:00Z"),
            Tasks::new(move |s| {
                s.task("dup", count.task()).hourly();
                s.task("dup", count.task()).daily();
            }),
        )
        .boot()
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("`dup`") && error.contains("registered twice"),
        "{error}"
    );
}

#[tokio::test]
async fn renamed_duplicates_are_fine() {
    let db = TestDb::new();
    let app = db
        .app(
            &clock("2026-10-25T05:00:00Z"),
            Tasks::new(|s| {
                let acme = SyncInvoices {
                    account: "acme".to_owned(),
                };
                let globex = SyncInvoices {
                    account: "globex".to_owned(),
                };
                s.job(acme).hourly().name("sync acme");
                s.job(globex).hourly().name("sync globex");
            }),
        )
        .override_value(Arc::new(InlineRuns::default()))
        .boot()
        .await
        .expect("boots");
    let outcomes = scheduler(&app)
        .await
        .tick(at("2026-10-25T05:00:00Z"))
        .await
        .unwrap();
    let mut names: Vec<_> = outcomes.into_iter().map(|o| o.task).collect();
    names.sort();
    assert_eq!(names, ["sync acme", "sync globex"]);
}

#[tokio::test]
async fn empty_or_overlong_task_names_fail_the_boot() {
    for name in [String::new(), "x".repeat(192)] {
        let db = TestDb::new();
        let count = Counter::default();
        let task_name = name.clone();
        let error = db
            .app(
                &clock("2026-10-25T05:00:00Z"),
                Tasks::new(move |s| {
                    s.task(task_name.clone(), count.task());
                }),
            )
            .boot()
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("name"), "{}: {error}", name.len());
    }
}

#[tokio::test]
async fn scheduling_a_job_registers_it_for_the_queue() {
    let db = TestDb::new();
    let app = db.boot(&clock("2026-10-25T05:00:00Z"), sync_acme()).await;
    let names: Vec<&str> = app
        .ctx()
        .contributions::<navree_contracts::JobRegistration>()
        .iter()
        .map(|c| c.value.name)
        .collect();
    assert_eq!(names, ["billing.sync_invoices"]);
}
