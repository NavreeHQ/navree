//! The `/api/integrations` routes: access, the connect flow over HTTP,
//! workspace scoping, organization fallback and the key uploads.

use navree_api::ApiModule;
use navree_integrations::{FakeGrant, Owner};
use serde_json::{Value, json};

use crate::support::{
    ADMIN, DEPUTY, OTHER, harness, principal, query_param, service_account_json, with_routes,
};

const TENANT: &str = "9188040d-6c67-4c5b-b112-36a304b66dad";

#[tokio::test]
async fn every_route_requires_integrations_manage() {
    let h = harness(with_routes(false)).await;
    for path in [
        "/api/integrations/providers",
        "/api/integrations/connections",
        "/api/integrations/microsoft/connect",
    ] {
        h.app.client().get(path).send().await.assert_status(401);
        h.app
            .acting_as(principal(OTHER))
            .get(path)
            .send()
            .await
            .assert_error_code(403, "forbidden");
    }
    h.app
        .acting_as(principal(OTHER))
        .post("/api/integrations/edr/api-key")
        .json(&json!({ "key": "k" }))
        .send()
        .await
        .assert_status(403);
}

#[tokio::test]
async fn an_admin_connects_checks_and_disconnects_over_the_api() {
    let h = harness(with_routes(false)).await;
    let admin = h.app.acting_as(principal(ADMIN));

    let providers = admin
        .get("/api/integrations/providers")
        .send()
        .await
        .assert_status(200)
        .json_value();
    let keys: Vec<&str> = providers["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, vec!["edr", "google", "hubspot", "microsoft", "slack"]);
    assert_eq!(providers["data"][3]["type"], json!("microsoft"));
    assert_eq!(providers["data"][3]["kinds"], json!(["app_only"]));
    assert_eq!(
        providers["data"][3]["scope_sets"][1],
        json!({
            "name": "directory",
            "description": "Read users and groups",
            "scopes": [
                "https://graph.microsoft.com/User.Read.All",
                "https://graph.microsoft.com/Group.Read.All"
            ]
        })
    );

    let redirect = admin
        .get("/api/integrations/microsoft/connect?return_to=/settings/integrations")
        .send()
        .await
        .assert_status(200)
        .json_value();
    let state = redirect["state"].as_str().unwrap().to_owned();
    assert_eq!(
        query_param(redirect["url"].as_str().unwrap(), "state"),
        Some(state.clone())
    );

    // The front end forwards the provider's query string.
    let callback = format!(
        "/api/integrations/microsoft/callback?tenant={TENANT}&state={state}&admin_consent=True&scope=https%3A%2F%2Fgraph.microsoft.com%2F.default"
    );
    let created = admin.get(&callback).send().await.assert_status(201);
    let id = created.json_value()["id"].as_str().unwrap().to_owned();
    assert_eq!(
        created.header("location"),
        Some(format!("/api/integrations/connections/{id}").as_str())
    );
    let created = created
        .assert_json_path("provider", json!("microsoft"))
        .assert_json_path("kind", json!("app_only"))
        .assert_json_path("external_id", json!(TENANT))
        .assert_json_path("owner_type", json!("app"))
        .assert_json_path("owner_id", Value::Null)
        .assert_json_path("status", json!("healthy"))
        .assert_json_path("connected_by", json!(ADMIN))
        .assert_json_path("return_to", json!("/settings/integrations"))
        .json_value();
    // Credentials never leave the server.
    assert!(created.get("credentials").is_none());
    assert!(created.get("access_token").is_none());

    // The state works once.
    admin
        .get(&callback)
        .send()
        .await
        .assert_error_code(400, "invalid_state");

    admin
        .get("/api/integrations/connections")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.0.id", json!(id));
    admin
        .get(&format!("/api/integrations/connections/{id}"))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("provider_name", json!("Microsoft 365"));
    admin
        .post(&format!("/api/integrations/connections/{id}/check"))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("status", json!("healthy"))
        .assert_json_path("connection_id", json!(id));
    admin
        .delete(&format!("/api/integrations/connections/{id}"))
        .send()
        .await
        .assert_status(204);
    admin
        .get(&format!("/api/integrations/connections/{id}"))
        .send()
        .await
        .assert_error_code(404, "connection_not_found");
}

#[tokio::test]
async fn refusals_answer_with_their_codes() {
    let h = harness(with_routes(false)).await;
    h.fake.on_code_error("slack", "bad", "invalid_code");
    let admin = h.app.acting_as(principal(ADMIN));
    admin
        .get("/api/integrations/zoom/connect")
        .send()
        .await
        .assert_error_code(404, "unknown_integration");
    admin
        .get("/api/integrations/slack/connect?scope_set=directory")
        .send()
        .await
        .assert_error_code(400, "unknown_scope_set");
    admin
        .get("/api/integrations/edr/connect")
        .send()
        .await
        .assert_error_code(400, "unsupported_connection");
    admin
        .get("/api/integrations/slack/connect?return_to=https://evil.test")
        .send()
        .await
        .assert_error_code(400, "invalid_return_to");
    let state = admin
        .get("/api/integrations/slack/connect")
        .send()
        .await
        .json_value()["state"]
        .as_str()
        .unwrap()
        .to_owned();
    admin
        .get(&format!(
            "/api/integrations/slack/callback?state={state}&code=bad"
        ))
        .send()
        .await
        .assert_error(400, "Slack refused the connection: invalid_code")
        .assert_error_code(400, "integration_refused");
    let state = admin
        .get("/api/integrations/slack/connect")
        .send()
        .await
        .json_value()["state"]
        .as_str()
        .unwrap()
        .to_owned();
    admin
        .get(&format!(
            "/api/integrations/slack/callback?state={state}&error=access_denied"
        ))
        .send()
        .await
        .assert_error_code(400, "consent_denied");
    // A state started by the admin can't be finished by anyone else.
    let state = admin
        .get("/api/integrations/slack/connect")
        .send()
        .await
        .json_value()["state"]
        .as_str()
        .unwrap()
        .to_owned();
    h.fake.on_code("slack", "good", FakeGrant::new());
    h.app
        .acting_as(principal(DEPUTY))
        .get(&format!(
            "/api/integrations/slack/callback?state={state}&code=good"
        ))
        .send()
        .await
        .assert_error_code(400, "invalid_state");
    assert!(h.fake.exchanged().iter().all(|(_, code)| code != "good"));
}

#[tokio::test]
async fn in_an_app_with_workspaces_the_routes_act_on_the_request_workspace() {
    let h = harness(with_routes(true)).await;
    let agra = h.workspace(OTHER, "agra").await;
    let _gulati = h.workspace(OTHER, "gulati").await;
    // OTHER owns both workspaces, so it holds every permission in them.
    let owner = h.app.acting_as(principal(OTHER));

    let state = owner
        .get("/api/integrations/microsoft/connect")
        .header("X-Workspace", "agra")
        .send()
        .await
        .assert_status(200)
        .json_value()["state"]
        .as_str()
        .unwrap()
        .to_owned();
    let created = owner
        .get(&format!(
            "/api/integrations/microsoft/callback?tenant={TENANT}&admin_consent=True&state={state}"
        ))
        .header("X-Workspace", "agra")
        .send()
        .await
        .assert_status(201)
        .assert_json_path("owner_type", json!("workspace"))
        .assert_json_path("owner_id", json!(agra.id))
        .json_value();
    let id = created["id"].as_str().unwrap().to_owned();

    owner
        .get("/api/integrations/connections")
        .header("X-Workspace", "gulati")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data", json!([]));
    owner
        .get(&format!("/api/integrations/connections/{id}"))
        .header("X-Workspace", "gulati")
        .send()
        .await
        .assert_status(404);
    owner
        .delete(&format!("/api/integrations/connections/{id}"))
        .header("X-Workspace", "gulati")
        .send()
        .await
        .assert_status(404);
    owner
        .get("/api/integrations/connections")
        .send()
        .await
        .assert_status(400);
    // A state for agra can't be finished in gulati.
    let state = owner
        .get("/api/integrations/microsoft/connect")
        .header("X-Workspace", "agra")
        .send()
        .await
        .json_value()["state"]
        .as_str()
        .unwrap()
        .to_owned();
    owner
        .get(&format!(
            "/api/integrations/microsoft/callback?tenant={TENANT}&admin_consent=True&state={state}"
        ))
        .header("X-Workspace", "gulati")
        .send()
        .await
        .assert_error_code(400, "invalid_state");
    // ADMIN (app-wide `integrations.manage`) is not a member of agra.
    h.app
        .acting_as(principal(ADMIN))
        .get("/api/integrations/connections")
        .header("X-Workspace", "agra")
        .send()
        .await
        .assert_status(403);
}

#[tokio::test]
async fn a_workspace_uses_its_organizations_connection_but_cannot_remove_it() {
    let h = harness(with_routes(true)).await;
    let agra = h.workspace(OTHER, "agra").await;
    let organization = h.organization(OTHER, "agra-group", &[agra.id]).await;
    let connection = h
        .connect_microsoft(Owner::Organization(organization), TENANT)
        .await;
    let owner = h.app.acting_as(principal(OTHER));
    owner
        .get("/api/integrations/connections")
        .header("X-Workspace", "agra")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.0.id", json!(connection.id))
        .assert_json_path("data.0.owner_type", json!("organization"));
    owner
        .get(&format!("/api/integrations/connections/{}", connection.id))
        .header("X-Workspace", "agra")
        .send()
        .await
        .assert_status(200);
    owner
        .delete(&format!("/api/integrations/connections/{}", connection.id))
        .header("X-Workspace", "agra")
        .send()
        .await
        .assert_error_code(404, "connection_not_found");

    // A workspace connection of its own comes first.
    let own = h
        .connect_microsoft(Owner::Workspace(agra.id), "contoso.onmicrosoft.com")
        .await;
    let integrations = h.integrations().await;
    let used = integrations
        .connection(Owner::Workspace(agra.id), "microsoft", "default")
        .await
        .unwrap();
    assert_eq!(used.id, own.id);
    let listed = integrations
        .connections(Owner::Workspace(agra.id))
        .await
        .unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].id, own.id);
}

#[tokio::test]
async fn keys_are_uploaded_over_the_api() {
    let h = harness(with_routes(false)).await;
    h.fake.api().on(
        http::Method::GET,
        "https://agra.edr.example/api/v1/health",
        navree_http_client::FakeResponse::new(200),
    );
    let admin = h.app.acting_as(principal(ADMIN));
    admin
        .post("/api/integrations/google/service-account")
        .json(&json!({
            "key": service_account_json(),
            "subject": "admin@acme.test",
            "scope_set": "directory"
        }))
        .send()
        .await
        .assert_status(201)
        .assert_json_path("kind", json!("service_account"))
        .assert_json_path("scope_set", json!("directory"))
        .assert_json_path("external_id", json!("acme.test"));
    // The key file's text works too (and replaces the connection).
    admin
        .post("/api/integrations/google/service-account")
        .json(&json!({
            "key": service_account_json().to_string(),
            "subject": "it@acme.test",
            "scope_set": "directory"
        }))
        .send()
        .await
        .assert_status(201)
        .assert_json_path("external_id", json!("acme.test"));
    admin
        .post("/api/integrations/google/service-account")
        .json(&json!({ "key": { "type": "service_account", "client_email": "x@y.test", "private_key": "nope" } }))
        .send()
        .await
        .assert_error_code(400, "invalid_credentials");
    admin
        .post("/api/integrations/edr/api-key")
        .json(&json!({
            "key": "edr-key",
            "api_url": "https://agra.edr.example/api",
            "fields": { "site_id": "17" }
        }))
        .send()
        .await
        .assert_status(201)
        .assert_json_path("kind", json!("api_key"));
    let all = h
        .integrations()
        .await
        .connections(Owner::App)
        .await
        .unwrap();
    assert_eq!(all.len(), 2);
}

#[tokio::test]
async fn routes_are_not_mounted_when_turned_off() {
    let h = harness(|b| b).await;
    h.app
        .acting_as(principal(ADMIN))
        .get("/api/integrations/providers")
        .send()
        .await
        .assert_status(404);
}

#[tokio::test]
async fn the_routes_are_in_the_openapi_document() {
    let h = harness(|b| with_routes(false)(b).module(ApiModule::from_config())).await;
    let document = h
        .app
        .client()
        .get("/api/openapi.json")
        .send()
        .await
        .assert_status(200)
        .json_value();
    let paths = document["paths"].as_object().unwrap();
    for path in [
        "/api/integrations/providers",
        "/api/integrations/connections",
        "/api/integrations/connections/{id}",
        "/api/integrations/connections/{id}/check",
        "/api/integrations/{provider}/connect",
        "/api/integrations/{provider}/callback",
        "/api/integrations/{provider}/service-account",
        "/api/integrations/{provider}/api-key",
    ] {
        assert!(paths.contains_key(path), "{path} is missing");
    }
    let schemas = document["components"]["schemas"].as_object().unwrap();
    for schema in [
        "ConnectionInfo",
        "ProviderInfo",
        "ConnectRedirect",
        "Health",
    ] {
        assert!(schemas.contains_key(schema), "{schema} is missing");
    }
}
