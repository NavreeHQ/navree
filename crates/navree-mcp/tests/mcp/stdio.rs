//! The stdio transport (`mcp:serve`): the process acts as one user, spoken
//! to over an in-memory pipe.

use navree_auth::{AuthModule, AuthService};
use navree_crypto::CryptoModule;
use navree_mcp::serve_stdio;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::stdio_wire::LineClient;
use crate::support::{builder, give_role, insert_deal};

/// The tool names a stdio client sees.
async fn tool_names(client: &mut LineClient, id: i64) -> Vec<String> {
    client.request(id, "tools/list", json!({})).await["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["name"].as_str().map(str::to_owned))
        .collect()
}

#[tokio::test]
async fn the_process_acts_as_the_user_given_by_id() {
    let app = builder().build().await;
    let rep = Uuid::now_v7();
    let deal = insert_deal(&app, rep, "Agra renewal", "lead").await;
    let mut client = LineClient::connect(&app, &rep.to_string());

    let info = client.initialize("2025-11-25").await;
    assert_eq!(info["serverInfo"]["name"], "navree-crm");

    let tools = tool_names(&mut client, 1).await;
    assert!(tools.iter().any(|tool| tool == "list_deals"));
    assert!(tools.iter().all(|tool| tool != "purge_deals"));

    let renamed = client
        .request(
            2,
            "tools/call",
            json!({ "name": "rename_deal", "arguments": { "deal_id": deal.id, "title": "Renamed" } }),
        )
        .await;
    assert_eq!(renamed["result"]["isError"], false);
    assert_eq!(renamed["result"]["structuredContent"]["title"], "Renamed");

    let whoami = client
        .request(3, "tools/call", json!({ "name": "whoami" }))
        .await;
    let whoami = &whoami["result"]["structuredContent"];
    assert_eq!(whoami["user_id"], json!(rep));
    assert_eq!(whoami["session"], true, "the stdio session has an id");
    assert_eq!(whoami["client"], "line-client");

    let read = client
        .request(
            4,
            "resources/read",
            json!({ "uri": format!("crm://deals/{}", deal.id) }),
        )
        .await;
    assert_eq!(read["result"]["contents"].as_array().unwrap().len(), 1);

    client
        .close()
        .await
        .expect("the server ends with the client");
}

#[tokio::test]
async fn roles_apply_to_the_stdio_user() {
    let app = builder().build().await;
    let admin = Uuid::now_v7();
    give_role(&app, admin, "admin", &[]).await;
    let mut client = LineClient::connect(&app, &admin.to_string());
    client.initialize("2025-11-25").await;
    let tools = tool_names(&mut client, 1).await;
    assert!(tools.iter().any(|tool| tool == "purge_deals"));
    client.close().await.unwrap();
}

#[tokio::test]
async fn the_user_may_be_named_by_email_with_auth_module() {
    let app = builder()
        .config("app.key", navree_crypto::generate_key())
        .config("hashing.memory_kib", 1024)
        .config("hashing.iterations", 1)
        .module(CryptoModule::from_config())
        .module(AuthModule::from_config())
        .build()
        .await;
    let ada = app
        .get::<AuthService>()
        .await
        .create_user("Anu", "anu@example.com", "a-long-password")
        .await
        .expect("user");
    let mut client = LineClient::connect(&app, "anu@example.com");
    client.initialize("2025-11-25").await;
    let whoami: Value = client
        .request(1, "tools/call", json!({ "name": "whoami" }))
        .await["result"]["structuredContent"]
        .clone();
    assert_eq!(whoami["user_id"], json!(ada.id));
    client.close().await.unwrap();

    let (_client_side, server_side) = tokio::io::duplex(1024);
    let (read, write) = tokio::io::split(server_side);
    let error = serve_stdio(app.ctx(), "nobody@example.com", read, write)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("no user with email"), "{error}");

    let (_client_side, server_side) = tokio::io::duplex(1024);
    let (read, write) = tokio::io::split(server_side);
    let error = serve_stdio(app.ctx(), &Uuid::now_v7().to_string(), read, write)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("no user with id"), "{error}");
}

#[tokio::test]
async fn an_email_needs_auth_module() {
    let app = builder().build().await;
    let (_client_side, server_side) = tokio::io::duplex(1024);
    let (read, write) = tokio::io::split(server_side);
    let error = serve_stdio(app.ctx(), "anu@example.com", read, write)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("needs AuthModule"), "{error}");
}

#[tokio::test]
async fn the_command_is_registered_and_owns_stdout() {
    let app = builder().build().await;
    let command = app
        .ctx()
        .commands()
        .into_iter()
        .find(|command| command.name == "mcp:serve")
        .expect("mcp:serve");
    assert!(command.stdio);
    let error = app.run_command("mcp:serve", &[]).await.unwrap_err();
    assert!(error.to_string().contains("--user"), "{error}");
}
