//! `lighthouse:prune` and retention.

use std::sync::Arc;

use chrono::{TimeDelta, Utc};
use navree_lighthouse::entity::{issue, occurrence, sample};
use navree_lighthouse::{IssueFilter, IssueStatus};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, EntityTrait, PaginatorTrait};
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
    let conn = store.db().conn();
    let old = Utc::now() - TimeDelta::days(30);

    // An old resolved issue with an old occurrence, and an old trace.
    let resolved_id = Uuid::now_v7();
    issue::ActiveModel {
        id: Set(resolved_id),
        fingerprint: Set("0".repeat(32)),
        kind: Set("exception".into()),
        level: Set("error".into()),
        status: Set(IssueStatus::Resolved.as_str().into()),
        title: Set("old".into()),
        culprit: Set("GET /old".into()),
        message: Set("old".into()),
        chain: Set("[\"old\"]".into()),
        location: Set(None),
        error_type: Set(None),
        count: Set(1),
        first_seen: Set(old),
        last_seen: Set(old),
        first_release: Set(None),
        last_release: Set(None),
        environment: Set(None),
        resolved_at: Set(Some(old)),
        resolved_in_release: Set(None),
        ignored_until: Set(None),
        ignore_remaining: Set(None),
        regressed_at: Set(None),
        comment: Set(None),
        created_at: Set(old),
        updated_at: Set(old),
    }
    .insert(conn)
    .await
    .unwrap();
    occurrence::ActiveModel {
        id: Set(Uuid::now_v7()),
        issue_id: Set(resolved_id),
        at: Set(old),
        level: Set("error".into()),
        message: Set("old".into()),
        chain: Set("[]".into()),
        backtrace: Set(None),
        location: Set(None),
        request_id: Set(None),
        job_id: Set(None),
        trace_id: Set(None),
        user_id: Set(None),
        workspace_id: Set(None),
        release: Set(None),
        environment: Set(None),
        server: Set(None),
        process: Set(None),
        method: Set(None),
        route: Set(None),
        path: Set(None),
        status: Set(None),
        sample_id: Set(None),
        context: Set("{}".into()),
    }
    .insert(conn)
    .await
    .unwrap();
    sample::ActiveModel {
        id: Set(Uuid::now_v7()),
        kind: Set("request".into()),
        name: Set("GET /old".into()),
        at: Set(old),
        duration_ms: Set(12.0),
        status: Set(Some(200)),
        outcome: Set("ok".into()),
        slow: Set(false),
        request_id: Set(None),
        job_id: Set(None),
        trace_id: Set(None),
        user_id: Set(None),
        workspace_id: Set(None),
        release: Set(None),
        environment: Set(None),
        server: Set(None),
        process: Set(None),
        spans: Set("[]".into()),
        context: Set("{}".into()),
        error: Set(None),
    }
    .insert(conn)
    .await
    .unwrap();

    let summary = store.prune(14).await.unwrap();
    assert_eq!(summary.samples, 1);
    assert_eq!(summary.occurrences, 1);
    assert_eq!(summary.issues, 1, "the resolved issue has nothing left");
    assert!(
        issue::Entity::find_by_id(resolved_id)
            .one(conn)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        issue::Entity::find_by_id(open.id)
            .one(conn)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(occurrence::Entity::find().count(conn).await.unwrap(), 1);

    // The command, with --days: the recent data stays.
    app.run_command("lighthouse:prune", &["--days", "7"])
        .await
        .unwrap();
    assert!(
        issue::Entity::find_by_id(open.id)
            .one(conn)
            .await
            .unwrap()
            .is_some()
    );
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
    sample::ActiveModel {
        id: Set(id),
        kind: Set("request".into()),
        name: Set("GET /old".into()),
        at: Set(Utc::now() - TimeDelta::days(40)),
        duration_ms: Set(1.0),
        status: Set(Some(200)),
        outcome: Set("ok".into()),
        slow: Set(false),
        request_id: Set(None),
        job_id: Set(None),
        trace_id: Set(None),
        user_id: Set(None),
        workspace_id: Set(None),
        release: Set(None),
        environment: Set(None),
        server: Set(None),
        process: Set(None),
        spans: Set("[]".into()),
        context: Set("{}".into()),
        error: Set(None),
    }
    .insert(store.db().conn())
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
    assert!(
        sample::Entity::find_by_id(id)
            .one(store.db().conn())
            .await
            .unwrap()
            .is_some()
    );
    clock.set(at(3, 10));
    scheduler.tick(at(3, 10)).await.unwrap();
    assert!(
        sample::Entity::find_by_id(id)
            .one(store.db().conn())
            .await
            .unwrap()
            .is_none(),
        "pruned at 03:10"
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn options_in_code_schedule_the_command_itself() {
    use navree_schedule::{ScheduleModule, Scheduler};

    let app = navree_testing::TestApp::builder()
        .temp_database()
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
