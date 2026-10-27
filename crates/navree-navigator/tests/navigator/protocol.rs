//! The MCP server spoken to by rmcp's client over an in-memory pipe.

use std::sync::Arc;

use navree_navigator::{ServeError, serve};
use rmcp::RoleClient;
use rmcp::ServiceExt;
use rmcp::model::CallToolRequestParams;
use rmcp::service::RunningService;
use serde_json::json;

use crate::support::Fixture;

async fn connect(
    fixture: &Fixture,
) -> (
    RunningService<RoleClient, ()>,
    tokio::task::JoinHandle<Result<(), ServeError>>,
) {
    let (client_side, server_side) = tokio::io::duplex(1 << 16);
    let (server_read, server_write) = tokio::io::split(server_side);
    let navigator = Arc::new(fixture.navigator());
    let server = tokio::spawn(serve(navigator, server_read, server_write));
    let (client_read, client_write) = tokio::io::split(client_side);
    let client = ().serve((client_read, client_write)).await.expect("initialize");
    (client, server)
}

fn args(value: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
    value.as_object().cloned().unwrap()
}

#[tokio::test]
async fn the_server_lists_twelve_read_only_tools() {
    let fixture = Fixture::new("[app]\nname = \"crm\"\n");
    let (client, server) = connect(&fixture).await;

    let info = client.peer_info().expect("server info");
    assert_eq!(
        info.server_info.as_ref().map(|s| s.name.as_str()),
        Some("navree")
    );
    assert!(
        info.instructions
            .as_deref()
            .unwrap_or_default()
            .contains("app_info")
    );

    let tools = client.list_all_tools().await.unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    assert_eq!(
        names,
        [
            "app_info",
            "list_routes",
            "module_graph",
            "list_providers",
            "list_commands",
            "describe_schema",
            "query_database",
            "read_logs",
            "last_error",
            "read_config",
            "search_docs",
            "read_doc"
        ]
    );
    for tool in &tools {
        let annotations = tool.annotations.as_ref().expect("annotations");
        assert_eq!(annotations.read_only_hint, Some(true), "{}", tool.name);
        assert_eq!(annotations.destructive_hint, Some(false), "{}", tool.name);
        assert_eq!(tool.input_schema.get("type"), Some(&json!("object")));
    }

    client.cancel().await.unwrap();
    server
        .await
        .unwrap()
        .expect("the server ends with the client");
}

#[tokio::test]
async fn an_app_with_lighthouse_gets_its_five_tools() {
    let fixture = Fixture::new("[app]
name = \"crm\"
");
    crate::support::write(
        &fixture.root().join("src/app.rs"),
        "Navree::app().module(LighthouseModule::from_config())",
    );
    let (client, server) = connect(&fixture).await;
    let instructions = client
        .peer_info()
        .and_then(|info| info.instructions.clone())
        .unwrap_or_default();
    assert!(instructions.contains("lighthouse_list_issues"), "{instructions}");
    let tools = client.list_all_tools().await.unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    assert_eq!(names.len(), 17);
    assert_eq!(names[12..], navree_navigator::lighthouse::LIGHTHOUSE_TOOLS);
    let issue = tools
        .iter()
        .find(|t| t.name == "lighthouse_get_issue")
        .unwrap();
    assert_eq!(issue.input_schema.get("required"), Some(&json!(["id"])));
    assert_eq!(
        issue.annotations.as_ref().unwrap().read_only_hint,
        Some(true)
    );

    let answer = client
        .call_tool(CallToolRequestParams::new("lighthouse_list_issues"))
        .await
        .unwrap();
    assert_eq!(answer.is_error, Some(true), "no database configured");
    client.cancel().await.unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn tools_answer_with_text_and_structured_content() {
    let fixture = Fixture::new("[app]\nname = \"crm\"\n[mail]\npassword = \"hunter2\"\n");
    let (client, server) = connect(&fixture).await;

    let config = client
        .call_tool(CallToolRequestParams::new("read_config"))
        .await
        .unwrap();
    assert_eq!(config.is_error, Some(false));
    let structured = config
        .structured_content
        .expect("an object answer is structured");
    assert_eq!(structured["app"]["name"], "crm");
    assert_eq!(structured["mail"]["password"], "[redacted]");
    let text = config.content[0].as_text().expect("text").text.clone();
    assert!(text.contains("\"name\": \"crm\""), "{text}");

    let doc = client
        .call_tool(
            CallToolRequestParams::new("read_doc")
                .with_arguments(args(json!({"slug": "ai/agent-skills"}))),
        )
        .await
        .unwrap();
    assert_eq!(doc.is_error, Some(false));
    assert!(
        doc.content[0]
            .as_text()
            .unwrap()
            .text
            .starts_with("# Agent skills")
    );

    let refused = client
        .call_tool(
            CallToolRequestParams::new("query_database")
                .with_arguments(args(json!({"sql": "DELETE FROM deals"}))),
        )
        .await
        .unwrap();
    assert_eq!(
        refused.is_error,
        Some(true),
        "a failed tool is a tool error"
    );

    let unknown = client
        .call_tool(CallToolRequestParams::new("drop_database"))
        .await;
    assert!(unknown.is_err(), "an unknown tool is a protocol error");

    client.cancel().await.unwrap();
    server.await.unwrap().unwrap();
}
