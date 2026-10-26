//! Telemetry: each run is a `ScheduledTaskRan` event, and the task runs in
//! its own telemetry context (with a trace).

use std::sync::{Arc, Mutex};

use navree_telemetry::{EventKind, TelemetryContext, TelemetryRecorder};

use crate::support::{Counter, Tasks, TestDb, at, clock, scheduler};

#[tokio::test]
async fn runs_are_reported_with_their_outcome() {
    let db = TestDb::new();
    let clock = clock("2026-10-25T04:00:07Z");
    let recorder = TelemetryRecorder::new();
    let counter = Counter::default();
    let counting = counter.clone();
    let app = db
        .app(
            &clock,
            Tasks::new(move |s| {
                s.task("report", counting.task()).hourly();
                s.task("sync", |_ctx| async { Err("connection refused".into()) })
                    .hourly();
                s.task("later", |_ctx| async { Ok(()) }).daily_at("08:00");
            }),
        )
        .module(recorder.module())
        .boot()
        .await
        .expect("boots");

    scheduler(&app)
        .await
        .tick(at("2026-10-25T04:00:07Z"))
        .await
        .unwrap();

    let mut runs = recorder.scheduled_tasks();
    runs.sort_by(|a, b| a.task.cmp(&b.task));
    assert_eq!(runs.len(), 2, "tasks that are not due are not reported: {runs:?}");
    assert_eq!(runs[0].task, "report");
    assert_eq!(runs[0].error, None);
    assert_eq!(runs[0].tick, at("2026-10-25T04:00:00Z"));
    assert_eq!(runs[1].task, "sync");
    assert_eq!(runs[1].error.as_deref(), Some("connection refused"));
    assert_eq!(counter.get(), 1);

    // A tick claimed already is skipped, and skips are not runs.
    scheduler(&app)
        .await
        .tick(at("2026-10-25T04:00:30Z"))
        .await
        .unwrap();
    assert_eq!(recorder.scheduled_tasks().len(), 2);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn tasks_run_in_a_traced_context() {
    let db = TestDb::new();
    let clock = clock("2026-10-25T04:00:07Z");
    let recorder = TelemetryRecorder::new();
    let seen: Arc<Mutex<Option<bool>>> = Arc::new(Mutex::new(None));
    let inside = seen.clone();
    let app = db
        .app(
            &clock,
            Tasks::new(move |s| {
                let inside = inside.clone();
                s.task("traced", move |_ctx| {
                    let traced = TelemetryContext::current().is_some_and(|c| c.trace().is_some());
                    *inside.lock().unwrap() = Some(traced);
                    async { Ok(()) }
                })
                .hourly();
            }),
        )
        .module(recorder.module())
        .boot()
        .await
        .expect("boots");

    scheduler(&app)
        .await
        .tick(at("2026-10-25T04:00:07Z"))
        .await
        .unwrap();
    assert_eq!(*seen.lock().unwrap(), Some(true));
    let record = &recorder.of_kind(EventKind::ScheduledTaskRan)[0];
    assert!(record.context.trace.is_some());
    app.shutdown().await.unwrap();
}
