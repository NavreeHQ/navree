//! `QueueModule` wiring: configuration, providers and boot errors.

use std::sync::Arc;

use navree_contracts::{Dispatcher, JobEnvelope};
use navree_db::DatabaseModule;
use navree_queue::{Queue, QueueDriver, QueueModule, QueueOptions};
use navree_testing::TestApp;

use crate::support::{JobsModule, Journal, Record};

#[tokio::test]
async fn from_config_reads_section_queue_and_memory_needs_no_database() {
    let app = TestApp::builder()
        .config("queue.driver", "memory")
        .config("queue.workers", 3)
        .config("queue.queues", "high,default")
        .module(QueueModule::from_config())
        .module(JobsModule)
        .build()
        .await;

    let queue = app.get::<Queue>().await;
    assert_eq!(queue.options().driver, QueueDriver::Memory);
    assert_eq!(queue.options().workers, 3);
    assert_eq!(queue.options().queues, ["high", "default"]);
    assert_eq!(
        *app.get::<QueueOptions>().await,
        queue.options().clone(),
        "the options are exported"
    );
}

#[tokio::test]
async fn plain_module_reads_the_configuration_too() {
    let app = TestApp::builder()
        .config("queue.driver", "memory")
        .config("queue.default_queue", "main")
        .module(QueueModule)
        .module(JobsModule)
        .build()
        .await;

    let queue = app.get::<Queue>().await;
    assert_eq!(queue.options().default_queue, "main");
}

#[tokio::test]
async fn the_queue_is_the_apps_dispatcher() {
    let app = TestApp::builder()
        .module(QueueModule::for_root(QueueOptions {
            driver: QueueDriver::Memory,
            ..QueueOptions::default()
        }))
        .module(JobsModule)
        .build()
        .await;

    let dispatcher = app.get::<dyn Dispatcher>().await;
    let id = dispatcher.dispatch(Record::new("x")).await.unwrap();

    assert!(!id.is_empty());
    assert_eq!(
        app.get::<Queue>()
            .await
            .pending_count("default")
            .await
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn database_driver_without_database_module_fails_the_boot() {
    let error = TestApp::builder()
        .module(QueueModule::for_root(QueueOptions::default()))
        .try_build()
        .await
        .unwrap_err()
        .to_string();

    assert!(error.contains("navree_db"), "{error}");
}

#[tokio::test]
async fn invalid_options_fail_the_boot() {
    let error = TestApp::builder()
        .module(QueueModule::for_root(QueueOptions {
            driver: QueueDriver::Memory,
            workers: 0,
            ..QueueOptions::default()
        }))
        .try_build()
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("workers"), "{error}");

    let error = TestApp::builder()
        .config("queue.driver", "redis")
        .module(QueueModule::from_config())
        .try_build()
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("queue"), "{error}");
}

#[tokio::test]
async fn jobs_without_a_queue_name_go_to_the_default_queue() {
    let app = TestApp::builder()
        .module(QueueModule::for_root(QueueOptions {
            driver: QueueDriver::Memory,
            default_queue: "main".into(),
            ..QueueOptions::default()
        }))
        .module(JobsModule)
        .override_value(Arc::new(Journal::default()))
        .build()
        .await;
    let queue = app.get::<Queue>().await;
    let mut envelope = JobEnvelope::of(&Record::new("x")).unwrap();
    envelope.options.queue = String::new();

    queue.push(envelope).await.unwrap();

    assert_eq!(queue.pending_count("main").await.unwrap(), 1);
    assert_eq!(queue.pending_count("").await.unwrap(), 0);
    assert_eq!(queue.options().worker_queues(), ["main"]);
    assert_eq!(queue.run_pending().await.unwrap().succeeded, 1);
}

#[tokio::test]
async fn debug_output_names_the_driver_and_job_types() {
    let app = TestApp::builder()
        .module(QueueModule::for_root(QueueOptions {
            driver: QueueDriver::Memory,
            ..QueueOptions::default()
        }))
        .module(JobsModule)
        .build()
        .await;

    let debug = format!("{:?}", app.get::<Queue>().await);
    assert!(debug.contains("Memory"), "{debug}");
    assert!(debug.contains("test.record"), "{debug}");
    let fake = format!("{:?}", navree_queue::QueueFake::new().queue());
    assert!(fake.contains("Fake"), "{fake}");
}

#[tokio::test]
async fn from_config_defaults_to_the_database_driver() {
    let app = TestApp::builder()
        .temp_database()
        .module(DatabaseModule)
        .module(QueueModule::from_config())
        .module(JobsModule)
        .override_value(Arc::new(Journal::default()))
        .build()
        .await;
    let queue = app.get::<Queue>().await;
    assert_eq!(queue.options().driver, QueueDriver::Database);

    queue.dispatch(Record::new("x")).await.unwrap();

    assert_eq!(queue.run_pending().await.unwrap().succeeded, 1);
    assert_eq!(app.get::<Journal>().await.labels(), ["x"]);
}
