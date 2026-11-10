//! The `schedule:run`, `schedule:work` and `schedule:list` commands.

use std::sync::Arc;

use navree_kernel::{AppContext, BoxError};
use tokio::sync::mpsc;

use crate::support::{Counter, Tasks, TestDb, at, clock, runs, within};

/// Runs the registered command `name` against `ctx`.
async fn command(ctx: &AppContext, name: &str) -> Result<(), BoxError> {
    let spec = ctx
        .commands()
        .into_iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("`{name}` is registered"));
    (spec.run)(ctx.clone(), Vec::new()).await
}

#[tokio::test]
async fn schedule_run_ticks_once_at_the_clock_time() {
    let db = TestDb::new();
    let count = Counter::default();
    let task = count.clone();
    let app = db
        .boot(
            &clock("2026-10-25T02:30:20Z"),
            Tasks::new(move |s| {
                s.task("report", task.task()).daily_at("02:30");
                s.task("later", task.task()).daily_at("03:00");
            }),
        )
        .await;

    app.run_command("schedule:run", Vec::new()).await.unwrap();
    assert_eq!(count.get(), 1);
    let rows = runs(&app).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].task, "report");
    assert_eq!(rows[0].tick, at("2026-10-25T02:30:00Z"));

    // Running it again in the same minute does nothing.
    app.run_command("schedule:run", Vec::new()).await.unwrap();
    assert_eq!(count.get(), 1);
}

#[tokio::test]
async fn schedule_run_fails_when_a_task_fails() {
    let db = TestDb::new();
    let app = db
        .boot(
            &clock("2026-10-25T02:30:20Z"),
            Tasks::new(|s| {
                s.task("broken", |_ctx| async { Err("no route to host".into()) });
            }),
        )
        .await;
    let error = app
        .run_command("schedule:run", Vec::new())
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("broken"), "{error}");
}

#[tokio::test]
async fn schedule_list_runs() {
    let db = TestDb::new();
    let count = Counter::default();
    let app = db
        .boot(
            &clock("2026-10-25T02:30:20Z"),
            Tasks::new(move |s| {
                s.task("report", count.task()).daily_at("02:30");
            }),
        )
        .await;
    app.run_command("schedule:list", Vec::new()).await.unwrap();
}

#[tokio::test]
async fn schedule_work_ticks_on_minute_boundaries_until_shutdown() {
    let db = TestDb::new();
    let (ran, mut ran_rx) = mpsc::unbounded_channel();
    let ran = Arc::new(ran);
    // 200 ms before the next minute.
    let app = db
        .boot(
            &clock("2026-10-25T10:00:59.800Z"),
            Tasks::new(move |s| {
                let ran = ran.clone();
                s.task("heartbeat", move |_ctx| {
                    let _ = ran.send(());
                    std::future::ready(Ok(()))
                });
            }),
        )
        .await;

    let ctx = app.ctx().clone();
    let worker = tokio::spawn(async move { command(&ctx, "schedule:work").await });
    within(ran_rx.recv()).await.expect("the task ran");

    // The run is recorded under the minute boundary the worker waited for.
    let rows = within(async {
        loop {
            let rows = runs(&app).await;
            if rows.iter().all(|r| r.status != "running") {
                break rows;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await;
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].task, "heartbeat");
    assert_eq!(rows[0].tick, at("2026-10-25T10:01:00Z"));
    assert_eq!(rows[0].status, "succeeded");

    app.ctx().shutdown_token().cancel();
    within(worker).await.unwrap().unwrap();
    within(app.shutdown()).await.unwrap();
}
