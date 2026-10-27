//! `QueueFake`: records dispatched jobs instead of queueing them.

use std::sync::Arc;
use std::time::Duration;

use navree_contracts::{Dispatcher, Job};
use navree_db::{DatabaseModule, Db};
use navree_queue::{
    Batch, JobBatch, Queue, QueueDriver, QueueFake, QueueModule, QueueOptions, WorkStats,
};
use navree_testing::TestApp;

use crate::support::{JobsModule, Journal, Record, Toggle, Urgent, harness};

async fn faked_app(fake: &QueueFake) -> TestApp {
    TestApp::builder()
        .temp_database()
        .module(DatabaseModule)
        .module(QueueModule::for_root(QueueOptions {
            driver: QueueDriver::Memory,
            ..QueueOptions::default()
        }))
        .module(JobsModule)
        .override_value(Arc::new(Journal::default()))
        .override_value(Arc::new(fake.queue()))
        .build()
        .await
}

#[tokio::test]
async fn queue_fake_records_dispatches_without_running_them() {
    let fake = QueueFake::new();
    let app = faked_app(&fake).await;
    let queue = app.get::<Queue>().await;

    queue.dispatch(Record::new("a")).await.unwrap();
    let dispatcher = app.get::<dyn Dispatcher>().await;
    dispatcher.dispatch(Record::new("b")).await.unwrap();
    queue
        .later(Urgent { label: "u".into() }, Duration::from_secs(60))
        .await
        .unwrap();

    assert_eq!(
        fake.dispatched::<Record>(),
        [Record::new("a"), Record::new("b")]
    );
    fake.assert_dispatched::<Record>(|job| job.label == "b");
    fake.assert_dispatched_times::<Record>(2);
    fake.assert_not_dispatched::<Toggle>();
    let urgent = fake
        .envelopes()
        .into_iter()
        .find(|e| e.job_type == Urgent::NAME)
        .expect("recorded");
    assert_eq!(urgent.options.queue, "high");
    assert_eq!(urgent.delay_secs, Some(60));
    assert_eq!(queue.pending_count("default").await.unwrap(), 2);

    // Nothing runs.
    assert_eq!(queue.run_pending().await.unwrap(), WorkStats::default());
    assert!(app.get::<Journal>().await.runs().is_empty());

    fake.clear();
    fake.assert_nothing_dispatched();
}

#[tokio::test]
async fn queue_fake_records_unit_of_work_dispatches_at_commit() {
    let fake = QueueFake::new();
    let app = faked_app(&fake).await;
    let queue = app.get::<Queue>().await;
    let db = app.get::<Db>().await;

    let uow = db.uow(None, None).await;
    queue
        .dispatch_in(&uow, Record::new("rolled back"))
        .await
        .unwrap();
    uow.rollback().await.unwrap();
    let uow = db.uow(None, None).await;
    queue
        .dispatch_in(&uow, Record::new("committed"))
        .await
        .unwrap();
    fake.assert_nothing_dispatched();
    uow.commit().await.unwrap();

    assert_eq!(fake.dispatched::<Record>(), [Record::new("committed")]);
}

#[tokio::test]
async fn queue_fake_is_a_dispatcher_on_its_own() {
    let fake = QueueFake::new();
    let app = TestApp::builder()
        .override_value(fake.dispatcher())
        .build()
        .await;

    let dispatcher = app.get::<dyn Dispatcher>().await;
    dispatcher
        .dispatch(Toggle { label: "t".into() })
        .await
        .unwrap();

    fake.assert_dispatched::<Toggle>(|job| job.label == "t");
}

#[test]
#[should_panic(expected = "test.record")]
fn assert_dispatched_names_the_missing_job_type() {
    QueueFake::new().assert_dispatched::<Record>(|_| true);
}

#[tokio::test]
#[should_panic(expected = "test.record")]
async fn assert_not_dispatched_names_the_dispatched_job_type() {
    let fake = QueueFake::new();
    fake.queue().dispatch(Record::new("a")).await.unwrap();
    fake.assert_not_dispatched::<Record>();
}

// ---------------------------------------------------------------------------
// Batches
// ---------------------------------------------------------------------------

fn import_batch() -> Batch {
    Batch::new("recipients.import")
        .add([Record::new("a"), Record::new("b")])
        .push(Urgent { label: "u".into() })
        .then(Record::new("then"))
        .catch(Toggle {
            label: "catch".into(),
        })
        .finally(Record::new("finally"))
}

#[tokio::test]
async fn queue_fake_records_batches_apart_from_single_jobs() {
    let fake = QueueFake::new();
    let queue = fake.queue();

    let batch = queue.dispatch_batch(import_batch()).await.unwrap();

    let recorded = fake.batches();
    assert_eq!(recorded.len(), 1);
    let recorded = &recorded[0];
    assert_eq!(recorded.name(), "recipients.import");
    assert_eq!(recorded.batch, batch);
    assert_eq!(
        recorded.jobs_of::<Record>(),
        [Record::new("a"), Record::new("b")]
    );
    assert_eq!(recorded.jobs_of::<Urgent>().len(), 1);
    assert_eq!(recorded.then_of::<Record>(), Some(Record::new("then")));
    assert_eq!(
        recorded.catch_of::<Toggle>(),
        Some(Toggle {
            label: "catch".into()
        })
    );
    assert_eq!(recorded.finally_of::<Record>(), Some(Record::new("finally")));
    assert_eq!(recorded.then_of::<Toggle>(), None);
    fake.assert_batched(|b| b.name() == "recipients.import" && b.jobs.len() == 3);
    // A batch's jobs are not single dispatches.
    fake.assert_nothing_dispatched();

    // The fake keeps the batch as dispatched and cancels it on demand.
    assert_eq!(
        queue.find_batch(&batch.id).await.unwrap(),
        Some(batch.clone())
    );
    assert_eq!(queue.batches(10).await.unwrap(), [batch.clone()]);
    let cancelled = queue.cancel_batch(&batch.id).await.unwrap().unwrap();
    assert!(cancelled.cancelled());
    assert_eq!(queue.run_pending().await.unwrap(), WorkStats::default());

    fake.clear();
    fake.assert_nothing_batched();
}

/// The fake stores a dispatched batch exactly as the drivers do.
#[tokio::test]
async fn queue_fake_batches_match_the_drivers() {
    let fake = QueueFake::new();
    let faked = fake.queue().dispatch_batch(import_batch()).await.unwrap();
    let comparable = |b: &JobBatch| {
        (
            b.name.clone(),
            b.total_jobs,
            b.pending_jobs,
            b.failed_jobs,
            b.failed_job_ids.clone(),
            b.allow_failures,
            b.operation_id,
            b.finished(),
            b.cancelled(),
        )
    };
    for driver in [QueueDriver::Memory, QueueDriver::Database] {
        let h = harness(driver).await;
        let stored = h.queue.dispatch_batch(import_batch()).await.unwrap();
        assert_eq!(comparable(&faked), comparable(&stored), "{driver:?}");
        let found = h.queue.find_batch(&stored.id).await.unwrap().unwrap();
        assert_eq!(found, stored, "{driver:?}");
    }

    // A batch with no jobs finishes at once and its callbacks are queued:
    // the fake records them as dispatched.
    let empty = fake
        .queue()
        .dispatch_batch(Batch::new("empty").then(Record::new("then")))
        .await
        .unwrap();
    assert!(empty.finished());
    fake.assert_dispatched::<Record>(|job| job.label == "then");
}

/// Like single jobs, a batch dispatched in a unit of work is recorded when
/// it commits and never when it rolls back.
#[tokio::test]
async fn queue_fake_records_unit_of_work_batches_at_commit() {
    let fake = QueueFake::new();
    let app = faked_app(&fake).await;
    let queue = app.get::<Queue>().await;
    let db = app.get::<Db>().await;

    let uow = db.uow(None, None).await;
    Batch::new("rolled back")
        .push(Record::new("r"))
        .dispatch_in(&queue, &uow)
        .await
        .unwrap();
    uow.rollback().await.unwrap();

    let uow = db.uow(None, None).await;
    let batch = Batch::new("committed")
        .push(Record::new("c"))
        .then(Record::new("then"))
        .dispatch_in(&queue, &uow)
        .await
        .unwrap();
    fake.assert_nothing_batched();
    uow.commit().await.unwrap();

    let recorded = fake.batches();
    assert_eq!(recorded.len(), 1, "{recorded:?}");
    assert_eq!(recorded[0].batch, batch);
    assert_eq!(recorded[0].jobs_of::<Record>(), [Record::new("c")]);

    // Outside a unit of work it is recorded at once.
    Batch::new("now")
        .push(Record::new("n"))
        .dispatch(&queue)
        .await
        .unwrap();
    fake.assert_batched(|b| b.name() == "now");
}

#[tokio::test]
#[should_panic(expected = "recipients.import")]
async fn assert_nothing_batched_names_the_batches() {
    let fake = QueueFake::new();
    fake.queue().dispatch_batch(import_batch()).await.unwrap();
    fake.assert_nothing_batched();
}
