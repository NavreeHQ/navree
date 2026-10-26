//! Prompts: arguments from the arguments struct, rendered messages,
//! embedded resources and errors.

use navree_mcp::testing::McpTestClient;
use serde_json::json;
use uuid::Uuid;

use crate::support::{app, insert_deal, user};

#[tokio::test]
async fn prompts_list_their_arguments() {
    let app = app().await;
    let mcp = McpTestClient::new(app.acting_as(user())).initialize().await;
    let prompts = mcp.list_prompts().await;
    let follow_up = serde_json::to_value(
        prompts
            .iter()
            .find(|prompt| prompt.name == "follow-up-email")
            .unwrap(),
    )
    .unwrap();
    assert_eq!(follow_up["title"], "Draft a follow-up email");
    assert_eq!(
        follow_up["description"],
        "Drafts a follow-up email to a deal's contact."
    );
    assert_eq!(
        follow_up["arguments"],
        json!([{ "name": "deal_id", "description": "The deal to follow up on.", "required": true }])
    );

    let summarize = serde_json::to_value(prompts.iter().find(|p| p.name == "summarize").unwrap())
        .unwrap();
    let arguments = summarize["arguments"].as_array().unwrap();
    let include_lost = arguments
        .iter()
        .find(|a| a["name"] == "include_lost")
        .unwrap();
    assert_eq!(include_lost["required"], false);
}

#[tokio::test]
async fn messages_embed_the_resources_they_name() {
    let app = app().await;
    let rep = user();
    let deal = insert_deal(&app, rep.user_id, "Acme renewal", "proposal").await;
    let mcp = McpTestClient::new(app.acting_as(rep)).initialize().await;

    let prompt = mcp
        .get_prompt("follow-up-email", json!({ "deal_id": deal.id.to_string() }))
        .await
        .assert_ok();
    assert_eq!(
        prompt.description().as_deref(),
        Some("Drafts a follow-up email to a deal's contact.")
    );
    let messages = prompt.messages();
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[0]["content"]["type"], "resource");
    let resource = &messages[0]["content"]["resource"];
    assert_eq!(resource["uri"], format!("crm://deals/{}", deal.id));
    assert_eq!(resource["mimeType"], "application/json");
    let embedded: serde_json::Value =
        serde_json::from_str(resource["text"].as_str().unwrap()).unwrap();
    assert_eq!(embedded["title"], "Acme renewal");
    assert!(prompt.text(1).unwrap().contains("Acme renewal"));
    assert_eq!(messages[2]["role"], "assistant");
    assert_eq!(prompt.text(2).as_deref(), Some("Here is a draft:"));
}

#[tokio::test]
async fn string_arguments_are_coerced_and_validated() {
    let app = app().await;
    let mcp = McpTestClient::new(app.acting_as(user())).initialize().await;
    let prompt = mcp
        .get_prompt("summarize", json!({ "days": "30", "include_lost": "true" }))
        .await
        .assert_ok();
    assert_eq!(
        prompt.text(0).as_deref(),
        Some("Summarize the last 30 days, lost deals included.")
    );
    mcp.get_prompt("summarize", json!({ "days": "365" }))
        .await
        .assert_error(400, None);
    mcp.get_prompt("summarize", json!({}))
        .await
        .assert_error(400, "days should not be empty");
}

#[tokio::test]
async fn failures_and_unknown_prompts() {
    let app = app().await;
    let mcp = McpTestClient::new(app.acting_as(user())).initialize().await;
    mcp.get_prompt("follow-up-email", json!({ "deal_id": Uuid::now_v7().to_string() }))
        .await
        .assert_error(404, "Not Found");
    let error = mcp
        .get_prompt("no-such-prompt", json!({}))
        .await
        .response()
        .clone()
        .error();
    assert_eq!(error["code"], -32602);
    assert_eq!(error["message"], "Unknown prompt: no-such-prompt");

    // A secret deal is refused when its resource is embedded.
    let secret = insert_deal(&app, Uuid::now_v7(), "Secret", "secret").await;
    mcp.get_prompt("follow-up-email", json!({ "deal_id": secret.id.to_string() }))
        .await
        .assert_error(403, "Forbidden");
}
