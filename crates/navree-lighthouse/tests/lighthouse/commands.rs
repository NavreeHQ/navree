//! `lighthouse:prune` and retention.

use std::sync::Arc;

use chrono::{TimeDelta, Utc};
use navree_lighthouse::entity::{issue, occurrence, sample};
use navree_lighthouse::{IssueFilter, IssueStatus};
use navree_orm::Model as _;
use uuid::Uuid;

use crate::support::{builder, flushed};

#[tokio::test]
async fn prune_deletes_what_is_older_than_the_retention_and_keeps_open_issues() {
    let app = builder().build().await;
    app.client()
        .get("/api/demo/boom/1")
        .send()
        .await
        .assert_status(500);
    let store = flushed(&app).await;
    let open = store.issues(&IssueFilter::default()).await.unwrap().data[0]
        .issue
        .clone();
    let conn = store.db();
    let old = Utc::now() - TimeDelta::days(30);

    // An old resolved issue with an old occurrence, and an old trace.
    let resolved_id = Uuid::now_v7();
    issue::Model::query()
        .insert(
            conn,
            [issue::Model {
                id: resolved_id,
                fingerprint: "0".repeat(32),
                kind: "exception".into(),
                level: "error".into(),
                status: IssueStatus::Resolved.as_str().into(),
                title: "old".into(),
                culprit: "GET /old".into(),
                message: "old".into(),
                chain: "[\"old\"]".into(),
                location: None,
                error_type: None,
                count: 1,
                first_seen: old,
                last_seen: old,
                first_release: None,
                last_release: None,
                environment: None,
                resolved_at: Some(old),
                resolved_in_release: None,
                ignored_until: None,
                ignore_remaining: None,
                regressed_at: None,
                comment: None,
                created_at: old,
                updated_at: old,
            }],
        )
        .await
        .unwrap();
    occurrence::Model::query()
        .insert(
            conn,
            [occurrence::Model {
                id: Uuid::now_v7(),
                issue_id: resolved_id,
                at: old,
                level: "error".into(),
                message: "old".into(),
                chain: "[]".into(),
                backtrace: None,
                location: None,
                request_id: None,
                job_id: None,
                trace_id: None,
                user_id: None,
                workspace_id: None,
                release: None,
                environment: None,
                server: None,
                process: None,
                method: None,
                route: None,
                path: None,
                status: None,
                sample_id: None,
                context: "{}".into(),
            }],
        )
        .await
        .unwrap();
    sample::Model::query()
        .insert(
            conn,
            [sample::Model {
                id: Uuid::now_v7(),
                kind: "request".into(),
                name: "GET /old".into(),
                at: old,
                duration_ms: 12.0,
                status: Some(200),
                outcome: "ok".into(),
                slow: false,
                request_id: None,
                job_id: None,
                trace_id: None,
                user_id: None,
                workspace_id: None,
                release: None,
                environment: None,
                server: None,
                process: None,
                spans: "[]".into(),
                context: "{}".into(),
                error: None,
            }],
        )
        .await
        .unwrap();

    let summary = store.prune(14).await.unwrap();
    assert_eq!(summary.samples, 1);
    assert_eq!(summary.occurrences, 1);
    assert_eq!(summary.issues, 1, "the resolved issue has nothing left");
    assert!(
        issue::Model::find(conn, resolved_id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(issue::Model::find(conn, open.id).await.unwrap().is_some());
    assert_eq!(occurrence::Model::query().count(conn).await.unwrap(), 1);

    // The command, with --days: the recent data stays.
    app.run_command("lighthouse:prune", &["--days", "7"])
        .await
        .unwrap();
    assert!(issue::Model::find(conn, open.id).await.unwrap().is_some());
    assert!(
        app.run_command("lighthouse:prune", &["--days", "zero"])
            .await
            .is_err()
    );
    app.shutdown().await.unwrap();
}

/// An old sample, for the pruner to find.
async fn old_sample(store: &navree_lighthouse::LighthouseStore) -> Uuid {
    let id = Uuid::now_v7();
    sample::Model::query()
        .insert(
            store.db(),
            [sample::Model {
                id,
                kind: "request".into(),
                name: "GET /old".into(),
                at: Utc::now() - TimeDelta::days(40),
                duration_ms: 1.0,
                status: Some(200),
                outcome: "ok".into(),
                slow: false,
                request_id: None,
                job_id: None,
                trace_id: None,
                user_id: None,
                workspace_id: None,
                release: None,
                environment: None,
                server: None,
                process: None,
                spans: "[]".into(),
                context: "{}".into(),
                error: None,
            }],
        )
        .await
        .unwrap();
    id
}

#[tokio::test]
async fn the_scheduler_prunes_daily_at_prune_at() {
    use navree_contracts::{Clock, FixedClock};
    use navree_schedule::{ScheduleModule, Scheduler};

    let at = |h, m| chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 10, 27, h, m, 0).unwrap();
    let clock = Arc::new(FixedClock::new(at(3, 9)));
    let app = builder()
        .config("lighthouse.prune_at", "03:10")
        .module(ScheduleModule::for_root())
        .override_value::<dyn Clock>(clock.clone())
        .build()
        .await;
    let store = flushed(&app).await;
    let scheduler = app.get::<Scheduler>().await;
    let task = scheduler
        .list(at(3, 9))
        .into_iter()
        .find(|task| task.name == "lighthouse:prune")
        .expect("Lighthouse schedules its pruning");
    assert!(task.without_overlapping);

    let id = old_sample(&store).await;
    scheduler.tick(at(3, 9)).await.unwrap();
    assert!(sample::Model::find(store.db(), id).await.unwrap().is_some());
    clock.set(at(3, 10));
    scheduler.tick(at(3, 10)).await.unwrap();
    assert!(
        sample::Model::find(store.db(), id).await.unwrap().is_none(),
        "pruned at 03:10"
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn options_in_code_schedule_the_command_itself() {
    use navree_schedule::{ScheduleModule, Scheduler};

    let app = crate::servers::database(navree_testing::TestApp::builder())
        .module(navree_db::DatabaseModule::from_config())
        .module(ScheduleModule::for_root())
        .module(navree_lighthouse::LighthouseModule::for_root(
            navree_lighthouse::LighthouseOptions {
                prune_at: "04:30".into(),
                dashboard: false,
                ..Default::default()
            },
        ))
        .build()
        .await;
    let task = app
        .get::<Scheduler>()
        .await
        .list(Utc::now())
        .into_iter()
        .find(|task| task.name == "lighthouse:prune")
        .expect("the command is scheduled");
    assert_eq!(task.expr, "30 4 * * *");
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn test_alerts_without_channels_say_so() {
    let app = builder().build().await;
    app.run_command("lighthouse:test-alerts", &[])
        .await
        .unwrap();
    assert!(
        app.run_command("lighthouse:test-alerts", &["slack"])
            .await
            .is_err()
    );
    app.shutdown().await.unwrap();
}
