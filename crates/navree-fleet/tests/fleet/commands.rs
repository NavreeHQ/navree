//! The `fleet:*` commands.

use std::sync::Arc;
use std::time::Duration;

use navree_fleet::testing::FakeSpawner;
use navree_fleet::{Master, WorkerCommand};
use navree_queue::WorkOptions;
use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::support::{Export, Harness, builder, eventually, fast};

fn supervisors() -> serde_json::Value {
    json!({
        "default": { "queues": ["default"] },
        "exports": { "queues": ["exports"], "balance": "off", "max_processes": 2 }
    })
}

#[tokio::test]
async fn pause_and_continue_take_an_optional_supervisor() {
    let h = Harness::new(builder().config("fleet.supervisors", supervisors())).await;
    h.app.run_command("fleet:pause", &["exports"]).await.unwrap();
    assert!(h.fleet.is_paused("exports").await.unwrap());
    assert!(!h.fleet.is_paused("default").await.unwrap());
    h.app.run_command("fleet:pause", &[]).await.unwrap();
    assert_eq!(h.fleet.paused().await.unwrap().len(), 2);
    h.app.run_command("fleet:continue", &["default"]).await.unwrap();
    assert!(!h.fleet.is_paused("default").await.unwrap());
    h.app.run_command("fleet:continue", &[]).await.unwrap();
    assert!(h.fleet.paused().await.unwrap().is_empty());

    let error = h.app.run_command("fleet:pause", &["nope"]).await.unwrap_err();
    assert!(error.to_string().contains("nope"), "{error}");
    let error = h.app.run_command("fleet:pause", &["a", "b"]).await.unwrap_err();
    assert!(error.to_string().contains("usage"), "{error}");
}

#[tokio::test]
async fn forget_deletes_one_failed_job() {
    let h = Harness::new(builder()).await;
    let id = h
        .queue
        .dispatch(Export {
            contact: "ada".into(),
            password: "p".into(),
        })
        .await
        .unwrap();
    h.queue
        .work(WorkOptions {
            queues: vec!["exports".into()],
            stop_when_empty: true,
            ..WorkOptions::default()
        })
        .await
        .unwrap();
    h.app.run_command("fleet:forget", &[&id]).await.unwrap();
    assert!(h.fleet.failed_jobs(None).await.unwrap().is_empty());
    let error = h.app.run_command("fleet:forget", &[&id]).await.unwrap_err();
    assert!(error.to_string().contains(&id), "{error}");
    assert!(h.app.run_command("fleet:forget", &[]).await.is_err());
}

#[tokio::test]
async fn status_fails_without_a_running_master() {
    let h = Harness::new(builder()).await;
    let error = h.app.run_command("fleet:status", &[]).await.unwrap_err();
    assert!(error.to_string().contains("no Fleet master"), "{error}");

    let spawner = Arc::new(FakeSpawner::new());
    let master = Master::from_app(h.app.ctx())
        .await
        .unwrap()
        .with_spawner(spawner.clone())
        .with_settings(fast());
    let stop = CancellationToken::new();
    let task = tokio::spawn(master.run(stop.clone()));
    eventually("a master", || async { !h.fleet.masters().await.unwrap().is_empty() }).await;
    h.app.run_command("fleet:status", &[]).await.unwrap();

    // `fleet:terminate` stops it.
    h.app.run_command("fleet:terminate", &[]).await.unwrap();
    tokio::time::timeout(Duration::from_secs(10), task)
        .await
        .expect("terminated")
        .unwrap()
        .unwrap();
    assert_eq!(spawner.commands(0), [WorkerCommand::Stop]);
}

#[tokio::test]
async fn bad_arguments_are_rejected() {
    let h = Harness::new(builder()).await;
    assert!(h.app.run_command("fleet", &["now"]).await.is_err());
    let error = h
        .app
        .run_command("fleet:work", &["--bogus"])
        .await
        .unwrap_err();
    assert!(error.to_string().contains("--bogus"), "{error}");
}
