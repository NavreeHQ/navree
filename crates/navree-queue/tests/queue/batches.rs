//! Batches: the behaviour every driver must have (run once per driver, see
//! `main.rs`).

use std::time::Duration;

use navree_db::{Db, Executor as _, Statement};
use navree_queue::{Batch, QueueDriver, QueueError, WorkStats};

use crate::support::{
    Cooperative, Record, Toggle, UniqueReport, Unregistered, harness_with, one_worker, options,
};

fn labels(names: &[&str]) -> Vec<Record> {
    names.iter().map(|name| Record::new(*name)).collect()
}

pub async fn batch_runs_its_jobs_then_and_finally(driver: QueueDriver) {
    let h = harness_with(one_worker(driver)).await;

    // The fluent form of `queue.dispatch_batch(batch)`.
    let batch = Batch::new("recipients.import")
        .add(labels(&["a", "b", "c"]))
        .then(Record::new("then"))
        .catch(Record::new("catch"))
        .finally(Record::new("finally"))
        .dispatch(&h.queue)
        .await
        .expect("dispatched");
    assert_eq!(batch.name, "recipients.import");
    assert_eq!(
        (batch.total_jobs, batch.pending_jobs, batch.failed_jobs),
        (3, 3, 0)
    );
    assert_eq!(batch.progress(), 0);
    assert!(!batch.finished() && !batch.cancelled() && !batch.has_failures());
    assert_eq!(
        h.queue.find_batch(&batch.id).await.unwrap(),
        Some(batch.clone())
    );
    assert_eq!(h.queue.pending_count("default").await.unwrap(), 3);

    let stats = h.drain().await;

    assert_eq!(h.journal.labels(), ["a", "b", "c", "then", "finally"]);
    assert_eq!(
        stats,
        WorkStats {
            processed: 5,
            succeeded: 5,
            ..WorkStats::default()
        }
    );
    let done = h.queue.find_batch(&batch.id).await.unwrap().unwrap();
    assert_eq!((done.pending_jobs, done.failed_jobs), (0, 0));
    assert_eq!(done.processed_jobs(), 3);
    assert_eq!(done.progress(), 100);
    assert!(done.finished() && !done.cancelled());
    assert!(done.failed_job_ids.is_empty());
}

pub async fn first_failure_runs_catch_cancels_the_rest_and_skips_then(driver: QueueDriver) {
    let h = harness_with(one_worker(driver)).await;
    h.journal.set_failing(true);

    let batch = h
        .queue
        .dispatch_batch(
            Batch::new("import")
                .push(Toggle { label: "x".into() })
                .add(labels(&["a", "b"]))
                .then(Record::new("then"))
                .catch(Record::new("catch"))
                .finally(Record::new("finally")),
        )
        .await
        .unwrap();

    let stats = h.drain().await;

    // `a` and `b` are skipped: the failure cancelled the batch.
    assert_eq!(h.journal.labels(), ["catch", "finally"]);
    assert_eq!((stats.failed, stats.cancelled, stats.succeeded), (1, 2, 2));
    let done = h.queue.find_batch(&batch.id).await.unwrap().unwrap();
    assert_eq!((done.pending_jobs, done.failed_jobs), (0, 1));
    assert!(done.finished() && done.cancelled() && done.has_failures());
    let failed = h.queue.failed().await.unwrap();
    assert_eq!(failed.len(), 1);
    assert_eq!(done.failed_job_ids, [failed[0].id.clone()]);
    assert_eq!(failed[0].job_type, "test.toggle");
}

pub async fn allow_failures_keeps_the_batch_running(driver: QueueDriver) {
    let h = harness_with(one_worker(driver)).await;
    h.journal.set_failing(true);

    let batch = h
        .queue
        .dispatch_batch(
            Batch::new("import")
                .push(Toggle { label: "x".into() })
                .add(labels(&["a", "b"]))
                .then(Record::new("then"))
                .catch(Record::new("catch"))
                .finally(Record::new("finally"))
                .allow_failures(),
        )
        .await
        .unwrap();
    assert!(batch.allow_failures);

    h.drain().await;

    assert_eq!(h.journal.labels(), ["a", "b", "catch", "finally"]);
    let done = h.queue.find_batch(&batch.id).await.unwrap().unwrap();
    assert!(done.finished() && !done.cancelled());
    assert_eq!(done.failed_jobs, 1);
}

pub async fn cancel_batch_skips_jobs_not_started(driver: QueueDriver) {
    let h = harness_with(one_worker(driver)).await;

    let batch = h
        .queue
        .dispatch_batch(
            Batch::new("import")
                .add(labels(&["a", "b"]))
                .then(Record::new("then"))
                .finally(Record::new("finally")),
        )
        .await
        .unwrap();
    let cancelled = h.queue.cancel_batch(&batch.id).await.unwrap().unwrap();
    assert!(cancelled.cancelled() && !cancelled.finished());

    let stats = h.drain().await;

    assert_eq!(h.journal.labels(), ["finally"]);
    assert_eq!((stats.cancelled, stats.succeeded), (2, 1));
    let done = h.queue.find_batch(&batch.id).await.unwrap().unwrap();
    assert!(done.finished() && done.cancelled());
    assert_eq!(done.failed_jobs, 0);

    // A finished batch stays as it is; an unknown one is `None`.
    let again = h.queue.cancel_batch(&batch.id).await.unwrap().unwrap();
    assert_eq!(again.cancelled_at, done.cancelled_at);
    assert_eq!(h.queue.cancel_batch("nope").await.unwrap(), None);
}

pub async fn running_jobs_see_their_batch_cancelled(driver: QueueDriver) {
    let h = harness_with(one_worker(driver)).await;
    let batch = h
        .queue
        .dispatch_batch(
            Batch::new("long")
                .push(Cooperative { label: "c".into() })
                .finally(Record::new("finally")),
        )
        .await
        .unwrap();

    let queue = h.queue.clone();
    let worker = tokio::spawn(async move { queue.run_pending().await });
    tokio::time::timeout(Duration::from_secs(10), h.journal.started.notified())
        .await
        .expect("the job starts");
    h.queue.cancel_batch(&batch.id).await.unwrap();
    let stats = tokio::time::timeout(Duration::from_secs(10), worker)
        .await
        .expect("the job stops at its next progress report")
        .unwrap()
        .unwrap();

    assert_eq!((stats.cancelled, stats.failed), (1, 0));
    assert!(h.queue.failed().await.unwrap().is_empty());
    // `finally` was queued when the job stopped.
    h.drain().await;
    assert_eq!(h.journal.labels(), ["finally"]);
}

pub async fn batch_in_a_rolled_back_unit_of_work_is_not_stored(driver: QueueDriver) {
    let h = harness_with(options(driver)).await;
    let db = h.app.get::<Db>().await;

    let uow = db.uow(None, None).await;
    let rolled_back = h
        .queue
        .dispatch_batch_in(&uow, Batch::new("import").add(labels(&["a"])))
        .await
        .unwrap();
    uow.rollback().await.unwrap();
    assert_eq!(h.queue.find_batch(&rolled_back.id).await.unwrap(), None);
    assert_eq!(h.queue.pending_count("default").await.unwrap(), 0);

    let uow = db.uow(None, None).await;
    // The fluent form of `queue.dispatch_batch_in(&uow, batch)`.
    let committed = Batch::new("import")
        .add(labels(&["b"]))
        .dispatch_in(&h.queue, &uow)
        .await
        .unwrap();
    assert_eq!(h.queue.find_batch(&committed.id).await.unwrap(), None);
    uow.commit().await.unwrap();
    assert_eq!(
        h.queue.find_batch(&committed.id).await.unwrap(),
        Some(committed)
    );
    h.drain().await;
    assert_eq!(h.journal.labels(), ["b"]);
}

pub async fn batch_dispatch_joins_the_current_unit_of_work(driver: QueueDriver) {
    let h = harness_with(options(driver)).await;
    let db = h.app.get::<Db>().await;

    let uow = db.uow(None, None).await;
    uow.fetch_all(&Statement::text("SELECT 1")).await.unwrap();
    let batch = uow
        .scope(
            h.queue
                .dispatch_batch(Batch::new("import").add(labels(&["a"]))),
        )
        .await
        .unwrap();
    assert_eq!(h.queue.find_batch(&batch.id).await.unwrap(), None);
    uow.commit().await.unwrap();
    assert!(h.queue.find_batch(&batch.id).await.unwrap().is_some());
}

pub async fn empty_batch_finishes_at_once(driver: QueueDriver) {
    let h = harness_with(one_worker(driver)).await;

    let batch = h
        .queue
        .dispatch_batch(
            Batch::new("nothing")
                .then(Record::new("then"))
                .finally(Record::new("finally")),
        )
        .await
        .unwrap();
    assert!(batch.finished());
    assert_eq!((batch.total_jobs, batch.progress()), (0, 100));

    h.drain().await;
    assert_eq!(h.journal.labels(), ["then", "finally"]);
}

pub async fn concurrent_workers_finish_a_batch_once(driver: QueueDriver) {
    let mut opts = options(driver);
    opts.workers = 4;
    let h = harness_with(opts).await;
    let names: Vec<String> = (0..24).map(|n| format!("job-{n}")).collect();

    let batch = h
        .queue
        .dispatch_batch(
            Batch::new("many")
                .add(names.iter().map(|name| Record::new(name.as_str())))
                .then(Record::new("then"))
                .finally(Record::new("finally")),
        )
        .await
        .unwrap();
    h.drain().await;
    // Callbacks queued by the last job may arrive after a worker stopped.
    h.drain().await;

    let mut ran = h.journal.labels();
    ran.sort();
    let mut expected: Vec<String> = names.clone();
    expected.extend(["finally".to_owned(), "then".to_owned()]);
    expected.sort();
    assert_eq!(ran, expected);
    let done = h.queue.find_batch(&batch.id).await.unwrap().unwrap();
    assert!(done.finished());
    assert_eq!(done.pending_jobs, 0);
}

pub async fn unique_jobs_in_a_batch_all_count(driver: QueueDriver) {
    let h = harness_with(one_worker(driver)).await;
    let report = || UniqueReport {
        account: "acme".into(),
    };

    let batch = h
        .queue
        .dispatch_batch(Batch::new("reports").push(report()).push(report()))
        .await
        .unwrap();
    assert_eq!(batch.total_jobs, 2);
    h.drain().await;
    assert_eq!(h.journal.labels(), ["acme", "acme"]);
    assert!(
        h.queue
            .find_batch(&batch.id)
            .await
            .unwrap()
            .unwrap()
            .finished()
    );
}

pub async fn unknown_job_types_reject_the_whole_batch(driver: QueueDriver) {
    let h = harness_with(options(driver)).await;

    for batch in [
        Batch::new("bad job")
            .push(Record::new("a"))
            .push(Unregistered),
        Batch::new("bad callback")
            .push(Record::new("a"))
            .then(Unregistered),
    ] {
        let error = h.queue.dispatch_batch(batch).await.unwrap_err();
        assert!(
            matches!(&error, QueueError::UnknownJob { job_type } if job_type == "test.unregistered"),
            "{error}"
        );
    }
    assert!(h.queue.batches(10).await.unwrap().is_empty());
    assert_eq!(h.queue.pending_count("default").await.unwrap(), 0);
}

pub async fn batches_are_listed_newest_first_and_pruned(driver: QueueDriver) {
    let h = harness_with(one_worker(driver)).await;

    let first = h
        .queue
        .dispatch_batch(Batch::new("first").add(labels(&["a"])))
        .await
        .unwrap();
    h.advance(1);
    let second = h
        .queue
        .dispatch_batch(Batch::new("second").add(labels(&["b"])))
        .await
        .unwrap();
    let names: Vec<String> = h
        .queue
        .batches(10)
        .await
        .unwrap()
        .into_iter()
        .map(|batch| batch.name)
        .collect();
    assert_eq!(names, ["second", "first"]);
    assert_eq!(h.queue.batches(1).await.unwrap()[0].id, second.id);

    // Unfinished batches are kept.
    h.advance(3 * 3600);
    assert_eq!(
        h.queue
            .prune_batches(Duration::from_secs(3600))
            .await
            .unwrap(),
        0
    );
    h.drain().await;
    assert_eq!(
        h.queue
            .prune_batches(Duration::from_secs(3600))
            .await
            .unwrap(),
        0,
        "they finished just now"
    );
    h.advance(2 * 3600);
    assert_eq!(
        h.queue
            .prune_batches(Duration::from_secs(3600))
            .await
            .unwrap(),
        2
    );
    assert_eq!(h.queue.find_batch(&first.id).await.unwrap(), None);
}
