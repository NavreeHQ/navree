//! `schedule_runs` keeps a week of runs: older rows are deleted hourly.

use chrono::{DateTime, Utc};
use navree_db::navree_schema::{IndexKind, Schema};
use navree_db::{Db, MIGRATIONS_TABLE};
use navree_kernel::App;
use sea_orm::sea_query::Query;
use sea_orm::{ConnectionTrait, Statement};

use crate::support::{Counter, Tasks, TestDb, at, clock, runs, scheduler};

async fn insert_run(app: &App, task: &str, tick: DateTime<Utc>) {
    let db = app.ctx().get::<Db>().await.unwrap();
    db.conn()
        .execute(
            Query::insert()
                .into_table("schedule_runs")
                .columns(["task", "tick", "started_at", "finished_at", "status"])
                .values_panic([
                    task.into(),
                    tick.into(),
                    tick.into(),
                    tick.into(),
                    "succeeded".into(),
                ]),
        )
        .await
        .unwrap();
}

async fn tasks_recorded(app: &App) -> Vec<String> {
    let mut tasks: Vec<String> = runs(app).await.into_iter().map(|r| r.task).collect();
    tasks.dedup();
    tasks
}

#[tokio::test]
async fn runs_older_than_a_week_are_pruned_hourly() {
    let db = TestDb::new();
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
#[tokio::test]
async fn the_runs_are_declared_a_hypertable() {
    let db = TestDb::new();
    let app = db
        .boot(&clock("2026-10-25T10:30:00Z"), Tasks::new(|_s| {}))
        .await;
    let pool = app.ctx().get::<Db>().await.unwrap();
    let applied: Vec<String> = pool
        .conn()
        .query_all_raw(Statement::from_string(
            pool.backend(),
            format!("SELECT name FROM {MIGRATIONS_TABLE} WHERE module = 'schedule' ORDER BY name"),
        ))
        .await
        .unwrap()
        .iter()
        .map(|row| row.try_get::<String>("", "name").unwrap())
        .collect();
    assert_eq!(
        applied,
        [
            "m20261025_000001_create_schedule_runs",
            "m20261027_000001_convert_schedule_runs_to_hypertable"
        ]
    );
    let schema = Schema::new(pool.conn());
    assert!(!schema.is_hypertable("schedule_runs").await.unwrap());
    assert!(
        schema
            .has_index("schedule_runs", ["tick"], Some(IndexKind::Index))
            .await
            .unwrap()
    );
    assert!(
        schema
            .has_index("schedule_runs", ["task", "tick"], Some(IndexKind::Primary))
            .await
            .unwrap()
    );
}
