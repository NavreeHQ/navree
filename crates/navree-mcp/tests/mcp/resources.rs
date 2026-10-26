//! Resources: fixed URIs and templates, JSON, text and binary contents,
//! record checks and visibility.

use navree_mcp::testing::McpTestClient;
use serde_json::json;
use uuid::Uuid;

use crate::support::{app, give_role, insert_deal, user};

#[tokio::test]
async fn fixed_resources_and_templates_are_listed_apart() {
    let app = app().await;
    let mcp = McpTestClient::new(app.acting_as(user())).initialize().await;

    let resources = mcp.list_resources().await;
    let uris: Vec<&str> = resources.iter().map(|r| r.uri.as_str()).collect();
    assert_eq!(uris, ["crm://pipeline"], "admin stats are hidden");
    let pipeline = serde_json::to_value(&resources[0]).unwrap();
    assert_eq!(pipeline["name"], "pipeline");
    assert_eq!(pipeline["title"], "Pipeline");
    assert_eq!(pipeline["mimeType"], "application/json");
    assert_eq!(
        pipeline["description"],
        "The pipeline: how many deals each stage has."
    );

    let templates = mcp.list_resource_templates().await;
    let mut uris: Vec<&str> = templates.iter().map(|t| t.uri_template.as_str()).collect();
    uris.sort();
    assert_eq!(
        uris,
        [
            "crm://deals/{id}",
            "crm://files/{name}",
            "crm://notes/{slug}",
            "crm://numbers/{n}"
        ]
    );
    assert!(templates.iter().any(|t| t.name == "number"));

    let admin = user();
    give_role(&app, admin.user_id, "admin", &[]).await;
    let mcp = McpTestClient::new(app.acting_as(admin)).initialize().await;
    let uris: Vec<String> = mcp
        .list_resources()
        .await
        .into_iter()
        .map(|r| r.uri)
        .collect();
    assert!(uris.contains(&"crm://admin/stats".to_owned()), "{uris:?}");
}

#[tokio::test]
async fn templates_load_and_authorize_their_record() {
    let app = app().await;
    let rep = user();
    let deal = insert_deal(&app, Uuid::now_v7(), "Acme renewal", "qualified").await;
    let mcp = McpTestClient::new(app.acting_as(rep)).initialize().await;

    let read = mcp
        .read_resource(&format!("crm://deals/{}", deal.id))
        .await
        .assert_ok()
        .assert_json_path("title", json!("Acme renewal"))
        .assert_json_path("stage", json!("qualified"));
    assert_eq!(read.mime_type().as_deref(), Some("application/json"));
    assert_eq!(
        read.contents()[0]["uri"],
        format!("crm://deals/{}", deal.id)
    );

    mcp.read_resource(&format!("crm://deals/{}", Uuid::now_v7()))
        .await
        .assert_error(404, "Not Found");
    mcp.read_resource("crm://deals/not-a-uuid")
        .await
        .assert_error(404, None);
}

#[tokio::test]
async fn record_checks_refuse_with_403() {
    let app = app().await;
    let deal = insert_deal(&app, Uuid::now_v7(), "Secret", "secret").await;
    let mcp = McpTestClient::new(app.acting_as(user())).initialize().await;
    mcp.read_resource(&format!("crm://deals/{}", deal.id))
        .await
        .assert_error(403, "Forbidden");

    let admin = user();
    give_role(&app, admin.user_id, "admin", &[]).await;
    let mcp = McpTestClient::new(app.acting_as(admin)).initialize().await;
    mcp.read_resource(&format!("crm://deals/{}", deal.id))
        .await
        .assert_ok()
        .assert_json_path("title", json!("Secret"));
}

#[tokio::test]
async fn resources_a_token_may_not_read_are_unknown() {
    let app = app().await;
    let deal = insert_deal(&app, Uuid::now_v7(), "Acme", "lead").await;
    // A token limited to creating activities cannot read deals.
    let mut limited = user();
    limited.token_abilities = Some(vec!["activities.create".to_owned()]);
    let mcp = McpTestClient::new(app.acting_as(limited))
        .initialize()
        .await;
    assert!(
        mcp.list_resource_templates()
            .await
            .iter()
            .all(|t| t.name != "deal")
    );
    let error = mcp
        .read_resource(&format!("crm://deals/{}", deal.id))
        .await
        .response()
        .clone()
        .error();
    assert_eq!(error["code"], -32002, "{error}");
}

#[tokio::test]
async fn values_strings_and_blobs() {
    let app = app().await;
    let rep = user();
    insert_deal(&app, rep.user_id, "A", "lead").await;
    insert_deal(&app, rep.user_id, "B", "lead").await;
    insert_deal(&app, rep.user_id, "C", "won").await;
    let mcp = McpTestClient::new(app.acting_as(rep)).initialize().await;

    mcp.read_resource("crm://pipeline")
        .await
        .assert_ok()
        .assert_json_path("lead", json!(2))
        .assert_json_path("won", json!(1));

    let note = mcp.read_resource("crm://notes/kickoff").await.assert_ok();
    assert_eq!(note.text().as_deref(), Some("Note kickoff"));
    assert_eq!(note.mime_type().as_deref(), Some("text/plain"));

    let decoded = mcp.read_resource("crm://notes/q3%20plan").await;
    assert_eq!(decoded.text().as_deref(), Some("Note q3 plan"));

    mcp.read_resource("crm://numbers/21")
        .await
        .assert_ok()
        .assert_json_path("", json!(42));
    mcp.read_resource("crm://numbers/many")
        .await
        .assert_error(404, None);

    let file = mcp.read_resource("crm://files/contract").await.assert_ok();
    assert_eq!(file.mime_type().as_deref(), Some("application/pdf"));
    assert_eq!(file.blob().as_deref(), Some(&b"%PDF contract"[..]));
}

#[tokio::test]
async fn unknown_uris_are_resource_not_found() {
    let app = app().await;
    let mcp = McpTestClient::new(app.acting_as(user())).initialize().await;
    for uri in ["crm://contacts/1", "crm://admin/stats", "crm://deals/"] {
        let error = mcp.read_resource(uri).await.response().clone().error();
        assert_eq!(error["code"], -32002, "{uri}: {error}");
    }
}
