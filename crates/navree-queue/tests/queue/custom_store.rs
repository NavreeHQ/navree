//! `queue.driver = "redis"` resolves the `dyn QueueStore` provider named
//! `redis` (here the reference store of `reference_store.rs`); the driver
//! suite runs on it from `main.rs`.

use navree_queue::{Batch, Queue, QueueDriver, QueueError, QueueModule, QueueOptions};
use navree_testing::TestApp;

use crate::reference_store::{NamedStoreModule, ReferenceStore};
use crate::support::{JobsModule, Record, harness_with, options};

#[tokio::test]
async fn the_redis_driver_is_the_store_named_redis() {
    let store = ReferenceStore::default();
    let app = TestApp::builder()
        .module(QueueModule::for_root(QueueOptions {
            driver: QueueDriver::Redis,
            ..QueueOptions::default()
        }))
        .module(NamedStoreModule(store.clone()))
        .module(JobsModule)
        .build()
        .await;
    let queue = app.get::<Queue>().await;
    assert!(format!("{queue:?}").contains("Redis"), "{queue:?}");
    queue.dispatch(Record::new("a")).await.unwrap();
    assert_eq!(store.len(), 1);
    assert_eq!(queue.pending_count("default").await.unwrap(), 1);
}

#[tokio::test]
async fn the_redis_driver_without_its_store_fails_the_boot() {
    let error = TestApp::builder()
        .module(QueueModule::for_root(QueueOptions {
            driver: QueueDriver::Redis,
            ..QueueOptions::default()
        }))
        .try_build()
        .await
        .expect_err("no store named redis");
    let message = error.to_string();
    assert!(
        message.contains("queue.driver = \"redis\"") && message.contains("RedisModule"),
        "{message}"
    );
}

#[tokio::test]
async fn batches_need_the_database_or_memory_driver() {
    let h = harness_with(options(QueueDriver::Redis)).await;

    let error = Batch::new("import")
        .push(Record::new("a"))
        .then(Record::new("then"))
        .dispatch(&h.queue)
        .await
        .expect_err("the store has no batches");
    assert!(
        matches!(&error, QueueError::Store(_))
            && error.to_string().contains("does not support batches"),
        "{error}"
    );
    // Nothing was queued; the batch listings are empty.
    assert_eq!(h.queue.pending_count("default").await.unwrap(), 0);
    assert!(h.queue.batches(10).await.unwrap().is_empty());
}
