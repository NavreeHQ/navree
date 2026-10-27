//! Automations: the catalog, drafts validated against it, versions,
//! enrollment on events, enrollment policies, waits, simulations,
//! templates and deletion.

use navree_kernel::Module;
use navree_workflows::{
    AUTOMATION_WORKFLOW, Automation, AutomationChanges, AutomationQuery, AutomationStatus,
    AutomationTemplate, Automations, BlockKind, EnrollmentPolicy, ModuleBuilderWorkflowsExt,
    NewAutomation, RunStatus, SimulateInput, WaitOutcome, WorkflowsError,
};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::{DAY, DealWon, Harness, TaskCompleted, follow_up, harness};

fn deal(owner_id: u64, amount_cents: i64) -> DealWon {
    DealWon {
        deal_id: owner_id * 100,
        owner_id,
        amount_cents,
        workspace_id: None,
    }
}

/// Creates and publishes `definition`.
async fn publish(h: &Harness, definition: Value, policy: EnrollmentPolicy) -> Automation {
    let automations = h.automations().await;
    h.in_uow(|uow| async move {
        let draft = automations
            .create(
                &uow,
                NewAutomation::new("Follow-up", definition).enrollment(policy),
            )
            .await
            .unwrap();
        automations.publish(&uow, draft.id, None).await.unwrap()
    })
    .await
}

async fn enrollment_statuses(h: &Harness, id: Uuid) -> Vec<RunStatus> {
    h.automations()
        .await
        .enrollments(id, navree_db::PageQuery::default())
        .await
        .unwrap()
        .data
        .into_iter()
        .map(|e| e.status)
        .collect()
}

#[tokio::test]
async fn the_catalog_lists_blocks_with_their_schemas() {
    let h = harness(|b| b).await;
    let automations = h.automations().await;
    let catalog = automations.catalog();
    let triggers = catalog.triggers();
    assert_eq!(
        triggers.iter().map(|b| b.block_type.as_str()).collect::<Vec<_>>(),
        ["deal.won", "task.completed"]
    );
    let won = &triggers[0];
    assert_eq!(won.kind, BlockKind::Trigger);
    assert_eq!(won.label, "Deal won");
    assert_eq!(won.event.as_deref(), Some("deal.won"));
    assert_eq!(won.subject.as_deref(), Some("owner_id"));
    assert_eq!(
        won.schema["properties"]["min_amount_cents"]["description"],
        "Only deals worth at least this much."
    );
    // The label defaults to the type name; `field` is built in.
    assert_eq!(triggers[1].label, "task.completed");
    let conditions: Vec<String> = catalog
        .conditions()
        .into_iter()
        .map(|b| b.block_type)
        .collect();
    assert_eq!(conditions, ["deal.amount_at_least", "field"]);
    let actions = catalog.actions();
    assert_eq!(actions.len(), 2);
    assert_eq!(actions[1].block_type, "note.send");
    assert_eq!(actions[1].schema["required"], json!(["template"]));
    assert!(
        catalog
            .block(BlockKind::Action, "note.send")
            .is_some_and(|b| b.label == "Send a note")
    );
}

#[tokio::test]
async fn drafts_need_the_right_shape_and_publishing_needs_known_blocks() {
    let h = harness(|b| b).await;
    let automations = h.automations().await;
    let bad_shape = h
        .in_uow(|uow| {
            let automations = automations.clone();
            async move {
                automations
                    .create(&uow, NewAutomation::new("Broken", json!({"nodes": 3})))
                    .await
            }
        })
        .await
        .unwrap_err();
    let WorkflowsError::InvalidDefinition(issues) = bad_shape else {
        panic!("expected an invalid definition, got {bad_shape:?}");
    };
    assert_eq!(issues[0].path, "/definition");

    let definition = json!({
        "trigger": {"type": "deal.won", "config": {"min_amount_cents": "lots"}},
        "nodes": [
            {"id": "a", "kind": "action", "type": "nope", "next": "b"},
            {"id": "b", "kind": "delay", "seconds": 0, "next": "c"},
            {"id": "a", "kind": "action", "type": "note.send", "config": {}},
            {"id": "w", "kind": "wait", "trigger": {"type": "deal.won"},
             "timeout_seconds": 0, "next": "w"}
        ]
    });
    let draft = h
        .in_uow(|uow| {
            let automations = automations.clone();
            let definition = definition.clone();
            async move {
                automations
                    .create(&uow, NewAutomation::new("  Draft  ", definition))
                    .await
            }
        })
        .await
        .unwrap();
    assert_eq!(draft.name, "Draft");
    assert_eq!(draft.status, AutomationStatus::Draft);
    assert_eq!(draft.version, None);
    let details = automations.details(draft.id).await.unwrap();
    let issues: Vec<(String, String)> = details
        .issues
        .iter()
        .map(|i| (i.path.clone(), i.message.clone()))
        .collect();
    let paths: Vec<&str> = issues.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(
        paths,
        [
            "/trigger/config",
            "/nodes/0/type",
            "/nodes/1/seconds",
            "/nodes/2/id",
            "/nodes/2/config",
            "/nodes/3/timeout_seconds",
            "/nodes/1/next",
            "/nodes/3/next",
        ]
    );
    assert!(issues[1].1.contains("unknown action `nope`"));
    assert!(issues[3].1.contains("two nodes have the id `a`"));
    assert!(issues[6].1.contains("no node has the id `c`"));
    assert!(issues[7].1.contains("makes a loop"));
    assert_eq!(automations.validate(&definition).issues, details.issues);
    assert!(automations.validate(&follow_up()).valid);

    let error = h
        .in_uow(|uow| {
            let automations = automations.clone();
            async move { automations.publish(&uow, draft.id, None).await }
        })
        .await
        .unwrap_err();
    let WorkflowsError::InvalidDefinition(issues) = error else {
        panic!("expected an invalid definition");
    };
    assert_eq!(issues[0].path, "/definition/trigger/config");
    assert!(matches!(
        automations.get(Uuid::nil()).await,
        Err(WorkflowsError::UnknownAutomation(_))
    ));
}

#[tokio::test]
async fn a_published_automation_enrolls_matching_events() {
    let h = harness(|b| b).await;
    let automation = publish(&h, follow_up(), EnrollmentPolicy::EveryTime).await;
    assert_eq!(automation.status, AutomationStatus::Active);
    assert_eq!(automation.version, Some(1));
    assert_eq!(automation.trigger_event.as_deref(), Some("deal.won"));

    h.emit(deal(7, 250_000)).await;
    h.emit(deal(8, 5_000)).await;
    // The first action ran inline; both enrollments sleep for a day.
    assert_eq!(h.journal.calls(), ["note:thanks:7", "note:thanks:8"]);
    assert_eq!(
        enrollment_statuses(&h, automation.id).await,
        [RunStatus::Sleeping, RunStatus::Sleeping]
    );
    assert_eq!(h.advance(DAY).await, 2);
    let mut calls = h.journal.calls();
    calls.sort();
    assert_eq!(
        calls,
        [
            "note:call:7",
            "note:survey:8",
            "note:thanks:7",
            "note:thanks:8"
        ]
    );
    let page = h
        .automations()
        .await
        .enrollments(automation.id, navree_db::PageQuery::default())
        .await
        .unwrap();
    assert_eq!(page.meta.total, 2);
    let enrollment = &page.data[1];
    assert_eq!(enrollment.status, RunStatus::Completed);
    assert_eq!(enrollment.subject.as_deref(), Some("7"));
    assert_eq!(enrollment.version, 1);
    // The run carries the outputs of the nodes.
    let run = h.workflows().await.run(enrollment.run_id).await.unwrap();
    assert_eq!(run.workflow, AUTOMATION_WORKFLOW);
    assert_eq!(run.output.as_ref().unwrap()["big"], json!(true));
    assert_eq!(
        run.output.as_ref().unwrap()["thanks"],
        json!({"sent": "thanks", "to": "7"})
    );
    let steps = h.steps(enrollment.run_id).await;
    assert_eq!(steps["$definition"], "completed");
    assert_eq!(steps["wait-a-day"], "completed");
}

#[tokio::test]
async fn trigger_settings_filter_events() {
    let h = harness(|b| b).await;
    let mut definition = follow_up();
    definition["trigger"]["config"] = json!({"min_amount_cents": 100_000});
    let automation = publish(&h, definition, EnrollmentPolicy::EveryTime).await;
    h.emit(deal(1, 99_999)).await;
    assert!(enrollment_statuses(&h, automation.id).await.is_empty());
    h.emit(deal(1, 100_000)).await;
    assert_eq!(enrollment_statuses(&h, automation.id).await.len(), 1);
}

#[tokio::test]
async fn enrollment_policies_decide_who_enrolls_again() {
    let h = harness(|b| b).await;
    let one_at_a_time = publish(&h, follow_up(), EnrollmentPolicy::OneAtATime).await;
    let once = publish(&h, follow_up(), EnrollmentPolicy::Once).await;
    let every_time = publish(&h, follow_up(), EnrollmentPolicy::EveryTime).await;
    h.emit(deal(7, 1)).await;
    h.emit(deal(7, 1)).await;
    h.emit(deal(8, 1)).await;
    assert_eq!(enrollment_statuses(&h, one_at_a_time.id).await.len(), 2);
    assert_eq!(enrollment_statuses(&h, once.id).await.len(), 2);
    assert_eq!(enrollment_statuses(&h, every_time.id).await.len(), 3);
    // Once 7's enrollments finished, one-at-a-time enrolls 7 again; once
    // never does.
    h.advance(DAY).await;
    h.emit(deal(7, 1)).await;
    assert_eq!(enrollment_statuses(&h, one_at_a_time.id).await.len(), 3);
    assert_eq!(enrollment_statuses(&h, once.id).await.len(), 2);
    assert_eq!(enrollment_statuses(&h, every_time.id).await.len(), 4);
}

/// Thanks, then waits up to a week for the owner's next task.
fn wait_for_task() -> Value {
    json!({
        "trigger": {"type": "deal.won"},
        "nodes": [
            {"id": "thanks", "kind": "action", "type": "note.send",
             "config": {"template": "thanks"}, "next": "next-task"},
            {"id": "next-task", "kind": "wait", "trigger": {"type": "task.completed"},
             "timeout_seconds": 604800, "next": "praise", "on_timeout": "chase"},
            {"id": "praise", "kind": "action", "type": "note.send",
             "config": {"template": "praise"}},
            {"id": "chase", "kind": "action", "type": "note.send",
             "config": {"template": "chase"}}
        ]
    })
}

#[tokio::test]
async fn a_wait_node_waits_for_the_subjects_event_or_times_out() {
    let h = harness(|b| b).await;
    let automation = publish(&h, wait_for_task(), EnrollmentPolicy::EveryTime).await;
    h.emit(deal(7, 1)).await;
    h.emit(deal(8, 1)).await;
    assert_eq!(
        enrollment_statuses(&h, automation.id).await,
        [RunStatus::Waiting, RunStatus::Waiting]
    );
    // Another owner's task does not end 7's wait.
    h.emit(TaskCompleted {
        owner_id: 9,
        title: "x".into(),
    })
    .await;
    h.emit(TaskCompleted {
        owner_id: 7,
        title: "Call back".into(),
    })
    .await;
    assert!(h.journal.calls().contains(&"note:praise:7".to_owned()));
    assert!(!h.journal.calls().contains(&"note:praise:8".to_owned()));
    // 8's wait times out after a week.
    assert_eq!(h.advance(7 * DAY).await, 1);
    assert!(h.journal.calls().contains(&"note:chase:8".to_owned()));
    assert_eq!(
        enrollment_statuses(&h, automation.id).await,
        [RunStatus::Completed, RunStatus::Completed]
    );
}

#[tokio::test]
async fn enrollments_keep_their_version() {
    let h = harness(|b| b).await;
    let automation = publish(&h, follow_up(), EnrollmentPolicy::EveryTime).await;
    h.emit(deal(7, 1)).await;
    // Version 2 sends a different note after the day.
    let mut v2 = follow_up();
    v2["nodes"][4]["config"]["template"] = json!("feedback");
    let automations = h.automations().await;
    let updated = h
        .in_uow(|uow| {
            let automations = automations.clone();
            async move {
                let changes = AutomationChanges {
                    definition: Some(v2),
                    name: Some("Follow-up v2".into()),
                    ..AutomationChanges::default()
                };
                automations.update(&uow, automation.id, changes).await.unwrap();
                automations.publish(&uow, automation.id, None).await.unwrap()
            }
        })
        .await;
    assert_eq!(updated.version, Some(2));
    assert_eq!(updated.name, "Follow-up v2");
    h.emit(deal(8, 1)).await;
    h.advance(DAY).await;
    let calls = h.journal.calls();
    assert!(calls.contains(&"note:survey:7".to_owned()), "{calls:?}");
    assert!(calls.contains(&"note:feedback:8".to_owned()), "{calls:?}");
    let versions = automations.versions(automation.id).await.unwrap();
    assert_eq!(
        versions.iter().map(|v| v.version).collect::<Vec<_>>(),
        [2, 1]
    );
}

#[tokio::test]
async fn paused_automations_enroll_nobody() {
    let h = harness(|b| b).await;
    let automation = publish(&h, follow_up(), EnrollmentPolicy::EveryTime).await;
    let automations = h.automations().await;
    let paused = h
        .in_uow(|uow| {
            let automations = automations.clone();
            async move { automations.pause(&uow, automation.id).await.unwrap() }
        })
        .await;
    assert_eq!(paused.status, AutomationStatus::Paused);
    h.emit(deal(7, 1)).await;
    assert!(enrollment_statuses(&h, automation.id).await.is_empty());
    let error = h
        .in_uow(|uow| {
            let automations = automations.clone();
            async move { automations.pause(&uow, automation.id).await }
        })
        .await
        .unwrap_err();
    assert_eq!(error.code(), Some("automation_not_active"));
    h.in_uow(|uow| {
        let automations = automations.clone();
        async move { automations.resume(&uow, automation.id).await.unwrap() }
    })
    .await;
    h.emit(deal(7, 1)).await;
    assert_eq!(enrollment_statuses(&h, automation.id).await.len(), 1);
}

#[tokio::test]
async fn workspace_automations_enroll_their_workspace_events_only() {
    let h = harness(|b| b).await;
    let workspace = Uuid::now_v7();
    let automations = h.automations().await;
    let automation = h
        .in_uow(|uow| {
            let automations = automations.clone();
            async move {
                let draft = automations
                    .create(
                        &uow,
                        NewAutomation::new("Team follow-up", follow_up())
                            .workspace(workspace)
                            .enrollment(EnrollmentPolicy::EveryTime),
                    )
                    .await
                    .unwrap();
                automations.publish(&uow, draft.id, None).await.unwrap()
            }
        })
        .await;
    h.emit(deal(7, 1)).await;
    h.emit(DealWon {
        workspace_id: Some(Uuid::now_v7()),
        ..deal(7, 1)
    })
    .await;
    assert!(enrollment_statuses(&h, automation.id).await.is_empty());
    h.emit(DealWon {
        workspace_id: Some(workspace),
        ..deal(7, 1)
    })
    .await;
    let enrollments = automations
        .enrollments(automation.id, navree_db::PageQuery::default())
        .await
        .unwrap();
    assert_eq!(enrollments.data.len(), 1);
    assert_eq!(enrollments.data[0].workspace_id, Some(workspace));
    let run = h
        .workflows()
        .await
        .run(enrollments.data[0].run_id)
        .await
        .unwrap();
    assert_eq!(run.workspace_id, Some(workspace));
    let listed = automations
        .list(&AutomationQuery {
            scope: Some(Some(workspace)),
            ..AutomationQuery::default()
        })
        .await
        .unwrap();
    assert_eq!(listed.len(), 1);
    let app_wide = automations
        .list(&AutomationQuery {
            scope: Some(None),
            ..AutomationQuery::default()
        })
        .await
        .unwrap();
    assert!(app_wide.is_empty());
}

#[tokio::test]
async fn simulations_walk_the_path_without_running_actions() {
    let h = harness(|b| b).await;
    let automations = h.automations().await;
    let draft = h
        .in_uow(|uow| {
            let automations = automations.clone();
            async move {
                automations
                    .create(&uow, NewAutomation::new("Sim", follow_up()))
                    .await
                    .unwrap()
            }
        })
        .await;
    let event = json!({"deal_id": 1, "owner_id": 7, "amount_cents": 500_000, "workspace_id": null});
    let simulation = automations
        .simulate(
            draft.id,
            SimulateInput {
                event: event.clone(),
                ..SimulateInput::default()
            },
        )
        .await
        .unwrap();
    // A draft enrolls nobody.
    assert!(!simulation.enrolls);
    assert_eq!(simulation.reason.as_deref(), Some("the automation is draft"));
    assert_eq!(simulation.subject.as_deref(), Some("7"));
    let path: Vec<(&str, &str, u64)> = simulation
        .steps
        .iter()
        .map(|s| (s.node.as_str(), s.outcome.as_str(), s.at_seconds))
        .collect();
    assert_eq!(
        path,
        [
            ("thanks", "would_run", 0),
            ("wait-a-day", "waits", 0),
            ("big", "true", 86_400),
            ("call", "would_run", 86_400),
        ]
    );
    assert_eq!(
        simulation.steps[0].detail,
        json!({"would_send": "thanks", "to": "7"})
    );
    // Nothing ran, nothing was stored.
    assert!(h.journal.calls().is_empty());
    assert!(h.workflows().await.runs(&Default::default()).await.unwrap().is_empty());

    // A sample that is not the trigger's event.
    let error = automations
        .simulate(
            draft.id,
            SimulateInput {
                event: json!({"owner_id": "seven"}),
                ..SimulateInput::default()
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), Some("invalid_automation_event"));

    // Published: it would enroll; a wait that times out takes the other
    // path.
    let automation = publish(&h, wait_for_task(), EnrollmentPolicy::OneAtATime).await;
    let simulation = automations
        .simulate(
            automation.id,
            SimulateInput {
                event,
                version: Some(1),
                waits: WaitOutcome::TimedOut,
            },
        )
        .await
        .unwrap();
    assert!(simulation.enrolls);
    assert_eq!(simulation.version, Some(1));
    let path: Vec<(&str, &str, u64)> = simulation
        .steps
        .iter()
        .map(|s| (s.node.as_str(), s.outcome.as_str(), s.at_seconds))
        .collect();
    assert_eq!(
        path,
        [
            ("thanks", "would_run", 0),
            ("next-task", "timed_out", 0),
            ("chase", "would_run", 604_800),
        ]
    );
    let error = automations
        .simulate(
            automation.id,
            SimulateInput {
                event: json!({"deal_id": 1, "owner_id": 7, "amount_cents": 1}),
                version: Some(9),
                ..SimulateInput::default()
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), Some("unknown_automation_version"));
}

#[tokio::test]
async fn templates_start_drafts() {
    let h = harness(|b| b).await;
    let automations = h.automations().await;
    let templates = automations.templates();
    assert_eq!(templates.len(), 1);
    assert_eq!(templates[0].key, "big-deal-follow-up");
    assert_eq!(templates[0].category.as_deref(), Some("Sales"));
    let workspace = Uuid::now_v7();
    let draft = h
        .in_uow(|uow| {
            let automations = automations.clone();
            async move {
                automations
                    .create_from_template(&uow, "big-deal-follow-up", Some(workspace), None)
                    .await
                    .unwrap()
            }
        })
        .await;
    assert_eq!(draft.name, "Big deal follow-up");
    assert_eq!(draft.template.as_deref(), Some("big-deal-follow-up"));
    assert_eq!(draft.workspace_id, Some(workspace));
    assert_eq!(draft.status, AutomationStatus::Draft);
    let error = h
        .in_uow(|uow| {
            let automations = automations.clone();
            async move {
                automations
                    .create_from_template(&uow, "nope", None, Some("x".into()))
                    .await
            }
        })
        .await
        .unwrap_err();
    assert!(matches!(error, WorkflowsError::UnknownTemplate(_)));
}

#[tokio::test]
async fn an_invalid_template_fails_the_boot() {
    struct BadTemplate;
    impl Module for BadTemplate {
        fn register(&self, m: &mut navree_kernel::ModuleBuilder) {
            m.automation_template(AutomationTemplate::new(
                "broken",
                "Broken",
                json!({"trigger": {"type": "nope"}, "nodes": []}),
            ));
        }
    }
    let database = crate::support::TestDatabase::new().await;
    let result = crate::support::builder(
        &database,
        std::sync::Arc::new(navree_contracts::FixedClock::new(crate::support::start())),
        std::sync::Arc::default(),
    )
    .module(BadTemplate)
    .try_build()
    .await;
    // The engine is built lazily: resolving it reports the template.
    let error = match result {
        Err(error) => error.to_string(),
        Ok(app) => app
            .ctx()
            .get::<Automations>()
            .await
            .map(|_| ())
            .unwrap_err()
            .to_string(),
    };
    assert!(
        error.contains("automation template `broken` is invalid"),
        "{error}"
    );
}

#[tokio::test]
async fn manual_enrollment_and_cancellation() {
    let h = harness(|b| b).await;
    let automations = h.automations().await;
    let draft = h
        .in_uow(|uow| {
            let automations = automations.clone();
            async move {
                automations
                    .create(&uow, NewAutomation::new("Manual", follow_up()))
                    .await
                    .unwrap()
            }
        })
        .await;
    let error = automations
        .enroll(draft.id, json!({"deal_id": 1, "owner_id": 3, "amount_cents": 1}))
        .await
        .unwrap_err();
    assert_eq!(error.code(), Some("automation_not_published"));
    let automation = publish(&h, follow_up(), EnrollmentPolicy::Once).await;
    let enrollment = automations
        .enroll(
            automation.id,
            json!({"deal_id": 1, "owner_id": 3, "amount_cents": 1}),
        )
        .await
        .unwrap();
    assert_eq!(enrollment.status, RunStatus::Sleeping);
    assert_eq!(enrollment.subject.as_deref(), Some("3"));
    assert_eq!(h.journal.calls(), ["note:thanks:3"]);
    let cancelled = automations.cancel_enrollment(enrollment.id).await.unwrap();
    assert_eq!(cancelled.status, RunStatus::Cancelled);
    let error = automations.cancel_enrollment(enrollment.id).await.unwrap_err();
    assert_eq!(error.code(), Some("enrollment_finished"));
    assert!(matches!(
        automations.enrollment(Uuid::nil()).await,
        Err(WorkflowsError::UnknownEnrollment(_))
    ));
    let error = automations
        .enroll(automation.id, json!({"owner_id": "x"}))
        .await
        .unwrap_err();
    assert_eq!(error.code(), Some("invalid_automation_event"));
}

#[tokio::test]
async fn deleting_an_automation_cancels_its_enrollments() {
    let h = harness(|b| b).await;
    let automation = publish(&h, follow_up(), EnrollmentPolicy::EveryTime).await;
    h.emit(deal(7, 1)).await;
    let automations = h.automations().await;
    let run_id = automations
        .enrollments(automation.id, navree_db::PageQuery::default())
        .await
        .unwrap()
        .data[0]
        .run_id;
    h.in_uow(|uow| {
        let automations = automations.clone();
        async move { automations.delete(&uow, automation.id).await.unwrap() }
    })
    .await;
    assert_eq!(h.status(run_id).await, RunStatus::Cancelled);
    assert!(matches!(
        automations.get(automation.id).await,
        Err(WorkflowsError::UnknownAutomation(_))
    ));
    h.advance(DAY).await;
    assert_eq!(h.journal.calls(), ["note:thanks:7"]);
}

#[tokio::test]
async fn a_failing_action_fails_the_enrollment() {
    let h = harness(|b| b).await;
    let definition = json!({
        "trigger": {"type": "deal.won"},
        "nodes": [{"id": "boom", "kind": "action", "type": "explode", "config": {}}]
    });
    let automation = publish(&h, definition, EnrollmentPolicy::EveryTime).await;
    h.emit(deal(7, 1)).await;
    // Three attempts with the default backoff (10 s, then 20 s).
    h.advance(std::time::Duration::from_secs(10)).await;
    h.advance(std::time::Duration::from_secs(20)).await;
    let enrollment = h
        .automations()
        .await
        .enrollments(automation.id, navree_db::PageQuery::default())
        .await
        .unwrap()
        .data
        .remove(0);
    assert_eq!(enrollment.status, RunStatus::Failed);
    assert_eq!(
        enrollment.error.as_deref(),
        Some("step `boom` failed after 3 attempt(s): the action failed")
    );
}
