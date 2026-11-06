//! The HTTP routes: `/api/workflows/...` (runs) and `/api/automations/...`
//! (the editor's catalog, drafts, versions, simulation, enrollments),
//! their permissions and their error shapes.

use navree_access::{AbilityCtx, AccessModule, AccessOptions, Rules};
use navree_contracts::{AuthVia, Principal};
use navree_testing::TestClient;
use navree_workflows::{RunStatus, StartOptions};
use navree_workspace::events::WorkspaceDeleted;
use navree_workspace::{NewWorkspace, WorkspaceModule, Workspaces};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::{DAY, Flaky, Greet, Harness, Onboarding, follow_up, harness};

/// Holds every workflows and automations permission.
const ADMIN: Uuid = Uuid::from_u128(1);
/// Holds the view permissions only.
const VIEWER: Uuid = Uuid::from_u128(2);
/// Owns the workspaces of the workspace test.
const OWNER: Uuid = Uuid::from_u128(3);

fn abilities(ctx: &AbilityCtx, rules: &mut Rules) {
    let Some(user) = &ctx.user else {
        return;
    };
    let permissions: &[&str] = if user.id == ADMIN {
        &[
            "workflows.view",
            "workflows.manage",
            "automations.view",
            "automations.manage",
        ]
    } else if user.id == VIEWER {
        &["workflows.view", "automations.view"]
    } else {
        &[]
    };
    let permissions: Vec<String> = permissions.iter().map(|p| (*p).to_owned()).collect();
    rules.from_permissions(&permissions, &ctx.permission_subjects);
}

async fn routed() -> Harness {
    harness(|b| {
        b.config("workflows.routes", true)
            .config("workflows.automation_routes", true)
            .module(AccessModule::for_root(AccessOptions {
                abilities,
                ..AccessOptions::default()
            }))
    })
    .await
}

fn as_user(h: &Harness, user: Uuid) -> TestClient {
    h.app.acting_as(Principal {
        user_id: user,
        via: AuthVia::Test,
        token_abilities: None,
        session_id: None,
    })
}

fn ids(page: &Value) -> Vec<String> {
    page["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn the_runs_page_lists_shows_cancels_and_retries() {
    let h = routed().await;
    let done = h.start(Greet { name: "Anu".into() }).await;
    let sleeping = h.start(Onboarding { user_id: 7 }).await;
    let admin = as_user(&h, ADMIN);
    let viewer = as_user(&h, VIEWER);

    let index = viewer
        .get("/api/workflows")
        .send()
        .await
        .assert_status(200)
        .json_value();
    let workflows = index["data"].as_array().unwrap();
    // The tests' workflows and the built-in `automation`.
    assert!(workflows.iter().any(|w| w["name"] == "onboarding"));
    assert!(workflows.iter().any(|w| w["name"] == "automation"));
    let greet = workflows
        .iter()
        .find(|w| w["name"] == "greet")
        .cloned()
        .unwrap();
    assert_eq!(
        greet,
        json!({"name": "greet", "version": 1, "min_version": 1, "description": "Says hello."})
    );

    let page = viewer
        .get("/api/workflows/runs?status=sleeping,waiting")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(ids(&page), [sleeping.to_string()]);
    assert_eq!(page["meta"]["total"], 1);
    let page = viewer
        .get("/api/workflows/runs?workflow=greet")
        .send()
        .await
        .json_value();
    assert_eq!(ids(&page), [done.to_string()]);
    viewer
        .get("/api/workflows/runs?status=asleep")
        .send()
        .await
        .assert_error_code(400, "validation_failed");

    let shown = viewer
        .get(&format!("/api/workflows/runs/{done}"))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(shown["status"], "completed");
    assert_eq!(shown["output"], "Hello, Anu");
    assert_eq!(shown["steps"][0]["key"], "hello");
    viewer
        .get(&format!("/api/workflows/runs/{}", Uuid::nil()))
        .send()
        .await
        .assert_error_code(404, "workflow_run_not_found");

    // Changes need workflows.manage.
    viewer
        .post(&format!("/api/workflows/runs/{sleeping}/cancel"))
        .send()
        .await
        .assert_status(403);
    h.app
        .client()
        .get("/api/workflows/runs")
        .send()
        .await
        .assert_status(401);
    let cancelled = admin
        .post(&format!("/api/workflows/runs/{sleeping}/cancel"))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(cancelled["status"], "cancelled");
    admin
        .post(&format!("/api/workflows/runs/{sleeping}/cancel"))
        .send()
        .await
        .assert_error_code(409, "workflow_run_finished");
    admin
        .post(&format!("/api/workflows/runs/{done}/retry"))
        .send()
        .await
        .assert_error_code(409, "workflow_run_not_failed");

    // A failed run runs again.
    let flaky = h.start(Flaky { failures: 3 }).await;
    h.advance(std::time::Duration::from_secs(10)).await;
    h.advance(std::time::Duration::from_secs(10)).await;
    let retried = admin
        .post(&format!("/api/workflows/runs/{flaky}/retry"))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(retried["status"], "completed");
}

#[tokio::test]
async fn the_editor_builds_publishes_simulates_and_enrolls() {
    let h = routed().await;
    let admin = as_user(&h, ADMIN);
    let viewer = as_user(&h, VIEWER);

    // The palette: blocks with their JSON Schemas, and the node kinds.
    let catalog = viewer
        .get("/api/automations/catalog")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(
        catalog["nodes"],
        json!(["action", "condition", "delay", "wait"])
    );
    assert_eq!(catalog["triggers"][0]["type"], "deal.won");
    assert_eq!(catalog["triggers"][0]["kind"], "trigger");
    assert_eq!(
        catalog["triggers"][0]["schema"]["properties"]["min_amount_cents"]["description"],
        "Only deals worth at least this much."
    );
    assert_eq!(
        catalog["actions"][1]["schema"]["required"],
        json!(["template"])
    );
    let templates = viewer
        .get("/api/automations/templates")
        .send()
        .await
        .json_value();
    assert_eq!(templates["data"][0]["key"], "big-deal-follow-up");

    // Validation answers with JSON pointers.
    let checked = viewer
        .post("/api/automations/validate")
        .json(&json!({"definition": {"trigger": {"type": "nope"}, "nodes": []}}))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(checked["valid"], false);
    assert_eq!(checked["issues"][0]["path"], "/trigger/type");

    // Drafts: a bad shape is a 400; a good one is created.
    admin
        .post("/api/automations")
        .json(&json!({"name": "Broken", "definition": {"nodes": 1}}))
        .send()
        .await
        .assert_error_code(400, "validation_failed");
    viewer
        .post("/api/automations")
        .json(&json!({"name": "No", "definition": follow_up()}))
        .send()
        .await
        .assert_status(403);
    let created = admin
        .post("/api/automations")
        .json(&json!({"name": "Follow-up", "definition": follow_up(), "enrollment": "every_time"}))
        .send()
        .await
        .assert_status(201);
    let draft = created.json_value();
    let id = draft["id"].as_str().unwrap().to_owned();
    assert_eq!(
        created.header("location"),
        Some(format!("/api/automations/{id}").as_str())
    );
    assert_eq!(draft["status"], "draft");
    let shown = viewer
        .get(&format!("/api/automations/{id}"))
        .send()
        .await
        .json_value();
    assert_eq!(shown["issues"], json!([]));
    let renamed = admin
        .patch(&format!("/api/automations/{id}"))
        .json(&json!({"name": "Deal follow-up"}))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(renamed["name"], "Deal follow-up");
    let listed = viewer.get("/api/automations").send().await.json_value();
    assert_eq!(ids(&listed), std::slice::from_ref(&id));

    // A simulation of the draft walks the path without running anything.
    let event = json!({"deal_id": 1, "owner_id": 7, "amount_cents": 500_000, "workspace_id": null});
    let simulation = viewer
        .post(&format!("/api/automations/{id}/simulate"))
        .json(&json!({"event": event}))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(simulation["enrolls"], false);
    let path: Vec<&str> = simulation["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["node"].as_str().unwrap())
        .collect();
    assert_eq!(path, ["thanks", "wait-a-day", "big", "call"]);
    assert!(h.journal.calls().is_empty());

    // Publish, enroll by hand, read and cancel the enrollment.
    admin
        .post(&format!("/api/automations/{id}/enroll"))
        .json(&json!({"event": event}))
        .send()
        .await
        .assert_error_code(409, "automation_not_published");
    let published = admin
        .post(&format!("/api/automations/{id}/publish"))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(published["status"], "active");
    assert_eq!(published["version"], 1);
    let versions = viewer
        .get(&format!("/api/automations/{id}/versions"))
        .send()
        .await
        .json_value();
    assert_eq!(versions["data"][0]["version"], 1);
    assert_eq!(versions["data"][0]["published_by"], ADMIN.to_string());
    let enrolled = admin
        .post(&format!("/api/automations/{id}/enroll"))
        .json(&json!({"event": event}))
        .send()
        .await
        .assert_status(201);
    let enrollment = enrolled.json_value();
    let enrollment_id = enrollment["id"].as_str().unwrap().to_owned();
    assert_eq!(
        enrolled.header("location"),
        Some(format!("/api/automations/{id}/enrollments/{enrollment_id}").as_str())
    );
    assert_eq!(enrollment["status"], "sleeping");
    assert_eq!(h.journal.calls(), ["note:thanks:7"]);
    admin
        .post(&format!("/api/automations/{id}/enroll"))
        .json(&json!({"event": {"owner_id": "seven"}}))
        .send()
        .await
        .assert_error_code(400, "invalid_automation_event");
    let page = viewer
        .get(&format!("/api/automations/{id}/enrollments"))
        .send()
        .await
        .json_value();
    assert_eq!(ids(&page), std::slice::from_ref(&enrollment_id));
    let one = viewer
        .get(&format!(
            "/api/automations/{id}/enrollments/{enrollment_id}"
        ))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(one["subject"], "7");
    let cancelled = admin
        .post(&format!(
            "/api/automations/{id}/enrollments/{enrollment_id}/cancel"
        ))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(cancelled["status"], "cancelled");
    viewer
        .get(&format!(
            "/api/automations/{id}/enrollments/{}",
            Uuid::nil()
        ))
        .send()
        .await
        .assert_error_code(404, "enrollment_not_found");

    // Pause and resume; then delete.
    let paused = admin
        .post(&format!("/api/automations/{id}/pause"))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(paused["status"], "paused");
    admin
        .post(&format!("/api/automations/{id}/pause"))
        .send()
        .await
        .assert_error_code(409, "automation_not_active");
    admin
        .post(&format!("/api/automations/{id}/resume"))
        .send()
        .await
        .assert_status(200);
    admin
        .delete(&format!("/api/automations/{id}"))
        .send()
        .await
        .assert_status(204);
    viewer
        .get(&format!("/api/automations/{id}"))
        .send()
        .await
        .assert_error_code(404, "automation_not_found");
    h.advance(DAY).await;
    assert_eq!(h.journal.calls(), ["note:thanks:7"]);
}

#[tokio::test]
async fn a_template_starts_a_draft_over_http() {
    let h = routed().await;
    let admin = as_user(&h, ADMIN);
    let created = admin
        .post("/api/automations/templates/big-deal-follow-up")
        .json(&json!({"name": "Our follow-up"}))
        .send()
        .await
        .assert_status(201);
    let draft = created.json_value();
    assert_eq!(draft["name"], "Our follow-up");
    assert_eq!(draft["template"], "big-deal-follow-up");
    assert_eq!(
        created.header("location"),
        Some(format!("/api/automations/{}", draft["id"].as_str().unwrap()).as_str())
    );
    admin
        .post("/api/automations/templates/nope")
        .send()
        .await
        .assert_error_code(404, "automation_template_not_found");
}

#[tokio::test]
async fn in_an_app_with_workspaces_the_routes_act_on_the_request_workspace() {
    let h = harness(|b| {
        b.config("workflows.routes", true)
            .config("workflows.automation_routes", true)
            .config("workspace.users_table", "")
            .module(AccessModule::for_root(AccessOptions {
                abilities: |ctx, rules| ctx.grant_permissions(rules),
                ..AccessOptions::default()
            }))
            .module(WorkspaceModule::from_config())
    })
    .await;
    let workspaces = h.app.get::<Workspaces>().await;
    let (agra, gulati) = h
        .in_uow(|uow| async move {
            let mut created = Vec::new();
            for slug in ["agra", "gulati"] {
                let workspace = workspaces
                    .create(
                        &uow,
                        OWNER,
                        NewWorkspace {
                            name: slug.to_owned(),
                            slug: Some(slug.to_owned()),
                        },
                    )
                    .await
                    .unwrap();
                created.push(workspace.id);
            }
            (created[0], created[1])
        })
        .await;
    // OWNER owns both workspaces: every permission in them.
    let owner = as_user(&h, OWNER);

    let created = owner
        .post("/api/automations")
        .header("X-Workspace", "agra")
        .json(&json!({"name": "Agra follow-up", "definition": follow_up()}))
        .send()
        .await
        .assert_status(201)
        .json_value();
    assert_eq!(created["workspace_id"], agra.to_string());
    let id = created["id"].as_str().unwrap().to_owned();
    let in_gulati = owner
        .get("/api/automations")
        .header("X-Workspace", "gulati")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(in_gulati["data"], json!([]));
    owner
        .get(&format!("/api/automations/{id}"))
        .header("X-Workspace", "gulati")
        .send()
        .await
        .assert_error_code(404, "automation_not_found");
    owner
        .get(&format!("/api/automations/{id}"))
        .header("X-Workspace", "agra")
        .send()
        .await
        .assert_status(200);

    // Runs too: a run of agra is listed and shown in agra only.
    let run = h
        .workflows()
        .await
        .spawn_with(
            Onboarding { user_id: 1 },
            StartOptions::new().workspace(agra),
        )
        .await
        .unwrap();
    let listed = owner
        .get("/api/workflows/runs")
        .header("X-Workspace", "agra")
        .send()
        .await
        .json_value();
    assert_eq!(ids(&listed), [run.to_string()]);
    let listed = owner
        .get("/api/workflows/runs")
        .header("X-Workspace", "gulati")
        .send()
        .await
        .json_value();
    assert_eq!(listed["data"], json!([]));
    owner
        .post(&format!("/api/workflows/runs/{run}/cancel"))
        .header("X-Workspace", "gulati")
        .send()
        .await
        .assert_error_code(404, "workflow_run_not_found");
    // A workspace is required; a stranger is refused.
    owner
        .get("/api/workflows/runs")
        .send()
        .await
        .assert_status(400);
    as_user(&h, VIEWER)
        .get("/api/automations")
        .header("X-Workspace", "agra")
        .send()
        .await
        .assert_status(403);

    // Deleting agra cancels its runs and deletes its automations.
    h.emit(WorkspaceDeleted {
        workspace_id: agra,
        slug: "agra".to_owned(),
    })
    .await;
    assert_eq!(h.status(run).await, RunStatus::Cancelled);
    assert!(
        h.automations()
            .await
            .get(id.parse().unwrap())
            .await
            .is_err()
    );
    let _ = gulati;
}

#[tokio::test]
async fn routes_are_off_unless_enabled() {
    let h = harness(|b| b).await;
    let admin = as_user(&h, ADMIN);
    admin.get("/api/workflows").send().await.assert_status(404);
    admin
        .get("/api/automations/catalog")
        .send()
        .await
        .assert_status(404);
}
