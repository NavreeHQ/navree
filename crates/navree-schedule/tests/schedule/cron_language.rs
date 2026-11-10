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

/// Ticks every minute in `[from, to)` and returns the minutes each task ran.
async fn ran_at(
    scheduler: &navree_schedule::Scheduler,
    from: &str,
    to: &str,
) -> std::collections::BTreeMap<String, Vec<chrono::DateTime<chrono::Utc>>> {
    let mut ran = std::collections::BTreeMap::<String, Vec<_>>::new();
    let mut minute = at(from);
    while minute < at(to) {
        for outcome in scheduler.tick(minute).await.unwrap() {
            ran.entry(outcome.task).or_default().push(minute);
        }
        minute += chrono::TimeDelta::minutes(1);
    }
    ran
}

#[tokio::test]
async fn interval_tasks_run_through_a_clock_change_and_fixed_ones_once() {
    let db = TestDb::new();
    let runs = Counter::default();
    let app = db
        .boot(
            &clock("2026-11-01T00:00:00Z"),
            Tasks::new(move |s| {
                // Wildcard: follows the wall clock, in both passes of 01:00.
                s.task("hourly", runs.task())
                    .cron("0 * * * *")
                    .in_zone("America/New_York");
                s.task("quarter", runs.task())
                    .cron("*/15 * * * *")
                    .in_zone("America/New_York");
                // Fixed-time: the repeated 01:30 runs once.
                s.task("nightly", runs.task())
                    .cron("30 1 * * *")
                    .in_zone("America/New_York");
            }),
        )
        .await;
    let scheduler = scheduler(&app).await;
    // 2026-11-01: 01:59 EDT (05:59Z) is followed by 01:00 EST (06:00Z).
    let ran = ran_at(&scheduler, "2026-11-01T04:00:00Z", "2026-11-01T09:00:00Z").await;
    let hours: Vec<_> = ran["hourly"]
        .iter()
        .map(|t| t.format("%H").to_string())
        .collect();
    assert_eq!(hours, ["04", "05", "06", "07", "08"]);
    // Five real hours, a run every real quarter of an hour: 20.
    assert_eq!(ran["quarter"].len(), 20);
    assert_eq!(ran["nightly"], [at("2026-11-01T05:30:00Z")]);

    // `schedule:list` reads the same rule as the real runs above.
    let list = scheduler.list(at("2026-11-01T05:00:00Z"));
    let next: Vec<_> = list.iter().map(|t| (t.name.as_str(), t.next_run)).collect();
    assert_eq!(
        next,
        [
            ("hourly", Some(at("2026-11-01T06:00:00Z"))),
            ("quarter", Some(at("2026-11-01T05:15:00Z"))),
            ("nightly", Some(at("2026-11-01T05:30:00Z"))),
        ]
    );
    let list = scheduler.list(at("2026-11-01T05:59:00Z"));
    assert_eq!(list[0].next_run, Some(at("2026-11-01T06:00:00Z")));
    let list = scheduler.list(at("2026-11-01T05:30:00Z"));
    assert_eq!(list[2].next_run, Some(at("2026-11-02T06:30:00Z")));
}

#[tokio::test]
async fn interval_tasks_do_not_catch_up_a_gap() {
    let db = TestDb::new();
    let runs = Counter::default();
    let app = db
        .boot(
            &clock("2026-03-08T00:00:00Z"),
            Tasks::new(move |s| {
                s.task("half", runs.task())
                    .cron("30 * * * *")
                    .in_zone("America/New_York");
            }),
        )
        .await;
    let scheduler = scheduler(&app).await;
    // 2026-03-08: 01:59 EST (06:59Z) is followed by 03:00 EDT (07:00Z).
    let ran = ran_at(&scheduler, "2026-03-08T05:00:00Z", "2026-03-08T09:00:00Z").await;
    assert_eq!(
        ran["half"],
        [
            at("2026-03-08T05:30:00Z"),
            at("2026-03-08T06:30:00Z"),
            at("2026-03-08T07:30:00Z"),
            at("2026-03-08T08:30:00Z"),
        ]
    );
}
