//! The `/api/webhooks` routes: access, endpoints, the log and workspace
//! scoping.

use std::sync::Arc;

use http::Method;
use navree_access::{AbilityCtx, AccessModule, AccessOptions, MANAGE, Rules};
use navree_contracts::{AuthVia, FixedClock, Principal};
use navree_db::Db;
use navree_http_client::{FakeResponse, HttpClient};
use navree_testing::TestApp;
use navree_webhooks::StandardWebhooks;
use navree_workspace::{NewWorkspace, WorkspaceModule, Workspaces};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::{Harness, builder, start};

const ADMIN: Uuid = Uuid::from_u128(1);
const OTHER: Uuid = Uuid::from_u128(2);

/// The admin manages webhooks app-wide; everyone else has only the
/// permissions of their roles (in a workspace: their workspace role's).
fn abilities(ctx: &AbilityCtx, rules: &mut Rules) {
    ctx.grant_permissions(rules);
    if ctx.user.as_ref().is_some_and(|user| user.id == ADMIN) {
        rules.can(MANAGE, "webhooks");
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

async fn routes_app(workspaces: bool) -> Harness {
    let (client, server) = HttpClient::fake();
    server.on(
        Method::POST,
        "https://hooks.lovelace.io/*",
        FakeResponse::new(200).text("ok"),
    );
    let clock = Arc::new(FixedClock::new(start()));
    let mut app = builder(client, clock.clone())
        .config("webhooks.routes", true)
        .config("workspace.users_table", "")
        .module(AccessModule::for_root(AccessOptions {
            abilities,
            ..AccessOptions::default()
        }));
    if workspaces {
        app = app.module(WorkspaceModule::from_config());
    }
    Harness {
        app: app.build().await,
        server,
        clock,
    }
}

#[tokio::test]
async fn every_route_requires_webhooks_manage() {
    let h = routes_app(false).await;
    h.app
        .client()
        .get("/api/webhooks/endpoints")
        .send()
        .await
        .assert_status(401);
    h.app
        .acting_as(principal(OTHER))
        .get("/api/webhooks/endpoints")
        .send()
        .await
        .assert_status(403);
    h.app
        .acting_as(principal(OTHER))
        .post("/api/webhooks/endpoints")
        .json(&json!({ "url": "https://hooks.lovelace.io/x", "events": ["*"] }))
        .send()
        .await
        .assert_status(403);
}

#[tokio::test]
async fn endpoints_are_managed_over_the_api() {
    let h = routes_app(false).await;
    let admin = h.app.acting_as(principal(ADMIN));

    admin
        .get("/api/webhooks/event-types")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.1.name", json!("deal.won"))
        .assert_json_path("data.1.description", json!("A deal moved to won."));

    let created = admin
        .post("/api/webhooks/endpoints")
        .json(&json!({
            "url": "https://hooks.lovelace.io/navree",
            "events": ["deal.won"],
            "description": "Revenue dashboard",
            "headers": { "X-Team": "revenue" }
        }))
        .send()
        .await
        .assert_status(201)
        .json_value();
    let id = created["id"].as_str().unwrap().to_owned();
    let secret = created["secret"].as_str().unwrap().to_owned();
    assert!(secret.starts_with("whsec_"));
    assert_eq!(created["events"], json!(["deal.won"]));
    assert_eq!(created["workspace_id"], Value::Null);

    admin
        .post("/api/webhooks/endpoints")
        .json(&json!({ "url": "http://hooks.lovelace.io/x", "events": ["*"] }))
        .send()
        .await
        .assert_validation_error("url");
    admin
        .post("/api/webhooks/endpoints")
        .json(&json!({ "url": "https://hooks.lovelace.io/x", "events": ["nope"] }))
        .send()
        .await
        .assert_validation_error("events");

    let listed = admin
        .get("/api/webhooks/endpoints")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(listed["data"].as_array().unwrap().len(), 1);
    let shown = admin
        .get(&format!("/api/webhooks/endpoints/{id}"))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("description", json!("Revenue dashboard"))
        .assert_json_path("headers.x-team", json!("revenue"))
        .json_value();
    assert!(shown.get("secret").is_none(), "the secret is shown once");

    admin
        .patch(&format!("/api/webhooks/endpoints/{id}"))
        .json(&json!({ "events": ["*"], "enabled": false }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("events", json!(["*"]))
        .assert_json_path("enabled", json!(false))
        .assert_json_path("disabled_reason", json!("manual"));
    admin
        .patch(&format!("/api/webhooks/endpoints/{id}"))
        .json(&json!({ "enabled": true }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("enabled", json!(true));

    let rotated = admin
        .post(&format!("/api/webhooks/endpoints/{id}/rotate-secret"))
        .send()
        .await
        .assert_status(200)
        .json_value();
    let new_secret = rotated["secret"].as_str().unwrap().to_owned();
    assert_ne!(new_secret, secret);

    admin
        .post(&format!("/api/webhooks/endpoints/{id}/test"))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("succeeded", json!(true))
        .assert_json_path("status_code", json!(200))
        .assert_json_path("response_body", json!("ok"));
    let request = h.server.recorded().pop().unwrap();
    StandardWebhooks::verify(&new_secret, request.headers(), request.body(), None).unwrap();
    StandardWebhooks::verify(&secret, request.headers(), request.body(), None).unwrap();

    let page = admin
        .get(&format!("/api/webhooks/endpoints/{id}/deliveries"))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("meta.total", json!(1))
        .assert_json_path("data.0.event_type", json!("webhook.test"))
        .assert_json_path("data.0.status", json!("succeeded"))
        .json_value();
    let delivery = page["data"][0]["id"].as_str().unwrap().to_owned();
    admin
        .post(&format!("/api/webhooks/deliveries/{delivery}/retry"))
        .send()
        .await
        .assert_status(409);

    admin
        .delete(&format!("/api/webhooks/endpoints/{id}"))
        .send()
        .await
        .assert_status(204);
    admin
        .get(&format!("/api/webhooks/endpoints/{id}"))
        .send()
        .await
        .assert_status(404);
    admin
        .post(&format!("/api/webhooks/deliveries/{delivery}/retry"))
        .send()
        .await
        .assert_status(404);
}

#[tokio::test]
async fn failed_deliveries_are_retried_over_the_api() {
    let h = routes_app(false).await;
    h.server.on(
        Method::POST,
        "https://hooks.gone.io/*",
        FakeResponse::new(410),
    );
    let admin = h.app.acting_as(principal(ADMIN));
    let id = admin
        .post("/api/webhooks/endpoints")
        .json(&json!({ "url": "https://hooks.gone.io/x", "events": ["*"] }))
        .send()
        .await
        .assert_status(201)
        .json_value()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    admin
        .post(&format!("/api/webhooks/endpoints/{id}/test"))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("succeeded", json!(false))
        .assert_json_path("status_code", json!(410));
    let delivery = admin
        .get(&format!("/api/webhooks/endpoints/{id}/deliveries"))
        .send()
        .await
        .json_value()["data"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    admin
        .post(&format!("/api/webhooks/deliveries/{delivery}/retry"))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("status", json!("pending"))
        .assert_json_path("max_attempts", json!(2));
    // A test message never disables its endpoint.
    admin
        .get(&format!("/api/webhooks/endpoints/{id}"))
        .send()
        .await
        .assert_json_path("enabled", json!(true));
}

#[tokio::test]
async fn in_an_app_with_workspaces_the_routes_act_on_the_request_workspace() {
    let h = routes_app(true).await;
    let workspaces = h.app.get::<Workspaces>().await;
    let uow = h.app.get::<Db>().await.uow(None, None).await;
    let acme = workspaces
        .create(
            &uow,
            OTHER,
            NewWorkspace {
                name: "Acme".to_owned(),
                slug: Some("acme".to_owned()),
            },
        )
        .await
        .unwrap();
    let globex = workspaces
        .create(
            &uow,
            OTHER,
            NewWorkspace {
                name: "Globex".to_owned(),
                slug: Some("globex".to_owned()),
            },
        )
        .await
        .unwrap();
    uow.commit().await.unwrap();
    // OTHER owns both workspaces, so it holds every permission in them.
    let owner = h.app.acting_as(principal(OTHER));

    let created = owner
        .post("/api/webhooks/endpoints")
        .header("X-Workspace", "acme")
        .json(&json!({ "url": "https://hooks.lovelace.io/acme", "events": ["*"] }))
        .send()
        .await
        .assert_status(201)
        .assert_json_path("workspace_id", json!(acme.id))
        .json_value();
    let id = created["id"].as_str().unwrap().to_owned();

    owner
        .get("/api/webhooks/endpoints")
        .header("X-Workspace", "globex")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data", json!([]));
    owner
        .get(&format!("/api/webhooks/endpoints/{id}"))
        .header("X-Workspace", "globex")
        .send()
        .await
        .assert_status(404);
    owner
        .get(&format!("/api/webhooks/endpoints/{id}"))
        .header("X-Workspace", "acme")
        .send()
        .await
        .assert_status(200);
    owner
        .get("/api/webhooks/endpoints")
        .send()
        .await
        .assert_status(400);
    // ADMIN (app-wide `webhooks.manage`) is not a member of acme.
    h.app
        .acting_as(principal(ADMIN))
        .get("/api/webhooks/endpoints")
        .header("X-Workspace", "acme")
        .send()
        .await
        .assert_status(403);
    let _ = globex;
}

#[tokio::test]
async fn routes_are_not_mounted_when_turned_off() {
    let (client, _server) = HttpClient::fake();
    let app: TestApp = builder(client, Arc::new(FixedClock::new(start())))
        .build()
        .await;
    app.acting_as(principal(ADMIN))
        .get("/api/webhooks/endpoints")
        .send()
        .await
        .assert_status(404);
}
