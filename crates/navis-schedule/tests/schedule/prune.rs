//! `schedule_runs` keeps a week of runs: older rows are deleted hourly.

use chrono::{DateTime, Utc};
use navis_db::Db;
use navis_kernel::App;
use sea_orm::ConnectionTrait;
use sea_orm::sea_query::Query;

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
