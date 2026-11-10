//! With a queue: every execution is a `workflows.run` job, timers are
//! delayed jobs, and events wake runs through the queue.

use std::sync::Arc;

use navree_queue::{Queue, QueueModule, WorkOptions};
use navree_workflows::RunStatus;
use serde_json::json;

use crate::support::{AwaitCourse, CourseCompleted, DAY, Harness, Onboarding, harness};

async fn queued() -> Harness {
    harness(|b| {
        b.config("queue.driver", "memory")
            .config("workflows.queue", "flows")
            .module(QueueModule::from_config())
    })
    .await
}

async fn work(h: &Harness) -> u64 {
    let queue: Arc<Queue> = h.app.get::<Queue>().await;
    queue
        .work(WorkOptions {
            queues: vec!["flows".to_owned()],
            stop_when_empty: true,
            ..WorkOptions::default()
        })
        .await
        .unwrap()
        .succeeded
}

#[tokio::test]
async fn executions_are_jobs_and_timers_are_delayed_jobs() {
    let h = queued().await;
    let id = h.start(Onboarding { user_id: 7 }).await;
    // Queued on `workflows.queue`, not run yet.
    assert_eq!(h.status(id).await, RunStatus::Pending);
    assert!(h.journal.calls().is_empty());
    let queue = h.app.get::<Queue>().await;
    assert_eq!(queue.pending_count("flows").await.unwrap(), 1);

    assert_eq!(work(&h).await, 1);
    assert_eq!(h.status(id).await, RunStatus::Sleeping);
    assert_eq!(h.journal.calls(), ["welcome"]);
    // The wake-up waits in the queue, delayed by three days.
    assert_eq!(queue.pending_count("flows").await.unwrap(), 1);
    assert_eq!(work(&h).await, 0);

    h.clock.advance(3 * DAY);
    assert_eq!(work(&h).await, 1);
    assert_eq!(h.status(id).await, RunStatus::Completed);
    assert_eq!(h.journal.calls(), ["welcome", "nudge"]);
}

#[tokio::test]
async fn an_event_wakes_its_run_through_the_queue() {
    let h = queued().await;
    let id = h.start(AwaitCourse { user_id: 7 }).await;
    work(&h).await;
    assert_eq!(h.status(id).await, RunStatus::Waiting);
    h.emit(CourseCompleted {
        user_id: 7,
        course_id: 1,
        score: 80,
    })
    .await;
    // Resolved, queued, not run yet.
    assert_eq!(h.status(id).await, RunStatus::Pending);
    work(&h).await;
    let run = h.workflows().await.run(id).await.unwrap();
    assert_eq!(run.status, RunStatus::Completed);
    assert_eq!(run.output, Some(json!(80)));
}
