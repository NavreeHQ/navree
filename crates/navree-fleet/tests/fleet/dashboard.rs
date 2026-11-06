//! The dashboard: its pages and its JSON API, behind the `Fleet` abilities
//! `view` and `operate`. (`pages.rs` signs real people in on the session.)

use navree_access::{AbilityCtx, AccessModule, AccessOptions, AccessService, Rules};
use navree_contracts::{AuthVia, Principal};
use navree_db::Db;
use navree_fleet::{CompletedJob, REDACTED};
use navree_queue::{AttemptOutcome, WorkOptions};
use navree_testing::TestAppBuilder;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::{Export, Harness, Record, builder};

const VIEWER: Uuid = Uuid::from_u128(1);
const OPERATOR: Uuid = Uuid::from_u128(2);
const MANAGER: Uuid = Uuid::from_u128(3);
const OTHER: Uuid = Uuid::from_u128(4);

/// Code-granted abilities for the viewer and the operator; everyone else
/// has the permissions stored for them (the manager: `fleet.view`).
fn abilities(ctx: &AbilityCtx, rules: &mut Rules) {
    ctx.grant_permissions(rules);
    match ctx.user.as_ref().map(|user| user.id) {
        Some(id) if id == VIEWER => {
            rules.can("view", "Fleet");
        }
        Some(id) if id == OPERATOR => {
            rules.can("view", "Fleet");
            rules.can("operate", "Fleet");
        }
        _ => {}
    }
}

fn principal(user_id: Uuid) -> Principal {
    Principal {
        user_id,
        via: AuthVia::Test,
        token_abilities: None,
        session_id: None,
    }
}

fn dashboard_builder() -> TestAppBuilder {
    builder()
        .config("fleet.dashboard", true)
        .config("http.per_ip_rate_limit", Value::Null)
        .module(AccessModule::for_root(AccessOptions {
            abilities,
            ..AccessOptions::default()
        }))
}

async fn dashboard() -> Harness {
    Harness::new(dashboard_builder()).await
}

async fn fail_one_export(h: &Harness) -> String {
    let id = h
        .queue
        .dispatch(Export {
            contact: "ada".into(),
            password: "hunter2".into(),
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
    id
}

#[tokio::test]
async fn the_pages_need_the_view_ability_and_the_api_keeps_answering_json() {
    let h = dashboard().await;
    // A guest has no way in; a person without the ability is refused.
    h.app
        .client()
        .get("/_fleet")
        .send()
        .await
        .assert_status(401);
    h.app
        .acting_as(principal(OTHER))
        .get("/_fleet")
        .send()
        .await
        .assert_status(403);
    // A viewer gets the page: HTML, with no script of its own to run.
    let page = h
        .app
        .acting_as(principal(VIEWER))
        .get("/_fleet")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(200);
    assert!(
        page.header("content-type")
            .is_some_and(|kind| kind.starts_with("text/html")),
        "{:?}",
        page.header("content-type")
    );
    let csp = page.header("content-security-policy").unwrap();
    assert!(csp.contains("script-src"), "{csp}");
    assert!(!csp.contains("unsafe-inline"), "{csp}");
    let html = page.text();
    assert!(html.contains("Fleet"), "{html}");
    assert!(!html.contains("<script>"), "no inline script: {html}");
    // The old assets are gone.
    h.app
        .client()
        .get("/_fleet/assets/fleet.js")
        .send()
        .await
        .assert_status(404);
    // The API answers in JSON even to a client that asks for HTML.
    let api = h
        .app
        .client()
        .get("/_fleet/api/overview")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(401);
    assert!(
        !api.header("content-type")
            .unwrap_or_default()
            .starts_with("text/html"),
        "{:?}",
        api.header("content-type")
    );
}

#[tokio::test]
async fn reading_needs_the_view_ability() {
    let h = dashboard().await;
    h.app
        .client()
        .get("/_fleet/api/overview")
        .send()
        .await
        .assert_status(401);
    h.app
        .acting_as(principal(OTHER))
        .get("/_fleet/api/overview")
        .send()
        .await
        .assert_status(403);
    let viewer = h.app.acting_as(principal(VIEWER));
    viewer
        .get("/_fleet/api/overview")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("status", json!("inactive"))
        .assert_json_path("workload.0.queue", json!("default"));
    viewer
        .get("/_fleet/api/masters")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data", json!([]));
    viewer
        .get("/_fleet/api/metrics?period=day")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("period", json!("day"))
        .assert_json_path("queues", json!([]));
    viewer
        .get("/_fleet/api/metrics?period=year")
        .send()
        .await
        .assert_status(400);
}

#[tokio::test]
async fn a_stored_fleet_view_permission_opens_the_dashboard() {
    let h = dashboard().await;
    let access = h.app.get::<AccessService>().await;
    let db = h.app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    access
        .grant_to_user(&uow, MANAGER, "fleet.view")
        .await
        .unwrap();
    uow.commit().await.unwrap();
    let manager = h.app.acting_as(principal(MANAGER));
    manager
        .get("/_fleet/api/overview")
        .send()
        .await
        .assert_status(200);
    manager
        .post("/_fleet/api/pause")
        .send()
        .await
        .assert_status(403);
}

#[tokio::test]
async fn jobs_are_listed_with_redacted_payloads() {
    let h = dashboard().await;
    let id = h.queue.dispatch(Record::new("secretive")).await.unwrap();
    h.queue
        .dispatch(crate::support::SendDigest {
            user: "ada".into(),
            api_token: "tok_123".into(),
        })
        .await
        .unwrap();
    h.fleet
        .record(
            None,
            &[CompletedJob {
                job_id: "done-1".into(),
                queue: "default".into(),
                job_type: "test.record".into(),
                attempt: 1,
                outcome: AttemptOutcome::Succeeded,
                runtime_ms: 12,
                finished_at: h.fleet.now(),
            }],
        )
        .await
        .unwrap();
    let viewer = h.app.acting_as(principal(VIEWER));
    let jobs = viewer
        .get("/_fleet/api/jobs")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(jobs["pending"].as_array().unwrap().len(), 2);
    assert_eq!(jobs["pending"][1]["payload"]["api_token"], json!(REDACTED));
    assert_eq!(jobs["completed"][0]["job_id"], json!("done-1"));
    assert_eq!(jobs["completed"][0]["outcome"], json!("succeeded"));
    viewer
        .get("/_fleet/api/jobs?search=mail")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("pending.0.queue", json!("mail"));
    viewer
        .get(&format!("/_fleet/api/jobs/{id}"))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("payload.label", json!("secretive"))
        .assert_json_path("state", json!("waiting"));
    viewer
        .get("/_fleet/api/jobs/missing")
        .send()
        .await
        .assert_status(404);
}

#[tokio::test]
async fn failed_jobs_are_read_by_viewers_and_handled_by_operators() {
    let h = dashboard().await;
    let first = fail_one_export(&h).await;
    let second = fail_one_export(&h).await;
    let viewer = h.app.acting_as(principal(VIEWER));
    let operator = h.app.acting_as(principal(OPERATOR));

    let failed = viewer
        .get("/_fleet/api/failed?search=mail%20server")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(failed["data"].as_array().unwrap().len(), 2);
    assert_eq!(failed["data"][0]["payload"]["password"], json!(REDACTED));
    assert_eq!(failed["data"][0]["payload"]["contact"], json!("ada"));
    assert_eq!(failed["data"][0]["error"], json!("the mail server is down"));
    viewer
        .get(&format!("/_fleet/api/failed/{first}"))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("job_type", json!("contacts.export"));

    for (method, path) in [
        ("POST", format!("/_fleet/api/failed/{first}/retry")),
        ("POST", "/_fleet/api/failed/retry".to_owned()),
        ("DELETE", format!("/_fleet/api/failed/{first}")),
        ("POST", "/_fleet/api/pause".to_owned()),
        ("POST", "/_fleet/api/continue".to_owned()),
    ] {
        let request = match method {
            "POST" => viewer.post(&path),
            _ => viewer.delete(&path),
        };
        request.send().await.assert_status(403);
    }

    operator
        .post(&format!("/_fleet/api/failed/{first}/retry"))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("retried", json!(1));
    assert_eq!(h.queue.pending_count("exports").await.unwrap(), 1);
    operator
        .post(&format!("/_fleet/api/failed/{first}/retry"))
        .send()
        .await
        .assert_status(404);
    operator
        .delete(&format!("/_fleet/api/failed/{second}"))
        .send()
        .await
        .assert_status(204);
    operator
        .delete(&format!("/_fleet/api/failed/{second}"))
        .send()
        .await
        .assert_status(404);

    fail_one_export(&h).await;
    operator
        .post("/_fleet/api/failed/retry?search=contacts.export")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("retried", json!(2));
    assert!(h.fleet.failed_jobs(None).await.unwrap().is_empty());
}

#[tokio::test]
async fn operators_pause_and_continue_supervisors() {
    let h = dashboard().await;
    let operator = h.app.acting_as(principal(OPERATOR));
    operator
        .post("/_fleet/api/pause")
        .json(&json!({ "supervisor": "default" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("paused", json!(["default"]));
    operator
        .post("/_fleet/api/pause")
        .json(&json!({ "supervisor": "nope" }))
        .send()
        .await
        .assert_status(404);
    operator
        .post("/_fleet/api/continue")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("paused", json!([]));
    operator
        .post("/_fleet/api/pause")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("paused", json!(["default"]));
}

#[tokio::test]
async fn the_path_is_configurable_and_the_dashboard_can_be_off() {
    let h = Harness::new(dashboard_builder().config("fleet.path", "/ops/fleet/")).await;
    h.app
        .acting_as(principal(VIEWER))
        .get("/ops/fleet")
        .send()
        .await
        .assert_status(200);
    h.app
        .acting_as(principal(VIEWER))
        .get("/ops/fleet/failed")
        .send()
        .await
        .assert_status(200);
    h.app
        .acting_as(principal(VIEWER))
        .get("/ops/fleet/api/overview")
        .send()
        .await
        .assert_status(200);
    h.app
        .acting_as(principal(VIEWER))
        .get("/_fleet")
        .send()
        .await
        .assert_status(404);

    let off = Harness::new(builder()).await;
    off.app
        .client()
        .get("/_fleet")
        .send()
        .await
        .assert_status(404);
    off.app
        .acting_as(principal(VIEWER))
        .get("/_fleet/failed")
        .send()
        .await
        .assert_status(404);
    off.app
        .acting_as(principal(VIEWER))
        .get("/_fleet/api/overview")
        .send()
        .await
        .assert_status(404);
}
