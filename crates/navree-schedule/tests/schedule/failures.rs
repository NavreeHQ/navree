//! Failed and panicking tasks are recorded, and do not stop other tasks.

use std::fmt;

use navree_schedule::RunStatus;

use crate::databases::Scratch;
use crate::on_every_server;
use crate::support::{Counter, Tasks, TestDb, at, clock, runs, scheduler};

/// An error with a source, to check the stored message keeps the chain.
#[derive(Debug)]
struct SyncFailed(std::io::Error);

impl fmt::Display for SyncFailed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invoice sync failed")
    }
}

impl std::error::Error for SyncFailed {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

async fn failed_task_records_error(scratch: Scratch) {
    let db = TestDb::on(&scratch);
    let clock = clock("2026-10-25T04:00:07Z");
    let app = db
        .boot(
            &clock,
            Tasks::new(|s| {
                s.task("sync", |_ctx| async {
                    Err(SyncFailed(std::io::Error::other("connection refused")).into())
                })
                .hourly();
            }),
        )
        .await;

    let outcomes = scheduler(&app)
        .await
        .tick(at("2026-10-25T04:00:07Z"))
        .await
        .unwrap();
    assert_eq!(
        outcomes[0].status,
        RunStatus::Failed("invoice sync failed: connection refused".to_owned())
    );

    let rows = runs(&app).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status, "failed");
    assert_eq!(
        rows[0].error.as_deref(),
        Some("invoice sync failed: connection refused")
    );
    assert_eq!(rows[0].finished_at, Some(at("2026-10-25T04:00:07Z")));
}

async fn panicking_task_is_recorded_as_failed_and_others_still_run(scratch: Scratch) {
    let db = TestDb::on(&scratch);
    let count = Counter::default();
    let task = count.clone();
    let app = db
        .boot(
            &clock("2026-10-25T04:00:00Z"),
            Tasks::new(move |s| {
                s.task("explodes", |_ctx| async { panic!("kaboom") });
                s.task("fine", task.task());
            }),
        )
        .await;

    let outcomes = scheduler(&app)
        .await
        .tick(at("2026-10-25T04:00:00Z"))
        .await
        .unwrap();
    let explodes = outcomes.iter().find(|o| o.task == "explodes").unwrap();
    match &explodes.status {
        RunStatus::Failed(message) => assert!(
            message.contains("panicked") && message.contains("kaboom"),
            "{message}"
        ),
        other => panic!("expected a failure, got {other:?}"),
    }
    let fine = outcomes.iter().find(|o| o.task == "fine").unwrap();
    assert_eq!(fine.status, RunStatus::Succeeded);
    assert_eq!(count.get(), 1);

    let rows = runs(&app).await;
    let failed = rows.iter().find(|r| r.task == "explodes").unwrap();
    assert_eq!(failed.status, "failed");
    assert!(failed.error.as_deref().unwrap().contains("kaboom"));
}

on_every_server!(
    "navree_schedule";
    failed_task_records_error,
    panicking_task_is_recorded_as_failed_and_others_still_run,
);
