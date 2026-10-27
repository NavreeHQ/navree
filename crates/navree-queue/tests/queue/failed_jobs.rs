//! The database driver's failed jobs are navree-orm models of
//! `failed_jobs`: typed payloads, options and failure times, queried like
//! any model, and written by the driver through the ORM.

use chrono::TimeDelta;
use navree_contracts::Job;
use navree_db::Db;
use navree_orm::Model as _;
use navree_queue::{FAILED_JOBS_TABLE, FailedJob, QueueDriver, RetryTarget};
use serde_json::{Value, json};

use crate::support::{Panics, Sleepy, harness, start};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn failed_jobs_are_orm_models() {
    let h = harness(QueueDriver::Database).await;
    let panicked = h.queue.dispatch(Panics).await.unwrap();
    h.drain().await;
    h.advance(5);
    let slow = h
        .queue
        .dispatch(Sleepy {
            label: "slow".into(),
        })
        .await
        .unwrap();
    h.drain().await;
    let db = h.app.get::<Db>().await;

    assert_eq!(FailedJob::table_name(), FAILED_JOBS_TABLE);
    let failed = FailedJob::query()
        .where_(FailedJob::JOB_TYPE.eq(Panics::NAME))
        .get(db.conn())
        .await
        .unwrap()
        .into_models();
    assert_eq!(failed.len(), 1);
    let job = &failed[0];
    assert_eq!(job.id, panicked);
    assert_eq!(job.queue, "default");
    assert_eq!(job.payload, Value::Null);
    assert_eq!(job.options, Panics::options());
    assert_eq!(job.attempts, 1);
    assert_eq!(job.failed_at, start());
    assert!(job.error.contains("panicked on purpose"), "{}", job.error);

    let newest = FailedJob::query()
        .latest_by(FailedJob::FAILED_AT)
        .first(db.conn())
        .await
        .unwrap()
        .expect("a failed job");
    assert_eq!(newest.id, slow);
    assert_eq!(newest.payload, json!({ "label": "slow" }));
    assert_eq!(newest.failed_at, start() + TimeDelta::seconds(5));
    assert_eq!(
        FailedJob::query()
            .where_(FailedJob::FAILED_AT.lt(start() + TimeDelta::seconds(1)))
            .count(db.conn())
            .await
            .unwrap(),
        1,
        "the failure time compares as a date-time"
    );
    assert_eq!(
        h.queue.failed().await.unwrap(),
        [newest.into_inner(), job.clone()]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn failed_jobs_stored_by_hand_are_listed_and_retried_by_the_queue() {
    let h = harness(QueueDriver::Database).await;
    let db = h.app.get::<Db>().await;
    let stored = FailedJob {
        id: "imported-1".to_owned(),
        queue: "default".to_owned(),
        job_type: Panics::NAME.to_owned(),
        payload: Value::Null,
        options: Panics::options(),
        error: "failed elsewhere".to_owned(),
        attempts: 2,
        failed_at: start() - TimeDelta::hours(1),
    };
    FailedJob::insert(db.conn(), [stored.clone()])
        .await
        .unwrap();

    assert_eq!(
        h.queue.failed_job("imported-1").await.unwrap(),
        Some(stored.clone())
    );
    assert_eq!(h.queue.failed().await.unwrap(), [stored]);
    assert_eq!(
        h.queue
            .retry(RetryTarget::Id("imported-1".to_owned()))
            .await
            .unwrap(),
        1
    );
    assert!(h.queue.failed().await.unwrap().is_empty());
    assert_eq!(h.queue.pending_count("default").await.unwrap(), 1);
    assert_eq!(FailedJob::query().count(db.conn()).await.unwrap(), 0);
}
