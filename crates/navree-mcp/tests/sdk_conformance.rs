//! Conformance with the official MCP TypeScript SDK: boots an app with an
//! MCP server, serves it over a real TCP socket (Streamable HTTP with a
//! bearer token from `AuthModule`, and the stdio transport behind a TCP
//! relay), and runs `e2e/mcp-sdk/conformance.mjs`, whose SDK client checks
//! the handshake, tools, resources and prompts over both transports.
//!
//! Needs Node.js 20 or later and the SDK installed (once):
//!
//! ```sh
//! npm ci --prefix e2e/mcp-sdk
//! cargo test -p navree-mcp --test sdk_conformance -- --ignored --nocapture
//! ```

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use garde::Validate;
use navree_auth::{AuthModule, AuthService, ClientMeta};
use navree_crypto::CryptoModule;
use navree_db::DatabaseModule;
use navree_http::AppError;
use navree_kernel::{Module, ModuleBuilder};
use navree_macros::{injectable, mcp_server};
use navree_mcp::{McpContext, McpModule, ModuleBuilderMcpExt, PromptMessage, serve_stdio};
use navree_testing::TestApp;
use serde::Deserialize;
use serde_json::{Value, json};

/// `greet`'s arguments.
#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct Greet {
    /// Who to greet.
    #[garde(length(min = 1, max = 50))]
    pub name: String,
}

/// `review`'s arguments.
#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct Review {
    /// What to review.
    #[garde(length(min = 1))]
    pub topic: String,
}

/// The server the SDK client speaks to.
#[injectable]
pub struct ConformanceMcp;

#[mcp_server]
#[authenticated]
impl ConformanceMcp {
    /// Greets someone by name.
    #[mcp_tool(read_only)]
    async fn greet(&self, args: Greet) -> Result<Value, AppError> {
        Ok(json!({ "greeting": format!("Hello, {}!", args.name) }))
    }

    /// Counts to two, reporting progress.
    #[mcp_tool]
    async fn count(&self, cx: &McpContext) -> Result<u32, AppError> {
        cx.progress(1.0, Some(2.0), "one").await;
        cx.progress(2.0, Some(2.0), "two").await;
        Ok(2)
    }

    /// The welcome note.
    #[mcp_resource("notes://welcome", mime = "text/plain")]
    async fn welcome(&self) -> Result<String, AppError> {
        Ok("Welcome to the notes.".to_owned())
    }

    /// A note by id.
    #[mcp_resource("notes://items/{id}", mime = "application/json")]
    async fn item(&self, id: u32) -> Result<Value, AppError> {
        Ok(json!({ "id": id, "text": format!("Note {id}") }))
    }

    /// Asks for a short review.
    #[mcp_prompt]
    async fn review(&self, args: Review) -> Result<Vec<PromptMessage>, AppError> {
        Ok(vec![
            PromptMessage::user(format!("Review {} in three sentences.", args.topic)),
            PromptMessage::assistant("Here is the review:"),
        ])
    }
}

/// Registers [`ConformanceMcp`].
pub struct ConformanceModule;

impl Module for ConformanceModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.mcp_server::<ConformanceMcp>();
    }
}

/// `e2e/mcp-sdk`.
fn sdk_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("e2e")
        .join("mcp-sdk")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs Node.js and `npm ci --prefix e2e/mcp-sdk`; run with --ignored"]
async fn the_official_typescript_sdk_speaks_to_the_server() {
    let dir = sdk_dir();
    assert!(
        dir.join("node_modules/@modelcontextprotocol/sdk").is_dir(),
        "install the SDK first: npm ci --prefix e2e/mcp-sdk"
    );
    let app = TestApp::builder()
        .temp_database()
        .config("http.per_ip_rate_limit", Value::Null)
        .config("app.name", "navree-sdk")
        .config("mcp.title", "Navree SDK conformance")
        .config("mcp.instructions", "Greets people and keeps notes.")
        .config("app.key", navree_crypto::generate_key())
        .config("hashing.memory_kib", 1024)
        .config("hashing.iterations", 1)
        .module(DatabaseModule)
        .module(CryptoModule::from_config())
        .module(AuthModule::from_config())
        .module(McpModule::from_config())
        .module(ConformanceModule)
        .build()
        .await;
    let auth = app.get::<AuthService>().await;
    let ada = auth
        .create_user("Ada", "ada@example.com", "a-long-password")
        .await
        .expect("user");
    let token = auth
        .issue_tokens(&ada, ClientMeta::default())
        .await
        .expect("tokens")
        .access_token;

    // Streamable HTTP on a real socket, through the app's full pipeline.
    let router = navree_http::build_router(app.ctx()).await.expect("router");
    let http = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let http_addr = http.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(
            http,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });

    // The stdio transport, one connection per client, as `mcp:serve` runs
    // it (the bridge relays the SDK's child-process stdio here).
    let stdio = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let stdio_port = stdio.local_addr().unwrap().port();
    let ctx = app.ctx().clone();
    let user = ada.id.to_string();
    tokio::spawn(async move {
        while let Ok((socket, _)) = stdio.accept().await {
            let (read, write) = socket.into_split();
            let ctx = ctx.clone();
            let user = user.clone();
            tokio::spawn(async move {
                if let Err(error) = serve_stdio(&ctx, &user, read, write).await {
                    eprintln!("stdio session failed: {error}");
                }
            });
        }
    });

    let run = tokio::process::Command::new("node")
        .arg(dir.join("conformance.mjs"))
        .current_dir(&dir)
        .env("MCP_HTTP_URL", format!("http://{http_addr}/mcp"))
        .env("MCP_TOKEN", &token)
        .env("MCP_STDIO_PORT", stdio_port.to_string())
        .kill_on_drop(true)
        .output();
    let output = tokio::time::timeout(Duration::from_secs(120), run)
        .await
        .expect("the conformance run finishes")
        .expect("node runs (is Node.js installed?)");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    println!("{stdout}");
    eprintln!("{stderr}");
    assert!(
        output.status.success(),
        "the SDK client failed:\n{stdout}\n{stderr}"
    );
    assert!(stdout.contains("ok stdio"), "{stdout}");
    assert!(stdout.contains("ok streamable-http"), "{stdout}");
}
