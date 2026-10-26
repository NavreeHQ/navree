//! `Scheduler::tick`: which tasks are due, and what a run records.

use navis_schedule::RunStatus;

use crate::support::{Counter, Tasks, TestDb, at, clock, scheduler};

#[tokio::test]
async fn daily_at_due_only_at_that_minute() {
    let db = TestDb::new();
    let runs = Counter::default();
    let task = runs.clone();
    let app = db
        .boot(
            &clock("2026-10-25T00:00:00Z"),
            Tasks::new(move |s| {
                s.task("report", task.task()).daily_at("02:30");
            }),
        )
        .await;
    let scheduler = scheduler(&app).await;

    let before = scheduler.tick(at("2026-10-25T02:29:59Z")).await.unwrap();
    assert!(before.is_empty(), "not due yet: {before:?}");

    let due = scheduler.tick(at("2026-10-25T02:30:42Z")).await.unwrap();
    assert_eq!(due.len(), 1, "{due:?}");
    assert_eq!(due[0].task, "report");
    assert_eq!(due[0].tick, at("2026-10-25T02:30:00Z"));
    assert_eq!(due[0].status, RunStatus::Succeeded);
    assert_eq!(runs.get(), 1);

    let after = scheduler.tick(at("2026-10-25T02:31:00Z")).await.unwrap();
    assert!(after.is_empty(), "no longer due: {after:?}");

    let next_day = scheduler.tick(at("2026-10-26T02:30:00Z")).await.unwrap();
    assert_eq!(next_day.len(), 1);
    assert_eq!(runs.get(), 2);
}

#[tokio::test]
async fn cron_expression_parsed() {
    let db = TestDb::new();
    let runs = Counter::default();
    let task = runs.clone();
    let app = db
        .boot(
            &clock("2026-10-25T00:00:00Z"),
            Tasks::new(move |s| {
                s.task("business-hours", task.task())
                    .cron("*/10 9-17 * * MON-FRI");
            }),
        )
        .await;
    let scheduler = scheduler(&app).await;

    // 2026-10-26 is a Monday, 2026-10-31 a Saturday.
    let monday = scheduler.tick(at("2026-10-26T09:10:00Z")).await.unwrap();
    assert_eq!(monday.len(), 1, "{monday:?}");
    for not_due in [
        "2026-10-26T09:15:00Z",
        "2026-10-26T08:50:00Z",
        "2026-10-26T18:00:00Z",
        "2026-10-31T09:10:00Z",
    ] {
        let outcomes = scheduler.tick(at(not_due)).await.unwrap();
        assert!(outcomes.is_empty(), "{not_due}: {outcomes:?}");
    }
    assert_eq!(runs.get(), 1);
}

#[tokio::test]
async fn invalid_cron_expression_fails_the_boot() {
    for expr in ["61 * * * *", "0 */5 * * * *", "every day"] {
        let db = TestDb::new();
        let runs = Counter::default();
        let error = db
            .app(
                &clock("2026-10-25T00:00:00Z"),
                Tasks::new(move |s| {
                    s.task("broken", runs.task()).cron(expr);
                }),
            )
            .boot()
            .await
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("`broken`") && error.contains(expr) && error.contains("Tasks"),
            "{expr}: {error}"
        );
    }
}

#[tokio::test]
async fn malformed_daily_at_fails_the_boot() {
    let db = TestDb::new();
    let runs = Counter::default();
    let error = db
        .app(
            &clock("2026-10-25T00:00:00Z"),
            Tasks::new(move |s| {
                s.task("nightly", runs.task()).daily_at("25:00");
            }),
        )
        .boot()
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("`nightly`") && error.contains("25:00"),
        "{error}"
    );
}

#[tokio::test]
async fn list_shows_next_run() {
    let db = TestDb::new();
    let runs = Counter::default();
    let app = db
        .boot(
            &clock("2026-10-25T00:00:00Z"),
            Tasks::new(move |s| {
                s.task("report", runs.task()).daily_at("02:30");
                s.task("sync", runs.task()).every_five_minutes();
                s.task("digest", runs.task()).weekly();
            }),
        )
        .await;

    // 2026-10-25 is a Sunday.
    let list = scheduler(&app).await.list(at("2026-10-25T02:31:10Z"));
    let rows: Vec<_> = list
        .iter()
        .map(|t| (t.name.as_str(), t.expr.as_str(), t.next_run))
        .collect();
    assert_eq!(
        rows,
        [
            ("report", "30 2 * * *", Some(at("2026-10-26T02:30:00Z"))),
            ("sync", "*/5 * * * *", Some(at("2026-10-25T02:35:00Z"))),
            ("digest", "0 0 * * 0", Some(at("2026-11-01T00:00:00Z"))),
        ]
    );
}
