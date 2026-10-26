//! [`McpTestClient`]: speaks MCP to a test app in process, through its full
//! HTTP pipeline, as whoever its `TestClient` acts as.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use bytes::{Buf, Bytes};
use http::header::{ACCEPT, ORIGIN};
use http_body_util::BodyExt;
use navree_testing::{TestClient, TestRequest, TestResponse};
use rmcp::model::{Prompt, Resource, ResourceTemplate, Tool};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

/// The protocol version the client asks for.
const PROTOCOL_VERSION: &str = "2025-11-25";

/// An MCP client for tests. Build it from any `TestClient`, so it acts as
/// whoever that client acts as (its `acting_as` principal, its cookies or
/// guest status; add a bearer token with
/// [`bearer`](McpTestClient::bearer)), then
/// [`initialize`](McpTestClient::initialize):
///
/// ```no_run
/// # async fn demo(app: navree_testing::TestApp, user: navree_contracts::Principal) {
/// use navree_mcp::testing::McpTestClient;
/// use serde_json::json;
///
/// let mcp = McpTestClient::new(app.acting_as(user)).initialize().await;
/// assert!(mcp.list_tools().await.iter().any(|tool| tool.name == "log_activity"));
/// mcp.call_tool("log_activity", json!({ "subject": "Call with Dana" }))
///     .await
///     .assert_ok()
///     .assert_json_path("subject", json!("Call with Dana"));
/// # }
/// ```
///
/// Requests carry no `Origin` header (MCP clients are not browsers) and
/// accept JSON and SSE answers; notifications that arrive on a request's
/// SSE stream (progress, list changes) are kept for
/// [`notifications`](McpTestClient::notifications).
pub struct McpTestClient {
    client: TestClient,
    path: String,
    bearer: Option<String>,
    protocol_version: String,
    session_id: Option<String>,
    server: Value,
    next_id: AtomicI64,
    notifications: Arc<Mutex<Vec<Value>>>,
}

/// A JSON-RPC answer: a result or an error.
#[derive(Clone, Debug, PartialEq)]
pub enum McpResponse {
    /// The `result` member.
    Result(Value),
    /// The `error` member: `{code, message, data}`.
    Error(Value),
}

impl McpResponse {
    /// The result, or a panic showing the error.
    pub fn result(self) -> Value {
        match self {
            McpResponse::Result(result) => result,
            McpResponse::Error(error) => {
                panic!("expected a result, got the JSON-RPC error {error}")
            }
        }
    }

    /// The error, or a panic showing the result.
    pub fn error(self) -> Value {
        match self {
            McpResponse::Error(error) => error,
            McpResponse::Result(result) => {
                panic!("expected a JSON-RPC error, got the result {result}")
            }
        }
    }
}

impl McpTestClient {
    /// A client speaking to `/mcp` through `client`.
    pub fn new(client: TestClient) -> Self {
        Self {
            client,
            path: "/mcp".to_owned(),
            bearer: None,
            protocol_version: PROTOCOL_VERSION.to_owned(),
            session_id: None,
            server: Value::Null,
            next_id: AtomicI64::new(1),
            notifications: Arc::default(),
        }
    }

    /// Speaks to `path` instead of `/mcp` (when `mcp.path` is set).
    pub fn path(mut self, path: impl Into<String>) -> Self {
        self.path = path.into();
        self
    }

    /// Sends `Authorization: Bearer <token>` with every request, as a
    /// remote client does (a navree-auth access token, a personal access
    /// token, ...).
    pub fn bearer(mut self, token: impl Into<String>) -> Self {
        self.bearer = Some(token.into());
        self
    }

    /// Asks for another protocol version in [`initialize`](McpTestClient::initialize)
    /// (default `2025-11-25`).
    pub fn protocol_version(mut self, version: impl Into<String>) -> Self {
        self.protocol_version = version.into();
        self
    }

    /// Performs the handshake (`initialize`, then
    /// `notifications/initialized`) and keeps the session id.
    ///
    /// # Panics
    ///
    /// When the server refuses the handshake (a 401 for a guest, for
    /// example).
    pub async fn initialize(mut self) -> Self {
        let message = json!({
            "jsonrpc": "2.0",
            "id": 0,
            "method": "initialize",
            "params": {
                "protocolVersion": self.protocol_version,
                "capabilities": {},
                "clientInfo": { "name": "navree-test", "version": env!("CARGO_PKG_VERSION") },
            },
        });
        let response = self.post(&message).await;
        assert_eq!(
            response.status(),
            200,
            "MCP initialize failed: {} {}",
            response.status(),
            response.text()
        );
        self.session_id = response.header("mcp-session-id").map(str::to_owned);
        let answer = self.answer(&response, 0);
        self.server = answer.result();
        if let Some(version) = self.server["protocolVersion"].as_str() {
            self.protocol_version = version.to_owned();
        }
        self.notify("notifications/initialized", Value::Null).await;
        // The server handles notifications on their own tasks: a round trip
        // lets it finish with `initialized` before the test goes on.
        self.request("ping", Value::Null).await.result();
        self
    }

    /// The session id the server gave (none for sessionless protocols).
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// The `initialize` result: `serverInfo`, `capabilities`,
    /// `instructions`, `protocolVersion`.
    pub fn server_info(&self) -> &Value {
        &self.server
    }

    /// Sends a request and returns its answer.
    ///
    /// # Panics
    ///
    /// When the HTTP response is not a 200 with a JSON-RPC answer.
    pub async fn request(&self, method: &str, params: Value) -> McpResponse {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut message = json!({ "jsonrpc": "2.0", "id": id, "method": method });
        if !params.is_null() {
            message["params"] = params;
        }
        let response = self.post(&message).await;
        assert_eq!(
            response.status(),
            200,
            "MCP {method} failed: {} {}",
            response.status(),
            response.text()
        );
        self.answer(&response, id)
    }

    /// Sends a notification; returns the HTTP status (202 when accepted).
    pub async fn notify(&self, method: &str, params: Value) -> u16 {
        let mut message = json!({ "jsonrpc": "2.0", "method": method });
        if !params.is_null() {
            message["params"] = params;
        }
        self.post(&message).await.status().as_u16()
    }

    /// The tools this user sees.
    pub async fn list_tools(&self) -> Vec<Tool> {
        self.list("tools/list", "tools").await
    }

    /// The fixed resources this user sees.
    pub async fn list_resources(&self) -> Vec<Resource> {
        self.list("resources/list", "resources").await
    }

    /// The resource templates this user sees.
    pub async fn list_resource_templates(&self) -> Vec<ResourceTemplate> {
        self.list("resources/templates/list", "resourceTemplates")
            .await
    }

    /// The prompts this user sees.
    pub async fn list_prompts(&self) -> Vec<Prompt> {
        self.list("prompts/list", "prompts").await
    }

    async fn list<T: DeserializeOwned>(&self, method: &str, member: &str) -> Vec<T> {
        let result = self.request(method, json!({})).await.result();
        serde_json::from_value(result[member].clone())
            .unwrap_or_else(|error| panic!("{method} answered {result}: {error}"))
    }

    /// Calls tool `name` with `arguments`.
    pub async fn call_tool(&self, name: &str, arguments: Value) -> ToolResult {
        let answer = self
            .request(
                "tools/call",
                json!({ "name": name, "arguments": arguments }),
            )
            .await;
        ToolResult::new(answer)
    }

    /// Calls tool `name` asking for progress notifications (token
    /// `progress-<n>`); they are kept in
    /// [`notifications`](McpTestClient::notifications).
    pub async fn call_tool_with_progress(&self, name: &str, arguments: Value) -> ToolResult {
        let token = format!("progress-{}", self.next_id.load(Ordering::Relaxed));
        let answer = self
            .request(
                "tools/call",
                json!({ "name": name, "arguments": arguments, "_meta": { "progressToken": token } }),
            )
            .await;
        ToolResult::new(answer)
    }

    /// Reads the resource at `uri`.
    pub async fn read_resource(&self, uri: &str) -> ResourceResult {
        ResourceResult::new(self.request("resources/read", json!({ "uri": uri })).await)
    }

    /// Subscribes to the resource at `uri`.
    pub async fn subscribe(&self, uri: &str) -> McpResponse {
        self.request("resources/subscribe", json!({ "uri": uri }))
            .await
    }

    /// Renders prompt `name` with `arguments` (an object of strings, as
    /// clients send them).
    pub async fn get_prompt(&self, name: &str, arguments: Value) -> PromptResult {
        let answer = self
            .request(
                "prompts/get",
                json!({ "name": name, "arguments": arguments }),
            )
            .await;
        PromptResult::new(answer)
    }

    /// Notifications the server sent this session on request streams, in
    /// order.
    pub fn notifications(&self) -> Vec<Value> {
        self.notifications
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Opens the session's notification stream (`GET`), for messages the
    /// server starts (resource updates, list changes).
    ///
    /// # Panics
    ///
    /// Without a session, or when the server refuses the stream.
    pub async fn listen(&self) -> NotificationStream {
        let session = self
            .session_id
            .clone()
            .expect("listen needs a session: call initialize first");
        let response = self
            .prepare(self.client.get(&self.path))
            .header(ACCEPT, "text/event-stream")
            .header("mcp-session-id", session)
            .header("mcp-protocol-version", self.protocol_version.clone())
            .send_stream()
            .await;
        assert_eq!(
            response.status(),
            200,
            "the MCP notification stream was refused"
        );
        NotificationStream {
            body: response.into_body(),
            buffer: String::new(),
        }
    }

    /// Ends the session (`DELETE`); returns the HTTP status.
    pub async fn close(&self) -> u16 {
        let mut request = self.prepare(self.client.delete(&self.path));
        if let Some(session) = &self.session_id {
            request = request.header("mcp-session-id", session.clone());
        }
        request.send().await.status().as_u16()
    }

    /// A request as an MCP client sends it: no `Origin` (not a browser),
    /// and the bearer token when there is one.
    fn prepare(&self, request: TestRequest) -> TestRequest {
        let request = request.without_header(ORIGIN);
        match &self.bearer {
            Some(token) => request.bearer(token),
            None => request,
        }
    }

    async fn post(&self, message: &Value) -> TestResponse {
        let mut request = self
            .prepare(self.client.post(&self.path))
            .header(ACCEPT, "application/json, text/event-stream")
            .json(message);
        if let Some(session) = &self.session_id {
            request = request
                .header("mcp-session-id", session.clone())
                .header("mcp-protocol-version", self.protocol_version.clone());
        }
        request.send().await
    }

    /// The answer to request `id` in `response` (JSON or SSE), keeping the
    /// notifications that came first.
    fn answer(&self, response: &TestResponse, id: i64) -> McpResponse {
        let messages = if response
            .header("content-type")
            .is_some_and(|value| value.starts_with("text/event-stream"))
        {
            sse_messages(&response.text())
        } else {
            vec![response.json_value()]
        };
        let mut answer = None;
        for message in messages {
            if message["id"] == json!(id)
                && (message.get("result").is_some() || message.get("error").is_some())
            {
                answer = Some(match message.get("error") {
                    Some(error) => McpResponse::Error(error.clone()),
                    None => McpResponse::Result(message["result"].clone()),
                });
            } else if message.get("method").is_some() && message.get("id").is_none() {
                self.notifications
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(message);
            }
        }
        answer.unwrap_or_else(|| panic!("no answer to request {id} in {}", response.text()))
    }
}

/// The JSON messages of an SSE body (`data:` lines; events without JSON,
/// such as the priming event, are skipped).
fn sse_messages(body: &str) -> Vec<Value> {
    body.split("\n\n")
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

/// The session's notification stream ([`McpTestClient::listen`]).
pub struct NotificationStream {
    body: axum::body::Body,
    buffer: String,
}

impl NotificationStream {
    /// The next notification, or `None` when none arrives within `wait` (or
    /// the stream ended).
    pub async fn next(&mut self, wait: Duration) -> Option<Value> {
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            while let Some(end) = self.buffer.find("\n\n") {
                let event: String = self.buffer.drain(..end + 2).collect();
                if let Some(message) = sse_messages(&event).into_iter().next()
                    && message.get("method").is_some()
                {
                    return Some(message);
                }
            }
            let frame = tokio::time::timeout_at(deadline, self.body.frame()).await;
            match frame {
                Ok(Some(Ok(frame))) => {
                    if let Ok(mut data) = frame.into_data() {
                        let bytes: Bytes = data.copy_to_bytes(data.remaining());
                        self.buffer
                            .push_str(&String::from_utf8_lossy(&bytes).replace("\r\n", "\n"));
                    }
                }
                _ => return None,
            }
        }
    }
}

/// Navree's error body (`{statusCode, message, error}`) matches `status`
/// and, when given, `message` (the string, or one element of the list).
fn assert_error_body(body: &Value, status: u16, message: Option<&str>, context: &str) {
    assert_eq!(
        body["statusCode"],
        json!(status),
        "expected a {status} error, got {context}"
    );
    if let Some(message) = message {
        let matches = match &body["message"] {
            Value::String(text) => text == message,
            Value::Array(items) => items.iter().any(|item| item == message),
            _ => false,
        };
        assert!(matches, "expected the message {message:?}, got {context}");
    }
}

/// The value at a dotted path (`data.0.title`).
fn at_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    if path.is_empty() {
        return Some(value);
    }
    path.split('.')
        .try_fold(value, |current, key| match current {
            Value::Array(items) => key.parse::<usize>().ok().and_then(|i| items.get(i)),
            _ => current.get(key),
        })
}

/// The answer to `tools/call`.
#[derive(Clone, Debug)]
pub struct ToolResult {
    answer: McpResponse,
}

impl ToolResult {
    fn new(answer: McpResponse) -> Self {
        Self { answer }
    }

    /// The raw answer.
    pub fn response(&self) -> &McpResponse {
        &self.answer
    }

    /// Whether the call failed: a result marked `isError`, or a JSON-RPC
    /// error (an unknown tool).
    pub fn is_error(&self) -> bool {
        match &self.answer {
            McpResponse::Result(result) => result["isError"] == json!(true),
            McpResponse::Error(_) => true,
        }
    }

    /// The text of the result's text content, joined.
    pub fn text(&self) -> String {
        match &self.answer {
            McpResponse::Result(result) => result["content"]
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item["text"].as_str())
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default(),
            McpResponse::Error(error) => error["message"].as_str().unwrap_or_default().to_owned(),
        }
    }

    /// The structured content, when the result has one.
    pub fn structured(&self) -> Option<&Value> {
        match &self.answer {
            McpResponse::Result(result) => result.get("structuredContent"),
            McpResponse::Error(_) => None,
        }
    }

    /// The returned value: the text parsed as JSON (a string when it is
    /// not JSON). For a failed call, Navree's error body.
    pub fn json_value(&self) -> Value {
        let text = self.text();
        serde_json::from_str(&text).unwrap_or(Value::String(text))
    }

    /// The returned value as a `T`.
    ///
    /// # Panics
    ///
    /// When it does not deserialize.
    pub fn json<T: DeserializeOwned>(&self) -> T {
        let value = self.json_value();
        serde_json::from_value(value.clone()).unwrap_or_else(|error| {
            panic!("the tool result {value} is not the expected type: {error}")
        })
    }

    /// Asserts the call succeeded.
    #[track_caller]
    pub fn assert_ok(self) -> Self {
        assert!(
            !self.is_error(),
            "expected the tool to succeed, got {:?}",
            self.answer
        );
        self
    }

    /// Asserts the call failed with Navree's error `status` and, when given,
    /// `message`.
    #[track_caller]
    pub fn assert_error<'a>(self, status: u16, message: impl Into<Option<&'a str>>) -> Self {
        assert!(
            self.is_error(),
            "expected the tool to fail, got {:?}",
            self.answer
        );
        let context = format!("{:?}", self.answer);
        assert_error_body(&self.json_value(), status, message.into(), &context);
        self
    }

    /// Asserts the value at a dotted path (`subject`, `data.0.title`) of
    /// the returned value.
    #[track_caller]
    pub fn assert_json_path(self, path: &str, expected: Value) -> Self {
        let value = self.json_value();
        assert_eq!(
            at_path(&value, path),
            Some(&expected),
            "at `{path}` in the tool result {value}"
        );
        self
    }
}

/// The answer to `resources/read`.
#[derive(Clone, Debug)]
pub struct ResourceResult {
    answer: McpResponse,
}

impl ResourceResult {
    fn new(answer: McpResponse) -> Self {
        Self { answer }
    }

    /// The raw answer.
    pub fn response(&self) -> &McpResponse {
        &self.answer
    }

    /// The contents (`[{uri, mimeType, text | blob}]`); empty for an error.
    pub fn contents(&self) -> Vec<Value> {
        match &self.answer {
            McpResponse::Result(result) => {
                result["contents"].as_array().cloned().unwrap_or_default()
            }
            McpResponse::Error(_) => Vec::new(),
        }
    }

    /// The text of the first contents.
    pub fn text(&self) -> Option<String> {
        self.contents()
            .first()
            .and_then(|contents| contents["text"].as_str().map(str::to_owned))
    }

    /// The MIME type of the first contents.
    pub fn mime_type(&self) -> Option<String> {
        self.contents()
            .first()
            .and_then(|contents| contents["mimeType"].as_str().map(str::to_owned))
    }

    /// The decoded bytes of the first contents, when they are binary.
    pub fn blob(&self) -> Option<Vec<u8>> {
        self.contents()
            .first()
            .and_then(|contents| contents["blob"].as_str().map(str::to_owned))
            .and_then(|blob| STANDARD.decode(blob).ok())
    }

    /// The first contents' text parsed as JSON; for an error, Navree's error
    /// body (the JSON-RPC error's `data`).
    pub fn json_value(&self) -> Value {
        match &self.answer {
            McpResponse::Error(error) => error["data"].clone(),
            McpResponse::Result(_) => {
                let text = self.text().unwrap_or_default();
                serde_json::from_str(&text).unwrap_or(Value::String(text))
            }
        }
    }

    /// The contents as a `T`.
    ///
    /// # Panics
    ///
    /// When they do not deserialize.
    pub fn json<T: DeserializeOwned>(&self) -> T {
        let value = self.json_value();
        serde_json::from_value(value.clone()).unwrap_or_else(|error| {
            panic!("the resource {value} is not the expected type: {error}")
        })
    }

    /// Asserts the read succeeded.
    #[track_caller]
    pub fn assert_ok(self) -> Self {
        assert!(
            matches!(self.answer, McpResponse::Result(_)),
            "expected the resource, got {:?}",
            self.answer
        );
        self
    }

    /// Asserts the read failed with Navree's error `status` and, when
    /// given, `message`.
    #[track_caller]
    pub fn assert_error<'a>(self, status: u16, message: impl Into<Option<&'a str>>) -> Self {
        let McpResponse::Error(error) = &self.answer else {
            panic!("expected the read to fail, got {:?}", self.answer);
        };
        assert_error_body(&error["data"], status, message.into(), &error.to_string());
        self
    }

    /// Asserts the value at a dotted path of the contents.
    #[track_caller]
    pub fn assert_json_path(self, path: &str, expected: Value) -> Self {
        let value = self.json_value();
        assert_eq!(
            at_path(&value, path),
            Some(&expected),
            "at `{path}` in the resource {value}"
        );
        self
    }
}

/// The answer to `prompts/get`.
#[derive(Clone, Debug)]
pub struct PromptResult {
    answer: McpResponse,
}

impl PromptResult {
    fn new(answer: McpResponse) -> Self {
        Self { answer }
    }

    /// The raw answer.
    pub fn response(&self) -> &McpResponse {
        &self.answer
    }

    /// The messages (`[{role, content}]`); empty for an error.
    pub fn messages(&self) -> Vec<Value> {
        match &self.answer {
            McpResponse::Result(result) => {
                result["messages"].as_array().cloned().unwrap_or_default()
            }
            McpResponse::Error(_) => Vec::new(),
        }
    }

    /// The prompt's description.
    pub fn description(&self) -> Option<String> {
        match &self.answer {
            McpResponse::Result(result) => result["description"].as_str().map(str::to_owned),
            McpResponse::Error(_) => None,
        }
    }

    /// The text of message `index` (`None` for a resource message).
    pub fn text(&self, index: usize) -> Option<String> {
        self.messages()
            .get(index)
            .and_then(|message| message["content"]["text"].as_str().map(str::to_owned))
    }

    /// Asserts the prompt rendered.
    #[track_caller]
    pub fn assert_ok(self) -> Self {
        assert!(
            matches!(self.answer, McpResponse::Result(_)),
            "expected the prompt, got {:?}",
            self.answer
        );
        self
    }

    /// Asserts the prompt failed with Navree's error `status` and, when
    /// given, `message`.
    #[track_caller]
    pub fn assert_error<'a>(self, status: u16, message: impl Into<Option<&'a str>>) -> Self {
        let McpResponse::Error(error) = &self.answer else {
            panic!("expected the prompt to fail, got {:?}", self.answer);
        };
        assert_error_body(&error["data"], status, message.into(), &error.to_string());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_bodies_yield_their_json_messages() {
        let body = "id: 0\nretry: 3000\ndata:\n\nevent: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n\n";
        assert_eq!(
            sse_messages(body),
            [json!({ "jsonrpc": "2.0", "id": 1, "result": {} })]
        );
    }

    #[test]
    fn dotted_paths_walk_objects_and_arrays() {
        let value = json!({ "data": [{ "title": "Acme" }] });
        assert_eq!(at_path(&value, "data.0.title"), Some(&json!("Acme")));
        assert_eq!(at_path(&value, "data.1.title"), None);
        assert_eq!(at_path(&value, ""), Some(&value));
    }
}
