//! MCP with the app's own OAuth server (`ai/mcp-servers.mdx`, OAuth and
//! bearer tokens): with `mcp.auth = "oauth"` an unauthenticated client is
//! pointed at the protected resource metadata, which names the app's
//! authorization server and its scopes, and a personal access token from
//! `OAuthServerModule` reaches `/mcp` narrowed to its scopes. Runs with
//! `cargo test -p navree --features mcp,oauth --test mcp_oauth`.

// Inside this crate's own tests the macros name `crate::<module>`.
use navree::{access, kernel, mcp};

use navree::access::AccessService;
use navree::auth::AuthService;
use navree::db::Db;
use navree::mcp::mcp_server;
use navree::mcp::testing::McpTestClient;
use navree::oauth::PersonalAccessTokens;
use navree::prelude::*;
use navree::testing::{TestApp, TestAppBuilder};
use serde_json::{Value, json};

const APP_URL: &str = "https://crm.example.com";
const PASSWORD: &str = "correct horse battery";

/// One tool per permission.
#[injectable]
pub struct DealsMcp;

#[mcp_server]
impl DealsMcp {
    /// Lists the deals.
    #[mcp_tool(read_only)]
    #[permissions("deals.read")]
    async fn list_deals(&self) -> Result<Vec<String>> {
        Ok(vec!["Agra renewal".to_owned()])
    }

    /// Creates a deal.
    #[mcp_tool]
    #[permissions("deals.create")]
    async fn create_deal(&self) -> Result<String> {
        Ok("created".to_owned())
    }
}

/// Registers [`DealsMcp`].
#[module(register = Self::wire)]
pub struct DealsModule;

impl DealsModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.mcp_server::<DealsMcp>();
    }
}

fn builder() -> TestAppBuilder {
    TestApp::builder()
        .temp_database()
        .config("app.key", navree::crypto::generate_key())
        .config("app.url", APP_URL)
        .config("hashing.memory_kib", 1024)
        .config("hashing.iterations", 1)
        .config("http.per_ip_rate_limit", Value::Null)
        .config("oauth.consent_url", format!("{APP_URL}/oauth/consent"))
        .config(
            "oauth.scopes",
            json!({
                "deals:read": { "description": "Read your deals", "permissions": ["deals.read"] },
                "deals:write": {
                    "description": "Create and change your deals",
                    "permissions": ["deals.read", "deals.create"]
                },
            }),
        )
        .config("mcp.auth", "oauth")
        .module(CryptoModule::from_config())
        .module(DatabaseModule::from_config())
        .module(AuthModule::from_config())
        .module(AccessModule::for_root(
            navree::access::AccessOptions::default(),
        ))
        .module(OAuthServerModule::from_config())
        .module(McpModule::from_config())
        .module(DealsModule)
}

/// A user whose role grants `deals.read` and `deals.create`; returns the
/// user's id and a navree-auth access token.
async fn rep(app: &TestApp) -> (uuid::Uuid, String) {
    let auth = app.get::<AuthService>().await;
    let user = auth
        .create_user("Anu Lakhotia", "anu@example.com", PASSWORD)
        .await
        .expect("the user is created");
    let access = app.get::<AccessService>().await;
    let uow = app.get::<Db>().await.uow(None, None).await;
    access.create_role(&uow, "rep", None).await.unwrap();
    for permission in ["deals.read", "deals.create"] {
        access.grant(&uow, "rep", permission).await.unwrap();
    }
    access.assign_role(&uow, user.id, "rep").await.unwrap();
    uow.commit().await.unwrap();
    let token = app
        .client()
        .post("/api/auth/login")
        .json(&json!({ "email": "anu@example.com", "password": PASSWORD }))
        .send()
        .await
        .assert_status(200)
        .json_value()["access_token"]
        .as_str()
        .expect("an access token")
        .to_owned();
    (user.id, token)
}

/// The tool names a client with `token` sees.
async fn tools(app: &TestApp, token: &str) -> Vec<String> {
    let mcp = McpTestClient::new(app.client())
        .bearer(token)
        .initialize()
        .await;
    let mut names: Vec<String> = mcp
        .list_tools()
        .await
        .iter()
        .map(|tool| tool.name.to_string())
        .collect();
    names.sort();
    names
}

#[test]
fn the_macros_name_the_facade() {
    let _ = (access::READ, kernel::ModuleId::of::<DealsModule>());
    let _ = mcp::McpAuth::Oauth;
}

#[tokio::test]
async fn clients_find_the_apps_authorization_server_and_scopes() {
    let app = builder().build().await;

    let refused = app
        .client()
        .post("/mcp")
        .header("accept", "application/json, text/event-stream")
        .json(&json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }))
        .send()
        .await
        .assert_status(401);
    let metadata_url = format!("{APP_URL}/.well-known/oauth-protected-resource/mcp");
    assert_eq!(
        refused.header("www-authenticate"),
        Some(format!("Bearer resource_metadata=\"{metadata_url}\"").as_str())
    );

    let metadata = app
        .client()
        .get("/.well-known/oauth-protected-resource/mcp")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(metadata["resource"], format!("{APP_URL}/mcp"));
    assert_eq!(metadata["authorization_servers"], json!([APP_URL]));
    // The OAuth server's scopes, unless `mcp.oauth.scopes` lists others.
    assert_eq!(
        metadata["scopes_supported"],
        json!(["deals:read", "deals:write"])
    );

    // The authorization server it names is the app's own.
    app.client()
        .get("/.well-known/oauth-authorization-server")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("issuer", json!(APP_URL));
}

#[tokio::test]
async fn personal_access_tokens_reach_mcp_narrowed_to_their_scopes() {
    let app = builder().build().await;
    let (user_id, jwt) = rep(&app).await;

    // Signed in, the user sees every tool their role allows.
    assert_eq!(tools(&app, &jwt).await, ["create_deal", "list_deals"]);

    // A personal access token limited to `deals:read` sees less.
    let pats = app.get::<PersonalAccessTokens>().await;
    let uow = app.get::<Db>().await.uow(None, None).await;
    let created = pats
        .create(&uow, user_id, "Claude", ["deals:read"], None)
        .await
        .expect("the token is created");
    uow.commit().await.unwrap();
    assert!(created.token.starts_with("nvp_"), "{}", created.token);
    assert_eq!(tools(&app, &created.token).await, ["list_deals"]);

    let mcp = McpTestClient::new(app.client())
        .bearer(&created.token)
        .initialize()
        .await;
    mcp.call_tool("list_deals", json!({}))
        .await
        .assert_ok()
        .assert_json_path("0", json!("Agra renewal"));
    let hidden = mcp.call_tool("create_deal", json!({})).await;
    assert_eq!(
        hidden.response().clone().error()["message"],
        "Unknown tool: create_deal"
    );

    // A token that is not one is refused.
    app.client()
        .post("/mcp")
        .bearer("nvp_not-a-token")
        .json(&json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }))
        .send()
        .await
        .assert_status(401);
}
