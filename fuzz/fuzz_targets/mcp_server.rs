//! `navree_mcp`, end to end: an MCP server booted once in the process
//! (in-memory database, a hand-written server with a tool, a resource
//! template and a prompt), spoken to with attacker-chosen bytes over the
//! HTTP transport (`POST /mcp`, with and without a session) and over stdio.
//!
//! The input is a mode byte, then messages separated by NUL bytes.
//!
//! Invariants: a request never takes the server down or past a timeout;
//! bad input is the client's error, never an internal one (no 5xx, no
//! JSON-RPC `-32603`); every line the stdio server writes is a JSON-RPC 2.0
//! message; the stdio server ends when the client closes its side.
#![no_main]

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use garde::Validate;
use libfuzzer_sys::fuzz_target;
use navree_fuzz::block_on;
use navree_http::AppError;
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_mcp::{
    Call, McpItem, McpModule, McpOutput, McpServer, ModuleBuilderMcpExt, PromptDef, ResourceDef,
    ToolDef,
};
use navree_testing::{TestApp, TestClient, principal};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use uuid::Uuid;

const PATIENCE: Duration = Duration::from_secs(20);

#[derive(Deserialize, Validate, utoipa::ToSchema)]
struct Echo {
    /// What to repeat.
    #[garde(length(max = 64))]
    text: String,
    /// How many times.
    #[garde(range(max = 100))]
    times: Option<u32>,
}

#[derive(Deserialize, Validate, utoipa::ToSchema)]
struct Greet {
    /// Who to greet.
    #[garde(length(min = 1, max = 32))]
    name: String,
}

struct FuzzServer;

impl Injectable for FuzzServer {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(FuzzServer) })
    }
}

impl McpServer for FuzzServer {
    fn items(self: Arc<Self>) -> Vec<McpItem> {
        vec![
            McpItem::tool(
                ToolDef::new("echo")
                    .description("Repeats text.")
                    .read_only()
                    .authenticated()
                    .input::<Echo>(),
                |call: Call| async move {
                    let args: Echo = call.args().await?;
                    let times = args.times.unwrap_or(1) as usize;
                    Ok::<_, AppError>(McpOutput::Json(json!({ "text": args.text.repeat(times) })))
                },
            ),
            McpItem::tool(
                ToolDef::new("whoami").read_only().public(),
                |call: Call| async move {
                    Ok::<_, AppError>(McpOutput::Json(json!({
                        "user": call.context().user_id(),
                        "arguments": call.arguments().clone(),
                    })))
                },
            ),
            McpItem::resource(
                ResourceDef::new("fuzz://items/{id}", "item")
                    .mime_type("application/json")
                    .authenticated(),
                |call: Call| async move {
                    let id: u32 = call.param("id")?;
                    Ok::<_, AppError>(McpOutput::Json(json!({ "id": id })))
                },
            ),
            McpItem::resource(
                ResourceDef::new("fuzz://about", "about").authenticated(),
                |_call: Call| async move { Ok::<_, AppError>(McpOutput::Json(json!("about"))) },
            ),
            McpItem::prompt(
                PromptDef::new("greet").input::<Greet>().authenticated(),
                |call: Call| async move {
                    let args: Greet = call.args().await?;
                    Ok::<_, AppError>(McpOutput::Json(json!(format!("Say hello to {}.", args.name))))
                },
            ),
        ]
    }
}

struct FuzzModule;

impl Module for FuzzModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.mcp_server::<FuzzServer>();
    }
}

struct Fixture {
    app: TestApp,
    client: TestClient,
    user: Uuid,
}

fn fixture() -> &'static Fixture {
    static FIXTURE: OnceLock<Fixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        block_on(async {
            let app = TestApp::builder()
                .memory_database()
                .config("http.per_ip_rate_limit", Value::Null)
                .config("app.name", "navree-fuzz")
                .module(McpModule::from_config())
                .module(FuzzModule)
                .build()
                .await;
            let user = Uuid::now_v7();
            let client = app.acting_as(principal(user));
            Fixture { app, client, user }
        })
    })
}

const VERSIONS: [&str; 5] = [
    "2024-11-05",
    "2025-03-26",
    "2025-06-18",
    "2025-11-25",
    "2026-07-28",
];

fn initialize(version: &str) -> Value {
    json!({
        "jsonrpc": "2.0", "id": 0, "method": "initialize",
        "params": {
            "protocolVersion": version,
            "capabilities": {},
            "clientInfo": { "name": "fuzz", "version": "1" },
        },
    })
}

/// No answer may be an internal error: the input was the client's.
fn check_answer(answer: &Value) {
    let messages = match answer {
        Value::Array(items) => items.iter().collect(),
        other => vec![other],
    };
    for message in messages {
        assert_eq!(message["jsonrpc"], "2.0", "not JSON-RPC: {message}");
        assert_ne!(
            message["error"]["code"],
            json!(-32603),
            "an internal error: {message}"
        );
    }
}

async fn http(version: usize, session: bool, messages: &[&[u8]]) {
    let fixture = fixture();
    let version = VERSIONS[version % VERSIONS.len()];
    let post = |body: Vec<u8>| {
        fixture
            .client
            .post("/mcp")
            .without_header("origin")
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json")
            .body(body)
    };
    let mut session_id = None;
    if session {
        let response = post(initialize(version).to_string().into_bytes())
            .header("mcp-protocol-version", version)
            .send()
            .await;
        session_id = response.header("mcp-session-id").map(str::to_owned);
    }
    for message in messages {
        let mut request = post(message.to_vec()).header("mcp-protocol-version", version);
        if let Some(id) = &session_id {
            request = request.header("mcp-session-id", id.as_str());
        }
        let response = request.send().await;
        let status = response.status().as_u16();
        assert!(status < 500, "HTTP {status} for {:?}", String::from_utf8_lossy(message));
        let is_json = response
            .header("content-type")
            .is_some_and(|kind| kind.starts_with("application/json"));
        if is_json && !response.bytes().is_empty() {
            if let Ok(answer) = serde_json::from_slice::<Value>(response.bytes()) {
                if answer.get("jsonrpc").is_some() || answer.is_array() {
                    check_answer(&answer);
                }
            }
        }
    }
    if let Some(id) = session_id {
        let _ = fixture
            .client
            .delete("/mcp")
            .header("mcp-session-id", id.as_str())
            .header("mcp-protocol-version", version)
            .send()
            .await;
    }
}

async fn stdio(version: usize, handshake: bool, messages: &[&[u8]]) {
    let fixture = fixture();
    let (client_side, server_side) = tokio::io::duplex(1 << 16);
    let (server_read, server_write) = tokio::io::split(server_side);
    let ctx = fixture.app.ctx().clone();
    let user = fixture.user.to_string();
    let server = tokio::spawn(async move {
        navree_mcp::serve_stdio(&ctx, &user, server_read, server_write).await
    });
    let (reader, mut writer) = tokio::io::split(client_side);
    let version = VERSIONS[version % VERSIONS.len()];
    let mut lines: Vec<Vec<u8>> = Vec::new();
    if handshake {
        lines.push(initialize(version).to_string().into_bytes());
        lines.push(
            json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })
                .to_string()
                .into_bytes(),
        );
    }
    lines.extend(messages.iter().map(|message| message.to_vec()));
    let write = async move {
        for line in lines {
            writer.write_all(&line).await.expect("the pipe is open");
            writer.write_all(b"\n").await.expect("the pipe is open");
        }
        writer.shutdown().await.expect("the pipe closes");
        // Keep the write half until the reader is done: dropping it is
        // enough to end the server's input, which `shutdown` already did.
    };
    let read = async move {
        let mut reader = BufReader::new(reader);
        let mut line = String::new();
        loop {
            line.clear();
            if reader.read_line(&mut line).await.expect("a read") == 0 {
                break;
            }
            let message: Value = serde_json::from_str(&line)
                .unwrap_or_else(|error| panic!("the server wrote {line:?}: {error}"));
            check_answer(&message);
        }
    };
    tokio::join!(write, read);
    tokio::time::timeout(PATIENCE, server)
        .await
        .expect("the server ends with the client")
        .expect("the server does not panic")
        .expect("the server ends cleanly");
}

fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    let messages: Vec<&[u8]> = rest.split(|byte| *byte == 0).take(8).collect();
    // Boot the server outside the runtime that drives the requests.
    let _ = fixture();
    let version = usize::from(mode >> 2);
    block_on(async {
        let run = async {
            match mode & 3 {
                0 => http(version, false, &messages).await,
                1 => http(version, true, &messages).await,
                2 => stdio(version, true, &messages).await,
                _ => stdio(version, false, &messages).await,
            }
        };
        tokio::time::timeout(PATIENCE, run)
            .await
            .expect("a request took too long");
    });
});
