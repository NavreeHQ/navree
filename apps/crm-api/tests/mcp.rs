//! The CRM's MCP server at `/mcp` (`DealsMcp` from crm-deals and
//! `ActivitiesMcp` from crm-activities, merged by `McpModule`), end to end
//! through `crm_api::app`: what each role sees, the access checks of every
//! call, the resources and the follow-up prompt.

use crm_testkit::{CrmClient, Role, TestCrm};
use navree::mcp::testing::McpTestClient;
use serde_json::{Value, json};

async fn crm() -> TestCrm {
    TestCrm::builder().app(crm_api::app).build().await
}

/// POSTs a company and a deal in it as `client`; returns the deal.
async fn create_deal(client: &CrmClient, title: &str) -> Value {
    let company = client
        .post("/api/v1/companies")
        .json(&json!({ "name": format!("{title} Ltd") }))
        .send()
        .await
        .assert_status(201)
        .json_value();
    client
        .post("/api/v1/deals")
        .json(&json!({ "title": title, "company_id": company["id"], "amount_cents": 500_000 }))
        .send()
        .await
        .assert_status(201)
        .json_value()
}

/// An initialized MCP session as `client`.
async fn mcp(client: &CrmClient) -> McpTestClient {
    McpTestClient::new(client.inner().clone())
        .bearer(client.access_token())
        .initialize()
        .await
}

fn id(record: &Value) -> &str {
    record["id"].as_str().expect("an id")
}

/// The documentation's example (`ai/mcp-servers.mdx`, Testing).
#[tokio::test]
async fn reps_log_activity_but_cannot_move_others_deals() {
    let crm = crm().await;
    let owner = crm.login_as(Role::Rep1).await;
    let deal = create_deal(&owner, "Agra renewal").await;

    let rep = crm.login_as(Role::Rep2).await;
    let mcp = McpTestClient::new(rep.inner().clone())
        .bearer(rep.access_token())
        .initialize()
        .await;

    let tools = mcp.list_tools().await;
    assert!(tools.iter().any(|tool| tool.name == "log_activity"));

    mcp.call_tool(
        "log_activity",
        json!({ "deal_id": deal["id"], "kind": "call", "subject": "Daljit approved the budget" }),
    )
    .await
    .assert_ok()
    .assert_json_path("subject", json!("Daljit approved the budget"));

    mcp.call_tool(
        "move_deal",
        json!({ "deal_id": deal["id"], "stage": "won" }),
    )
    .await
    .assert_error(403, "Forbidden");

    mcp.read_resource(&format!("crm://deals/{}", deal["id"].as_str().unwrap()))
        .await
        .assert_json_path("stage", json!("lead"));
}

#[tokio::test]
async fn the_server_offers_the_deals_and_activities_items_with_hints() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let mcp = mcp(&rep).await;
    let info = mcp.server_info();
    assert_eq!(info["serverInfo"]["name"], "navree-crm");
    assert_eq!(info["serverInfo"]["title"], "Navree CRM");
    assert_eq!(info["instructions"], crm_core::MCP_INSTRUCTIONS);

    let tools = mcp.list_tools().await;
    let mut names: Vec<&str> = tools.iter().map(|tool| tool.name.as_ref()).collect();
    names.sort_unstable();
    assert_eq!(names, ["list_deals", "log_activity", "move_deal"]);
    let list = tools.iter().find(|tool| tool.name == "list_deals").unwrap();
    let hints = list.annotations.as_ref().expect("annotations");
    assert_eq!(hints.read_only_hint, Some(true));
    assert_eq!(hints.destructive_hint, Some(false));
    assert!(
        list.description
            .as_deref()
            .is_some_and(|text| text.starts_with("Lists the deals the user can see")),
        "{:?}",
        list.description
    );
    let schema = Value::Object((*list.input_schema).clone());
    assert_eq!(schema["properties"]["mine"]["type"], "boolean", "{schema}");
    let moving = tools.iter().find(|tool| tool.name == "move_deal").unwrap();
    let hints = moving.annotations.as_ref().expect("annotations");
    assert_eq!(hints.destructive_hint, Some(true));
    assert_eq!(hints.read_only_hint, Some(false));

    let resources = mcp.list_resources().await;
    assert_eq!(
        resources
            .iter()
            .map(|resource| resource.uri.as_str())
            .collect::<Vec<_>>(),
        ["crm://pipeline"]
    );
    let templates = mcp.list_resource_templates().await;
    assert_eq!(
        templates
            .iter()
            .map(|template| template.uri_template.as_str())
            .collect::<Vec<_>>(),
        ["crm://deals/{id}"]
    );
    let prompts = mcp.list_prompts().await;
    assert_eq!(prompts.len(), 1);
    assert_eq!(prompts[0].name, "follow-up-email");
    assert_eq!(prompts[0].title.as_deref(), Some("Draft a follow-up email"));
}

#[tokio::test]
async fn list_deals_filters_by_stage_title_and_owner() {
    let crm = crm().await;
    let rep1 = crm.login_as(Role::Rep1).await;
    let rep2 = crm.login_as(Role::Rep2).await;
    let mine = create_deal(&rep1, "Agra renewal").await;
    let theirs = create_deal(&rep2, "Gulati expansion").await;
    let mcp = mcp(&rep1).await;

    let all = mcp.call_tool("list_deals", json!({})).await.assert_ok();
    let titles: Vec<String> = all
        .json::<Vec<Value>>()
        .iter()
        .map(|deal| deal["title"].as_str().unwrap().to_owned())
        .collect();
    // Newest first; reps read every deal.
    assert_eq!(titles, ["Gulati expansion", "Agra renewal"]);

    let own = mcp
        .call_tool("list_deals", json!({ "mine": true }))
        .await
        .json::<Vec<Value>>();
    assert_eq!(own.len(), 1);
    assert_eq!(own[0]["id"], mine["id"]);

    let found = mcp
        .call_tool("list_deals", json!({ "query": "gulati" }))
        .await
        .json::<Vec<Value>>();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0]["id"], theirs["id"]);

    // A blank query searches for nothing; a stage and a query combine.
    let blank = mcp
        .call_tool("list_deals", json!({ "query": "   " }))
        .await
        .json::<Vec<Value>>();
    assert_eq!(blank.len(), 2);
    let combined = mcp
        .call_tool(
            "list_deals",
            json!({ "query": "agra", "stage": "lead", "mine": true }),
        )
        .await
        .json::<Vec<Value>>();
    assert_eq!(combined.len(), 1);
    assert_eq!(combined[0]["id"], mine["id"]);

    let won = mcp
        .call_tool("list_deals", json!({ "stage": "won" }))
        .await
        .json::<Vec<Value>>();
    assert!(won.is_empty());

    // Invalid arguments never reach the service: a 400 with field messages.
    let refused = mcp
        .call_tool("list_deals", json!({ "query": "x".repeat(101) }))
        .await
        .assert_error(400, None)
        .json_value();
    assert!(refused["errors"]["query"].is_array(), "{refused}");
}

#[tokio::test]
async fn owners_move_their_deals_and_lost_needs_a_reason() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let deal = create_deal(&rep, "Agra renewal").await;
    let mcp = mcp(&rep).await;

    mcp.call_tool(
        "move_deal",
        json!({ "deal_id": deal["id"], "stage": "lost" }),
    )
    .await
    .assert_error(400, None);
    mcp.call_tool(
        "move_deal",
        json!({ "deal_id": deal["id"], "stage": "qualified" }),
    )
    .await
    .assert_ok()
    .assert_json_path("stage", json!("qualified"));

    // The write committed: the REST API sees it.
    rep.get(&format!("/api/v1/deals/{}", id(&deal)))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("stage", json!("qualified"));

    mcp.call_tool(
        "move_deal",
        json!({ "deal_id": deal["id"], "stage": "lost", "lost_reason": "Went with a competitor" }),
    )
    .await
    .assert_ok()
    .assert_json_path("lost_reason", json!("Went with a competitor"));

    mcp.call_tool(
        "move_deal",
        json!({ "deal_id": "0199e0c0-0000-7000-8000-000000000000", "stage": "won" }),
    )
    .await
    .assert_error(404, None);
}

#[tokio::test]
async fn the_pipeline_and_deal_resources_read_with_the_users_access() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let deal = create_deal(&rep, "Agra renewal").await;
    let mcp = mcp(&rep).await;

    let pipeline = mcp.read_resource("crm://pipeline").await.assert_ok();
    assert_eq!(pipeline.mime_type().as_deref(), Some("application/json"));
    let board = pipeline.json_value();
    let stages = board["stages"].as_array().expect("stages");
    assert_eq!(stages[0]["stage"], "lead");
    assert_eq!(stages[0]["deals"][0]["id"], deal["id"]);

    mcp.read_resource(&format!("crm://deals/{}", id(&deal)))
        .await
        .assert_ok()
        .assert_json_path("title", json!("Agra renewal"))
        .assert_json_path("company.name", json!("Agra renewal Ltd"));
    mcp.read_resource("crm://deals/0199e0c0-0000-7000-8000-000000000000")
        .await
        .assert_error(404, None);
}

#[tokio::test]
async fn the_follow_up_prompt_embeds_the_deal() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let deal = create_deal(&rep, "Agra renewal").await;
    let mcp = mcp(&rep).await;

    let prompt = mcp
        .get_prompt("follow-up-email", json!({ "deal_id": id(&deal) }))
        .await
        .assert_ok();
    let messages = prompt.messages();
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert_eq!(messages[0]["content"]["type"], "resource");
    assert_eq!(
        messages[0]["content"]["resource"]["uri"],
        format!("crm://deals/{}", id(&deal))
    );
    let text = prompt.text(1).expect("the instruction");
    assert!(text.contains("the main contact"), "{text}");

    mcp.get_prompt("follow-up-email", json!({ "deal_id": "not-a-uuid" }))
        .await
        .assert_error(400, None);
}

#[tokio::test]
async fn activities_logged_over_mcp_are_the_users_and_audited() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let deal = create_deal(&rep, "Agra renewal").await;
    let mcp = mcp(&rep).await;

    let logged = mcp
        .call_tool(
            "log_activity",
            json!({
                "deal_id": deal["id"],
                "kind": "task",
                "subject": "  Send the contract  ",
                "due_at": "2026-11-02T09:00:00Z",
            }),
        )
        .await
        .assert_ok()
        .assert_json_path("subject", json!("Send the contract"))
        .json_value();
    assert_eq!(logged["kind"], "task");

    let listed = rep
        .get(&format!(
            "/api/v1/activities?filter[subject_type]=deal&filter[subject_id]={}",
            id(&deal)
        ))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(listed["data"][0]["id"], logged["id"]);

    mcp.call_tool(
        "log_activity",
        json!({ "deal_id": deal["id"], "kind": "call", "subject": "" }),
    )
    .await
    .assert_error(400, None);
}

#[tokio::test]
async fn the_endpoint_needs_a_token() {
    let crm = crm().await;
    let response = crm
        .client()
        .post("/mcp")
        .json(&json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }))
        .send()
        .await
        .assert_status(401);
    assert!(
        response
            .header("www-authenticate")
            .is_some_and(|value| value.starts_with("Bearer")),
        "{:?}",
        response.header("www-authenticate")
    );
}

#[tokio::test]
async fn the_openapi_document_lists_the_endpoint_once_per_method() {
    let crm = crm().await;
    let doc = crm
        .client()
        .get("/api/openapi.json")
        .send()
        .await
        .assert_status(200)
        .json_value();
    for (method, operation_id) in [
        ("post", "mcp.message"),
        ("get", "mcp.stream"),
        ("delete", "mcp.close"),
    ] {
        let operation = &doc["paths"]["/mcp"][method];
        assert_eq!(operation["operationId"], operation_id, "{operation}");
        assert_eq!(operation["tags"], json!(["MCP"]), "{operation}");
    }
}
