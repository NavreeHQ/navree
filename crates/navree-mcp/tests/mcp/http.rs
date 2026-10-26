//! The streamable HTTP transport on the app's router: authentication and
//! its challenge, OAuth metadata, origins, sessions and limits.

use std::sync::Arc;

use http::header::ORIGIN;
use navree_auth::{AuthModule, AuthService, ClientMeta};
use navree_cache::Cache;
use navree_crypto::CryptoModule;
use navree_http::route_table;
use navree_mcp::testing::McpTestClient;
use navree_testing::TestClient;
use serde_json::{Value, json};

use crate::support::{app, builder, user};

fn initialize() -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "curl", "version": "1" },
        },
    })
}

async fn post(client: &TestClient, path: &str) -> navree_testing::TestResponse {
    client
        .post(path)
        .without_header(ORIGIN)
        .header("accept", "application/json, text/event-stream")
        .json(&initialize())
        .send()
        .await
}

#[tokio::test]
async fn guests_get_a_401_with_a_bearer_challenge() {
    let app = app().await;
    let response = post(&app.client(), "/mcp").await;
    assert_eq!(response.status(), 401);
    assert_eq!(response.header("www-authenticate"), Some("Bearer"));
    response.assert_error(401, "Unauthorized");
}

#[tokio::test]
async fn navree_auth_access_tokens_are_bearer_tokens() {
    let app = builder()
        .config("app.key", navree_crypto::generate_key())
        .config("hashing.memory_kib", 1024)
        .config("hashing.iterations", 1)
        .module(CryptoModule::from_config())
        .module(AuthModule::from_config())
        .build()
        .await;
    let auth = app.get::<AuthService>().await;
    let ada = auth
        .create_user("Ada", "ada@example.com", "a-long-password")
        .await
        .expect("user");
    let tokens = auth
        .issue_tokens(&ada, ClientMeta::default())
        .await
        .expect("tokens");

    // The guest client carries the access token on every request.
    let mcp = McpTestClient::new(app.client())
        .bearer(&tokens.access_token)
        .initialize()
        .await;
    mcp.call_tool("whoami", json!({}))
        .await
        .assert_ok()
        .assert_json_path("user_id", json!(ada.id));
    assert_eq!(mcp.close().await, 202, "DELETE carries the token too");

    // A token the authenticators refuse is a 401 with their challenge.
    let refused = app
        .client()
        .post("/mcp")
        .bearer("not-a-jwt")
        .without_header(ORIGIN)
        .header("accept", "application/json, text/event-stream")
        .json(&initialize())
        .send()
        .await;
    assert_eq!(refused.status(), 401);
    assert!(
        refused
            .header("www-authenticate")
            .is_some_and(|challenge| challenge.starts_with("Bearer")),
        "{:?}",
        refused.header("www-authenticate")
    );
}

#[tokio::test]
async fn oauth_mode_serves_the_protected_resource_metadata() {
    let app = builder()
        .config("mcp.auth", "oauth")
        .config("app.url", "https://crm.example.com")
        .config(
            "mcp.oauth.scopes",
            json!(["deals.read", "activities.create"]),
        )
        .config("mcp.title", "Navree CRM")
        .build()
        .await;

    let response = post(&app.client(), "/mcp").await;
    assert_eq!(response.status(), 401);
    assert_eq!(
        response.header("www-authenticate"),
        Some(
            "Bearer resource_metadata=\"https://crm.example.com/.well-known/oauth-protected-resource/mcp\""
        )
    );

    let expected = json!({
        "resource": "https://crm.example.com/mcp",
        "authorization_servers": ["https://crm.example.com"],
        "bearer_methods_supported": ["header"],
        "resource_name": "Navree CRM",
        "scopes_supported": ["deals.read", "activities.create"],
    });
    for path in [
        "/.well-known/oauth-protected-resource",
        "/.well-known/oauth-protected-resource/mcp",
    ] {
        let document = app.client().get(path).send().await.assert_status(200);
        assert_eq!(document.json_value(), expected, "{path}");
    }

    // Signed-in clients use the endpoint as usual.
    McpTestClient::new(app.acting_as(user())).initialize().await;
}

#[tokio::test]
async fn only_allowed_browser_origins_may_call() {
    let app = builder()
        .config("mcp.allowed_origins", "https://crm.example.com")
        .build()
        .await;
    let client = app.acting_as(user());
    let refused = client
        .post("/mcp")
        .header(ORIGIN, "https://evil.example")
        .header("accept", "application/json, text/event-stream")
        .json(&initialize())
        .send()
        .await;
    refused.assert_error(403, "This origin may not call the MCP endpoint");

    let allowed = client
        .post("/mcp")
        .header(ORIGIN, "https://crm.example.com")
        .header("accept", "application/json, text/event-stream")
        .json(&initialize())
        .send()
        .await;
    assert_eq!(allowed.status(), 200, "{}", allowed.text());

    // No Origin header: not a browser, allowed.
    assert_eq!(post(&client, "/mcp").await.status(), 200);
}

#[tokio::test]
async fn by_default_every_browser_origin_is_refused() {
    let app = app().await;
    let response = app
        .acting_as(user())
        .post("/mcp")
        .header(ORIGIN, "http://localhost:3000")
        .header("accept", "application/json, text/event-stream")
        .json(&initialize())
        .send()
        .await;
    assert_eq!(response.status(), 403);
}

#[tokio::test]
async fn a_session_belongs_to_the_user_who_opened_it() {
    let app = app().await;
    let mcp = McpTestClient::new(app.acting_as(user())).initialize().await;
    let session = mcp.session_id().unwrap().to_owned();

    let intruder = app
        .acting_as(user())
        .post("/mcp")
        .without_header(ORIGIN)
        .header("accept", "application/json, text/event-stream")
        .header("mcp-session-id", session)
        .header("mcp-protocol-version", "2025-11-25")
        .json(&json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }))
        .send()
        .await;
    intruder.assert_error(404, "Session not found");
    assert_eq!(mcp.list_tools().await.len(), 8, "the owner still uses it");
}

#[tokio::test]
async fn delete_ends_the_session() {
    let app = app().await;
    let mcp = McpTestClient::new(app.acting_as(user())).initialize().await;
    assert_eq!(mcp.close().await, 202);
    let after = app
        .acting_as(user())
        .post("/mcp")
        .without_header(ORIGIN)
        .header("accept", "application/json, text/event-stream")
        .header("mcp-session-id", mcp.session_id().unwrap())
        .json(&json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }))
        .send()
        .await;
    assert_eq!(after.status(), 404);
}

#[tokio::test]
async fn the_path_comes_from_the_configuration() {
    let app = builder().config("mcp.path", "/ai/mcp").build().await;
    let mcp = McpTestClient::new(app.acting_as(user()))
        .path("/ai/mcp")
        .initialize()
        .await;
    assert!(!mcp.list_tools().await.is_empty());
    let routes = route_table(app.ctx()).await.unwrap();
    let mcp_routes: Vec<String> = routes
        .iter()
        .filter(|route| {
            route
                .name
                .as_deref()
                .is_some_and(|name| name.starts_with("mcp."))
        })
        .map(|route| {
            format!(
                "{} {} {} {}",
                route.method,
                route.full_path,
                route.access.describe(),
                route.name.as_deref().unwrap_or_default(),
            )
        })
        .collect();
    assert_eq!(
        mcp_routes,
        [
            "POST /ai/mcp auth mcp.message",
            "GET /ai/mcp auth mcp.stream",
            "DELETE /ai/mcp auth mcp.close"
        ]
    );
}

#[tokio::test]
async fn every_route_has_its_own_name_and_api_summary() {
    let app = builder()
        .config("mcp.auth", "oauth")
        .config("app.url", "https://crm.example.com")
        .build()
        .await;
    let routes = route_table(app.ctx()).await.unwrap();
    let mcp_routes: Vec<_> = routes
        .iter()
        .filter(|route| {
            route
                .name
                .as_deref()
                .is_some_and(|name| name.starts_with("mcp."))
        })
        .collect();
    let mut names: Vec<&str> = mcp_routes
        .iter()
        .filter_map(|route| route.name.as_deref())
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "mcp.close",
            "mcp.message",
            "mcp.metadata",
            "mcp.metadata.endpoint",
            "mcp.stream"
        ],
        "operation ids must be unique"
    );
    for route in mcp_routes {
        let api = route.api.as_ref().expect("OpenAPI metadata");
        assert_eq!(api.tags, ["MCP"], "{}", route.full_path);
        assert!(api.summary.is_some(), "{}", route.full_path);
    }
}

#[tokio::test]
async fn oversized_messages_are_refused() {
    let app = builder().config("mcp.max_request_bytes", 256).build().await;
    let rep = user();
    let mcp = McpTestClient::new(app.acting_as(rep.clone()))
        .initialize()
        .await;
    let response = app
        .acting_as(rep)
        .post("/mcp")
        .without_header(ORIGIN)
        .header("accept", "application/json, text/event-stream")
        .header("mcp-session-id", mcp.session_id().unwrap())
        .json(&json!({
            "jsonrpc": "2.0", "id": 9, "method": "tools/call",
            "params": { "name": "create_deal", "arguments": { "title": "x".repeat(1024), "amount_cents": 1 } },
        }))
        .send()
        .await;
    assert_eq!(response.status(), 413);
}

#[tokio::test]
async fn sessions_in_the_cache_survive_a_move_to_another_process() {
    let cache = Arc::new(Cache::memory());
    let first = builder()
        .override_value::<Cache>(cache.clone())
        .build()
        .await;
    let rep = user();
    let mcp = McpTestClient::new(first.acting_as(rep.clone()))
        .initialize()
        .await;
    let session = mcp.session_id().unwrap().to_owned();

    // The next request reaches another process sharing the cache.
    let second = builder()
        .override_value::<Cache>(cache.clone())
        .build()
        .await;
    let request = |client: TestClient| {
        let session = session.clone();
        async move {
            client
                .post("/mcp")
                .without_header(ORIGIN)
                .header("accept", "application/json, text/event-stream")
                .header("mcp-session-id", session)
                .header("mcp-protocol-version", "2025-11-25")
                .json(&json!({ "jsonrpc": "2.0", "id": 5, "method": "tools/list" }))
                .send()
                .await
        }
    };
    let restored = request(second.acting_as(rep)).await;
    assert_eq!(restored.status(), 200, "{}", restored.text());
    assert!(
        restored.text().contains("list_deals"),
        "{}",
        restored.text()
    );

    // Another user cannot take the session over there either.
    let taken = request(second.acting_as(user())).await;
    assert_eq!(taken.status(), 404);
}
