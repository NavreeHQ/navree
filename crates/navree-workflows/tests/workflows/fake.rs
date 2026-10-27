//! `WorkflowsFake`: records starts instead of running them.

use std::sync::Arc;

use navree_workflows::{RunQuery, RunStatus, StartOptions, Workflows, WorkflowsFake};

use crate::support::{Greet, Onboarding, harness};

#[tokio::test]
async fn the_fake_records_what_app_code_starts() {
    let fake = WorkflowsFake::new();
    let h = harness(|b| b.override_value(Arc::new(fake.workflows()))).await;
    let workflows: Arc<Workflows> = h.workflows().await;
    let id = h.start(Greet { name: "Ada".into() }).await;
    workflows.spawn(Onboarding { user_id: 7 }).await.unwrap();
    let keyed = workflows
        .spawn_with(Onboarding { user_id: 8 }, StartOptions::new().key("u-8"))
        .await
        .unwrap();
    let again = workflows
        .spawn_with(Onboarding { user_id: 8 }, StartOptions::new().key("u-8"))
        .await
        .unwrap();
    assert_eq!(keyed, again);

    // Nothing ran.
    assert!(h.journal.calls().is_empty());
    fake.assert_started::<Greet>(|input| input.name == "Ada");
    fake.assert_started_times::<Onboarding>(2);
    assert_eq!(
        fake.started::<Onboarding>()
            .iter()
            .map(|o| o.user_id)
            .collect::<Vec<_>>(),
        [7, 8]
    );
    assert_eq!(fake.started_runs().len(), 3);

    // Reads see pending runs; cancel is recorded.
    assert_eq!(workflows.run(id).await.unwrap().status, RunStatus::Pending);
    assert_eq!(
        workflows
            .runs(&RunQuery::new().workflow("onboarding"))
            .await
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        workflows.cancel(id).await.unwrap().status,
        RunStatus::Cancelled
    );
    fake.assert_cancelled(id);
    assert_eq!(
        workflows.retry(id).await.unwrap_err().code(),
        Some("workflow_run_not_failed")
    );
    assert_eq!(workflows.tick().await.unwrap(), 0);

    fake.clear();
    fake.assert_nothing_started();
    fake.assert_not_started::<Greet>();
}

#[test]
#[should_panic(expected = "expected a start of workflow `greet` matching the predicate")]
fn a_failed_assertion_names_the_workflow() {
    WorkflowsFake::new().assert_started::<Greet>(|_| true);
}
