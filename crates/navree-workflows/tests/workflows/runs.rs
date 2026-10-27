//! Durable runs: steps replayed on resume, timers on the app's clock,
//! retries, failures, crashes, cancellation, deduplication and versions.

use std::sync::Arc;
use std::time::Duration;

use navree_workflows::{
    Execution, RunQuery, RunStatus, StartOptions, StepKind, StepStatus, Workflow, WorkflowsError,
};
use serde_json::json;

use crate::support::{
    Compensating, Crashy, DAY, Fanout, Flaky, Greet, HOUR, Onboarding, Panicky, UpgradeModule,
    UpgradeV1, UpgradeV2, UpgradeV3, harness, harness_on, start,
};

#[tokio::test]
async fn a_run_starts_when_its_unit_of_work_commits() {
    let h = harness(|b| b).await;
    let workflows = h.workflows().await;
    let uow = h.db().await.uow(None, None).await;
    let id = workflows
        .start(&uow, Greet { name: "Ada".into() })
        .await
        .unwrap();
    // Inside the transaction, nothing ran yet.
    assert!(h.journal.calls().is_empty());
    uow.commit().await.unwrap();
    // No job dispatcher: the run executed inline after the commit.
    let run = workflows.run(id).await.unwrap();
    assert_eq!(run.status, RunStatus::Completed);
    assert_eq!(run.output, Some(json!("Hello, Ada")));
    assert_eq!(run.workflow, "greet");
    assert_eq!(run.version, 1);
    assert_eq!(run.finished_at, Some(start()));
    assert_eq!(h.journal.calls(), ["hello"]);
    let details = workflows.details(id).await.unwrap();
    assert_eq!(details.steps.len(), 1);
    assert_eq!(details.steps[0].key, "hello");
    assert_eq!(details.steps[0].kind, StepKind::Step);
    assert_eq!(details.steps[0].output, Some(json!("Hello, Ada")));
}

#[tokio::test]
async fn a_rolled_back_start_never_runs() {
    let h = harness(|b| b).await;
    let workflows = h.workflows().await;
    let uow = h.db().await.uow(None, None).await;
    let id = workflows
        .start(&uow, Greet { name: "Ada".into() })
        .await
        .unwrap();
    uow.rollback().await.unwrap();
    assert!(matches!(
        workflows.run(id).await,
        Err(WorkflowsError::UnknownRun(_))
    ));
    assert!(h.journal.calls().is_empty());
}

#[tokio::test]
async fn sleeps_wait_for_the_clock_and_steps_are_not_run_again() {
    let h = harness(|b| b).await;
    let id = h.start(Onboarding { user_id: 7 }).await;
    let workflows = h.workflows().await;
    let run = workflows.run(id).await.unwrap();
    assert_eq!(run.status, RunStatus::Sleeping);
    assert_eq!(run.wake_at, Some(start() + chrono::TimeDelta::days(3)));
    assert_eq!(h.journal.calls(), ["welcome"]);

    // A day later nothing is due.
    assert_eq!(h.advance(DAY).await, 0);
    assert_eq!(h.status(id).await, RunStatus::Sleeping);

    // Two more days: the run replays `welcome` (not run again) and goes on.
    assert_eq!(h.advance(2 * DAY).await, 1);
    let run = workflows.run(id).await.unwrap();
    assert_eq!(run.status, RunStatus::Completed);
    assert_eq!(h.journal.calls(), ["welcome", "nudge"]);
    assert_eq!(
        run.output,
        Some(json!(start() + chrono::TimeDelta::days(3)))
    );
    let steps = h.steps(id).await;
    assert_eq!(steps["$sleep"], "completed");
    assert_eq!(steps["$now"], "completed");
    // Executing a finished run does nothing.
    assert_eq!(workflows.execute(id).await.unwrap(), Execution::Skipped);
}

#[tokio::test]
async fn a_failed_step_is_retried_after_its_backoff() {
    let h = harness(|b| b).await;
    let id = h.start(Flaky { failures: 2 }).await;
    let workflows = h.workflows().await;
    let run = workflows.run(id).await.unwrap();
    assert_eq!(run.status, RunStatus::Sleeping);
    assert_eq!(run.wake_at, Some(start() + chrono::TimeDelta::seconds(10)));
    let details = workflows.details(id).await.unwrap();
    assert_eq!(details.steps[0].status, StepStatus::Retrying);
    assert_eq!(details.steps[0].attempts, 1);
    assert_eq!(details.steps[0].error.as_deref(), Some("card declined (1)"));

    assert_eq!(h.advance(Duration::from_secs(5)).await, 0);
    assert_eq!(h.advance(Duration::from_secs(5)).await, 1);
    assert_eq!(h.status(id).await, RunStatus::Sleeping);
    assert_eq!(h.advance(Duration::from_secs(10)).await, 1);
    let run = workflows.run(id).await.unwrap();
    assert_eq!(run.status, RunStatus::Completed);
    assert_eq!(run.output, Some(json!(3)));
    let step = &workflows.details(id).await.unwrap().steps[0];
    assert_eq!(step.attempts, 3);
    assert_eq!(step.status, StepStatus::Completed);
}

#[tokio::test]
async fn a_step_out_of_attempts_fails_the_run_and_retry_runs_it_again() {
    let h = harness(|b| b).await;
    let id = h.start(Flaky { failures: 5 }).await;
    h.advance(Duration::from_secs(10)).await;
    h.advance(Duration::from_secs(10)).await;
    let workflows = h.workflows().await;
    let run = workflows.run(id).await.unwrap();
    assert_eq!(run.status, RunStatus::Failed);
    assert_eq!(
        run.error.as_deref(),
        Some("step `charge` failed after 3 attempt(s): card declined (3)")
    );
    assert_eq!(h.journal.count("charge"), 3);

    // Only a failed run can be retried.
    let greet = h.start(Greet { name: "x".into() }).await;
    let error = workflows.retry(greet).await.unwrap_err();
    assert_eq!(error.code(), Some("workflow_run_not_failed"));

    // Retry forgets the failed step: three fresh attempts (4, 5 fail; 6
    // goes through).
    let run = workflows.retry(id).await.unwrap();
    assert_eq!(run.status, RunStatus::Sleeping);
    assert_eq!(run.error, None);
    h.advance(Duration::from_secs(10)).await;
    h.advance(Duration::from_secs(10)).await;
    let run = workflows.run(id).await.unwrap();
    assert_eq!(run.status, RunStatus::Completed);
    assert_eq!(run.output, Some(json!(6)));
}

#[tokio::test]
async fn a_workflow_can_catch_a_failed_step_and_compensate() {
    let h = harness(|b| b).await;
    let id = h.start(Compensating).await;
    let run = h.workflows().await.run(id).await.unwrap();
    assert_eq!(run.status, RunStatus::Completed);
    assert_eq!(run.output, Some(json!("refunded after: card declined")));
    assert_eq!(h.journal.calls(), ["refund"]);
    assert_eq!(h.steps(id).await["charge"], "failed");
}

#[tokio::test]
async fn a_panic_fails_the_run() {
    let h = harness(|b| b).await;
    let id = h.start(Panicky).await;
    let run = h.workflows().await.run(id).await.unwrap();
    assert_eq!(run.status, RunStatus::Failed);
    assert_eq!(run.error.as_deref(), Some("the workflow panicked: boom"));
}

#[tokio::test]
async fn concurrent_and_repeated_steps_get_their_own_keys() {
    let h = harness(|b| b).await;
    let id = h.start(Fanout).await;
    let run = h.workflows().await.run(id).await.unwrap();
    assert_eq!(run.output, Some(json!([1, 2, 10, 11, 12])));
    let keys: Vec<String> = h.steps(id).await.into_keys().collect();
    assert_eq!(keys, ["left", "repeat", "repeat#2", "repeat#3", "right"]);
}

#[tokio::test]
async fn a_crashed_execution_is_taken_over_once_its_lease_runs_out() {
    let h = harness(|b| b.config("workflows.lease_secs", 60)).await;
    let workflows = h.workflows().await;
    // Started a second from now, so the start does not run it inline.
    let id = workflows
        .spawn_with(Crashy, StartOptions::new().delay(Duration::from_secs(1)))
        .await
        .unwrap();
    h.clock.advance(Duration::from_secs(1));
    h.journal.block(true);
    let worker = {
        let workflows = workflows.clone();
        tokio::spawn(async move { workflows.execute(id).await })
    };
    // The worker is inside `block`; then its process dies.
    h.journal.entered.notified().await;
    worker.abort();
    let _ = worker.await;
    assert_eq!(h.status(id).await, RunStatus::Running);
    assert_eq!(h.journal.calls(), ["before", "block"]);

    // Its lease still holds: nobody takes the run over.
    h.journal.block(false);
    assert_eq!(h.advance(Duration::from_secs(30)).await, 0);
    assert_eq!(h.status(id).await, RunStatus::Running);

    // Once the lease ran out, the tick resumes it: `before` is replayed,
    // `block` (which never finished) runs again, then `after`.
    assert_eq!(h.advance(Duration::from_secs(31)).await, 1);
    assert_eq!(h.status(id).await, RunStatus::Completed);
    assert_eq!(h.journal.calls(), ["before", "block", "block", "after"]);
}

#[tokio::test]
async fn cancelled_runs_never_wake() {
    let h = harness(|b| b).await;
    let id = h.start(Onboarding { user_id: 1 }).await;
    let workflows = h.workflows().await;
    let run = workflows.cancel(id).await.unwrap();
    assert_eq!(run.status, RunStatus::Cancelled);
    assert_eq!(run.finished_at, Some(start()));
    assert_eq!(h.advance(4 * DAY).await, 0);
    assert_eq!(h.journal.calls(), ["welcome"]);
    let error = workflows.cancel(id).await.unwrap_err();
    assert_eq!(error.code(), Some("workflow_run_finished"));
    assert!(matches!(
        workflows.cancel(uuid::Uuid::nil()).await,
        Err(WorkflowsError::UnknownRun(_))
    ));
}

#[tokio::test]
async fn a_key_keeps_one_unfinished_run() {
    let h = harness(|b| b).await;
    let workflows = h.workflows().await;
    let options = || StartOptions::new().key("user-7");
    let first = workflows
        .spawn_with(Onboarding { user_id: 7 }, options())
        .await
        .unwrap();
    let again = workflows
        .spawn_with(Onboarding { user_id: 7 }, options())
        .await
        .unwrap();
    assert_eq!(first, again);
    assert_eq!(h.journal.count("welcome"), 1);
    h.advance(3 * DAY).await;
    assert_eq!(h.status(first).await, RunStatus::Completed);
    let next = workflows
        .spawn_with(Onboarding { user_id: 7 }, options())
        .await
        .unwrap();
    assert_ne!(next, first);
    assert_eq!(
        workflows.run(next).await.unwrap().key.as_deref(),
        Some("user-7")
    );
}

#[tokio::test]
async fn a_delayed_start_waits() {
    let h = harness(|b| b).await;
    let workflows = h.workflows().await;
    let workspace = uuid::Uuid::now_v7();
    let id = workflows
        .spawn_with(
            Greet {
                name: "later".into(),
            },
            StartOptions::new().delay(HOUR).workspace(workspace),
        )
        .await
        .unwrap();
    let run = workflows.run(id).await.unwrap();
    assert_eq!(run.status, RunStatus::Sleeping);
    assert_eq!(run.workspace_id, Some(workspace));
    assert_eq!(h.advance(HOUR / 2).await, 0);
    assert_eq!(h.advance(HOUR / 2).await, 1);
    assert_eq!(h.status(id).await, RunStatus::Completed);
}

#[tokio::test]
async fn runs_are_listed_and_filtered() {
    let h = harness(|b| b).await;
    let workflows = h.workflows().await;
    let a = h.start(Greet { name: "a".into() }).await;
    h.clock.advance(Duration::from_secs(1));
    let b = h.start(Onboarding { user_id: 1 }).await;
    h.clock.advance(Duration::from_secs(1));
    let c = h.start(Greet { name: "c".into() }).await;
    let all = workflows.runs(&RunQuery::new()).await.unwrap();
    assert_eq!(all.iter().map(|r| r.id).collect::<Vec<_>>(), [c, b, a]);
    let greets = workflows
        .runs(&RunQuery::new().workflow("greet"))
        .await
        .unwrap();
    assert_eq!(greets.len(), 2);
    let sleeping = workflows
        .runs(&RunQuery::new().status(RunStatus::Sleeping))
        .await
        .unwrap();
    assert_eq!(sleeping.iter().map(|r| r.id).collect::<Vec<_>>(), [b]);
    let page = workflows
        .page(&RunQuery::new(), navree_db::PageQuery::new(2, 2))
        .await
        .unwrap();
    assert_eq!(page.meta.total, 3);
    assert_eq!(page.data.iter().map(|r| r.id).collect::<Vec<_>>(), [a]);
}

#[tokio::test]
async fn starting_an_unregistered_workflow_names_the_fix() {
    #[derive(serde::Serialize, serde::Deserialize)]
    struct Stray;
    impl Workflow for Stray {
        const NAME: &'static str = "stray";
        type Output = ();
        async fn run(
            self,
            _ctx: navree_workflows::WorkflowCtx,
        ) -> Result<(), navree_kernel::BoxError> {
            Ok(())
        }
    }
    let h = harness(|b| b).await;
    let error = h.workflows().await.spawn(Stray).await.unwrap_err();
    assert!(
        error
            .to_string()
            .contains("import WorkflowsModule::for_feature::<Stray>()"),
        "{error}"
    );
}

#[tokio::test]
async fn runs_resume_on_the_version_they_started_on() {
    // Deploy 1 knows version 1 only.
    let first = harness(|b| b.module(UpgradeModule(&[1]))).await;
    let old = first.start(UpgradeV1).await;
    assert_eq!(first.status(old).await, RunStatus::Sleeping);
    let (database, clock, journal) = (
        first.database.clone(),
        first.clock.clone(),
        first.journal.clone(),
    );
    first.app.shutdown().await.unwrap();

    // Deploy 2 ships version 2, which resumes version 1 runs.
    let second = harness_on(database, clock, journal, |b| b.module(UpgradeModule(&[2]))).await;
    let new = second.start(UpgradeV2).await;
    second.advance(HOUR).await;
    let workflows = second.workflows().await;
    let old_run = workflows.run(old).await.unwrap();
    assert_eq!(old_run.version, 1);
    assert_eq!(old_run.output, Some(json!("v2 ran a version 1 run")));
    let new_run = workflows.run(new).await.unwrap();
    assert_eq!(new_run.version, 2);
    assert_eq!(new_run.output, Some(json!("v2 ran a version 2 run")));
    // `a` ran once per run: replays never ran it again.
    assert_eq!(second.journal.count("a"), 2);
    assert_eq!(second.journal.count("b-v1"), 1);
    assert_eq!(second.journal.count("b-new"), 1);
}

#[tokio::test]
async fn an_app_may_keep_several_versions_registered() {
    let h = harness(|b| b.module(UpgradeModule(&[1, 3]))).await;
    let v1 = h.start(UpgradeV1).await;
    let v3 = h.start(UpgradeV3).await;
    h.advance(HOUR).await;
    let workflows = h.workflows().await;
    assert_eq!(
        workflows.run(v1).await.unwrap().output,
        Some(json!("v1 ran a version 1 run"))
    );
    assert_eq!(
        workflows.run(v3).await.unwrap().output,
        Some(json!("v3 ran a version 3 run"))
    );
    let names: Vec<(String, u32)> = workflows
        .definitions()
        .into_iter()
        .filter(|d| d.name == "upgrade")
        .map(|d| (d.name.to_owned(), d.version))
        .collect();
    assert_eq!(names, [("upgrade".into(), 3), ("upgrade".into(), 1)]);
}

#[tokio::test]
async fn runs_no_registered_version_resumes_wait_or_fail() {
    let first = harness(|b| b.module(UpgradeModule(&[1, 3]))).await;
    let old = first.start(UpgradeV1).await;
    let newer = first.start(UpgradeV3).await;
    let (database, clock, journal) = (
        first.database.clone(),
        first.clock.clone(),
        first.journal.clone(),
    );
    first.app.shutdown().await.unwrap();

    // A deploy with only version 2: the version 3 run waits for a deploy
    // that has it; the version 1 run is resumed by version 2.
    let second = harness_on(database.clone(), clock.clone(), journal.clone(), |b| {
        b.module(UpgradeModule(&[2]))
    })
    .await;
    second.advance(HOUR).await;
    assert_eq!(second.status(old).await, RunStatus::Completed);
    let run = second.workflows().await.run(newer).await.unwrap();
    assert_eq!(run.status, RunStatus::Sleeping);
    second.app.shutdown().await.unwrap();

    // A deploy with only version 3 (MIN_VERSION 2): a fresh version 1 run
    // fails, naming the fix.
    let third = harness_on(database, clock, journal, |b| b.module(UpgradeModule(&[3]))).await;
    third.advance(Duration::from_secs(600)).await;
    assert_eq!(third.status(newer).await, RunStatus::Completed);
    let fourth_run = {
        let fourth = harness(|b| b.module(UpgradeModule(&[1]))).await;
        let id = fourth.start(UpgradeV1).await;
        let (database, clock, journal) = (
            fourth.database.clone(),
            fourth.clock.clone(),
            fourth.journal.clone(),
        );
        fourth.app.shutdown().await.unwrap();
        let only_three =
            harness_on(database, clock, journal, |b| b.module(UpgradeModule(&[3]))).await;
        only_three.advance(HOUR).await;
        only_three.workflows().await.run(id).await.unwrap()
    };
    assert_eq!(fourth_run.status, RunStatus::Failed);
    assert!(
        fourth_run
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("no registered implementation of workflow `upgrade` resumes version 1"),
        "{:?}",
        fourth_run.error
    );
}

#[tokio::test]
async fn prune_deletes_finished_runs_only() {
    let h = harness(|b| b).await;
    let done = h.start(Greet { name: "a".into() }).await;
    let sleeping = h.start(Onboarding { user_id: 1 }).await;
    h.clock.advance(DAY);
    let workflows = h.workflows().await;
    assert_eq!(workflows.prune(start()).await.unwrap(), 0);
    let pruned = workflows
        .prune(start() + chrono::TimeDelta::hours(1))
        .await
        .unwrap();
    assert_eq!(pruned, 1);
    assert!(workflows.run(done).await.is_err());
    assert!(workflows.run(sleeping).await.is_ok());
    let _ = Arc::clone(&workflows);
}

#[tokio::test]
async fn the_tick_runs_a_start_whose_wake_up_was_lost() {
    let h = harness(|b| b).await;
    // A run stored without being woken (a crash right after the commit).
    let store = navree_workflows::DatabaseWorkflowStore::new((*h.db().await).clone());
    let run = navree_workflows::WorkflowRun {
        id: uuid::Uuid::now_v7(),
        workflow: "greet".into(),
        version: 1,
        status: RunStatus::Pending,
        input: json!({"name": "lost"}),
        output: None,
        error: None,
        workspace_id: None,
        key: None,
        wake_at: None,
        created_at: start(),
        updated_at: start(),
        finished_at: None,
    };
    navree_workflows::WorkflowStore::insert_run(&store, None, &run)
        .await
        .unwrap();
    // Within a lease its wake-up may still come.
    assert_eq!(h.advance(Duration::from_secs(60)).await, 0);
    assert_eq!(h.advance(Duration::from_secs(240)).await, 1);
    assert_eq!(h.status(run.id).await, RunStatus::Completed);
}
