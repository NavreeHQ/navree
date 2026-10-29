//! `schedule_runs` keeps a week of runs: older rows are deleted hourly.

use chrono::{DateTime, Utc};
use navree_db::navree_schema::{IndexKind, Schema};
use navree_db::{Db, Executor, ExecutorExt, MIGRATIONS_TABLE, Statement, prune_time_series};
use navree_kernel::App;

use crate::databases::Scratch;
use crate::on_every_server;
use crate::support::{Counter, Tasks, TestDb, at, clock, runs, scheduler};

async fn insert_run(app: &App, task: &str, tick: DateTime<Utc>) {
    let db = app.ctx().get::<Db>().await.unwrap();
    db.execute(&Statement::portable(
        "INSERT INTO schedule_runs (task, tick, started_at, finished_at, status) VALUES (?, ?, ?, ?, ?)",
        [
            task.into(),
            tick.into(),
            tick.into(),
            tick.into(),
            "succeeded".into(),
        ],
    ))
    .await
    .unwrap();
}

async fn tasks_recorded(app: &App) -> Vec<String> {
    let mut tasks: Vec<String> = runs(app).await.into_iter().map(|r| r.task).collect();
    tasks.dedup();
    tasks
}

async fn runs_older_than_a_week_are_pruned_hourly(scratch: Scratch) {
    let db = TestDb::on(&scratch);
    let count = Counter::default();
    let app = db
        .boot(
            &clock("2026-10-25T10:30:00Z"),
            Tasks::new(move |s| {
                s.task("report", count.task());
            }),
        )
        .await;
    insert_run(&app, "eight days old", at("2026-10-17T10:00:00Z")).await;
    insert_run(&app, "six days old", at("2026-10-19T12:00:00Z")).await;
    let scheduler = scheduler(&app).await;

    scheduler.tick(at("2026-10-25T10:30:00Z")).await.unwrap();
    assert_eq!(
        tasks_recorded(&app).await,
        ["eight days old", "six days old", "report"],
        "pruning only runs at the top of the hour"
    );

    scheduler.tick(at("2026-10-25T11:00:00Z")).await.unwrap();
    assert_eq!(tasks_recorded(&app).await, ["six days old", "report"]);
}

/// `schedule_runs` is time series: a TimescaleDB hypertable on `tick` (its
/// key, `(task, tick)`, already holds the time), a regular table with a
/// `tick` index elsewhere, which hourly pruning reads.
async fn the_runs_are_declared_a_hypertable(scratch: Scratch) {
    let db = TestDb::on(&scratch);
    let app = db
        .boot(&clock("2026-10-25T10:30:00Z"), Tasks::new(|_s| {}))
        .await;
    let pool = app.ctx().get::<Db>().await.unwrap();
    let applied: Vec<String> = pool
        .fetch_column(&Statement::text(format!(
            "SELECT name FROM {MIGRATIONS_TABLE} WHERE module = 'schedule' ORDER BY name"
        )))
        .await
        .unwrap();
    assert_eq!(
        applied,
        [
            "m20261025_000001_create_schedule_runs",
            "m20261027_000001_convert_schedule_runs_to_hypertable"
        ]
    );
    let schema = Schema::new(&pool);
    // A hypertable where the database has TimescaleDB, else a regular table
    // with the `tick` index the hourly pruning reads.
    let timescale = schema.target().await.unwrap().timescaledb().is_some();
    assert_eq!(
        schema.is_hypertable("schedule_runs").await.unwrap(),
        timescale
    );
    if !timescale {
        assert!(
            schema
                .has_index("schedule_runs", ["tick"], Some(IndexKind::Index))
                .await
                .unwrap()
        );
    }
    assert!(
        schema
            .has_index("schedule_runs", ["task", "tick"], Some(IndexKind::Primary))
            .await
            .unwrap()
    );
}

/// `prune_time_series` cuts the run table (whole chunks on TimescaleDB)
/// and counts the rows it removed.
async fn pruning_counts_what_it_deletes(scratch: Scratch) {
    let db = TestDb::on(&scratch);
    let app = db
        .boot(&clock("2026-10-25T10:30:00Z"), Tasks::new(|_s| {}))
        .await;
    for (task, tick) in [
        ("old", "2026-10-01T00:00:00Z"),
        ("older", "2026-09-01T00:00:00Z"),
        ("new", "2026-10-24T00:00:00Z"),
    ] {
        insert_run(&app, task, at(tick)).await;
    }
    let pool = app.ctx().get::<Db>().await.unwrap();
    let deleted = prune_time_series(&*pool, "schedule_runs", "tick", at("2026-10-10T00:00:00Z"))
        .await
        .unwrap();
    assert_eq!(deleted, 2);
    assert_eq!(tasks_recorded(&app).await, ["new"]);
    let again = prune_time_series(&*pool, "schedule_runs", "tick", at("2026-10-10T00:00:00Z"))
        .await
        .unwrap();
    assert_eq!(again, 0);
}

on_every_server!(
    "navree_schedule";
    pruning_counts_what_it_deletes,
    runs_older_than_a_week_are_pruned_hourly,
    the_runs_are_declared_a_hypertable,
);
