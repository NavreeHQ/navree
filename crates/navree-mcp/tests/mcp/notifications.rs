//! Notifications: progress of long calls, resource updates to subscribed
//! sessions, list changes to every session and after an access change.

use std::time::Duration;

use navree_mcp::McpNotifier;
use navree_mcp::testing::{McpTestClient, NotificationStream};
use serde_json::{Value, json};

use crate::support::{app, give_role, user};

const WAIT: Duration = Duration::from_secs(5);

/// The next notification with `method` on `stream`, skipping others.
async fn next_with(stream: &mut NotificationStream, method: &str) -> Option<Value> {
    while let Some(notification) = stream.next(WAIT).await {
        if notification["method"] == method {
            return Some(notification);
        }
    }
    None
}

#[tokio::test]
async fn long_calls_report_progress_on_the_request_stream() {
    let app = app().await;
    let mcp = McpTestClient::new(app.acting_as(user())).initialize().await;
    mcp.call_tool_with_progress("export_deals", json!({}))
        .await
        .assert_ok()
        .assert_json_path("", json!(2));
    let progress: Vec<Value> = mcp
        .notifications()
        .into_iter()
        .filter(|n| n["method"] == "notifications/progress")
        .collect();
    assert_eq!(progress.len(), 2, "{progress:?}");
    assert_eq!(progress[0]["params"]["progress"], 1.0);
    assert_eq!(progress[0]["params"]["total"], 2.0);
    assert_eq!(progress[0]["params"]["message"], "half way");
    assert!(progress[1]["params"].get("message").is_none());

    // Without a progress token, nothing is sent.
    let quiet = McpTestClient::new(app.acting_as(user())).initialize().await;
    quiet.call_tool("export_deals", json!({})).await.assert_ok();
    assert!(
        quiet
            .notifications()
            .iter()
            .all(|n| n["method"] != "notifications/progress")
    );
}

#[tokio::test]
async fn subscribed_sessions_hear_about_resource_updates() {
    let app = app().await;
    let notifier = app.get::<McpNotifier>().await;
    let mcp = McpTestClient::new(app.acting_as(user())).initialize().await;
    let mut stream = mcp.listen().await;
    assert!(notifier.sessions() >= 1);

    mcp.subscribe("crm://pipeline").await.result();
    assert_eq!(notifier.resource_updated("crm://pipeline").await, 1);
    let update = next_with(&mut stream, "notifications/resources/updated")
        .await
        .expect("the update arrives");
    assert_eq!(update["params"]["uri"], "crm://pipeline");

    assert_eq!(notifier.resource_updated("crm://notes/x").await, 0, "not subscribed");
    mcp.request("resources/unsubscribe", json!({ "uri": "crm://pipeline" }))
        .await
        .result();
    assert_eq!(notifier.resource_updated("crm://pipeline").await, 0);
}

#[tokio::test]
async fn subscribing_to_a_resource_the_user_cannot_read_fails() {
    let app = app().await;
    let mcp = McpTestClient::new(app.acting_as(user())).initialize().await;
    let error = mcp.subscribe("crm://admin/stats").await.error();
    assert_eq!(error["code"], -32002);
}

#[tokio::test]
async fn list_changes_reach_every_session() {
    let app = app().await;
    let notifier = app.get::<McpNotifier>().await;
    let first = McpTestClient::new(app.acting_as(user())).initialize().await;
    let second = McpTestClient::new(app.acting_as(user())).initialize().await;
    let mut first_stream = first.listen().await;
    let mut second_stream = second.listen().await;

    assert_eq!(notifier.tools_changed().await, 2);
    for stream in [&mut first_stream, &mut second_stream] {
        assert!(next_with(stream, "notifications/tools/list_changed").await.is_some());
    }
    assert_eq!(notifier.prompts_changed().await, 2);
    assert!(
        next_with(&mut first_stream, "notifications/prompts/list_changed")
            .await
            .is_some()
    );
    assert_eq!(notifier.resources_changed().await, 2);
    assert!(
        next_with(&mut second_stream, "notifications/resources/list_changed")
            .await
            .is_some()
    );

    // A closed session is forgotten.
    assert_eq!(second.close().await, 202);
    assert_eq!(notifier.tools_changed().await, 1);
}

#[tokio::test]
async fn an_access_change_is_announced_with_the_next_request() {
    let app = app().await;
    let rep = user();
    let mcp = McpTestClient::new(app.acting_as(rep.clone())).initialize().await;
    let mut stream = mcp.listen().await;
    let before = mcp.list_tools().await;
    assert!(before.iter().all(|tool| tool.name != "purge_deals"));

    give_role(&app, rep.user_id, "admin", &[]).await;
    mcp.call_tool("whoami", json!({}))
        .await
        .assert_ok()
        .assert_json_path("roles", json!(["admin"]));

    let on_stream = next_with(&mut stream, "notifications/tools/list_changed").await;
    let on_request = mcp
        .notifications()
        .into_iter()
        .any(|n| n["method"] == "notifications/tools/list_changed");
    assert!(on_stream.is_some() || on_request, "the list change is announced");
    assert!(
        mcp.list_tools()
            .await
            .iter()
            .any(|tool| tool.name == "purge_deals")
    );
}
