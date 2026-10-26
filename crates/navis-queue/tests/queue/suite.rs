//! The behaviour every driver must have. Each function runs once per driver
//! (see `main.rs`): `database::<name>` and `memory_driver_parity::<name>`.

use std::time::{Duration, Instant};

use chrono::TimeDelta;
use navis_contracts::{Dispatcher, Job};
use navis_db::Db;
use navis_queue::{
    QueueDriver, QueueError, QueueOptions, RetryTarget, STALE_RESERVATION_GRACE_SECS, WorkOptions,
    WorkStats,
};
use serde_json::json;

use crate::support::{
    AlwaysFails, Hang, HangLast, Harness, Panics, Record, Sleepy, Toggle, UniqueReport,
    Unregistered, Urgent, harness, harness_with, options, start,
};

pub async fn dispatch_and_work_runs_handler(driver: QueueDriver) {
    let h = harness(driver).await;

    let id = h
        .queue
        .dispatch(Record::new("a"))
        .await
        .expect("dispatched");
    assert!(!id.is_empty());
    assert_eq!(h.queue.pending_count("default").await.unwrap(), 1);

    let stats = h.drain().await;

    assert_eq!(stats.processed, 1);
    assert_eq!(stats.succeeded, 1);
    let runs = h.journal.runs();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].label, "a");
    assert_eq!(runs[0].attempt, 1);
    assert_eq!(h.queue.pending_count("default").await.unwrap(), 0);
    assert!(h.queue.failed().await.unwrap().is_empty());
}

pub async fn dispatch_in_uow_rolled_back_is_not_queued(driver: QueueDriver) {
    let h = harness(driver).await;
    let db = h.app.get::<Db>().await;

    let uow = db.uow(None, None).await;
    let rolled_back = h
        .queue
        .dispatch_in(&uow, Record::new("rolled back"))
        .await
        .expect("dispatched in the unit of work");
    assert!(!rolled_back.is_empty());
    uow.rollback().await.expect("rolls back");
    assert_eq!(h.queue.pending_count("default").await.unwrap(), 0);

    let uow = db.uow(None, None).await;
    let committed = h
        .queue
        .dispatch_in(&uow, Record::new("committed"))
        .await
        .expect("dispatched in the unit of work");
    // Not visible before the commit.
    assert_eq!(h.queue.pending_count("default").await.unwrap(), 0);
    uow.commit().await.expect("commits");
    assert_eq!(h.queue.pending_count("default").await.unwrap(), 1);

    h.drain().await;
    assert_eq!(h.journal.labels(), ["committed"]);
    assert_ne!(rolled_back, committed);
}

/// Dispatching inside a unit of work's scope (a request handler, or
/// `Db::transaction`) joins it: on SQLite the open transaction holds the
/// write lock, so an insert through another connection would wait
/// `busy_timeout` and fail.
pub async fn dispatch_joins_the_current_unit_of_work(driver: QueueDriver) {
    let h = harness(driver).await;
    let db = h.app.get::<Db>().await;
    let dispatcher = h.app.get::<dyn Dispatcher>().await;
    let quick = Duration::from_secs(3);

    let uow = db.uow(None, None).await;
    // BEGIN IMMEDIATE: the unit of work now holds SQLite's write lock.
    drop(uow.tx().await.expect("transaction"));
    uow.scope(async {
        let dispatched = async {
            h.queue.dispatch(Record::new("dispatch")).await?;
            dispatcher
                .dispatch(Record::new("dyn"))
                .await
                .map_err(|e| QueueError::UnknownJob {
                    job_type: e.to_string(),
                })?;
            h.queue
                .later(Record::new("later"), Duration::from_secs(5))
                .await?;
            Ok::<_, QueueError>(())
        };
        tokio::time::timeout(quick, dispatched)
            .await
            .expect("dispatching does not wait for the write lock")
            .expect("dispatched");
    })
    .await;
    assert_eq!(
        h.queue.pending_count("default").await.unwrap(),
        0,
        "not visible before the commit"
    );
    uow.commit().await.expect("commits");
    assert_eq!(h.queue.pending_count("default").await.unwrap(), 3);

    let rolled_back = db.uow(None, None).await;
    rolled_back
        .scope(h.queue.dispatch(Record::new("rolled back")))
        .await
        .expect("dispatched");
    rolled_back.rollback().await.expect("rolls back");
    assert_eq!(h.queue.pending_count("default").await.unwrap(), 3);

    // A finished current unit of work (a handler that committed early) is
    // not joined: the job is queued at once.
    let finished = db.uow(None, None).await;
    finished
        .scope(async {
            finished.commit().await.expect("commits");
            h.queue.dispatch(Record::new("after commit")).await
        })
        .await
        .expect("dispatched");
    assert_eq!(h.queue.pending_count("default").await.unwrap(), 4);

    h.advance(5);
    h.drain().await;
    let mut labels = h.journal.labels();
    labels.sort();
    assert_eq!(labels, ["after commit", "dispatch", "dyn", "later"]);
}

pub async fn retry_with_backoff_then_fail_to_failed_jobs(driver: QueueDriver) {
    let h = harness(driver).await;
    let id = h
        .queue
        .dispatch(AlwaysFails { label: "x".into() })
        .await
        .unwrap();

    // Attempt 1 fails: released with the first backoff (10 s).
    let stats = h.drain().await;
    assert_eq!(
        stats,
        WorkStats {
            processed: 1,
            released: 1,
            ..WorkStats::default()
        }
    );
    assert_eq!(h.queue.pending_count("default").await.unwrap(), 1);
    assert!(h.queue.failed().await.unwrap().is_empty());
    h.advance(9);
    assert_eq!(h.drain().await.processed, 0, "not before the backoff");
    h.advance(1);

    // Attempt 2 fails: the backoff doubles (20 s).
    assert_eq!(h.drain().await.released, 1);
    h.advance(19);
    assert_eq!(h.drain().await.processed, 0, "not before the backoff");
    h.advance(1);

    // Attempt 3 is the last one: the job moves to the failed jobs.
    let stats = h.drain().await;
    assert_eq!(stats.failed, 1);
    assert_eq!(stats.released, 0);
    let attempts: Vec<u32> = h.journal.runs().iter().map(|run| run.attempt).collect();
    assert_eq!(attempts, [1, 2, 3]);
    assert_eq!(h.queue.pending_count("default").await.unwrap(), 0);

    let failed = h.queue.failed().await.unwrap();
    assert_eq!(failed.len(), 1);
    let job = &failed[0];
    assert_eq!(job.id, id);
    assert_eq!(job.queue, "default");
    assert_eq!(job.job_type, AlwaysFails::NAME);
    assert_eq!(job.payload, json!({ "label": "x" }));
    assert_eq!(job.attempts, 3);
    assert!(job.error.contains("boom on attempt 3"), "{}", job.error);
    assert_eq!(job.failed_at, start() + TimeDelta::seconds(30));
}

pub async fn timeout_counts_as_failure(driver: QueueDriver) {
    let h = harness(driver).await;
    h.queue
        .dispatch(Sleepy {
            label: "slow".into(),
        })
        .await
        .unwrap();

    let started = Instant::now();
    let stats = tokio::time::timeout(Duration::from_secs(10), h.drain())
        .await
        .expect("the job's 1 s timeout stops it long before its 30 s sleep");

    assert!(started.elapsed() >= Duration::from_secs(1));
    assert_eq!(stats.failed, 1);
    assert_eq!(h.journal.labels(), ["slow"], "the attempt started");
    let failed = h.queue.failed().await.unwrap();
    assert_eq!(failed.len(), 1);
    assert!(failed[0].error.contains("timed out"), "{}", failed[0].error);
}

pub async fn panicking_job_counts_as_failure(driver: QueueDriver) {
    let h = harness(driver).await;
    h.queue.dispatch(Panics).await.unwrap();
    h.queue.dispatch(Record::new("after")).await.unwrap();

    let stats = h.drain().await;

    assert_eq!(stats.failed, 1);
    assert_eq!(stats.succeeded, 1, "the worker survives the panic");
    let failed = h.queue.failed().await.unwrap();
    assert!(
        failed[0].error.contains("the job panicked on purpose"),
        "{}",
        failed[0].error
    );
}

pub async fn unique_job_deduplicated(driver: QueueDriver) {
    let h = harness(driver).await;
    let acme = || UniqueReport {
        account: "acme".into(),
    };

    let first = h.queue.dispatch(acme()).await.unwrap();
    let again = h.queue.dispatch(acme()).await.unwrap();
    assert_eq!(again, first, "a duplicate gets the pending job's id");
    let dispatcher = h.app.get::<dyn Dispatcher>().await;
    assert_eq!(dispatcher.dispatch(acme()).await.unwrap(), first);
    let other = h
        .queue
        .dispatch(UniqueReport {
            account: "globex".into(),
        })
        .await
        .unwrap();
    assert_ne!(other, first, "another payload is another job");
    assert_eq!(h.queue.pending_count("default").await.unwrap(), 2);

    // Once the 60 s window has passed, an identical job is queued again.
    h.advance(61);
    let after_window = h.queue.dispatch(acme()).await.unwrap();
    assert_ne!(after_window, first);
    assert_eq!(h.queue.pending_count("default").await.unwrap(), 3);

    // A finished job no longer blocks its duplicates.
    assert_eq!(h.drain().await.succeeded, 3);
    let after_run = h.queue.dispatch(acme()).await.unwrap();
    assert_ne!(after_run, after_window);
    assert_eq!(h.queue.pending_count("default").await.unwrap(), 1);
}

pub async fn delayed_job_not_reserved_early(driver: QueueDriver) {
    let h = harness(driver).await;
    h.queue
        .later(Record::new("later"), Duration::from_secs(60))
        .await
        .unwrap();
    // `dyn Dispatcher` rounds 90.5 s up to 91 s.
    let dispatcher = h.app.get::<dyn Dispatcher>().await;
    dispatcher
        .dispatch_later(Record::new("dispatcher"), Duration::from_millis(90_500))
        .await
        .unwrap();
    assert_eq!(h.queue.pending_count("default").await.unwrap(), 2);

    assert_eq!(h.drain().await.processed, 0);
    h.advance(59);
    assert_eq!(h.drain().await.processed, 0, "not before its delay");
    h.advance(1);
    assert_eq!(h.drain().await.processed, 1);
    assert_eq!(h.journal.labels(), ["later"]);

    h.advance(30);
    assert_eq!(h.drain().await.processed, 0, "not before its delay");
    h.advance(1);
    assert_eq!(h.drain().await.processed, 1);
    assert_eq!(h.journal.labels(), ["later", "dispatcher"]);
}

pub async fn concurrent_workers_never_run_same_job(driver: QueueDriver) {
    let h = harness(driver).await;
    for i in 0..50 {
        h.queue.dispatch(Record::new(i.to_string())).await.unwrap();
    }

    // Two workers (each with two concurrent loops) drain the same queue.
    let (a, b) = tokio::join!(h.drain(), h.drain());

    assert_eq!(a.succeeded + b.succeeded, 50);
    assert_handled_once(&h.journal.labels(), 50);
    assert_eq!(h.queue.pending_count("default").await.unwrap(), 0);
}

/// Every label `0..n` appears exactly once.
pub fn assert_handled_once(labels: &[String], n: usize) {
    let mut sorted: Vec<usize> = labels.iter().map(|l| l.parse().unwrap()).collect();
    sorted.sort_unstable();
    assert_eq!(
        sorted,
        (0..n).collect::<Vec<_>>(),
        "each job ran exactly once"
    );
}

/// Starts a worker on the next job and kills it mid-run, leaving the job
/// reserved, as a crashed process would.
async fn crash_worker_mid_job(h: &Harness) {
    let queue = h.queue.clone();
    let worker = tokio::spawn(async move {
        queue
            .work(WorkOptions {
                once: true,
                ..WorkOptions::default()
            })
            .await
    });
    h.journal.hanging.notified().await;
    worker.abort();
    assert!(worker.await.unwrap_err().is_cancelled());
}

pub async fn stale_reservation_released(driver: QueueDriver) {
    let h = harness(driver).await;
    h.queue.dispatch(Hang { label: "h".into() }).await.unwrap();
    crash_worker_mid_job(&h).await;

    // A fresh reservation is never taken by another worker; one older than
    // the job's timeout (5 s) plus the grace period is.
    assert_eq!(h.drain().await.processed, 0);
    h.advance(5 + STALE_RESERVATION_GRACE_SECS - 1);
    assert_eq!(h.drain().await.processed, 0, "still within timeout + grace");
    h.advance(1);
    let stats = h.drain().await;

    assert_eq!(stats.succeeded, 1);
    let runs = h.journal.runs();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].attempt, 2, "the abandoned attempt counts");
    assert_eq!(h.queue.pending_count("default").await.unwrap(), 0);
}

pub async fn stale_job_on_its_last_attempt_fails(driver: QueueDriver) {
    let h = harness(driver).await;
    let id = h
        .queue
        .dispatch(HangLast { label: "h".into() })
        .await
        .unwrap();
    crash_worker_mid_job(&h).await;
    h.advance(5 + STALE_RESERVATION_GRACE_SECS);

    let stats = h.drain().await;

    assert_eq!(stats.failed, 1);
    let failed = h.queue.failed().await.unwrap();
    assert_eq!(failed[0].id, id);
    assert_eq!(failed[0].attempts, 2);
    assert!(
        failed[0].error.contains("attempted too many times"),
        "{}",
        failed[0].error
    );
    assert!(h.journal.runs().is_empty(), "the handler did not run again");
}

/// Dispatches failing [`Toggle`] jobs `labels`, runs them into the failed
/// jobs and stops the failing; returns their ids.
async fn failed_toggles(h: &Harness, labels: &[&str]) -> Vec<String> {
    h.journal.set_failing(true);
    let mut ids = Vec::new();
    for label in labels {
        ids.push(
            h.queue
                .dispatch(Toggle {
                    label: (*label).to_owned(),
                })
                .await
                .unwrap(),
        );
    }
    assert_eq!(h.drain().await.failed, labels.len() as u64);
    h.journal.set_failing(false);
    ids
}

pub async fn retry_failed_requeues(driver: QueueDriver) {
    let h = harness(driver).await;
    let ids = failed_toggles(&h, &["one", "two", "three"]).await;
    assert_eq!(h.queue.failed().await.unwrap().len(), 3);

    // One by id: back on its queue, same id, attempts start over.
    assert_eq!(h.queue.retry(ids[0].as_str()).await.unwrap(), 1);
    assert_eq!(h.queue.failed().await.unwrap().len(), 2);
    assert_eq!(h.queue.pending_count("default").await.unwrap(), 1);
    assert_eq!(h.drain().await.succeeded, 1);
    let runs = h.journal.runs();
    assert_eq!((runs[0].label.as_str(), runs[0].attempt), ("one", 1));

    let missing = h.queue.retry("no-such-id").await.unwrap_err();
    assert!(
        matches!(missing, QueueError::FailedJobNotFound { ref id } if id == "no-such-id"),
        "{missing:?}"
    );

    // All of them.
    assert_eq!(h.queue.retry(RetryTarget::All).await.unwrap(), 2);
    assert!(h.queue.failed().await.unwrap().is_empty());
    assert_eq!(h.drain().await.succeeded, 2);
    let mut labels = h.journal.labels();
    labels.sort();
    assert_eq!(labels, ["one", "three", "two"]);
}

pub async fn flush_failed_deletes_failed_jobs(driver: QueueDriver) {
    let h = harness(driver).await;
    failed_toggles(&h, &["one", "two"]).await;

    assert_eq!(h.queue.flush_failed().await.unwrap(), 2);

    assert!(h.queue.failed().await.unwrap().is_empty());
    assert_eq!(h.queue.retry("all").await.unwrap(), 0);
}

pub async fn unknown_job_type_is_rejected_at_dispatch(driver: QueueDriver) {
    let h = harness(driver).await;

    let error = h.queue.dispatch(Unregistered).await.unwrap_err();
    assert!(
        matches!(error, QueueError::UnknownJob { ref job_type } if job_type == Unregistered::NAME),
        "{error:?}"
    );
    assert!(error.to_string().contains("m.job::<J>()"), "{error}");
    let dispatcher = h.app.get::<dyn Dispatcher>().await;
    assert!(dispatcher.dispatch(Unregistered).await.is_err());
    assert_eq!(h.queue.pending_count("default").await.unwrap(), 0);
}

pub async fn queues_are_worked_in_priority_order(driver: QueueDriver) {
    let h = harness_with(QueueOptions {
        workers: 1,
        queues: vec!["high".into(), "default".into()],
        ..options(driver)
    })
    .await;
    h.queue.dispatch(Record::new("a")).await.unwrap();
    h.queue
        .dispatch(Urgent { label: "u".into() })
        .await
        .unwrap();
    h.queue.dispatch(Record::new("b")).await.unwrap();
    assert_eq!(h.queue.pending_count("high").await.unwrap(), 1);

    h.drain().await;

    assert_eq!(h.journal.labels(), ["u", "a", "b"]);
}

pub async fn work_options_select_queues_and_limit_jobs(driver: QueueDriver) {
    let h = harness(driver).await;
    for label in ["a", "b", "c"] {
        h.queue.dispatch(Record::new(label)).await.unwrap();
    }
    let urgent = |label: &str| Urgent {
        label: label.to_owned(),
    };
    h.queue.dispatch(urgent("u")).await.unwrap();

    // Only the listed queues.
    let stats = h
        .queue
        .work(WorkOptions {
            queues: vec!["high".into()],
            stop_when_empty: true,
            ..WorkOptions::default()
        })
        .await
        .unwrap();
    assert_eq!(stats.succeeded, 1);
    assert_eq!(h.journal.labels(), ["u"]);

    // `once`: one job.
    let once = WorkOptions {
        once: true,
        ..WorkOptions::default()
    };
    assert_eq!(h.queue.work(once).await.unwrap().succeeded, 1);

    // `max_jobs`: across all workers.
    let limited = WorkOptions {
        max_jobs: Some(1),
        stop_when_empty: true,
        ..WorkOptions::default()
    };
    assert_eq!(h.queue.work(limited).await.unwrap().succeeded, 1);
    assert_eq!(h.queue.pending_count("default").await.unwrap(), 1);

    // By default workers take the module's queues: just `default` here.
    h.queue.dispatch(urgent("v")).await.unwrap();
    assert_eq!(h.drain().await.succeeded, 1);
    assert_eq!(h.queue.pending_count("default").await.unwrap(), 0);
    assert_eq!(h.queue.pending_count("high").await.unwrap(), 1);
}

pub async fn worker_polls_until_the_app_shuts_down(driver: QueueDriver) {
    let h = harness(driver).await;
    let queue = h.queue.clone();
    let worker = tokio::spawn(async move { queue.work(WorkOptions::default()).await });

    // An idle worker keeps polling: a job dispatched later still runs.
    tokio::time::sleep(Duration::from_millis(100)).await;
    h.queue.dispatch(Record::new("late")).await.unwrap();
    wait_until(|| h.journal.labels() == ["late"]).await;
    assert!(!worker.is_finished());

    h.app.ctx().shutdown_token().cancel();
    let stats = tokio::time::timeout(Duration::from_secs(5), worker)
        .await
        .expect("the worker stops at shutdown")
        .unwrap()
        .unwrap();
    assert_eq!(stats.succeeded, 1);
}

/// Waits (up to 10 s) until `done` holds.
pub async fn wait_until(done: impl Fn() -> bool) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !done() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the condition holds in time");
}

pub async fn run_pending_drains_available_jobs_in_order(driver: QueueDriver) {
    let h = harness(driver).await;
    for label in ["a", "b", "c"] {
        h.queue.dispatch(Record::new(label)).await.unwrap();
    }
    h.queue
        .later(Record::new("later"), Duration::from_secs(1))
        .await
        .unwrap();

    let stats = h.queue.run_pending().await.unwrap();

    assert_eq!(stats.succeeded, 3);
    assert_eq!(h.journal.labels(), ["a", "b", "c"]);
    assert_eq!(
        h.queue.pending_count("default").await.unwrap(),
        1,
        "the delayed job waits"
    );
}
