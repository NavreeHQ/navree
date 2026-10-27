//! The stateless protocol (2026-07-28): no `initialize` and no sessions;
//! every request carries its own `_meta` and standard headers, and is
//! authenticated and checked like a session's. Notifications travel on a
//! `subscriptions/listen` stream.

use std::time::Duration;

use http::header::ORIGIN;
use navree_mcp::McpNotifier;
use navree_mcp::testing::{McpTestClient, NotificationStream};
use navree_testing::{TestClient, TestResponse};
use serde_json::{Value, json};

use crate::support::{app, insert_deal, names, user};

const STATELESS: &str = "2026-07-28";
const WAIT: Duration = Duration::from_secs(5);

/// A stateless client acting as `client`.
fn stateless(client: TestClient) -> McpTestClient {
    McpTestClient::new(client).protocol_version(STATELESS)
}

/// The next notification with `method` on `stream`, skipping others.
async fn next_with(stream: &mut NotificationStream, method: &str) -> Option<Value> {
    while let Some(notification) = stream.next(WAIT).await {
        if notification["method"] == method {
            return Some(notification);
        }
    }
    None
}

/// POSTs `body` with the stateless protocol's headers, as written.
async fn raw(client: &TestClient, method: &str, body: Value) -> TestResponse {
    client
        .post("/mcp")
        .without_header(ORIGIN)
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", STATELESS)
        .header("mcp-method", method)
        .json(&body)
        .send()
        .await
}

fn meta() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": STATELESS,
        "io.modelcontextprotocol/clientInfo": { "name": "curl", "version": "1" },
        "io.modelcontextprotocol/clientCapabilities": {},
    })
}

#[tokio::test]
async fn discovery_replaces_the_handshake() {
    let app = app().await;
    let mcp = stateless(app.acting_as(user())).initialize().await;
    assert_eq!(mcp.session_id(), None);
    let info = mcp.server_info();
    assert!(
        info["supportedVersions"]
            .as_array()
            .is_some_and(|versions| versions.contains(&json!(STATELESS))),
        "{info}"
    );
    assert_eq!(
        info["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        "navree-crm"
    );
    assert_eq!(info["capabilities"]["tools"]["listChanged"], true);
}

#[tokio::test]
async fn every_request_is_checked_on_its_own() {
    let app = app().await;
    let (owner, rep) = (user(), user());
    let mine = insert_deal(&app, rep.user_id, "Acme renewal", "lead").await;
    let theirs = insert_deal(&app, owner.user_id, "Globex", "lead").await;
    let mcp = stateless(app.acting_as(rep)).initialize().await;

    let tools = names(&mcp.list_tools().await);
    assert!(tools.contains(&"list_deals".to_owned()), "{tools:?}");
    assert!(!tools.contains(&"purge_deals".to_owned()), "{tools:?}");

    mcp.call_tool("list_deals", json!({ "mine": true }))
        .await
        .assert_ok()
        .assert_json_path("0.title", json!("Acme renewal"));
    mcp.call_tool("move_deal", json!({ "deal_id": theirs.id, "stage": "won" }))
        .await
        .assert_error(403, "Forbidden");
    let hidden = mcp.call_tool("purge_deals", json!({})).await;
    assert_eq!(
        hidden.response().clone().error()["message"],
        "Unknown tool: purge_deals"
    );

    mcp.read_resource(&format!("crm://deals/{}", mine.id))
        .await
        .assert_ok()
        .assert_json_path("title", json!("Acme renewal"));
    mcp.read_resource(&format!("crm://deals/{}", theirs.id))
        .await
        .assert_ok();
    let prompt = mcp
        .get_prompt("summarize", json!({ "days": "7" }))
        .await
        .assert_ok();
    assert_eq!(
        prompt.text(0).as_deref(),
        Some("Summarize the last 7 days.")
    );
}

#[tokio::test]
async fn answers_are_json_unless_progress_comes_first() {
    let app = app().await;
    let client = app.acting_as(user());
    let listed = raw(
        &client,
        "tools/list",
        json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": { "_meta": meta() } }),
    )
    .await;
    assert_eq!(listed.status(), 200);
    assert!(
        listed
            .header("content-type")
            .is_some_and(|value| value.starts_with("application/json")),
        "{:?}",
        listed.header("content-type")
    );

    let mut with_progress = meta();
    with_progress["progressToken"] = json!("p-1");
    let exported = client
        .post("/mcp")
        .without_header(ORIGIN)
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", STATELESS)
        .header("mcp-method", "tools/call")
        .header("mcp-name", "export_deals")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": { "name": "export_deals", "arguments": {}, "_meta": with_progress },
        }))
        .send()
        .await;
    assert_eq!(exported.status(), 200);
    assert!(
        exported
            .header("content-type")
            .is_some_and(|value| value.starts_with("text/event-stream")),
        "{:?}",
        exported.header("content-type")
    );
}

#[tokio::test]
async fn progress_reaches_the_request_stream() {
    let app = app().await;
    let mcp = stateless(app.acting_as(user())).initialize().await;
    let exported = mcp
        .call_tool_with_progress("export_deals", json!({}))
        .await
        .assert_ok();
    assert_eq!(exported.json::<u32>(), 2);
    let progress: Vec<Value> = mcp
        .notifications()
        .into_iter()
        .filter(|message| message["method"] == "notifications/progress")
        .collect();
    assert_eq!(progress.len(), 2, "{progress:?}");
    assert_eq!(progress[0]["params"]["message"], "half way");
}

#[tokio::test]
async fn listen_streams_carry_list_changes_and_resource_updates() {
    let app = app().await;
    let notifier = app.get::<McpNotifier>().await;
    let mcp = stateless(app.acting_as(user())).initialize().await;
    mcp.subscribe("crm://pipeline").await.result();
    // A resource the user may not read is left out of the stream.
    mcp.subscribe("crm://admin/stats").await.result();
    let mut stream = mcp.listen().await;
    let acknowledged = stream.next(WAIT).await.expect("the acknowledgment");
    assert_eq!(
        acknowledged["method"],
        "notifications/subscriptions/acknowledged"
    );
    // The stream is registered once the server has loaded the user's
    // access, just after the acknowledgment.
    let deadline = tokio::time::Instant::now() + WAIT;
    while notifier.sessions() == 0 && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    assert_eq!(notifier.tools_changed().await, 1);
    assert!(
        next_with(&mut stream, "notifications/tools/list_changed")
            .await
            .is_some()
    );
    assert_eq!(notifier.resource_updated("crm://pipeline").await, 1);
    let update = next_with(&mut stream, "notifications/resources/updated")
        .await
        .expect("the update arrives");
    assert_eq!(update["params"]["uri"], "crm://pipeline");
    assert_eq!(notifier.resource_updated("crm://admin/stats").await, 0);
}

#[tokio::test]
async fn requests_without_their_metadata_are_refused() {
    let app = app().await;
    let response = raw(
        &app.acting_as(user()),
        "tools/list",
        json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {} }),
    )
    .await;
    assert_eq!(response.status(), 400, "{}", response.text());
    let error = &response.json_value()["error"];
    assert_eq!(error["code"], -32602, "{error}");
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|message| message.contains("io.modelcontextprotocol/protocolVersion")),
        "{error}"
    );
}

#[tokio::test]
async fn guests_get_a_401() {
    let app = app().await;
    let response = raw(
        &app.client(),
        "tools/list",
        json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": { "_meta": meta() } }),
    )
    .await;
    assert_eq!(response.status(), 401);
}
