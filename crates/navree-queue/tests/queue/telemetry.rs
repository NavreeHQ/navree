//! Telemetry: `JobProcessed` and `JobFailed` events, and the job's
//! `TelemetryContext` (job id and trace) while its handler runs.

use std::sync::Arc;

use navree_contracts::{BoxError, Clock, FixedClock, Job, JobContext, ModuleBuilderJobsExt};
use navree_db::DatabaseModule;
use navree_kernel::{Module, ModuleBuilder};
use navree_queue::{Queue, QueueDriver, QueueModule};
use navree_telemetry::{EventKind, TelemetryContext, TelemetryRecorder};
use navree_testing::TestApp;
use serde::{Deserialize, Serialize};

use crate::support::{AlwaysFails, Journal, JobsModule, Record, options, start};

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
    let clock = Arc::new(FixedClock::new(start()));
    let journal = Arc::new(Journal::default());
    let recorder = TelemetryRecorder::new();
    let app = TestApp::builder()
        .temp_database()
        .module(DatabaseModule)
        .module(QueueModule::for_root(options(QueueDriver::Memory)))
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
    assert!(!failed[2].will_retry, "the last attempt moves it to the failed jobs");
    assert_eq!(failed[0].attempt, 1);
    assert_eq!(failed[2].attempt, 3);
    assert!(failed[0].error.contains("boom on attempt 1"), "{}", failed[0].error);
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
