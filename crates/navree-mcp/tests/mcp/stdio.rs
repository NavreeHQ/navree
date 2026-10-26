//! The stdio transport (`mcp:serve`), spoken to by the official SDK's
//! client over an in-memory pipe.

use navree_auth::{AuthModule, AuthService};
use navree_crypto::CryptoModule;
use navree_mcp::rmcp::ServiceExt;
use navree_mcp::rmcp::model::{CallToolRequestParams, ReadResourceRequestParams};
use navree_mcp::serve_stdio;
use navree_testing::TestApp;
use serde_json::json;
use uuid::Uuid;

use crate::support::{builder, give_role, insert_deal};

/// Serves `app` over a pipe as `user`; returns the connected client and the
/// server task.
async fn connect(
    app: &TestApp,
    user: &str,
) -> (
    navree_mcp::rmcp::service::RunningService<navree_mcp::rmcp::RoleClient, ()>,
    tokio::task::JoinHandle<Result<(), navree_kernel::BoxError>>,
) {
    let (client_side, server_side) = tokio::io::duplex(1 << 16);
    let (server_read, server_write) = tokio::io::split(server_side);
    let ctx = app.ctx().clone();
    let user = user.to_owned();
    let server =
        tokio::spawn(async move { serve_stdio(&ctx, &user, server_read, server_write).await });
    let (client_read, client_write) = tokio::io::split(client_side);
    let client = ().serve((client_read, client_write)).await.expect("initialize");
    (client, server)
}

#[tokio::test]
async fn the_process_acts_as_the_user_given_by_id() {
    let app = builder().build().await;
    let rep = Uuid::now_v7();
    let deal = insert_deal(&app, rep, "Acme renewal", "lead").await;
    let (client, server) = connect(&app, &rep.to_string()).await;

    let info = client.peer_info().expect("server info");
    assert_eq!(
        info.server_info.as_ref().map(|server| server.name.as_str()),
        Some("navree-crm")
    );

    let tools = client.list_all_tools().await.unwrap();
    assert!(tools.iter().any(|tool| tool.name == "list_deals"));
    assert!(tools.iter().all(|tool| tool.name != "purge_deals"));

    let arguments = json!({ "deal_id": deal.id, "title": "Renamed" });
    let result = client
        .call_tool(
            CallToolRequestParams::new("rename_deal")
                .with_arguments(arguments.as_object().unwrap().clone()),
        )
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(false));
    assert_eq!(result.structured_content.unwrap()["title"], "Renamed");

    let whoami = client
        .call_tool(CallToolRequestParams::new("whoami"))
        .await
        .unwrap();
    let whoami = whoami.structured_content.unwrap();
    assert_eq!(whoami["user_id"], json!(rep));
    assert_eq!(whoami["session"], true, "the stdio session has an id");

    let read = client
        .read_resource(ReadResourceRequestParams::new(format!(
            "crm://deals/{}",
            deal.id
        )))
        .await
        .unwrap();
    assert_eq!(read.contents.len(), 1);

    client.cancel().await.unwrap();
    server
        .await
        .unwrap()
        .expect("the server ends with the client");
}

#[tokio::test]
async fn roles_apply_to_the_stdio_user() {
    let app = builder().build().await;
    let admin = Uuid::now_v7();
    give_role(&app, admin, "admin", &[]).await;
    let (client, server) = connect(&app, &admin.to_string()).await;
    let tools = client.list_all_tools().await.unwrap();
    assert!(tools.iter().any(|tool| tool.name == "purge_deals"));
    client.cancel().await.unwrap();
    server.await.unwrap().unwrap();
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
        .create_user("Ada", "ada@example.com", "a-long-password")
        .await
        .expect("user");
    let (client, server) = connect(&app, "ada@example.com").await;
    let whoami = client
        .call_tool(CallToolRequestParams::new("whoami"))
        .await
        .unwrap()
        .structured_content
        .unwrap();
    assert_eq!(whoami["user_id"], json!(ada.id));
    client.cancel().await.unwrap();
    server.await.unwrap().unwrap();

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
    let error = serve_stdio(app.ctx(), "ada@example.com", read, write)
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
