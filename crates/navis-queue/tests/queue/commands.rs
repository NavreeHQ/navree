//! The `queue:*` commands, run against a booted app.

use navis_queue::QueueDriver;

use crate::support::{Record, Toggle, Urgent, harness};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn module_registers_the_queue_commands() {
    let h = harness(QueueDriver::Database).await;
    let names: Vec<&str> = h.app.ctx().commands().iter().map(|c| c.name).collect();
    for name in ["queue:work", "queue:failed", "queue:retry", "queue:flush"] {
        assert!(names.contains(&name), "{name} missing from {names:?}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queue_work_runs_jobs_with_its_options() {
    let h = harness(QueueDriver::Database).await;
    h.queue.dispatch(Record::new("a")).await.unwrap();
    h.queue.dispatch(Record::new("b")).await.unwrap();
    h.queue
        .dispatch(Urgent { label: "u".into() })
        .await
        .unwrap();

    h.app.run_command("queue:work", &["--once"]).await.unwrap();
    assert_eq!(h.journal.labels(), ["a"]);

    h.app
        .run_command("queue:work", &["--queue", "high", "--stop-when-empty"])
        .await
        .unwrap();
    assert_eq!(h.journal.labels(), ["a", "u"]);

    h.app
        .run_command("queue:work", &["--stop-when-empty"])
        .await
        .unwrap();
    assert_eq!(h.journal.labels(), ["a", "u", "b"]);

    let error = h
        .app
        .run_command("queue:work", &["--bogus"])
        .await
        .unwrap_err();
    assert!(error.to_string().contains("--bogus"), "{error}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failed_retry_and_flush_commands_manage_failed_jobs() {
    let h = harness(QueueDriver::Database).await;
    h.journal.set_failing(true);
    let mut ids = Vec::new();
    for label in ["one", "two", "three"] {
        ids.push(
            h.queue
                .dispatch(Toggle {
                    label: label.into(),
                })
                .await
                .unwrap(),
        );
    }
    h.drain().await;
    assert_eq!(h.queue.failed().await.unwrap().len(), 3);

    h.app.run_command("queue:failed", &[]).await.unwrap();

    h.app.run_command("queue:retry", &[&ids[0]]).await.unwrap();
    assert_eq!(h.queue.failed().await.unwrap().len(), 2);
    assert_eq!(h.queue.pending_count("default").await.unwrap(), 1);

    let usage = h.app.run_command("queue:retry", &[]).await.unwrap_err();
    assert!(
        usage.to_string().contains("queue:retry <id|all>"),
        "{usage}"
    );
    assert!(
        h.app
            .run_command("queue:retry", &["missing"])
            .await
            .is_err()
    );

    h.app.run_command("queue:retry", &["all"]).await.unwrap();
    assert!(h.queue.failed().await.unwrap().is_empty());
    assert_eq!(h.queue.pending_count("default").await.unwrap(), 3);

    h.drain().await;
    assert_eq!(h.drain().await.processed, 0);
    assert_eq!(h.queue.failed().await.unwrap().len(), 3, "still failing");
    h.app.run_command("queue:flush", &[]).await.unwrap();
    assert!(h.queue.failed().await.unwrap().is_empty());
}
