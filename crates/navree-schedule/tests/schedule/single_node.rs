//! Each tick of a task runs once across every scheduler sharing the database.

use navree_schedule::{RunStatus, SkipReason};

use crate::databases::Scratch;
use crate::on_every_server;
use crate::support::{Counter, Tasks, TestDb, at, clock, runs, scheduler};

async fn same_tick_runs_once_across_two_schedulers(scratch: Scratch) {
    let db = TestDb::on(&scratch);
    let clock = clock("2026-10-25T02:00:05Z");
    let count = Counter::default();
    let (first, second) = (count.clone(), count.clone());
    let one = db
        .boot(
            &clock,
            Tasks::new(move |s| {
                s.task("sync", first.task()).hourly().on_one_server();
            }),
        )
        .await;
    let two = db
        .boot(
            &clock,
            Tasks::new(move |s| {
                s.task("sync", second.task()).hourly().on_one_server();
            }),
        )
        .await;
    let (a, b) = (scheduler(&one).await, scheduler(&two).await);

    let now = at("2026-10-25T02:00:05Z");
    let (left, right) = tokio::join!(a.tick(now), b.tick(now));
    let mut statuses: Vec<RunStatus> = left
        .unwrap()
        .into_iter()
        .chain(right.unwrap())
        .map(|o| o.status)
        .collect();
    statuses.sort_by_key(|s| format!("{s:?}"));
    assert_eq!(
        statuses,
        vec![
            RunStatus::Skipped(SkipReason::AlreadyClaimed),
            RunStatus::Succeeded
        ]
    );
    assert_eq!(count.get(), 1);

    // A second tick in the same minute (cron calling `schedule:run` twice) is skipped too.
    let again = a.tick(at("2026-10-25T02:00:40Z")).await.unwrap();
    assert_eq!(
        again[0].status,
        RunStatus::Skipped(SkipReason::AlreadyClaimed)
    );
    assert_eq!(count.get(), 1);

    let rows = runs(&one).await;
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].task, "sync");
    assert_eq!(rows[0].tick, at("2026-10-25T02:00:00Z"));
    assert_eq!(rows[0].status, "succeeded");
    assert_eq!(rows[0].started_at, at("2026-10-25T02:00:05Z"));
    assert_eq!(rows[0].finished_at, Some(at("2026-10-25T02:00:05Z")));
    assert_eq!(rows[0].error, None);

    // The next hour runs again.
    let next = b.tick(at("2026-10-25T03:00:00Z")).await.unwrap();
    assert_eq!(next[0].status, RunStatus::Succeeded);
    assert_eq!(count.get(), 2);
}

on_every_server!(
    "navree_schedule";
    same_tick_runs_once_across_two_schedulers,
);
