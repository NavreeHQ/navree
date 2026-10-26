//! Telemetry: `JobProcessed` and `JobFailed` events, and the job's
//! `TelemetryContext` (job id and trace) while its handler runs.

use std::sync::Arc;

use navree_contracts::{BoxError, Clock, FixedClock, Job, JobContext, ModuleBuilderJobsExt};
use navree_db::DatabaseModule;
use navree_db::{Db, Migrator};
use navree_kernel::{Module, ModuleBuilder};
use navree_queue::{Queue, QueueDriver, QueueModule};
use navree_telemetry::{EventKind, Telemetry, TelemetryContext, TelemetryRecorder, TraceContext};
use navree_testing::TestApp;
use serde::{Deserialize, Serialize};

use crate::support::{AlwaysFails, JobsModule, Journal, Record, options, start};

/// Records the current job id and trace id as its label.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Traced;

impl Job for Traced {
    const NAME: &'static str = "test.traced";

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        let context = TelemetryContext::current().ok_or("no telemetry context")?;
        let label = format!(
            "{}|{}",
            context.job_id().unwrap_or_default(),
            navree_telemetry::current_trace_id()
                .map(|id| id.to_string())
                .unwrap_or_default()
        );
        ctx.app
            .get::<Journal>()
            .await?
            .record(Self::NAME, &label, ctx.attempt);
        Ok(())
    }
}

struct TracedModule;

impl Module for TracedModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.job::<Traced>();
    }
}

struct Setup {
    app: TestApp,
    queue: Arc<Queue>,
    clock: Arc<FixedClock>,
    journal: Arc<Journal>,
    recorder: TelemetryRecorder,
}

async fn setup() -> Setup {
    setup_with(QueueDriver::Memory).await
}

async fn setup_with(driver: QueueDriver) -> Setup {
    let clock = Arc::new(FixedClock::new(start()));
    let journal = Arc::new(Journal::default());
    let recorder = TelemetryRecorder::new();
    let app = TestApp::builder()
        .temp_database()
        .module(DatabaseModule)
        .module(QueueModule::for_root(options(driver)))
        .module(JobsModule)
        .module(TracedModule)
        .module(recorder.module())
        .override_value(clock.clone() as Arc<dyn Clock>)
        .override_value(journal.clone())
        .build()
        .await;
    let queue = app.get::<Queue>().await;
    Setup {
        app,
        queue,
        clock,
        journal,
        recorder,
    }
}

#[tokio::test]
async fn a_successful_attempt_is_reported() {
    let s = setup().await;
    let id = s.queue.dispatch(Record::new("a")).await.unwrap();
    s.queue.run_pending().await.unwrap();

    let processed = s.recorder.jobs_processed();
    assert_eq!(processed.len(), 1, "{processed:?}");
    let job = &processed[0];
    assert_eq!(job.queue, "default");
    assert_eq!(job.job_type, "test.record");
    assert_eq!(job.job_id, id);
    assert_eq!(job.attempt, 1);
    let record = &s.recorder.of_kind(EventKind::JobProcessed)[0];
    assert_eq!(record.context.job_id.as_deref(), Some(id.as_str()));
    assert!(record.context.trace.is_some());
    assert!(s.recorder.jobs_failed().is_empty());
    s.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn failed_attempts_are_reported_until_the_job_fails() {
    let s = setup().await;
    s.queue
        .dispatch(AlwaysFails {
            label: "x".to_owned(),
        })
        .await
        .unwrap();
    for _ in 0..3 {
        s.queue.run_pending().await.unwrap();
        s.clock.advance(std::time::Duration::from_secs(600));
    }

    let failed = s.recorder.jobs_failed();
    assert_eq!(failed.len(), 3, "{failed:?}");
    assert!(failed[0].will_retry && failed[1].will_retry);
    assert!(
        !failed[2].will_retry,
        "the last attempt moves it to the failed jobs"
    );
    assert_eq!(failed[0].attempt, 1);
    assert_eq!(failed[2].attempt, 3);
    assert!(
        failed[0].error.contains("boom on attempt 1"),
        "{}",
        failed[0].error
    );
    assert_eq!(failed[0].job_type, "test.always_fails");
    s.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn handlers_run_in_the_jobs_context() {
    let s = setup().await;
    let id = s.queue.dispatch(Traced).await.unwrap();
    s.queue.run_pending().await.unwrap();
    let labels = s.journal.labels();
    assert_eq!(labels.len(), 1, "{labels:?}");
    let (job_id, trace_id) = labels[0].split_once('|').unwrap();
    assert_eq!(job_id, id);
    assert_eq!(trace_id.len(), 32, "each attempt has a trace");
    s.app.shutdown().await.unwrap();
}

/// Dispatches `Traced` inside a request-like context with a sampled trace,
/// runs it and checks that the attempt continued the dispatcher's trace.
async fn a_dispatched_job_continues_the_trace(driver: QueueDriver) {
    let s = setup_with(driver).await;
    let dispatcher = TraceContext::root(true);
    let id = TelemetryContext::new(Telemetry::disabled())
        .with_request_id("req-1")
        .with_trace(dispatcher.clone())
        .scope(s.queue.dispatch(Traced))
        .await
        .unwrap();
    s.queue.run_pending().await.unwrap();

    let labels = s.journal.labels();
    assert_eq!(
        labels,
        [format!("{id}|{}", dispatcher.trace_id)],
        "the handler runs in the dispatcher's trace"
    );
    let record = &s.recorder.of_kind(EventKind::JobProcessed)[0];
    let trace = record.context.trace.as_ref().expect("a trace");
    assert_eq!(trace.trace_id, dispatcher.trace_id);
    assert_eq!(trace.parent_span_id, Some(dispatcher.span_id));
    assert_ne!(trace.span_id, dispatcher.span_id, "the job is its own span");
    assert!(trace.sampled);
    assert_eq!(record.context.request_id, None, "a job is not the request");
    s.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_job_continues_the_trace_it_was_dispatched_in_memory() {
    a_dispatched_job_continues_the_trace(QueueDriver::Memory).await;
}

#[tokio::test]
async fn a_job_continues_the_trace_it_was_dispatched_in_database() {
    a_dispatched_job_continues_the_trace(QueueDriver::Database).await;
}

#[tokio::test]
async fn a_job_dispatched_in_a_unit_of_work_keeps_the_trace() {
    for driver in [QueueDriver::Memory, QueueDriver::Database] {
        let s = setup_with(driver).await;
        let db = s.app.get::<Db>().await;
        let uow = db.uow(None, None).await;
        let dispatcher = TraceContext::root(true);
        TelemetryContext::new(Telemetry::disabled())
            .with_trace(dispatcher.clone())
            .scope(async {
                s.queue.dispatch_in(&uow, Traced).await.unwrap();
                uow.commit().await.unwrap();
            })
            .await;
        s.queue.run_pending().await.unwrap();
        let record = &s.recorder.of_kind(EventKind::JobProcessed)[0];
        let trace = record.context.trace.as_ref().expect("a trace");
        assert_eq!(trace.trace_id, dispatcher.trace_id, "{driver:?}");
        assert_eq!(trace.parent_span_id, Some(dispatcher.span_id), "{driver:?}");
        s.app.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn a_job_dispatched_outside_a_trace_starts_its_own() {
    let s = setup_with(QueueDriver::Database).await;
    s.queue.dispatch(Traced).await.unwrap();
    s.queue.run_pending().await.unwrap();
    let record = &s.recorder.of_kind(EventKind::JobProcessed)[0];
    let trace = record.context.trace.as_ref().expect("a trace");
    assert_eq!(trace.parent_span_id, None);
    s.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_trace_parent_column_is_reverted_and_applied_again() {
    let s = setup_with(QueueDriver::Database).await;
    let reverted = Migrator::rollback(s.app.ctx()).await.unwrap();
    assert!(
        reverted
            .iter()
            .any(|name| name == "queue/m20261026_000001_add_jobs_trace_parent"),
        "{reverted:?}"
    );
    Migrator::up(s.app.ctx()).await.unwrap();
    let dispatcher = TraceContext::root(true);
    TelemetryContext::new(Telemetry::disabled())
        .with_trace(dispatcher.clone())
        .scope(s.queue.dispatch(Traced))
        .await
        .unwrap();
    s.queue.run_pending().await.unwrap();
    let record = &s.recorder.of_kind(EventKind::JobProcessed)[0];
    assert_eq!(
        record.context.trace.as_ref().map(|trace| trace.trace_id),
        Some(dispatcher.trace_id)
    );
    s.app.shutdown().await.unwrap();
}
