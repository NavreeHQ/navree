//! Both drivers, one suite: every scenario runs on the database stores
//! (`workflows.store = "database"`) and on the memory stores
//! (`workflows.store = "memory"`), so the two keep the same semantics:
//! units of work, leases, timers, waits, cancellation, keys, listing,
//! pruning and the automation records.

use std::time::Duration;

use navree_db::PageQuery;
use navree_db::{Backend, Executor};
use navree_workflows::{
    AutomationChanges, AutomationStatus, EnrollmentPolicy, Execution, NewAutomation, RunQuery,
    RunStatus, StartOptions, StepStatus, WorkflowsError,
};
use serde_json::json;

use crate::support::{
    AwaitCourse, CourseCompleted, Crashy, DAY, DealWon, Flaky, Greet, HOUR, Harness, Onboarding,
    follow_up, harness, start,
};

/// Runs `scenario` once per store driver.
macro_rules! conformance {
    ($($name:ident),* $(,)?) => {$(
        mod $name {
            #[tokio::test]
            async fn database() {
                super::$name(crate::support::harness(|b| b).await).await;
            }

            #[tokio::test]
            async fn memory() {
                super::$name(
                    crate::support::harness(|b| b.config("workflows.store", "memory")).await,
                )
                .await;
            }
        }
    )*};
}

conformance!(
    units_of_work,
    timers_and_retries,
    event_waits,
    cancellation_keys_and_listing,
    leases,
    automation_records,
);

#[tokio::test]
async fn the_harness_runs_on_the_configured_server() {
    let h = harness(|b| b).await;
    let expected = match crate::servers::Server::current() {
        crate::servers::Server::Sqlite => Backend::Sqlite,
        crate::servers::Server::Postgres => Backend::Postgres,
        crate::servers::Server::MySql => Backend::MySql,
    };
    assert_eq!(Executor::backend(&*h.db().await), expected);
}

/// A start applies when its unit of work commits, and never when it rolls
/// back.
async fn units_of_work(h: Harness) {
    let workflows = h.workflows().await;
    let uow = h.db().await.uow(None, None).await;
    let id = workflows
        .start(&uow, Greet { name: "Ada".into() })
        .await
        .unwrap();
    assert!(h.journal.calls().is_empty());
    uow.commit().await.unwrap();
    let run = workflows.run(id).await.unwrap();
    assert_eq!(run.status, RunStatus::Completed);
    assert_eq!(run.output, Some(json!("Hello, Ada")));
    assert_eq!(run.finished_at, Some(start()));
    let steps = workflows.details(id).await.unwrap().steps;
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].output, Some(json!("Hello, Ada")));
    assert_eq!(workflows.execute(id).await.unwrap(), Execution::Skipped);

    let uow = h.db().await.uow(None, None).await;
    let rolled_back = workflows
        .start(&uow, Greet { name: "Bo".into() })
        .await
        .unwrap();
    uow.rollback().await.unwrap();
    assert!(matches!(
        workflows.run(rolled_back).await,
        Err(WorkflowsError::UnknownRun(_))
    ));
    assert_eq!(h.journal.calls(), ["hello"]);
}

/// Sleeps wake on the clock; failed steps retry after their backoff, and a
/// failed run runs again on `retry`.
async fn timers_and_retries(h: Harness) {
    let workflows = h.workflows().await;
    let sleeper = h.start(Onboarding { user_id: 7 }).await;
    let run = workflows.run(sleeper).await.unwrap();
    assert_eq!(run.status, RunStatus::Sleeping);
    assert_eq!(run.wake_at, Some(start() + chrono::TimeDelta::days(3)));
    assert_eq!(h.advance(DAY).await, 0);
    assert_eq!(h.advance(2 * DAY).await, 1);
    assert_eq!(h.status(sleeper).await, RunStatus::Completed);
    assert_eq!(h.journal.calls(), ["welcome", "nudge"]);

    // Fails three times (10 s apart), then works on the retry.
    let flaky = h.start(Flaky { failures: 3 }).await;
    let step = &workflows.details(flaky).await.unwrap().steps[0];
    assert_eq!(step.status, StepStatus::Retrying);
    assert_eq!(h.advance(Duration::from_secs(10)).await, 1);
    assert_eq!(h.advance(Duration::from_secs(10)).await, 1);
    let run = workflows.run(flaky).await.unwrap();
    assert_eq!(run.status, RunStatus::Failed);
    assert_eq!(
        run.error.as_deref(),
        Some("step `charge` failed after 3 attempt(s): card declined (3)")
    );
    let retried = workflows.retry(flaky).await.unwrap();
    assert_eq!(retried.status, RunStatus::Completed);
    assert_eq!(retried.output, Some(json!(4)));
    let error = workflows.retry(flaky).await.unwrap_err();
    assert_eq!(error.code(), Some("workflow_run_not_failed"));
}

/// A matching event resolves a wait; another times out.
async fn event_waits(h: Harness) {
    let workflows = h.workflows().await;
    let met = h.start(AwaitCourse { user_id: 7 }).await;
    let missed = h.start(AwaitCourse { user_id: 8 }).await;
    assert_eq!(h.status(met).await, RunStatus::Waiting);
    h.emit(CourseCompleted {
        user_id: 7,
        course_id: 1,
        score: 90,
    })
    .await;
    let run = workflows.run(met).await.unwrap();
    assert_eq!(run.status, RunStatus::Completed);
    assert_eq!(run.output, Some(json!(90)));
    assert_eq!(h.status(missed).await, RunStatus::Waiting);
    assert_eq!(h.advance(14 * DAY).await, 1);
    let run = workflows.run(missed).await.unwrap();
    assert_eq!(run.status, RunStatus::Completed);
    assert_eq!(run.output, Some(json!(null)));
    assert_eq!(h.journal.calls(), ["congratulate", "remind"]);
}

/// Cancelled runs never wake; a key keeps one unfinished run; runs are
/// listed, filtered, counted and pruned.
async fn cancellation_keys_and_listing(h: Harness) {
    let workflows = h.workflows().await;
    let cancelled = h.start(Onboarding { user_id: 1 }).await;
    assert_eq!(
        workflows.cancel(cancelled).await.unwrap().status,
        RunStatus::Cancelled
    );
    let error = workflows.cancel(cancelled).await.unwrap_err();
    assert_eq!(error.code(), Some("workflow_run_finished"));

    h.clock.advance(Duration::from_secs(1));
    let key = || StartOptions::new().key("user-2");
    let keyed = workflows
        .spawn_with(Onboarding { user_id: 2 }, key())
        .await
        .unwrap();
    assert_eq!(
        workflows
            .spawn_with(Onboarding { user_id: 2 }, key())
            .await
            .unwrap(),
        keyed
    );
    h.clock.advance(Duration::from_secs(1));
    let workspace = uuid::Uuid::now_v7();
    let later = workflows
        .spawn_with(
            Greet {
                name: "later".into(),
            },
            StartOptions::new().delay(HOUR).workspace(workspace),
        )
        .await
        .unwrap();

    let all = workflows.runs(&RunQuery::new()).await.unwrap();
    assert_eq!(
        all.iter().map(|r| r.id).collect::<Vec<_>>(),
        [later, keyed, cancelled]
    );
    let sleeping = workflows
        .runs(&RunQuery::new().status(RunStatus::Sleeping))
        .await
        .unwrap();
    assert_eq!(sleeping.len(), 2);
    let greets = workflows
        .runs(&RunQuery::new().workflow("greet"))
        .await
        .unwrap();
    assert_eq!(greets.iter().map(|r| r.id).collect::<Vec<_>>(), [later]);
    assert_eq!(greets[0].workspace_id, Some(workspace));
    let page = workflows
        .page(&RunQuery::new(), PageQuery::new(2, 2))
        .await
        .unwrap();
    assert_eq!(page.meta.total, 3);
    assert_eq!(
        page.data.iter().map(|r| r.id).collect::<Vec<_>>(),
        [cancelled]
    );

    // The cancelled run finished at the start; nothing else did.
    h.clock.advance(DAY);
    let pruned = workflows
        .prune(start() + chrono::TimeDelta::seconds(1))
        .await
        .unwrap();
    assert_eq!(pruned, 1);
    assert!(workflows.run(cancelled).await.is_err());
    assert!(workflows.run(keyed).await.is_ok());
}

/// An execution holds its run for its lease; once the lease ran out, the
/// tick takes the run over.
async fn leases(h: Harness) {
    let workflows = h.workflows().await;
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
    h.journal.entered.notified().await;
    worker.abort();
    let _ = worker.await;
    h.journal.block(false);
    assert_eq!(h.status(id).await, RunStatus::Running);
    // The default lease is 300 s.
    assert_eq!(h.advance(Duration::from_secs(200)).await, 0);
    assert_eq!(h.advance(Duration::from_secs(101)).await, 1);
    assert_eq!(h.status(id).await, RunStatus::Completed);
    assert_eq!(h.journal.calls(), ["before", "block", "block", "after"]);
}

/// Automations, versions and enrollments: writes apply with their unit of
/// work (and reads through it see them first), rollbacks leave nothing.
async fn automation_records(h: Harness) {
    let automations = h.automations().await;

    // Created and published in one unit of work.
    let published = h
        .in_uow(|uow| {
            let automations = automations.clone();
            async move {
                let draft = automations
                    .create(
                        &uow,
                        NewAutomation::new("Follow-up", follow_up())
                            .enrollment(EnrollmentPolicy::EveryTime),
                    )
                    .await
                    .unwrap();
                automations.publish(&uow, draft.id, None).await.unwrap()
            }
        })
        .await;
    assert_eq!(published.status, AutomationStatus::Active);
    assert_eq!(published.version, Some(1));

    // A rolled-back create leaves nothing.
    let uow = h.db().await.uow(None, None).await;
    let discarded = automations
        .create(&uow, NewAutomation::new("Discarded", follow_up()))
        .await
        .unwrap();
    uow.rollback().await.unwrap();
    assert!(matches!(
        automations.get(discarded.id).await,
        Err(WorkflowsError::UnknownAutomation(_))
    ));

    // An update and a publish in one unit of work publish the update.
    let mut v2 = follow_up();
    v2["nodes"][4]["config"]["template"] = json!("feedback");
    let updated = h
        .in_uow(|uow| {
            let automations = automations.clone();
            async move {
                let changes = AutomationChanges {
                    definition: Some(v2),
                    ..AutomationChanges::default()
                };
                automations
                    .update(&uow, published.id, changes)
                    .await
                    .unwrap();
                automations.publish(&uow, published.id, None).await.unwrap()
            }
        })
        .await;
    assert_eq!(updated.version, Some(2));
    let versions = automations.versions(published.id).await.unwrap();
    assert_eq!(versions.len(), 2);
    assert_eq!(
        versions[0].definition.nodes[4].step,
        updated.definition.nodes[4].step
    );

    // Events enroll; enrollments are listed and counted.
    for owner_id in [7, 8] {
        h.emit(DealWon {
            deal_id: owner_id,
            owner_id,
            amount_cents: 1,
            workspace_id: None,
        })
        .await;
    }
    let page = automations
        .enrollments(published.id, PageQuery::new(1, 1))
        .await
        .unwrap();
    assert_eq!(page.meta.total, 2);
    assert_eq!(page.data[0].subject.as_deref(), Some("8"));
    assert_eq!(page.data[0].version, 2);
    let enrollment = automations.enrollment(page.data[0].id).await.unwrap();
    assert_eq!(enrollment.status, RunStatus::Sleeping);
    h.advance(DAY).await;
    assert!(h.journal.calls().contains(&"note:feedback:8".to_owned()));

    // Deleting removes the automation with its versions and enrollments.
    h.in_uow(|uow| {
        let automations = automations.clone();
        async move { automations.delete(&uow, published.id).await.unwrap() }
    })
    .await;
    assert!(automations.get(published.id).await.is_err());
    assert!(matches!(
        automations.enrollment(enrollment.id).await,
        Err(WorkflowsError::UnknownEnrollment(_))
    ));
}
