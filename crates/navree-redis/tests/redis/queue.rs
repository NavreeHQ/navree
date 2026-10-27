//! The queue driver on a real Redis server (`NAVREE_TEST_REDIS_URL`): the
//! `QueueStore` contract directly, then jobs dispatched and worked through
//! `QueueModule` in two apps (two processes) on one server.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use navree_contracts::{
    BoxError, Clock, FixedClock, Job, JobContext, JobOptions, ModuleBuilderJobsExt,
};
use navree_kernel::{Module, ModuleBuilder, Navree};
use navree_queue::{
    NewJob, Queue, QueueDriver, QueueModule, QueueOptions, QueueStore, RetryTarget,
    STALE_RESERVATION_GRACE_SECS, WorkOptions,
};
use navree_redis::{RedisModule, RedisQueueStore};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::support::TestRedis;

const NOW: i64 = 1_761_552_000_000;

fn job(id: &str, queue: &str, available_at: i64) -> NewJob {
    NewJob {
        id: id.to_owned(),
        queue: queue.to_owned(),
        job_type: "test.record".to_owned(),
        payload: json!({ "label": id, "big": 9_007_199_254_740_993_u64, "ratio": 0.1 }),
        options: JobOptions {
            queue: queue.to_owned(),
            timeout_secs: 10,
            ..JobOptions::default()
        },
        available_at,
        created_at: NOW,
        unique_key: None,
        unique_for_ms: None,
        trace_parent: Some("00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01".to_owned()),
    }
}

fn store(t: &TestRedis) -> RedisQueueStore {
    RedisQueueStore::new(t.redis.connection().clone(), &t.prefix)
}

#[tokio::test]
async fn jobs_are_reserved_in_order_once_due() {
    let Some(t) = TestRedis::connect("jobs_are_reserved_in_order_once_due").await else {
        return;
    };
    let store = store(&t);
    store
        .insert(job("0001", "default", NOW + 5_000))
        .await
        .unwrap();
    store.insert(job("0002", "default", NOW)).await.unwrap();
    store.insert(job("0003", "default", NOW)).await.unwrap();
    store.insert(job("0004", "other", NOW)).await.unwrap();
    assert_eq!(store.pending_count("default").await.unwrap(), 3);

    let first = store.reserve("default", "w1", NOW).await.unwrap().unwrap();
    assert_eq!((first.id.as_str(), first.attempts), ("0002", 1));
    assert_eq!(
        first.payload,
        json!({ "label": "0002", "big": 9_007_199_254_740_993_u64, "ratio": 0.1 }),
        "the payload comes back unchanged"
    );
    assert_eq!(first.trace_parent, job("x", "q", 0).trace_parent);
    let second = store.reserve("default", "w2", NOW).await.unwrap().unwrap();
    assert_eq!(second.id, "0003");
    assert!(
        store.reserve("default", "w3", NOW).await.unwrap().is_none(),
        "0001 is not due yet"
    );
    let delayed = store
        .reserve("default", "w3", NOW + 5_000)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(delayed.id, "0001");
    assert_eq!(
        store.pending_count("default").await.unwrap(),
        3,
        "reserved still count"
    );

    store.complete(&first).await.unwrap();
    assert_eq!(store.pending_count("default").await.unwrap(), 2);
    t.cleanup().await;
}

#[tokio::test]
async fn release_and_fail_need_the_reservation() {
    let Some(t) = TestRedis::connect("release_and_fail_need_the_reservation").await else {
        return;
    };
    let store = store(&t);
    store.insert(job("0001", "default", NOW)).await.unwrap();
    let reserved = store.reserve("default", "w1", NOW).await.unwrap().unwrap();
    assert!(
        !store.release(&reserved, "w2", NOW).await.unwrap(),
        "w2 does not hold it"
    );
    assert!(store.release(&reserved, "w1", NOW + 1_000).await.unwrap());
    assert!(
        store.reserve("default", "w1", NOW).await.unwrap().is_none(),
        "backoff"
    );
    let again = store
        .reserve("default", "w2", NOW + 1_000)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(again.attempts, 2);

    assert!(!store.fail(&again, "w1", "boom", NOW + 2_000).await.unwrap());
    assert!(store.fail(&again, "w2", "boom", NOW + 2_000).await.unwrap());
    assert_eq!(store.pending_count("default").await.unwrap(), 0);
    let failed = store.failed().await.unwrap();
    assert_eq!(failed.len(), 1);
    assert_eq!(
        (
            failed[0].id.as_str(),
            failed[0].error.as_str(),
            failed[0].attempts
        ),
        ("0001", "boom", 2)
    );
    assert_eq!(failed[0].failed_at.timestamp_millis(), NOW + 2_000);
    t.cleanup().await;
}

#[tokio::test]
async fn stale_reservations_are_taken_over() {
    let Some(t) = TestRedis::connect("stale_reservations_are_taken_over").await else {
        return;
    };
    let store = store(&t);
    store.insert(job("0001", "default", NOW)).await.unwrap();
    let crashed = store.reserve("default", "w1", NOW).await.unwrap().unwrap();
    let stale_at = NOW + (10 + i64::try_from(STALE_RESERVATION_GRACE_SECS).unwrap()) * 1000;
    assert!(
        store
            .reserve("default", "w2", stale_at - 1)
            .await
            .unwrap()
            .is_none()
    );
    let taken = store
        .reserve("default", "w2", stale_at)
        .await
        .unwrap()
        .unwrap();
    assert_eq!((taken.id.as_str(), taken.attempts), ("0001", 2));
    assert!(
        !store.release(&crashed, "w1", stale_at).await.unwrap(),
        "w1 lost it"
    );
    store.complete(&taken).await.unwrap();
    assert!(t.keys().await.is_empty(), "{:?}", t.keys().await);
}

#[tokio::test]
async fn unique_jobs_are_deduplicated_while_pending() {
    let Some(t) = TestRedis::connect("unique_jobs_are_deduplicated_while_pending").await else {
        return;
    };
    let store = store(&t);
    let unique = |id: &str, created_at: i64| NewJob {
        unique_key: Some("report:1".to_owned()),
        unique_for_ms: Some(60_000),
        created_at,
        ..job(id, "default", created_at)
    };
    assert_eq!(store.insert(unique("0001", NOW)).await.unwrap(), "0001");
    assert_eq!(
        store.insert(unique("0002", NOW + 1_000)).await.unwrap(),
        "0001"
    );
    assert_eq!(store.pending_count("default").await.unwrap(), 1);
    // Outside the window the old job loses its key; both are queued.
    assert_eq!(
        store.insert(unique("0003", NOW + 60_000)).await.unwrap(),
        "0003"
    );
    assert_eq!(store.pending_count("default").await.unwrap(), 2);
    assert_eq!(
        store.insert(unique("0004", NOW + 60_001)).await.unwrap(),
        "0003"
    );

    // Once it finishes, the key is free again.
    let first = store
        .reserve("default", "w", NOW + 60_001)
        .await
        .unwrap()
        .unwrap();
    store.complete(&first).await.unwrap();
    let second = store
        .reserve("default", "w", NOW + 60_001)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(second.id, "0003");
    store.complete(&second).await.unwrap();
    assert_eq!(
        store.insert(unique("0005", NOW + 60_002)).await.unwrap(),
        "0005"
    );
    t.cleanup().await;
}

#[tokio::test]
async fn failed_jobs_are_retried_and_flushed() {
    let Some(t) = TestRedis::connect("failed_jobs_are_retried_and_flushed").await else {
        return;
    };
    let store = store(&t);
    for (id, at) in [("0001", NOW), ("0002", NOW + 1)] {
        store.insert(job(id, "default", NOW)).await.unwrap();
        let reserved = store.reserve("default", "w", NOW).await.unwrap().unwrap();
        assert_eq!(reserved.id, id);
        store.fail(&reserved, "w", "boom", at).await.unwrap();
    }
    let ids: Vec<String> = store
        .failed()
        .await
        .unwrap()
        .into_iter()
        .map(|f| f.id)
        .collect();
    assert_eq!(ids, ["0002", "0001"], "newest first");

    assert_eq!(
        store
            .retry(&RetryTarget::Id("0001".to_owned()), NOW + 10)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        store
            .retry(&RetryTarget::Id("nope".to_owned()), NOW + 10)
            .await
            .unwrap(),
        0
    );
    let retried = store
        .reserve("default", "w", NOW + 10)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (retried.id.as_str(), retried.attempts),
        ("0001", 1),
        "attempts start over"
    );

    assert_eq!(store.flush_failed().await.unwrap(), 1);
    assert!(store.failed().await.unwrap().is_empty());
    assert_eq!(store.retry(&RetryTarget::All, NOW).await.unwrap(), 0);
    t.cleanup().await;
}

#[tokio::test]
async fn concurrent_workers_never_reserve_the_same_job() {
    let Some(t) = TestRedis::connect("concurrent_workers_never_reserve_the_same_job").await else {
        return;
    };
    let store = Arc::new(store(&t));
    for i in 0..50 {
        store
            .insert(job(&format!("{i:04}"), "default", NOW))
            .await
            .unwrap();
    }
    let workers: Vec<_> = (0..8)
        .map(|w| {
            let store = store.clone();
            tokio::spawn(async move {
                let mut taken = Vec::new();
                while let Some(job) = store
                    .reserve("default", &format!("w{w}"), NOW)
                    .await
                    .unwrap()
                {
                    taken.push(job.id);
                }
                taken
            })
        })
        .collect();
    let mut all = Vec::new();
    for worker in workers {
        all.extend(worker.await.unwrap());
    }
    let unique: HashSet<&String> = all.iter().collect();
    assert_eq!((all.len(), unique.len()), (50, 50));
    t.cleanup().await;
}

/// Records each run in a shared list.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Record {
    label: String,
}

#[derive(Default)]
struct Runs(Mutex<Vec<String>>);

impl Job for Record {
    const NAME: &'static str = "test.record";

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        ctx.app
            .get::<Runs>()
            .await?
            .0
            .lock()
            .unwrap()
            .push(self.label);
        Ok(())
    }
}

struct JobsModule;

impl Module for JobsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.job::<Record>();
    }
}

#[tokio::test]
async fn jobs_dispatched_in_one_process_run_in_another() {
    let Some(t) = TestRedis::connect("jobs_dispatched_in_one_process_run_in_another").await else {
        return;
    };
    let clock = Arc::new(FixedClock::new("2026-10-27T09:00:00Z".parse().unwrap()));
    let boot = || {
        Navree::app()
            .module(RedisModule::for_root(t.options()))
            .module(QueueModule::for_root(QueueOptions {
                driver: QueueDriver::Redis,
                poll_interval_ms: 20,
                ..QueueOptions::default()
            }))
            .module(JobsModule)
            .override_value(clock.clone() as Arc<dyn Clock>)
            .override_value(Arc::new(Runs::default()))
            .boot()
    };
    let (web, worker) = (boot().await.unwrap(), boot().await.unwrap());
    let dispatcher = web.ctx().get::<Queue>().await.unwrap();
    dispatcher
        .dispatch(Record {
            label: "now".into(),
        })
        .await
        .unwrap();
    dispatcher
        .later(
            Record {
                label: "later".into(),
            },
            Duration::from_secs(30),
        )
        .await
        .unwrap();

    let queue = worker.ctx().get::<Queue>().await.unwrap();
    let stats = queue
        .work(WorkOptions {
            stop_when_empty: true,
            ..WorkOptions::default()
        })
        .await
        .unwrap();
    assert_eq!(stats.processed, 1);
    let runs = worker.ctx().get::<Runs>().await.unwrap();
    assert_eq!(*runs.0.lock().unwrap(), ["now"]);

    clock.advance(Duration::from_secs(30));
    queue.run_pending().await.unwrap();
    assert_eq!(*runs.0.lock().unwrap(), ["now", "later"]);
    assert_eq!(dispatcher.pending_count("default").await.unwrap(), 0);
    t.cleanup().await;
}
