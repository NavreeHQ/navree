//! The cron language as the scheduler sees it: errors at boot, expressions
//! that never match, and the documented forms.

use crate::support::{Counter, Tasks, TestDb, at, clock, scheduler};

#[tokio::test]
async fn invalid_expressions_name_the_field_and_the_token() {
    for (expr, expected) in [
        ("61 * * * *", "minute field: `61`"),
        ("0 0 * * FUN", "day of week field: `FUN`"),
        ("0 0 0 * * *", "has 6 fields"),
        ("@midnight", "`@midnight`"),
    ] {
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
        assert!(error.contains(expected), "{expr}: {error}");
    }
}

#[tokio::test]
async fn an_expression_that_never_matches_has_no_next_run_and_never_ticks() {
    let db = TestDb::new();
    let runs = Counter::default();
    let task = runs.clone();
    let app = db
        .boot(
            &clock("2026-10-25T00:00:00Z"),
            Tasks::new(move |s| {
                s.task("never", task.task()).cron("0 0 30 2 *");
                s.task("leap", runs.task()).cron("0 0 29 2 *");
            }),
        )
        .await;
    let scheduler = scheduler(&app).await;
    let list = scheduler.list(at("2026-10-25T00:00:00Z"));
    assert_eq!(list[0].next_run, None);
    assert_eq!(list[1].next_run, Some(at("2028-02-29T00:00:00Z")));
    for tick in ["2026-02-28T00:00:00Z", "2026-03-01T00:00:00Z"] {
        assert!(scheduler.tick(at(tick)).await.unwrap().is_empty());
    }
}

#[tokio::test]
async fn documented_forms_boot_and_list_their_next_run() {
    let db = TestDb::new();
    let runs = Counter::default();
    let app = db
        .boot(
            &clock("2026-10-25T00:00:00Z"),
            Tasks::new(move |s| {
                s.task("a", runs.task()).cron("*/10 9-17 * * MON-FRI");
                s.task("b", runs.task()).cron("0 6 L * *");
                s.task("c", runs.task()).cron("30 3 * * FRI#2");
                s.task("d", runs.task()).cron("@monthly");
                s.task("e", runs.task()).cron("0 0 1-7 * +MON");
            }),
        )
        .await;
    // 2026-10-25 is a Sunday.
    let next: Vec<_> = scheduler(&app)
        .await
        .list(at("2026-10-25T12:00:00Z"))
        .iter()
        .map(|t| t.next_run)
        .collect();
    assert_eq!(
        next,
        [
            Some(at("2026-10-26T09:00:00Z")),
            Some(at("2026-10-31T06:00:00Z")),
            Some(at("2026-11-13T03:30:00Z")),
            Some(at("2026-11-01T00:00:00Z")),
            Some(at("2026-11-02T00:00:00Z")),
        ]
    );
}
