//! The MCP tools, resource and prompt (feature `mcp`): what each user sees,
//! reading issues and figures, changing statuses, errors, and
//! `lighthouse.mcp = false`.

use navree_lighthouse::{IssueFilter, IssueStatus, LighthouseStore};
use navree_mcp::McpModule;
use navree_mcp::testing::McpTestClient;
use navree_testing::TestApp;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::{builder, flushed, token, user};

/// Every tool Lighthouse offers.
const READ_TOOLS: [&str; 11] = [
    "lighthouse_get_issue",
    "lighthouse_get_overview",
    "lighthouse_get_route",
    "lighthouse_get_trace",
    "lighthouse_list_alerts",
    "lighthouse_list_issues",
    "lighthouse_list_jobs",
    "lighthouse_list_occurrences",
    "lighthouse_list_outgoing",
    "lighthouse_list_queries",
    "lighthouse_list_routes",
];
const RESOLVE_TOOLS: [&str; 3] = [
    "lighthouse_ignore_issue",
    "lighthouse_reopen_issue",
    "lighthouse_resolve_issue",
];

/// The app with `McpModule`.
async fn app() -> TestApp {
    builder().module(McpModule::from_config()).build().await
}

/// An MCP client for a new user with `permissions` (through the role
/// `role`; none: no role), signed in with an access token.
async fn agent(app: &TestApp, email: &str, permissions: &[&str]) -> McpTestClient {
    let role = (!permissions.is_empty()).then(|| format!("role-{}", Uuid::now_v7().simple()));
    user(app, email, role.as_deref(), permissions).await;
    McpTestClient::new(app.client())
        .bearer(token(app, email).await)
        .initialize()
        .await
}

/// Two failures of one route, written, and the issue's id.
async fn failing_route(app: &TestApp) -> (LighthouseStore, Uuid) {
    for _ in 0..2 {
        app.client()
            .get("/api/demo/boom/7")
            .send()
            .await
            .assert_status(500);
    }
    let store = flushed(app).await;
    let issue = store.issues(&IssueFilter::default()).await.unwrap().data[0]
        .issue
        .id;
    (store, issue)
}

/// The names of the Lighthouse tools the client sees, sorted.
async fn lighthouse_tools(mcp: &McpTestClient) -> Vec<String> {
    let mut names: Vec<String> = mcp
        .list_tools()
        .await
        .iter()
        .map(|tool| tool.name.to_string())
        .filter(|name| name.starts_with("lighthouse_"))
        .collect();
    names.sort();
    names
}

#[tokio::test]
async fn users_see_the_tools_their_abilities_allow() {
    let app = app().await;
    let nobody = agent(&app, "dev@acme.test", &[]).await;
    assert!(lighthouse_tools(&nobody).await.is_empty());

    let viewer = agent(&app, "viewer@acme.test", &["lighthouse.view"]).await;
    assert_eq!(lighthouse_tools(&viewer).await, READ_TOOLS);

    let ops = agent(
        &app,
        "ops@acme.test",
        &["lighthouse.view", "lighthouse.resolve"],
    )
    .await;
    let mut every: Vec<&str> = READ_TOOLS.iter().chain(&RESOLVE_TOOLS).copied().collect();
    every.sort_unstable();
    assert_eq!(lighthouse_tools(&ops).await, every);

    // A viewer calling a resolve tool is refused as for an unknown tool.
    let refused = viewer
        .call_tool(
            "lighthouse_resolve_issue",
            json!({ "issue_id": Uuid::now_v7() }),
        )
        .await;
    let error = refused.response().clone().error();
    assert_eq!(
        error["message"], "Unknown tool: lighthouse_resolve_issue",
        "{error}"
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_tools_describe_their_arguments_and_hints() {
    let app = app().await;
    let ops = agent(
        &app,
        "ops@acme.test",
        &["lighthouse.view", "lighthouse.resolve"],
    )
    .await;
    let tools = ops.list_tools().await;
    let tool = |name: &str| {
        tools
            .iter()
            .find(|tool| tool.name == name)
            .unwrap_or_else(|| panic!("no tool {name}"))
            .clone()
    };
    let list = tool("lighthouse_list_issues");
    let schema = Value::Object((*list.input_schema).clone());
    for property in [
        "status",
        "kind",
        "level",
        "release",
        "environment",
        "route",
        "job",
        "tag",
        "user",
        "workspace",
        "search",
        "sort",
        "page",
        "per_page",
    ] {
        assert!(
            schema["properties"].get(property).is_some(),
            "lighthouse_list_issues has no {property}: {schema}"
        );
    }
    assert!(list.description.as_deref().unwrap_or("").contains("issues"));
    assert_eq!(
        list.annotations.as_ref().and_then(|a| a.read_only_hint),
        Some(true)
    );
    let resolve = tool("lighthouse_resolve_issue");
    let schema = Value::Object((*resolve.input_schema).clone());
    assert_eq!(schema["required"], json!(["issue_id"]));
    let hints = resolve.annotations.clone().unwrap();
    assert_ne!(hints.read_only_hint, Some(true));
    assert_eq!(hints.idempotent_hint, Some(true));
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn an_agent_finds_an_issue_reads_it_and_resolves_it_in_the_next_release() {
    let app = app().await;
    let (store, id) = failing_route(&app).await;
    let mcp = agent(
        &app,
        "agent@acme.test",
        &["lighthouse.view", "lighthouse.resolve"],
    )
    .await;

    let page = mcp
        .call_tool("lighthouse_list_issues", json!({ "sort": "first_seen" }))
        .await
        .assert_ok()
        .json_value();
    assert_eq!(page["meta"]["total"], 1, "{page}");
    assert_eq!(page["data"][0]["id"], json!(id));
    assert_eq!(page["data"][0]["title"], "invoice inv_7 not found");
    assert_eq!(page["data"][0]["status"], "open");

    let issue = mcp
        .call_tool("lighthouse_get_issue", json!({ "issue_id": id }))
        .await
        .assert_ok()
        .json_value();
    assert_eq!(issue["count"], 2);
    assert_eq!(issue["culprit"], "GET /api/demo/boom/{n}");
    assert!(
        issue["chain"][0]
            .as_str()
            .unwrap()
            .contains("invoice inv_7 not found")
    );
    assert_eq!(issue["latest"]["method"], "GET", "{issue}");

    mcp.call_tool(
        "lighthouse_resolve_issue",
        json!({ "issue_id": id, "in_next_release": true, "comment": "fixed in #812" }),
    )
    .await
    .assert_ok()
    .assert_json_path("status", json!("resolved_in_next_release"));
    let detail = store.issue_detail(id).await.unwrap();
    assert_eq!(detail.issue.status, IssueStatus::ResolvedInNextRelease);
    assert_eq!(detail.comment.as_deref(), Some("fixed in #812"));
    assert_eq!(detail.resolved_in_release.as_deref(), Some("r1"));

    // Resolved issues are not listed by default; `any` lists every status.
    let open = mcp
        .call_tool("lighthouse_list_issues", json!({}))
        .await
        .assert_ok()
        .json_value();
    assert_eq!(open["meta"]["total"], 0, "{open}");
    let any = mcp
        .call_tool("lighthouse_list_issues", json!({ "status": "any" }))
        .await
        .assert_ok()
        .json_value();
    assert_eq!(any["meta"]["total"], 1, "{any}");
    let resolved = mcp
        .call_tool(
            "lighthouse_list_issues",
            json!({ "status": "resolved_in_next_release", "kind": "exception", "level": "error" }),
        )
        .await
        .assert_ok()
        .json_value();
    assert_eq!(resolved["meta"]["total"], 1, "{resolved}");
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn issues_are_ignored_and_reopened() {
    let app = app().await;
    let (store, id) = failing_route(&app).await;
    let mcp = agent(
        &app,
        "agent@acme.test",
        &["lighthouse.view", "lighthouse.resolve"],
    )
    .await;

    mcp.call_tool(
        "lighthouse_ignore_issue",
        json!({ "issue_id": id, "occurrences": 100, "comment": "noisy" }),
    )
    .await
    .assert_ok()
    .assert_json_path("status", json!("ignored"));
    let detail = store.issue_detail(id).await.unwrap();
    assert_eq!(detail.ignore_remaining, Some(100));
    assert_eq!(detail.comment.as_deref(), Some("noisy"));

    mcp.call_tool("lighthouse_reopen_issue", json!({ "issue_id": id }))
        .await
        .assert_ok()
        .assert_json_path("status", json!("open"));

    mcp.call_tool(
        "lighthouse_ignore_issue",
        json!({ "issue_id": id, "until": "2099-01-01T00:00:00Z" }),
    )
    .await
    .assert_ok()
    .assert_json_path("status", json!("ignored"));
    assert!(store.issue_detail(id).await.unwrap().ignored_until.is_some());

    mcp.call_tool(
        "lighthouse_ignore_issue",
        json!({ "issue_id": id, "until": "2099-01-01T00:00:00Z", "occurrences": 5 }),
    )
    .await
    .assert_error(400, "give at most one of until and occurrences");
    mcp.call_tool(
        "lighthouse_ignore_issue",
        json!({ "issue_id": id, "occurrences": 0 }),
    )
    .await
    .assert_error(400, None);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn unknown_ids_and_invalid_arguments_are_tool_errors() {
    let app = app().await;
    let mcp = agent(
        &app,
        "agent@acme.test",
        &["lighthouse.view", "lighthouse.resolve"],
    )
    .await;
    let missing = Uuid::now_v7();
    mcp.call_tool("lighthouse_get_issue", json!({ "issue_id": missing }))
        .await
        .assert_error(404, "issue not found");
    mcp.call_tool("lighthouse_reopen_issue", json!({ "issue_id": missing }))
        .await
        .assert_error(404, "issue not found");
    mcp.call_tool("lighthouse_get_trace", json!({ "trace_id": missing }))
        .await
        .assert_error(404, None);
    mcp.call_tool("lighthouse_get_issue", json!({ "issue_id": "not-a-uuid" }))
        .await
        .assert_error(400, None);
    mcp.call_tool("lighthouse_list_issues", json!({ "per_page": 500 }))
        .await
        .assert_error(400, None);
    mcp.call_tool("lighthouse_list_routes", json!({ "period": "1y" }))
        .await
        .assert_error(400, None);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_figures_traces_and_alerts_are_read() {
    let app = app().await;
    app.client()
        .get("/api/demo/query")
        .send()
        .await
        .assert_status(200);
    let (store, id) = failing_route(&app).await;
    let sample = store
        .issue_detail(id)
        .await
        .unwrap()
        .latest
        .unwrap()
        .sample_id
        .unwrap();
    let mcp = agent(&app, "viewer@acme.test", &["lighthouse.view"]).await;

    let overview = mcp
        .call_tool("lighthouse_get_overview", json!({ "period": "1h" }))
        .await
        .assert_ok()
        .json_value();
    assert_eq!(overview["period"], "1h");
    // The three demo requests, and perhaps the MCP requests written since.
    assert!(overview["requests"].as_i64().unwrap() >= 3, "{overview}");
    assert!(overview["exceptions"].as_i64().unwrap() >= 2, "{overview}");

    let listed = mcp
        .call_tool("lighthouse_list_routes", json!({}))
        .await
        .assert_ok();
    // A list is the text and, as structured content, `{"result": [..]}`.
    assert!(listed.structured().unwrap()["result"].is_array());
    let routes = listed
        .json_value()
        .as_array()
        .expect("a list of routes")
        .clone();
    assert!(
        routes
            .iter()
            .any(|route| route["endpoint"] == "GET /api/demo/boom/{n}"),
        "{routes:?}"
    );

    let route = mcp
        .call_tool(
            "lighthouse_get_route",
            json!({ "endpoint": "GET /api/demo/boom/{n}", "period": "1h" }),
        )
        .await
        .assert_ok()
        .json_value();
    assert_eq!(route["stats"]["count"], 2, "{route}");
    assert!(!route["slowest"].as_array().unwrap().is_empty());

    let queries = mcp
        .call_tool("lighthouse_list_queries", json!({ "period": "24h" }))
        .await
        .assert_ok()
        .json_value();
    assert!(
        queries
            .as_array()
            .unwrap()
            .iter()
            .any(|query| query["statement"]
                .as_str()
                .unwrap_or("")
                .contains("SELECT")),
        "{queries}"
    );
    for tool in ["lighthouse_list_jobs", "lighthouse_list_outgoing"] {
        let value = mcp
            .call_tool(tool, json!({ "period": "7d" }))
            .await
            .assert_ok()
            .json_value();
        assert!(value.is_array(), "{tool}: {value}");
    }

    let trace = mcp
        .call_tool("lighthouse_get_trace", json!({ "trace_id": sample }))
        .await
        .assert_ok()
        .json_value();
    assert_eq!(trace["id"], json!(sample));
    assert!(trace["spans"].is_array(), "{trace}");

    let occurrences = mcp
        .call_tool(
            "lighthouse_list_occurrences",
            json!({ "issue_id": id, "per_page": 1 }),
        )
        .await
        .assert_ok()
        .json_value();
    assert_eq!(occurrences["meta"]["total"], 2, "{occurrences}");
    assert_eq!(occurrences["data"].as_array().unwrap().len(), 1);

    let alerts = mcp
        .call_tool("lighthouse_list_alerts", json!({}))
        .await
        .assert_ok()
        .json_value();
    assert!(alerts.is_array(), "{alerts}");
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn an_issue_is_a_resource_and_fixing_it_is_a_prompt() {
    let app = app().await;
    let (_, id) = failing_route(&app).await;
    let mcp = agent(&app, "viewer@acme.test", &["lighthouse.view"]).await;

    let templates = mcp.list_resource_templates().await;
    assert!(
        templates
            .iter()
            .any(|template| template.uri_template == "lighthouse://issues/{id}"),
        "{templates:?}"
    );
    let resource = mcp
        .read_resource(&format!("lighthouse://issues/{id}"))
        .await
        .assert_ok();
    assert_eq!(resource.mime_type().as_deref(), Some("application/json"));
    let issue = resource.json_value();
    assert_eq!(issue["title"], "invoice inv_7 not found");
    mcp.read_resource(&format!("lighthouse://issues/{}", Uuid::now_v7()))
        .await
        .assert_error(404, None);

    let prompts = mcp.list_prompts().await;
    let prompt = prompts
        .iter()
        .find(|prompt| prompt.name == "fix-lighthouse-issue")
        .expect("the fix-lighthouse-issue prompt");
    let arguments = prompt.arguments.clone().unwrap_or_default();
    assert_eq!(arguments.len(), 1);
    assert_eq!(arguments[0].name, "issue_id");
    assert_eq!(arguments[0].required, Some(true));

    let rendered = mcp
        .get_prompt("fix-lighthouse-issue", json!({ "issue_id": id.to_string() }))
        .await
        .assert_ok();
    let messages = rendered.messages();
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert_eq!(messages[0]["content"]["type"], "resource", "{messages:?}");
    assert!(
        messages[0]["content"]["resource"]["text"]
            .as_str()
            .unwrap()
            .contains("invoice inv_7 not found")
    );
    let text = rendered.text(1).expect("instructions");
    assert!(text.contains("invoice inv_7 not found"), "{text}");
    assert!(text.contains("GET /api/demo/boom/{n}"), "{text}");
    assert!(text.contains("lighthouse_resolve_issue"), "{text}");
    assert!(text.contains(&id.to_string()), "{text}");
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn nothing_is_offered_with_mcp_off_or_without_a_store() {
    for (key, value) in [("lighthouse.mcp", json!(false)), ("lighthouse.store", json!("off"))] {
        let app = builder()
            .config(key, value)
            .module(McpModule::from_config())
            .build()
            .await;
        let mcp = agent(
            &app,
            "ops@acme.test",
            &["lighthouse.view", "lighthouse.resolve"],
        )
        .await;
        assert!(lighthouse_tools(&mcp).await.is_empty(), "{key}");
        assert!(mcp.list_prompts().await.is_empty(), "{key}");
        assert!(mcp.list_resource_templates().await.is_empty(), "{key}");
        app.shutdown().await.unwrap();
    }
}
