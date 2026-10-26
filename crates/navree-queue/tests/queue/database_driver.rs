//! Database-driver behaviour beyond the shared suite: several processes
//! (apps with their own pools) on one database, and the stored rows.

use std::sync::Arc;
use std::time::Duration;

use navree_contracts::FixedClock;
use navree_db::Db;
use navree_queue::{Queue, QueueDriver, QueueError, QueueOptions, WorkOptions};
use navree_testing::temp_sqlite_url;
use sea_orm::ConnectionTrait;

use crate::suite::{assert_handled_once, wait_until};
use crate::support::{
    HideJobsTable, Journal, Record, app_on, app_without_jobs_on, harness, harness_with, options,
    start,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_workers_in_two_apps_never_run_same_job() {
    let (url, _guard) = temp_sqlite_url();
    let journal = Arc::new(Journal::default());
    let clock = Arc::new(FixedClock::new(start()));
    let first = app_on(
        &url,
        options(QueueDriver::Database),
        journal.clone(),
        clock.clone(),
    )
    .await;
    let second = app_on(&url, options(QueueDriver::Database), journal.clone(), clock).await;
    let (a, b) = (first.get::<Queue>().await, second.get::<Queue>().await);
    for i in 0..50 {
        a.dispatch(Record::new(i.to_string())).await.unwrap();
    }
    let drain = WorkOptions {
        stop_when_empty: true,
        ..WorkOptions::default()
    };

    let (from_a, from_b) = tokio::join!(a.work(drain.clone()), b.work(drain));

    let (from_a, from_b) = (from_a.unwrap(), from_b.unwrap());
    assert_eq!(from_a.succeeded + from_b.succeeded, 50);
    assert_handled_once(&journal.labels(), 50);
    assert_eq!(b.pending_count("default").await.unwrap(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn job_without_handler_fails_without_retries() {
    let (url, _guard) = temp_sqlite_url();
    let journal = Arc::new(Journal::default());
    let clock = Arc::new(FixedClock::new(start()));
    let dispatcher = app_on(
        &url,
        options(QueueDriver::Database),
        journal.clone(),
        clock.clone(),
    )
    .await;
    let worker = app_without_jobs_on(&url, options(QueueDriver::Database), journal, clock).await;
    let id = dispatcher
        .get::<Queue>()
        .await
        .dispatch(Record::new("orphan"))
        .await
        .unwrap();

    let stats = worker
        .get::<Queue>()
        .await
        .work(WorkOptions {
            stop_when_empty: true,
            ..WorkOptions::default()
        })
        .await
        .unwrap();

    assert_eq!(stats.failed, 1);
    let failed = worker.get::<Queue>().await.failed().await.unwrap();
    assert_eq!(failed[0].id, id);
    assert_eq!(failed[0].attempts, 1);
    assert!(
        failed[0]
            .error
            .contains("no handler is registered for job type `test.record`"),
        "{}",
        failed[0].error
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn long_running_worker_survives_database_errors() {
    let h = harness(QueueDriver::Database).await;
    let db = h.app.get::<Db>().await;
    let queue = h.queue.clone();
    let worker = tokio::spawn(async move { queue.work(WorkOptions::default()).await });
    tokio::time::sleep(Duration::from_millis(50)).await;

    // The table disappears under the worker, then comes back.
    db.conn()
        .execute_unprepared("ALTER TABLE jobs RENAME TO jobs_away")
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!worker.is_finished(), "the worker keeps polling");
    db.conn()
        .execute_unprepared("ALTER TABLE jobs_away RENAME TO jobs")
        .await
        .unwrap();
    h.queue.dispatch(Record::new("after")).await.unwrap();
    wait_until(|| h.journal.labels() == ["after"]).await;

    h.app.ctx().shutdown_token().cancel();
    let stats = tokio::time::timeout(Duration::from_secs(5), worker)
        .await
        .expect("stops at shutdown")
        .unwrap()
        .unwrap();
    assert_eq!(stats.succeeded, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn finite_worker_returns_database_errors() {
    let h = harness(QueueDriver::Database).await;
    let db = h.app.get::<Db>().await;
    db.conn()
        .execute_unprepared("DROP TABLE jobs")
        .await
        .unwrap();

    let error = h
        .queue
        .work(WorkOptions {
            stop_when_empty: true,
            ..WorkOptions::default()
        })
        .await
        .unwrap_err();

    assert!(matches!(error, QueueError::Db(_)), "{error:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn long_running_worker_survives_errors_recording_an_outcome() {
    let h = harness_with(QueueOptions {
        workers: 1,
        ..options(QueueDriver::Database)
    })
    .await;
    let db = h.app.get::<Db>().await;
    let queue = h.queue.clone();
    let worker = tokio::spawn(async move { queue.work(WorkOptions::default()).await });

    h.queue.dispatch(HideJobsTable).await.unwrap();
    wait_until(|| h.journal.runs().len() == 1).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        !worker.is_finished(),
        "the worker outlives the failed completion"
    );
    db.conn()
        .execute_unprepared("ALTER TABLE jobs_away RENAME TO jobs")
        .await
        .unwrap();
    h.queue.dispatch(Record::new("after")).await.unwrap();
    wait_until(|| h.journal.labels().contains(&"after".to_owned())).await;

    h.app.ctx().shutdown_token().cancel();
    tokio::time::timeout(Duration::from_secs(5), worker)
        .await
        .expect("stops at shutdown")
        .unwrap()
        .unwrap();
}
