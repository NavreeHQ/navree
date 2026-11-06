//! The protocol on the wire, as raw JSON-RPC over the streamable HTTP
//! transport: the handshake and version negotiation, the exact shapes of
//! lists and results, errors, sessions and the stateless protocol's
//! headers and metadata.

use http::header::ORIGIN;
use navree_testing::{TestClient, TestResponse};
use serde_json::{Value, json};

use crate::support::{app, builder, user};

const STATELESS: &str = "2026-07-28";

/// POSTs `body` to `/mcp` as an MCP client does (no Origin), with extra
/// headers.
pub async fn post(client: &TestClient, headers: &[(&str, &str)], body: &Value) -> TestResponse {
    let mut request = client
        .post("/mcp")
        .without_header(ORIGIN)
        .header("accept", "application/json, text/event-stream")
        .json(body);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    request.send().await
}

/// The JSON-RPC messages of a response, JSON or SSE.
pub fn messages(response: &TestResponse) -> Vec<Value> {
    let sse = response
        .header("content-type")
        .is_some_and(|value| value.starts_with("text/event-stream"));
    if !sse {
        return vec![response.json_value()];
    }
    response
        .text()
        .replace("\r\n", "\n")
        .split("\n\n")
        .filter_map(|event| {
            let data: Vec<&str> = event
                .lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .map(|data| data.strip_prefix(' ').unwrap_or(data))
                .collect();
            serde_json::from_str(&data.join("\n")).ok()
        })
        .collect()
}

/// The answer (result or error message) to request `id` in a response.
pub fn answer(response: &TestResponse, id: i64) -> Value {
    messages(response)
        .into_iter()
        .find(|message| message["id"] == json!(id))
        .unwrap_or_else(|| panic!("no answer to {id} in {}", response.text()))
}

fn initialize(version: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": version,
            "capabilities": {},
            "clientInfo": { "name": "wire", "version": "1" },
        },
    })
}

/// A session: its id and negotiated version.
pub struct Session {
    pub client: TestClient,
    pub id: String,
    pub version: String,
}

impl Session {
    pub async fn open(client: TestClient, version: &str) -> Session {
        let response = post(&client, &[], &initialize(version)).await;
        assert_eq!(response.status(), 200, "{}", response.text());
        let id = response
            .header("mcp-session-id")
            .expect("a session id")
            .to_owned();
        let version = answer(&response, 1)["result"]["protocolVersion"]
            .as_str()
            .unwrap()
            .to_owned();
        let session = Session {
            client,
            id,
            version,
        };
        let initialized = session
            .send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
            .await;
        assert_eq!(initialized.status(), 202);
        session
    }

    pub async fn send(&self, body: &Value) -> TestResponse {
        post(
            &self.client,
            &[
                ("mcp-session-id", self.id.as_str()),
                ("mcp-protocol-version", self.version.as_str()),
            ],
            body,
        )
        .await
    }

    /// Request `method` with `params`; the whole answer message.
    pub async fn call(&self, id: i64, method: &str, params: Value) -> Value {
        let response = self
            .send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
            .await;
        assert_eq!(response.status(), 200, "{}", response.text());
        answer(&response, id)
    }
}

fn meta() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": STATELESS,
        "io.modelcontextprotocol/clientInfo": { "name": "wire", "version": "1" },
        "io.modelcontextprotocol/clientCapabilities": {},
    })
}

/// One stateless request with its standard headers.
async fn stateless(client: &TestClient, id: i64, method: &str, mut params: Value) -> TestResponse {
    params["_meta"] = meta();
    let mut headers = vec![("mcp-protocol-version", STATELESS), ("mcp-method", method)];
    let name = params["name"]
        .as_str()
        .or(params["uri"].as_str())
        .map(str::to_owned);
    if let Some(name) = &name {
        headers.push(("mcp-name", name.as_str()));
    }
    post(
        client,
        &headers,
        &json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }),
    )
    .await
}

#[tokio::test]
async fn initialize_negotiates_the_protocol_version() {
    let app = app().await;
    for (asked, agreed) in [
        ("2024-11-05", "2024-11-05"),
        ("2025-03-26", "2025-03-26"),
        ("2025-06-18", "2025-06-18"),
        ("2025-11-25", "2025-11-25"),
        // The stateless protocol has no handshake; unknown versions get the
        // newest one that has.
        (STATELESS, "2025-11-25"),
        ("1999-01-01", "2025-11-25"),
    ] {
        let response = post(&app.acting_as(user()), &[], &initialize(asked)).await;
        assert_eq!(response.status(), 200, "{asked}: {}", response.text());
        let result = &answer(&response, 1)["result"];
        assert_eq!(result["protocolVersion"], agreed, "{asked}");
    }
}

#[tokio::test]
async fn initialize_describes_the_server_and_its_capabilities() {
    let app = builder()
        .config("mcp.title", "Navree CRM")
        .config("mcp.instructions", "Call list_deals first.")
        .build()
        .await;
    let response = post(&app.acting_as(user()), &[], &initialize("2025-11-25")).await;
    let id = response.header("mcp-session-id").unwrap();
    assert!(id.len() >= 16, "a long session id: {id}");
    assert!(
        id.bytes().all(|byte| (0x21..=0x7e).contains(&byte)),
        "visible ASCII: {id}"
    );
    let result = answer(&response, 1)["result"].clone();
    assert_eq!(
        result["capabilities"],
        json!({
            "tools": { "listChanged": true },
            "resources": { "subscribe": true, "listChanged": true },
            "prompts": { "listChanged": true },
        })
    );
    assert_eq!(result["serverInfo"]["name"], "navree-crm");
    assert_eq!(result["serverInfo"]["title"], "Navree CRM");
    assert_eq!(result["serverInfo"]["version"], "0.0.0");
    assert_eq!(result["instructions"], "Call list_deals first.");
}

#[tokio::test]
async fn lists_have_their_protocol_shape() {
    let app = app().await;
    let session = Session::open(app.acting_as(user()), "2025-11-25").await;

    let tools = session.call(2, "tools/list", json!({})).await["result"].clone();
    assert_eq!(tools["cacheScope"], "private");
    assert!(tools.get("nextCursor").is_none(), "one page: {tools}");
    assert!(tools.get("resultType").is_none(), "{tools}");
    let whoami = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "whoami")
        .unwrap()
        .clone();
    assert_eq!(
        whoami,
        json!({
            "name": "whoami",
            "description": "Who is calling.",
            "inputSchema": { "type": "object", "properties": {} },
            "annotations": {
                "readOnlyHint": true,
                "destructiveHint": false,
                "idempotentHint": false,
                "openWorldHint": false,
            },
        })
    );

    // A cursor is ignored: there is only one page.
    let again = session
        .call(3, "tools/list", json!({ "cursor": "abc" }))
        .await;
    assert_eq!(again["result"]["tools"], tools["tools"]);

    let resources = session.call(4, "resources/list", json!({})).await["result"].clone();
    assert_eq!(
        resources["resources"],
        json!([{
            "uri": "crm://pipeline",
            "name": "pipeline",
            "title": "Pipeline",
            "description": "The pipeline: how many deals each stage has.",
            "mimeType": "application/json",
        }])
    );
    let templates = session.call(5, "resources/templates/list", json!({})).await["result"].clone();
    let note = templates["resourceTemplates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|template| template["name"] == "note")
        .unwrap()
        .clone();
    assert_eq!(
        note,
        json!({ "uriTemplate": "crm://notes/{slug}", "name": "note", "description": "A note, as text." })
    );

    let prompts = session.call(6, "prompts/list", json!({})).await["result"].clone();
    assert_eq!(prompts["cacheScope"], "private");
    assert!(
        prompts["prompts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|prompt| prompt["name"] == "summarize")
    );
}

#[tokio::test]
async fn results_and_errors_have_their_protocol_shape() {
    let app = app().await;
    let session = Session::open(app.acting_as(user()), "2025-11-25").await;

    let created = session
        .call(
            2,
            "tools/call",
            json!({ "name": "create_deal", "arguments": { "title": "Indrani", "amount_cents": 5 } }),
        )
        .await["result"]
        .clone();
    assert_eq!(created["isError"], false);
    assert_eq!(created["structuredContent"]["title"], "Indrani");
    assert_eq!(created["content"][0]["type"], "text");
    let text: Value =
        serde_json::from_str(created["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(text, created["structuredContent"]);

    let failed = session
        .call(
            3,
            "tools/call",
            json!({ "name": "explode", "arguments": {} }),
        )
        .await["result"]
        .clone();
    assert_eq!(failed["isError"], true);
    assert_eq!(failed["structuredContent"]["statusCode"], 500);

    let unknown = session
        .call(4, "tools/call", json!({ "name": "nope", "arguments": {} }))
        .await["error"]
        .clone();
    assert_eq!(unknown["code"], -32602);

    let missing = session
        .call(5, "resources/read", json!({ "uri": "crm://contacts/1" }))
        .await["error"]
        .clone();
    assert_eq!(missing["code"], -32002);
    assert_eq!(missing["data"]["uri"], "crm://contacts/1");

    let read = session
        .call(6, "resources/read", json!({ "uri": "crm://notes/x" }))
        .await["result"]
        .clone();
    assert_eq!(
        read,
        json!({ "contents": [{ "uri": "crm://notes/x", "mimeType": "text/plain", "text": "Note x" }] })
    );

    let prompt = session
        .call(
            7,
            "prompts/get",
            json!({ "name": "summarize", "arguments": { "days": "7" } }),
        )
        .await["result"]
        .clone();
    assert_eq!(
        prompt,
        json!({
            "description": "Summarizes the recent pipeline.",
            "messages": [{ "role": "user", "content": { "type": "text", "text": "Summarize the last 7 days." } }],
        })
    );

    assert_eq!(
        session.call(8, "ping", Value::Null).await["result"],
        json!({})
    );
    let unknown_method = session.call(9, "no/such/method", json!({})).await;
    assert_eq!(unknown_method["error"]["code"], -32601, "{unknown_method}");
}

#[tokio::test]
async fn the_http_transport_checks_its_headers() {
    let app = app().await;
    let client = app.acting_as(user());
    let body = json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" });

    // Both answer types must be acceptable, and the body must be JSON.
    let refused = client
        .post("/mcp")
        .without_header(ORIGIN)
        .header("accept", "application/json")
        .json(&initialize("2025-11-25"))
        .send()
        .await;
    assert_eq!(refused.status(), 406);
    let refused = client
        .post("/mcp")
        .without_header(ORIGIN)
        .header("accept", "application/json, text/event-stream")
        .header("content-type", "text/plain")
        .body("{}")
        .send()
        .await;
    assert_eq!(refused.status(), 415);

    // Requests other than initialize need a session.
    assert_eq!(post(&client, &[], &body).await.status(), 400);
    // An unknown session is gone.
    assert_eq!(
        post(&client, &[("mcp-session-id", "no-such-session")], &body)
            .await
            .status(),
        404
    );

    let session = Session::open(client.clone(), "2025-11-25").await;
    // An unknown protocol version header is refused.
    let unknown = post(
        &client,
        &[
            ("mcp-session-id", session.id.as_str()),
            ("mcp-protocol-version", "1999-01-01"),
        ],
        &body,
    )
    .await;
    assert_eq!(unknown.status(), 400);

    // The GET stream needs SSE and a session.
    let stream = client
        .get("/mcp")
        .without_header(ORIGIN)
        .header("accept", "application/json")
        .header("mcp-session-id", session.id.as_str())
        .send()
        .await;
    assert_eq!(stream.status(), 406);
}

#[tokio::test]
async fn stateless_results_carry_their_cache_hints() {
    let app = app().await;
    let client = app.acting_as(user());
    let listed = stateless(&client, 1, "tools/list", json!({})).await;
    assert_eq!(listed.status(), 200, "{}", listed.text());
    let result = answer(&listed, 1)["result"].clone();
    assert_eq!(result["resultType"], "complete");
    assert_eq!(result["ttlMs"], 0);
    assert_eq!(result["cacheScope"], "private");

    let read = stateless(
        &client,
        2,
        "resources/read",
        json!({ "uri": "crm://notes/x" }),
    )
    .await;
    let result = answer(&read, 2)["result"].clone();
    assert_eq!(result["resultType"], "complete");
    assert_eq!(result["ttlMs"], 0);
    assert_eq!(result["cacheScope"], "private");
    assert_eq!(result["contents"][0]["text"], "Note x");

    let called = stateless(
        &client,
        3,
        "tools/call",
        json!({ "name": "whoami", "arguments": {} }),
    )
    .await;
    let result = answer(&called, 3)["result"].clone();
    assert_eq!(result["resultType"], "complete");
    assert_eq!(result["structuredContent"]["session"], false, "no session");

    // Resource-not-found is invalid params on the stateless protocol.
    let missing = stateless(
        &client,
        4,
        "resources/read",
        json!({ "uri": "crm://contacts/1" }),
    )
    .await;
    assert_eq!(answer(&missing, 4)["error"]["code"], -32602);

    let discovered = stateless(&client, 5, "server/discover", json!({})).await;
    let result = answer(&discovered, 5)["result"].clone();
    assert_eq!(
        result["supportedVersions"],
        json!([
            "2024-11-05",
            "2025-03-26",
            "2025-06-18",
            "2025-11-25",
            STATELESS
        ])
    );
    assert_eq!(result["resultType"], "complete");
    assert_eq!(result["cacheScope"], "private");
    assert_eq!(result["ttlMs"], 0);
}

#[tokio::test]
async fn the_stateless_protocol_checks_versions_methods_and_headers() {
    let app = app().await;
    let client = app.acting_as(user());

    // ping belongs to the handshake protocols: 404 with method not found.
    let ping = stateless(&client, 1, "ping", json!({})).await;
    assert_eq!(ping.status(), 404);
    assert_eq!(answer(&ping, 1)["error"]["code"], -32601);
    let subscribe = stateless(
        &client,
        2,
        "resources/subscribe",
        json!({ "uri": "crm://pipeline" }),
    )
    .await;
    assert_eq!(answer(&subscribe, 2)["error"]["code"], -32601);

    // An unsupported version in the request's metadata.
    let mut unsupported = meta();
    unsupported["io.modelcontextprotocol/protocolVersion"] = json!("2099-01-01");
    let response = post(
        &client,
        &[("mcp-protocol-version", "2099-01-01"), ("mcp-method", "tools/list")],
        &json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list", "params": { "_meta": unsupported } }),
    )
    .await;
    assert_eq!(response.status(), 400, "{}", response.text());
    let error = answer(&response, 3)["error"].clone();
    assert_eq!(error["code"], -32022);
    assert_eq!(error["data"]["requested"], "2099-01-01");

    // The method header must match the body.
    let mismatched = post(
        &client,
        &[("mcp-protocol-version", STATELESS), ("mcp-method", "tools/call")],
        &json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/list", "params": { "_meta": meta() } }),
    )
    .await;
    assert_eq!(mismatched.status(), 400);
    assert_eq!(answer(&mismatched, 4)["error"]["code"], -32020);
    // So must the name header.
    let mismatched = post(
        &client,
        &[
            ("mcp-protocol-version", STATELESS),
            ("mcp-method", "tools/call"),
            ("mcp-name", "explode"),
        ],
        &json!({
            "jsonrpc": "2.0", "id": 5, "method": "tools/call",
            "params": { "name": "whoami", "arguments": {}, "_meta": meta() },
        }),
    )
    .await;
    assert_eq!(mismatched.status(), 400);
    assert_eq!(answer(&mismatched, 5)["error"]["code"], -32020);

    // The version header must match the metadata.
    let mismatched = post(
        &client,
        &[("mcp-protocol-version", "2025-11-25"), ("mcp-method", "tools/list")],
        &json!({ "jsonrpc": "2.0", "id": 6, "method": "tools/list", "params": { "_meta": meta() } }),
    )
    .await;
    assert_eq!(mismatched.status(), 400);
    assert_eq!(answer(&mismatched, 6)["error"]["code"], -32020);
}
