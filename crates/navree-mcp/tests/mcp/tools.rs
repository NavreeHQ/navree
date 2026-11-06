//! Tools: lists filtered by access, typed and validated arguments, results,
//! errors, record checks and the call's unit of work.

use std::sync::Arc;

use navree_contracts::AuditSink;
use navree_http::{AppError, ModuleBuilderHttpExt};
use navree_kernel::{Module, ModuleBuilder};
use navree_macros::{injectable, mcp_server};
use navree_mcp::ModuleBuilderMcpExt;
use navree_mcp::testing::McpTestClient;
use serde_json::json;
use uuid::Uuid;

use crate::support::{
    DealResource, RecordingAudit, all_deals, app, builder, give_role, insert_deal, names, user,
};

#[tokio::test]
async fn initialize_describes_the_server() {
    let app = app().await;
    let mcp = McpTestClient::new(app.acting_as(user())).initialize().await;
    let info = mcp.server_info();
    assert_eq!(info["serverInfo"]["name"], "navree-crm");
    assert_eq!(info["serverInfo"]["version"], "0.0.0");
    assert_eq!(info["capabilities"]["tools"]["listChanged"], true);
    assert_eq!(info["capabilities"]["resources"]["subscribe"], true);
    assert_eq!(info["capabilities"]["prompts"]["listChanged"], true);
    assert!(mcp.session_id().is_some(), "a session id comes back");
}

#[tokio::test]
async fn the_list_shows_only_what_the_user_may_use() {
    let app = app().await;
    let rep = user();
    let mcp = McpTestClient::new(app.acting_as(rep.clone()))
        .initialize()
        .await;
    assert_eq!(
        names(&mcp.list_tools().await),
        [
            "create_deal",
            "create_then_fail",
            "explode",
            "export_deals",
            "list_deals",
            "move_deal",
            "rename_deal",
            "whoami",
        ]
    );

    let admin = user();
    give_role(&app, admin.user_id, "admin", &["reports.view"]).await;
    let mcp = McpTestClient::new(app.acting_as(admin)).initialize().await;
    let tools = names(&mcp.list_tools().await);
    assert!(tools.contains(&"purge_deals".to_owned()), "{tools:?}");
    assert!(tools.contains(&"report_sales".to_owned()), "{tools:?}");
}

#[tokio::test]
async fn tools_carry_descriptions_schemas_titles_and_hints() {
    let app = app().await;
    let admin = user();
    give_role(&app, admin.user_id, "admin", &["reports.view"]).await;
    let mcp = McpTestClient::new(app.acting_as(admin)).initialize().await;
    let tools = mcp.list_tools().await;
    let tool = |name: &str| {
        serde_json::to_value(tools.iter().find(|tool| tool.name == name).unwrap()).unwrap()
    };

    let list = tool("list_deals");
    assert_eq!(
        list["description"],
        "Lists the deals the user can see, newest first, optionally only one\nstage or the user's own."
    );
    assert_eq!(list["inputSchema"]["type"], "object");
    assert_eq!(
        list["inputSchema"]["properties"]["query"]["description"],
        "Words from the deal title."
    );
    assert!(list["inputSchema"]["$defs"]["Stage"].is_object(), "{list}");
    assert_eq!(list["annotations"]["readOnlyHint"], true);
    assert_eq!(list["annotations"]["destructiveHint"], false);

    assert_eq!(tool("move_deal")["annotations"]["destructiveHint"], true);
    assert_eq!(tool("rename_deal")["annotations"]["idempotentHint"], true);
    let report = tool("report_sales");
    assert_eq!(report["title"], "Sales report");
    assert_eq!(report["annotations"]["openWorldHint"], true);
    assert_eq!(tool("create_deal")["title"], "Create a deal");
    assert_eq!(
        tool("whoami")["inputSchema"],
        json!({ "type": "object", "properties": {} })
    );
}

#[tokio::test]
async fn results_are_structured_content_and_json_text() {
    let app = app().await;
    let rep = user();
    insert_deal(&app, rep.user_id, "Agra renewal", "lead").await;
    insert_deal(&app, Uuid::now_v7(), "Gulati", "won").await;
    let mcp = McpTestClient::new(app.acting_as(rep.clone()))
        .initialize()
        .await;

    let result = mcp
        .call_tool("list_deals", json!({}))
        .await
        .assert_ok()
        .assert_json_path("0.title", json!("Agra renewal"))
        .assert_json_path("1.title", json!("Gulati"));
    // An array is sent as `{"result": [...]}` structured content.
    assert_eq!(result.structured().unwrap()["result"][1]["stage"], "won");
    let deals: Vec<DealResource> = result.json();
    assert_eq!(deals.len(), 2);

    mcp.call_tool("list_deals", json!({ "mine": true }))
        .await
        .assert_ok()
        .assert_json_path("0.title", json!("Agra renewal"));
    let won = mcp
        .call_tool("list_deals", json!({ "stage": "won" }))
        .await
        .json::<Vec<DealResource>>();
    assert_eq!(won.len(), 1);

    let created = mcp
        .call_tool(
            "create_deal",
            json!({ "title": "Indrani", "amount_cents": 5000 }),
        )
        .await
        .assert_ok()
        .assert_json_path("title", json!("Indrani"));
    // An object is its own structured content.
    assert_eq!(created.structured().unwrap()["amount_cents"], 5000);

    let text = mcp.call_tool("whoami", json!({})).await.assert_ok();
    assert_eq!(text.json_value()["user_id"], json!(rep.user_id));
    assert_eq!(text.json_value()["session"], true);
    assert_eq!(text.json_value()["client"], "navree-test");
    assert_eq!(text.json_value()["request_id"], true);
}

#[tokio::test]
async fn strings_are_plain_text() {
    let app = app().await;
    let analyst = user();
    give_role(&app, analyst.user_id, "analyst", &["reports.view"]).await;
    let mcp = McpTestClient::new(app.acting_as(analyst))
        .initialize()
        .await;
    let report = mcp.call_tool("report_sales", json!({})).await.assert_ok();
    assert_eq!(report.text(), "Sales are up 12%.");
    assert!(report.structured().is_none());
}

#[tokio::test]
async fn writes_commit_on_ok_and_roll_back_on_error() {
    let app = app().await;
    let mcp = McpTestClient::new(app.acting_as(user())).initialize().await;

    mcp.call_tool("create_deal", json!({ "title": "Kept", "amount_cents": 1 }))
        .await
        .assert_ok();
    mcp.call_tool(
        "create_then_fail",
        json!({ "title": "Dropped", "amount_cents": 1 }),
    )
    .await
    .assert_error(409, "The pipeline is locked");

    let titles: Vec<String> = all_deals(&app).await.into_iter().map(|d| d.title).collect();
    assert_eq!(titles, ["Kept"]);
}

#[tokio::test]
async fn invalid_arguments_never_reach_the_method() {
    let app = app().await;
    let mcp = McpTestClient::new(app.acting_as(user())).initialize().await;

    let result = mcp
        .call_tool("create_deal", json!({ "title": "", "amount_cents": -1 }))
        .await
        .assert_error(400, None);
    let body = result.json_value();
    assert!(body["errors"]["title"].is_array(), "{body}");
    assert!(body["errors"]["amount_cents"].is_array(), "{body}");

    mcp.call_tool("create_deal", json!({ "amount_cents": 1 }))
        .await
        .assert_error(400, "title should not be empty");
    mcp.call_tool("list_deals", json!({ "stage": "closed" }))
        .await
        .assert_error(400, None);
    assert!(all_deals(&app).await.is_empty());
}

#[tokio::test]
async fn record_checks_answer_403_or_404_in_the_result() {
    let app = app().await;
    let owner = user();
    let other = user();
    let mine = insert_deal(&app, owner.user_id, "Mine", "lead").await;
    let theirs = insert_deal(&app, other.user_id, "Theirs", "lead").await;
    let mcp = McpTestClient::new(app.acting_as(owner)).initialize().await;

    // `cx.authorize` in the method.
    mcp.call_tool("move_deal", json!({ "deal_id": mine.id, "stage": "won" }))
        .await
        .assert_ok()
        .assert_json_path("stage", json!("won"));
    mcp.call_tool("move_deal", json!({ "deal_id": theirs.id, "stage": "won" }))
        .await
        .assert_error(403, "Forbidden");
    mcp.call_tool(
        "move_deal",
        json!({ "deal_id": Uuid::now_v7(), "stage": "won" }),
    )
    .await
    .assert_error(404, "Not Found");

    // `#[can(Update, Deal, from = "deal_id")]` before the method.
    mcp.call_tool(
        "rename_deal",
        json!({ "deal_id": mine.id, "title": "Renamed" }),
    )
    .await
    .assert_ok()
    .assert_json_path("title", json!("Renamed"));
    mcp.call_tool(
        "rename_deal",
        json!({ "deal_id": theirs.id, "title": "Hijacked" }),
    )
    .await
    .assert_error(403, "Forbidden");
    mcp.call_tool("rename_deal", json!({ "title": "No id" }))
        .await
        .assert_error(400, "deal_id should not be empty");

    let titles: Vec<String> = all_deals(&app).await.into_iter().map(|d| d.title).collect();
    assert_eq!(titles, ["Renamed", "Theirs"]);
}

#[tokio::test]
async fn hidden_and_unknown_tools_are_unknown() {
    let app = app().await;
    let mcp = McpTestClient::new(app.acting_as(user())).initialize().await;
    for name in ["purge_deals", "report_sales", "no_such_tool"] {
        let result = mcp.call_tool(name, json!({})).await;
        assert!(result.is_error());
        let error = result.response().clone().error();
        assert_eq!(error["code"], -32602, "{error}");
        assert_eq!(error["message"], format!("Unknown tool: {name}"));
    }
}

#[tokio::test]
async fn server_errors_do_not_leak() {
    let app = app().await;
    let mcp = McpTestClient::new(app.acting_as(user())).initialize().await;
    let result = mcp
        .call_tool("explode", json!({}))
        .await
        .assert_error(500, "Internal server error");
    assert!(!result.text().contains("hunter2"), "{}", result.text());
}

#[tokio::test]
async fn token_abilities_limit_what_a_client_sees() {
    let app = app().await;
    let mut limited = user();
    limited.token_abilities = Some(vec!["deals.read".to_owned()]);
    let mcp = McpTestClient::new(app.acting_as(limited))
        .initialize()
        .await;
    let tools = names(&mcp.list_tools().await);
    assert!(tools.contains(&"list_deals".to_owned()), "{tools:?}");
    assert!(!tools.contains(&"move_deal".to_owned()), "{tools:?}");
    assert!(!tools.contains(&"rename_deal".to_owned()), "{tools:?}");
}

/// The error a [`LockedMcp`] tool fails with, mapped by its module.
#[derive(Debug)]
pub struct PipelineLocked;

impl std::fmt::Display for PipelineLocked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the pipeline is locked until the quarter closes")
    }
}

impl std::error::Error for PipelineLocked {}

/// A server whose tool fails with an error the app maps.
#[injectable]
pub struct LockedMcp;

#[mcp_server]
#[authenticated]
impl LockedMcp {
    /// Fails with `PipelineLocked`, which the module maps to a 409.
    #[mcp_tool]
    async fn close_quarter(&self) -> Result<String, AppError> {
        Err(AppError::internal(PipelineLocked))
    }
}

pub struct LockedModule;

impl Module for LockedModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.mcp_server::<LockedMcp>()
            .map_error::<PipelineLocked>(|_| Some(AppError::conflict("The pipeline is locked")));
    }
}

#[tokio::test]
async fn the_apps_error_mappers_apply_to_tool_errors() {
    let app = builder().module(LockedModule).build().await;
    let mcp = McpTestClient::new(app.acting_as(user())).initialize().await;
    mcp.call_tool("close_quarter", json!({}))
        .await
        .assert_error(409, "The pipeline is locked");
}

#[tokio::test]
async fn refusals_are_audited_as_access_denied() {
    let audit = RecordingAudit::default();
    let app = builder()
        .override_value::<dyn AuditSink>(Arc::new(audit.clone()))
        .build()
        .await;
    let (owner, rep) = (user(), user());
    let theirs = insert_deal(&app, owner.user_id, "Theirs", "lead").await;
    let mcp = McpTestClient::new(app.acting_as(rep)).initialize().await;

    mcp.call_tool("move_deal", json!({ "deal_id": theirs.id, "stage": "won" }))
        .await
        .assert_error(403, "Forbidden");
    assert_eq!(audit.actions(), ["access.denied"]);

    // A 404 is not a refusal.
    mcp.call_tool(
        "move_deal",
        json!({ "deal_id": Uuid::now_v7(), "stage": "won" }),
    )
    .await
    .assert_error(404, None);
    assert_eq!(audit.actions(), ["access.denied"]);
}
