//! The transport's security requirements, one test (or more) each:
//! Origin checks against DNS rebinding, random session ids bound to their
//! user, body size and JSON depth limits, batches only where the protocol
//! allows them, unknown methods without side effects, tool errors that do
//! not leak, cancellation that stops work, bounded queues for slow
//! clients, and the authorization challenge.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use http::header::ORIGIN;
use http_body_util::BodyExt;
use navree_contracts::AuditSink;
use navree_http::AppError;
use navree_kernel::{Module, ModuleBuilder};
use navree_macros::{injectable, mcp_server};
use navree_mcp::protocol::MAX_JSON_DEPTH;
use navree_mcp::testing::McpTestClient;
use navree_mcp::{McpContext, McpModule, McpNotifier, ModuleBuilderMcpExt, PromptMessage};
use navree_testing::{TestApp, TestClient};
use navree_validation::Validate;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::stdio_wire::LineClient;
use crate::support::{RecordingAudit, all_deals, app, builder, insert_deal, user};
use crate::wire::{Session, answer, post};

const WAIT: Duration = Duration::from_secs(10);

/// How many times `count_slowly` counted, per counter (one per test, as
/// tests run in parallel).
static COUNTED: [AtomicU64; 3] = [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];

/// Which counter `count_slowly` counts on.
#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct Counter {
    /// The counter's index.
    #[validate(range(max = 2))]
    pub counter: usize,
}

/// A server whose items fail inside or run until cancelled.
#[injectable]
pub struct GuardedMcp;

#[mcp_server]
#[authenticated]
impl GuardedMcp {
    /// Panics with a secret in the message.
    #[mcp_tool]
    async fn panics(&self) -> Result<String, AppError> {
        panic!("the vault key is hunter2");
    }

    /// Fails with a message the client may see.
    #[mcp_tool]
    async fn refuses(&self) -> Result<String, AppError> {
        Err(AppError::conflict("The quarter is closed"))
    }

    /// Counts until it is cancelled.
    #[mcp_tool]
    async fn count_slowly(&self, _cx: &McpContext, args: Counter) -> Result<u64, AppError> {
        for _ in 0..1000 {
            COUNTED[args.counter].fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        Ok(0)
    }

    /// A secret that fails to load.
    #[mcp_resource("vault://secret")]
    async fn secret(&self) -> Result<String, AppError> {
        Err(AppError::internal("the vault password is hunter2"))
    }

    /// A prompt that fails to render.
    #[mcp_prompt]
    async fn broken(&self) -> Result<Vec<PromptMessage>, AppError> {
        Err(AppError::internal("the template path is /etc/hunter2"))
    }
}

pub struct GuardedModule;

impl Module for GuardedModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.mcp_server::<GuardedMcp>();
    }
}

async fn guarded() -> TestApp {
    builder().module(GuardedModule).build().await
}

fn request(id: i64, method: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
}

fn initialize(version: &str) -> Value {
    request(
        1,
        "initialize",
        json!({
            "protocolVersion": version,
            "capabilities": {},
            "clientInfo": { "name": "security", "version": "1" },
        }),
    )
}

/// Waits until `count_slowly` has counted a few more times on `counter`.
async fn wait_for_counting(counter: usize) {
    let counted = &COUNTED[counter];
    let deadline = tokio::time::Instant::now() + WAIT;
    let before = counted.load(Ordering::SeqCst);
    while counted.load(Ordering::SeqCst) < before + 3 {
        assert!(tokio::time::Instant::now() < deadline, "the tool never ran");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Asserts `count_slowly` stopped counting on `counter`.
async fn assert_stopped(counter: usize) {
    let counted = &COUNTED[counter];
    tokio::time::sleep(Duration::from_millis(100)).await;
    let stopped_at = counted.load(Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        counted.load(Ordering::SeqCst),
        stopped_at,
        "the call kept running"
    );
}

// ---- Origin (DNS rebinding) ----

#[tokio::test]
async fn browser_origins_must_be_allowed_on_every_method() {
    let app = builder()
        .config("mcp.allowed_origins", "https://crm.example.com")
        .build()
        .await;
    let client = app.acting_as(user());
    let session = Session::open(client.clone(), "2025-11-25").await;
    for origin in [
        "https://evil.example",
        "null",
        "https://crm.example.com.evil.example",
    ] {
        let posted = client
            .post("/mcp")
            .header(ORIGIN, origin)
            .header("accept", "application/json, text/event-stream")
            .json(&initialize("2025-11-25"))
            .send()
            .await;
        assert_eq!(posted.status(), 403, "POST from {origin}");
        let streamed = client
            .get("/mcp")
            .header(ORIGIN, origin)
            .header("accept", "text/event-stream")
            .header("mcp-session-id", session.id.as_str())
            .send()
            .await;
        assert_eq!(streamed.status(), 403, "GET from {origin}");
        let deleted = client
            .delete("/mcp")
            .header(ORIGIN, origin)
            .header("mcp-session-id", session.id.as_str())
            .send()
            .await;
        assert_eq!(deleted.status(), 403, "DELETE from {origin}");
    }
    // The refused DELETEs did not end the session.
    assert_eq!(
        session.call(2, "ping", Value::Null).await["result"],
        json!({})
    );
    // An allowed origin (any case) works.
    let allowed = client
        .post("/mcp")
        .header(ORIGIN, "HTTPS://CRM.EXAMPLE.COM")
        .header("accept", "application/json, text/event-stream")
        .json(&initialize("2025-11-25"))
        .send()
        .await;
    assert_eq!(allowed.status(), 200, "{}", allowed.text());
}

// ---- Session ids: random, bound to their user ----

#[tokio::test]
async fn session_ids_are_random_and_long() {
    let app = app().await;
    let client = app.acting_as(user());
    let mut ids = BTreeSet::new();
    for _ in 0..20 {
        let response = post(&client, &[], &initialize("2025-11-25")).await;
        let id = response.header("mcp-session-id").unwrap().to_owned();
        // 256 bits, base64url.
        assert_eq!(id.len(), 43, "{id}");
        assert!(
            id.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'),
            "{id}"
        );
        assert!(ids.insert(id), "session ids never repeat");
    }
}

#[tokio::test]
async fn a_session_is_bound_to_the_user_who_opened_it() {
    let app = app().await;
    let owner = user();
    let session = Session::open(app.acting_as(owner.clone()), "2025-11-25").await;
    let intruder = app.acting_as(user());
    let headers = [
        ("mcp-session-id", session.id.as_str()),
        ("mcp-protocol-version", "2025-11-25"),
    ];

    post(&intruder, &headers, &request(2, "tools/list", json!({})))
        .await
        .assert_error(404, "Session not found");
    let notified = post(
        &intruder,
        &headers,
        &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
    )
    .await;
    assert_eq!(notified.status(), 404);
    let stream = intruder
        .get("/mcp")
        .without_header(ORIGIN)
        .header("accept", "text/event-stream")
        .header("mcp-session-id", session.id.as_str())
        .send()
        .await;
    assert_eq!(stream.status(), 404);
    let deleted = intruder
        .delete("/mcp")
        .without_header(ORIGIN)
        .header("mcp-session-id", session.id.as_str())
        .send()
        .await;
    assert_eq!(deleted.status(), 404);

    // The owner's session is untouched.
    let listed = session.call(3, "tools/list", json!({})).await;
    assert!(
        listed["result"]["tools"]
            .as_array()
            .is_some_and(|tools| !tools.is_empty())
    );
}

// ---- Body size and JSON depth ----

#[tokio::test]
async fn bodies_over_the_limit_are_refused() {
    let app = builder().config("mcp.max_request_bytes", 512).build().await;
    let client = app.acting_as(user());
    let session = Session::open(client.clone(), "2025-11-25").await;
    let big = session
        .send(&request(
            2,
            "tools/call",
            json!({ "name": "whoami", "arguments": { "padding": "x".repeat(4096) } }),
        ))
        .await;
    assert_eq!(big.status(), 413);
    // The session goes on.
    assert_eq!(
        session.call(3, "ping", Value::Null).await["result"],
        json!({})
    );
}

/// `depth` nested arrays.
fn nested(depth: usize) -> String {
    format!("{}{}", "[".repeat(depth), "]".repeat(depth))
}

#[tokio::test]
async fn deeply_nested_json_is_refused_over_http() {
    let app = app().await;
    let client = app.acting_as(user());
    let session = Session::open(client.clone(), "2025-11-25").await;
    let body = format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"whoami","arguments":{{"deep":{}}}}}}}"#,
        nested(MAX_JSON_DEPTH)
    );
    let refused = client
        .post("/mcp")
        .without_header(ORIGIN)
        .header("accept", "application/json, text/event-stream")
        .header("content-type", "application/json")
        .header("mcp-session-id", session.id.as_str())
        .body(body)
        .send()
        .await;
    assert_eq!(refused.status(), 400);
    assert_eq!(refused.json_value()["error"]["code"], -32600);

    // Nesting within the limit is fine.
    let fine = session
        .call(
            3,
            "tools/call",
            json!({ "name": "whoami", "arguments": { "deep": serde_json::from_str::<Value>(&nested(10)).unwrap() } }),
        )
        .await;
    assert_eq!(fine["result"]["isError"], false, "{fine}");

    // Invalid JSON is a parse error.
    let garbage = client
        .post("/mcp")
        .without_header(ORIGIN)
        .header("accept", "application/json, text/event-stream")
        .header("content-type", "application/json")
        .body("{\"jsonrpc\": ")
        .send()
        .await;
    assert_eq!(garbage.status(), 400);
    assert_eq!(garbage.json_value()["error"]["code"], -32700);
}

#[tokio::test]
async fn stdio_refuses_deep_and_oversized_messages_and_goes_on() {
    let app = builder()
        .config("mcp.max_request_bytes", 1024)
        .build()
        .await;
    let mut client = LineClient::connect(&app, &Uuid::now_v7().to_string());
    client.initialize("2025-11-25").await;

    client
        .write_line(&format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"ping","params":{{"x":{}}}}}"#,
            nested(MAX_JSON_DEPTH + 1)
        ))
        .await;
    let deep = client.answer(&Value::Null).await;
    assert_eq!(deep["error"]["code"], -32600);

    client
        .write_line(&format!(
            r#"{{"jsonrpc":"2.0","id":3,"method":"ping","params":{{"x":"{}"}}}}"#,
            "x".repeat(4096)
        ))
        .await;
    let long = client.answer(&Value::Null).await;
    assert_eq!(long["error"]["code"], -32600);

    client.write_line("not json").await;
    assert_eq!(client.answer(&Value::Null).await["error"]["code"], -32700);

    assert_eq!(
        client.request(4, "ping", Value::Null).await["result"],
        json!({})
    );
    client.close().await.unwrap();
}

// ---- Batches ----

#[tokio::test]
async fn batches_are_refused_unless_the_protocol_allows_them() {
    let app = app().await;
    let client = app.acting_as(user());
    let batch = json!([
        request(2, "ping", Value::Null),
        request(3, "tools/list", json!({}))
    ]);

    for version in ["2024-11-05", "2025-06-18", "2025-11-25"] {
        let session = Session::open(client.clone(), version).await;
        let refused = session.send(&batch).await;
        assert_eq!(refused.status(), 400, "{version}");
        assert_eq!(refused.json_value()["error"]["code"], -32600, "{version}");
    }
    // Without a session, and on the stateless protocol.
    assert_eq!(post(&client, &[], &batch).await.status(), 400);
    let stateless = post(&client, &[("mcp-protocol-version", "2026-07-28")], &batch).await;
    assert_eq!(stateless.status(), 400);

    // 2025-03-26 allows them: one answer per request, in an array.
    let session = Session::open(client.clone(), "2025-03-26").await;
    let answered = session
        .send(&json!([
            request(2, "ping", Value::Null),
            request(3, "tools/call", json!({ "name": "whoami", "arguments": {} })),
            { "jsonrpc": "2.0", "method": "notifications/initialized" },
            request(4, "no/such/method", json!({})),
        ]))
        .await;
    assert_eq!(answered.status(), 200, "{}", answered.text());
    let answers = answered.json_value();
    let answers = answers.as_array().unwrap();
    assert_eq!(answers.len(), 3, "{answers:?}");
    let by_id = |id: i64| answers.iter().find(|answer| answer["id"] == id).unwrap();
    assert_eq!(by_id(2)["result"], json!({}));
    assert_eq!(by_id(3)["result"]["isError"], false);
    assert_eq!(by_id(4)["error"]["code"], -32601);
}

#[tokio::test]
async fn stdio_batches_follow_the_same_rule() {
    let app = app().await;
    let batch = json!([
        request(2, "ping", Value::Null),
        request(3, "ping", Value::Null)
    ]);

    let mut client = LineClient::connect(&app, &Uuid::now_v7().to_string());
    client.initialize("2025-11-25").await;
    client.send(&batch).await;
    assert_eq!(client.answer(&Value::Null).await["error"]["code"], -32600);
    client.close().await.unwrap();

    let mut client = LineClient::connect(&app, &Uuid::now_v7().to_string());
    client.initialize("2025-03-26").await;
    client.send(&batch).await;
    let answers = client.next(WAIT).await.expect("the batch's answers");
    let mut ids: Vec<i64> = answers
        .as_array()
        .unwrap()
        .iter()
        .map(|answer| answer["id"].as_i64().unwrap())
        .collect();
    ids.sort_unstable();
    assert_eq!(ids, [2, 3]);
    client.close().await.unwrap();
}

// ---- Unknown methods ----

#[tokio::test]
async fn unknown_methods_answer_method_not_found_without_side_effects() {
    let audit = RecordingAudit::default();
    let app = builder()
        .override_value::<dyn AuditSink>(Arc::new(audit.clone()))
        .build()
        .await;
    let rep = user();
    insert_deal(&app, rep.user_id, "Acme renewal", "lead").await;
    let notifier = app.get::<McpNotifier>().await;
    let session = Session::open(app.acting_as(rep.clone()), "2025-11-25").await;
    let before = (all_deals(&app).await, notifier.sessions());

    for (id, method) in [
        (2, "tools/delete"),
        (3, "deals/purge"),
        (4, "resources/write"),
        (5, "notifications/initialized"),
        (6, "logging/setLevel"),
    ] {
        let answer = session
            .call(
                id,
                method,
                json!({ "name": "create_deal", "arguments": { "title": "x", "amount_cents": 1 } }),
            )
            .await;
        assert_eq!(answer["error"]["code"], -32601, "{method}: {answer}");
        assert!(answer.get("result").is_none());
    }
    assert_eq!((all_deals(&app).await, notifier.sessions()), before);
    assert!(audit.actions().is_empty(), "{:?}", audit.actions());

    // On the stateless protocol: a 404 with the same error.
    let mut params = json!({});
    params["_meta"] = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {},
    });
    let stateless = post(
        &app.acting_as(rep),
        &[
            ("mcp-protocol-version", "2026-07-28"),
            ("mcp-method", "tools/delete"),
        ],
        &request(7, "tools/delete", params),
    )
    .await;
    assert_eq!(stateless.status(), 404);
    assert_eq!(answer(&stateless, 7)["error"]["code"], -32601);
}

// ---- Tool errors do not leak ----

#[tokio::test]
async fn internal_errors_never_reach_the_client() {
    let app = guarded().await;
    let mcp = McpTestClient::new(app.acting_as(user())).initialize().await;

    let panicked = mcp
        .call_tool("panics", json!({}))
        .await
        .assert_error(500, "Internal server error");
    assert!(!panicked.text().contains("hunter2"), "{}", panicked.text());

    let exploded = mcp.call_tool("refuses", json!({})).await;
    exploded.assert_error(409, "The quarter is closed");

    let read = mcp.read_resource("vault://secret").await;
    let error = read.response().clone().error();
    assert_eq!(error["code"], -32603);
    assert!(!error.to_string().contains("hunter2"), "{error}");

    let prompt = mcp.get_prompt("broken", json!({})).await;
    let error = prompt.response().clone().error();
    assert!(!error.to_string().contains("hunter2"), "{error}");

    // The server goes on after a panic.
    mcp.call_tool("refuses", json!({}))
        .await
        .assert_error(409, None);
}

// ---- Cancellation ----

#[tokio::test]
async fn cancelled_http_session_calls_stop() {
    let app = guarded().await;
    let rep = user();
    let session = Session::open(app.acting_as(rep.clone()), "2025-11-25").await;
    let caller = app.acting_as(rep);
    let headers = (session.id.clone(), session.version.clone());
    let call = tokio::spawn(async move {
        post(
            &caller,
            &[
                ("mcp-session-id", headers.0.as_str()),
                ("mcp-protocol-version", headers.1.as_str()),
            ],
            &request(
                9,
                "tools/call",
                json!({ "name": "count_slowly", "arguments": { "counter": 0 } }),
            ),
        )
        .await
    });
    wait_for_counting(0).await;
    let cancelled = session
        .send(&json!({
            "jsonrpc": "2.0",
            "method": "notifications/cancelled",
            "params": { "requestId": 9 },
        }))
        .await;
    assert_eq!(cancelled.status(), 202);
    assert_stopped(0).await;
    // No answer follows a cancelled request.
    let response = tokio::time::timeout(WAIT, call).await.unwrap().unwrap();
    assert!(
        crate::wire::messages(&response)
            .iter()
            .all(|message| message["id"] != 9),
        "{}",
        response.text()
    );
}

#[tokio::test]
async fn cancelled_stdio_calls_stop() {
    let app = guarded().await;
    let mut client = LineClient::connect(&app, &Uuid::now_v7().to_string());
    client.initialize("2025-11-25").await;
    client
        .send(&request(
            5,
            "tools/call",
            json!({ "name": "count_slowly", "arguments": { "counter": 1 } }),
        ))
        .await;
    wait_for_counting(1).await;
    client
        .send(&json!({
            "jsonrpc": "2.0",
            "method": "notifications/cancelled",
            "params": { "requestId": 5, "reason": "stop" },
        }))
        .await;
    assert_stopped(1).await;
    // The connection answers what comes next, and nothing for request 5.
    let pong = client.request(6, "ping", Value::Null).await;
    assert_eq!(pong["result"], json!({}));
    assert!(
        client
            .notifications
            .iter()
            .all(|message| message["id"] != 5)
    );
    client.close().await.unwrap();
}

/// On the stateless protocol, closing the connection cancels the request.
/// (In an app with `DatabaseModule`, a request whose client went away
/// before its answer started runs to completion and commits, like every
/// Navree request; an SSE answer still stops when its stream is dropped.)
#[tokio::test]
async fn a_stateless_call_stops_when_its_client_disconnects() {
    let app = TestApp::builder()
        .config("app.name", "navree-guarded")
        .module(McpModule::from_config())
        .module(GuardedModule)
        .build()
        .await;
    let client = app.acting_as(user());
    let before = COUNTED[2].load(Ordering::SeqCst);
    let body = request(
        1,
        "tools/call",
        json!({
            "name": "count_slowly",
            "arguments": { "counter": 2 },
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "io.modelcontextprotocol/clientCapabilities": {},
            },
        }),
    );
    let call = post(
        &client,
        &[
            ("mcp-protocol-version", "2026-07-28"),
            ("mcp-method", "tools/call"),
            ("mcp-name", "count_slowly"),
        ],
        &body,
    );
    // The client gives up (its request future is dropped) while the tool
    // runs.
    let gave_up = tokio::time::timeout(Duration::from_millis(500), call).await;
    assert!(gave_up.is_err(), "the call was still running");
    assert!(COUNTED[2].load(Ordering::SeqCst) > before, "the tool ran");
    assert_stopped(2).await;
}

// ---- Slow clients ----

#[tokio::test]
async fn a_client_that_does_not_read_cannot_grow_the_servers_memory() {
    let app = app().await;
    let notifier = app.get::<McpNotifier>().await;
    let rep = user();
    let mcp = McpTestClient::new(app.acting_as(rep.clone()))
        .initialize()
        .await;
    // Open the session's stream and never read it.
    let response = app
        .acting_as(rep)
        .get("/mcp")
        .without_header(ORIGIN)
        .header("accept", "text/event-stream")
        .header("mcp-session-id", mcp.session_id().unwrap())
        .header("mcp-protocol-version", "2025-11-25")
        .send_stream()
        .await;
    assert_eq!(response.status(), 200);

    let mut queued = 0;
    for _ in 0..10_000 {
        queued += notifier.tools_changed().await;
    }
    assert!(
        queued <= 128,
        "at most a small, fixed number of notifications wait for the client: {queued}"
    );
    assert!(queued > 0);

    // Reading resumes the stream: what was queued arrives.
    let mut body = response.into_body();
    let frame = tokio::time::timeout(WAIT, body.frame())
        .await
        .expect("a frame")
        .expect("the stream is open")
        .unwrap();
    let data = frame.into_data().unwrap();
    assert!(
        String::from_utf8_lossy(&data).contains("notifications/tools/list_changed"),
        "{data:?}"
    );
}

// ---- Authorization ----

#[tokio::test]
async fn every_method_challenges_guests_with_the_resource_metadata() {
    let app = builder()
        .config("mcp.auth", "oauth")
        .config("app.url", "https://crm.example.com")
        .build()
        .await;
    let guest: TestClient = app.client();
    let expected = "Bearer resource_metadata=\"https://crm.example.com/.well-known/oauth-protected-resource/mcp\"";
    let posted = post(&guest, &[], &initialize("2025-11-25")).await;
    assert_eq!(posted.status(), 401);
    assert_eq!(posted.header("www-authenticate"), Some(expected));
    let streamed = guest
        .get("/mcp")
        .without_header(ORIGIN)
        .header("accept", "text/event-stream")
        .send()
        .await;
    assert_eq!(streamed.status(), 401);
    assert_eq!(streamed.header("www-authenticate"), Some(expected));
    let deleted = guest
        .delete("/mcp")
        .without_header(ORIGIN)
        .header("mcp-session-id", "x")
        .send()
        .await;
    assert_eq!(deleted.status(), 401);
    assert_eq!(deleted.header("www-authenticate"), Some(expected));

    // The metadata itself is public.
    let metadata = guest
        .get("/.well-known/oauth-protected-resource/mcp")
        .send()
        .await
        .assert_status(200);
    assert_eq!(
        metadata.json_value()["resource"],
        "https://crm.example.com/mcp"
    );
}
