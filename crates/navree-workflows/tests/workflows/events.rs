//! Event waits: matched events resume runs, others do not, timeouts end
//! the wait, and only registered events can be awaited.

use navree_workflows::{RunStatus, StepStatus, Workflows};
use serde_json::json;

use crate::support::{AwaitCourse, AwaitUnregistered, CourseCompleted, DAY, harness, start};

#[tokio::test]
async fn a_matching_event_resumes_a_waiting_run() {
    let h = harness(|b| b).await;
    // An event before the wait does not count.
    h.emit(CourseCompleted {
        user_id: 7,
        course_id: 1,
        score: 50,
    })
    .await;
    let id = h.start(AwaitCourse { user_id: 7 }).await;
    let workflows = h.workflows().await;
    let run = workflows.run(id).await.unwrap();
    assert_eq!(run.status, RunStatus::Waiting);
    assert_eq!(run.wake_at, Some(start() + chrono::TimeDelta::days(14)));
    let wait = &workflows.details(id).await.unwrap().steps[0];
    assert_eq!(wait.key, "$wait:course.completed");
    assert_eq!(wait.status, StepStatus::Waiting);
    assert_eq!(wait.event.as_deref(), Some("course.completed"));
    assert_eq!(wait.filter, Some(json!({"fields": {"user_id": 7}})));

    // Another learner's event leaves it waiting.
    h.emit(CourseCompleted {
        user_id: 8,
        course_id: 1,
        score: 70,
    })
    .await;
    assert_eq!(h.status(id).await, RunStatus::Waiting);

    // Its own resumes it (inline, without a queue).
    h.emit(CourseCompleted {
        user_id: 7,
        course_id: 2,
        score: 90,
    })
    .await;
    let run = workflows.run(id).await.unwrap();
    assert_eq!(run.status, RunStatus::Completed);
    assert_eq!(run.output, Some(json!(90)));
    assert_eq!(h.journal.calls(), ["congratulate"]);
    let wait = &workflows.details(id).await.unwrap().steps[0];
    assert_eq!(
        wait.output,
        Some(json!({"event": {"user_id": 7, "course_id": 2, "score": 90}}))
    );

    // A later event changes nothing.
    h.emit(CourseCompleted {
        user_id: 7,
        course_id: 3,
        score: 10,
    })
    .await;
    assert_eq!(h.journal.calls(), ["congratulate"]);
}

#[tokio::test]
async fn a_wait_times_out() {
    let h = harness(|b| b).await;
    let id = h.start(AwaitCourse { user_id: 7 }).await;
    assert_eq!(h.advance(13 * DAY).await, 0);
    assert_eq!(h.advance(DAY).await, 1);
    let run = h.workflows().await.run(id).await.unwrap();
    assert_eq!(run.status, RunStatus::Completed);
    assert_eq!(run.output, Some(json!(null)));
    assert_eq!(h.journal.calls(), ["remind"]);
    // The event after the timeout is too late.
    h.emit(CourseCompleted {
        user_id: 7,
        course_id: 1,
        score: 99,
    })
    .await;
    assert_eq!(h.journal.calls(), ["remind"]);
}

#[tokio::test]
async fn events_can_be_delivered_by_name() {
    let h = harness(|b| b).await;
    let id = h.start(AwaitCourse { user_id: 3 }).await;
    let workflows: std::sync::Arc<Workflows> = h.workflows().await;
    let woken = workflows
        .deliver(
            "course.completed",
            &json!({"user_id": 3, "course_id": 1, "score": 12}),
        )
        .await
        .unwrap();
    assert_eq!(woken, 1);
    assert_eq!(
        workflows.run(id).await.unwrap().output,
        Some(json!(12))
    );
}

#[tokio::test]
async fn waiting_for_an_unregistered_event_fails_the_run_with_the_fix() {
    let h = harness(|b| b).await;
    let id = h.start(AwaitUnregistered).await;
    let run = h.workflows().await.run(id).await.unwrap();
    assert_eq!(run.status, RunStatus::Failed);
    let error = run.error.unwrap_or_default();
    assert!(
        error.contains("register it with m.workflow_event::<Unregistered>()"),
        "{error}"
    );
}
