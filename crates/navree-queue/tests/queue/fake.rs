//! `QueueFake`: records dispatched jobs instead of queueing them.

use std::sync::Arc;
use std::time::Duration;

use navree_contracts::{Dispatcher, Job};
use navree_db::{DatabaseModule, Db};
use navree_queue::{Queue, QueueDriver, QueueFake, QueueModule, QueueOptions, WorkStats};
use navree_testing::TestApp;

use crate::support::{JobsModule, Journal, Record, Toggle, Urgent};

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
