//! The `lighthouse` and `mcp` features together turn on navree-lighthouse's
//! `mcp` feature: Lighthouse's tools, resource and prompt are served by
//! `McpModule`, as `content/docs/tools/lighthouse.mdx` shows. Runs with
//! `cargo test -p navree --features mcp --test lighthouse_mcp`.

use navree::access::{AccessOptions, AccessService};
use navree::db::Db;
use navree::lighthouse::mcp::{FIX_ISSUE_PROMPT, ISSUE_RESOURCE};
use navree::lighthouse::{IssueFilter, Lighthouse, LighthouseMcp};
use navree::mcp::testing::McpTestClient;
use navree::prelude::*;
use navree::testing::{TestApp, principal};
use serde_json::json;

fn abilities(ctx: &AbilityCtx, rules: &mut Rules) {
    ctx.grant_permissions(rules);
}

#[tokio::test]
async fn an_agent_reads_and_resolves_issues_over_mcp() {
    let _ = std::any::type_name::<LighthouseMcp>();
    let app = TestApp::builder()
        .temp_database()
        .module(DatabaseModule::from_config())
        .module(AccessModule::for_root(AccessOptions {
            abilities,
            super_admin_role: None,
        }))
        .module(LighthouseModule::from_config())
        .module(McpModule::from_config())
        .build()
        .await;

    // An agent's user, with the role `ops`.
    let id = uuid::Uuid::now_v7();
    let access = app.get::<AccessService>().await;
    let db = app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    access.create_role(&uow, "ops", None).await.unwrap();
    access.grant(&uow, "ops", "lighthouse.view").await.unwrap();
    access
        .grant(&uow, "ops", "lighthouse.resolve")
        .await
        .unwrap();
    access.assign_role(&uow, id, "ops").await.unwrap();
    uow.commit().await.unwrap();

    // Something breaks.
    let lighthouse = app.get::<Lighthouse>().await;
    lighthouse.report(&std::io::Error::other("card declined"));
    lighthouse.flush().await;
    let issue = lighthouse
        .store()
        .unwrap()
        .issues(&IssueFilter::default())
        .await
        .unwrap()
        .data[0]
        .issue
        .id;

    let mcp = McpTestClient::new(app.acting_as(principal(id)))
        .initialize()
        .await;
    let tools: Vec<String> = mcp
        .list_tools()
        .await
        .iter()
        .map(|tool| tool.name.to_string())
        .collect();
    assert!(
        tools.contains(&"lighthouse_resolve_issue".to_owned()),
        "{tools:?}"
    );
    let page = mcp
        .call_tool("lighthouse_list_issues", json!({}))
        .await
        .assert_ok()
        .json_value();
    assert_eq!(page["data"][0]["title"], "card declined");
    mcp.call_tool(
        "lighthouse_resolve_issue",
        json!({ "issue_id": issue, "comment": "retry declined cards" }),
    )
    .await
    .assert_ok()
    .assert_json_path("status", json!("resolved"));
    assert_eq!(ISSUE_RESOURCE, "lighthouse://issues/{id}");
    assert!(
        mcp.list_prompts()
            .await
            .iter()
            .any(|prompt| prompt.name == FIX_ISSUE_PROMPT)
    );
    app.shutdown().await.unwrap();
}
