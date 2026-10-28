//! The MCP server: [`Navigator`]'s tools over the Model Context Protocol,
//! on stdio for `navree mcp` or any byte stream.
//!
//! A small server of its own (this crate sits below navree-mcp, which
//! serves apps): newline-delimited JSON-RPC 2.0, the `initialize` handshake
//! at the versions navree-mcp speaks (`2024-11-05` to `2025-11-25`), the
//! stateless `2026-07-28` protocol (`server/discover`, `_meta` on every
//! request), `tools/list` and `tools/call`, and `notifications/cancelled`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use serde_json::{Map, Value, json};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use tokio::task::{AbortHandle, JoinSet};

use crate::tools::{Navigator, NavigatorOptions, ToolDef, ToolOutput};

/// What the server tells the agent when it connects.
pub const INSTRUCTIONS: &str = "Navigator answers questions about this Navree app from the app itself. \
Call app_info first in a new project. Check list_routes before adding a route, module_graph or \
list_providers before wiring a provider, describe_schema before writing a migration or a query, and \
last_error or read_logs when a request fails. search_docs and read_doc read the docs of the Navree \
version in Cargo.lock. Every tool is read-only.";

/// Added to [`INSTRUCTIONS`] when the app includes Lighthouse.
pub const LIGHTHOUSE_INSTRUCTIONS: &str = " The app includes Lighthouse: lighthouse_list_issues and \
lighthouse_get_issue show what failed (exceptions with their request and backtrace), and \
lighthouse_slow_routes, lighthouse_slow_queries and lighthouse_failed_jobs show what is slow or failing.";

/// Every protocol version the server speaks, oldest first.
pub const SUPPORTED_VERSIONS: [&str; 5] = [
    "2024-11-05",
    "2025-03-26",
    "2025-06-18",
    "2025-11-25",
    STATELESS_VERSION,
];

/// The first stateless protocol version (no handshake).
const STATELESS_VERSION: &str = "2026-07-28";
/// The newest version with a handshake.
const LATEST_HANDSHAKE_VERSION: &str = "2025-11-25";
/// The longest message the server reads (longer lines are refused).
const MAX_LINE: usize = 4 * 1024 * 1024;
/// How deeply a message's arrays and objects may nest.
const MAX_DEPTH: usize = 64;
/// `_meta` keys of the stateless protocol.
const META_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
const META_CAPABILITIES: &str = "io.modelcontextprotocol/clientCapabilities";

/// The MCP server around a [`Navigator`].
#[derive(Clone, Debug)]
pub struct NavigatorServer {
    navigator: Arc<Navigator>,
}

impl NavigatorServer {
    /// A server answering with `navigator`.
    pub fn new(navigator: Arc<Navigator>) -> NavigatorServer {
        NavigatorServer { navigator }
    }

    /// The server's name, title, version and website.
    fn implementation() -> Value {
        json!({
            "name": "navree",
            "title": "Navigator",
            "version": env!("CARGO_PKG_VERSION"),
            "websiteUrl": "https://navree.com/docs/ai/navigator/",
        })
    }

    fn instructions(&self) -> String {
        if self.navigator.includes_lighthouse() {
            format!("{INSTRUCTIONS}{LIGHTHOUSE_INSTRUCTIONS}")
        } else {
            INSTRUCTIONS.to_owned()
        }
    }

    /// Serves `reader` and `writer` (one JSON-RPC message per line) until
    /// the client closes the stream.
    pub async fn serve<R, W>(self, reader: R, writer: W) -> Result<(), ServeError>
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
    {
        let (out, mut queue) = mpsc::channel::<Value>(64);
        let writing = tokio::spawn(async move {
            let mut writer = writer;
            while let Some(message) = queue.recv().await {
                let mut line = serde_json::to_vec(&message).unwrap_or_default();
                line.push(b'\n');
                if writer.write_all(&line).await.is_err() || writer.flush().await.is_err() {
                    break;
                }
            }
        });
        let mut reader = BufReader::new(reader);
        let mut connection = Connection {
            server: self,
            lifecycle: Lifecycle::Pending,
            running: Arc::default(),
            tasks: JoinSet::new(),
            out,
        };
        loop {
            let line = read_line(&mut reader)
                .await
                .map_err(|error| ServeError::Session(error.to_string()))?;
            match line {
                None => break,
                Some(Err(())) => {
                    connection
                        .send(error(
                            &Value::Null,
                            -32600,
                            &format!("Invalid Request: a message is longer than {MAX_LINE} bytes"),
                        ))
                        .await;
                }
                Some(Ok(bytes)) if bytes.iter().all(u8::is_ascii_whitespace) => {}
                Some(Ok(bytes)) => connection.receive(&bytes).await,
            }
            while connection.tasks.try_join_next().is_some() {}
        }
        connection.tasks.abort_all();
        while connection.tasks.join_next().await.is_some() {}
        drop(connection);
        let _ = writing.await;
        Ok(())
    }
}

/// Where the connection's lifecycle stands.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Lifecycle {
    Pending,
    /// After `initialize`, at the agreed version.
    Session(String),
    /// The stateless protocol: every request carries `_meta`.
    Inline,
}

/// One connection.
struct Connection {
    server: NavigatorServer,
    lifecycle: Lifecycle,
    /// Calls in flight, by their JSON id.
    running: Arc<Mutex<HashMap<String, AbortHandle>>>,
    tasks: JoinSet<()>,
    out: mpsc::Sender<Value>,
}

impl Connection {
    async fn send(&self, message: Value) {
        let _ = self.out.send(message).await;
    }

    async fn receive(&mut self, bytes: &[u8]) {
        if exceeds_depth(bytes) {
            let message = error(
                &Value::Null,
                -32600,
                &format!("Invalid Request: JSON nested deeper than {MAX_DEPTH} levels"),
            );
            return self.send(message).await;
        }
        let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
            return self.send(error(&Value::Null, -32700, "Parse error")).await;
        };
        match value {
            Value::Array(items) => {
                let allowed = self.lifecycle == Lifecycle::Session("2025-03-26".to_owned());
                if !allowed || items.is_empty() {
                    let message = error(
                        &Value::Null,
                        -32600,
                        "Invalid Request: JSON-RPC batches are allowed only on protocol 2025-03-26",
                    );
                    return self.send(message).await;
                }
                let mut answers = Vec::new();
                for item in items {
                    if let Some(answer) = self.message(item, false).await {
                        answers.push(answer);
                    }
                }
                if !answers.is_empty() {
                    self.send(Value::Array(answers)).await;
                }
            }
            value => {
                if let Some(answer) = self.message(value, true).await {
                    self.send(answer).await;
                }
            }
        }
    }

    /// Handles one message; its answer, when it has one now. With
    /// `detach`, tool calls run on their own task and answer later.
    async fn message(&mut self, value: Value, detach: bool) -> Option<Value> {
        let Value::Object(mut message) = value else {
            return Some(error(
                &Value::Null,
                -32600,
                "Invalid Request: not a JSON-RPC message",
            ));
        };
        let id = message.remove("id");
        let answer_id = id.clone().filter(valid_id).unwrap_or(Value::Null);
        if message.get("jsonrpc") != Some(&json!("2.0")) {
            return Some(error(
                &answer_id,
                -32600,
                "Invalid Request: jsonrpc must be \"2.0\"",
            ));
        }
        let Some(method) = message
            .get("method")
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            if message.contains_key("result") || message.contains_key("error") {
                return None;
            }
            return Some(error(&answer_id, -32600, "Invalid Request: no method"));
        };
        let params = match message.remove("params") {
            None | Some(Value::Null) => Map::new(),
            Some(Value::Object(params)) => params,
            Some(_) => {
                return Some(error(
                    &answer_id,
                    -32602,
                    "Invalid params: params must be an object",
                ));
            }
        };
        let Some(id) = id else {
            self.notification(&method, &params);
            return None;
        };
        if !valid_id(&id) {
            return Some(error(
                &Value::Null,
                -32600,
                "Invalid Request: the id must be a string or an integer",
            ));
        }
        self.request(id, &method, params, detach).await
    }

    fn notification(&self, method: &str, params: &Map<String, Value>) {
        if method == "notifications/cancelled"
            && let Some(id) = params.get("requestId")
            && let Some(running) = self
                .running
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&id.to_string())
        {
            running.abort();
        }
    }

    async fn request(
        &mut self,
        id: Value,
        method: &str,
        params: Map<String, Value>,
        detach: bool,
    ) -> Option<Value> {
        let session_version = match (&self.lifecycle, method) {
            (Lifecycle::Pending, "initialize") => {
                let Some(requested) = params.get("protocolVersion").and_then(Value::as_str) else {
                    return Some(error(
                        &id,
                        -32602,
                        "Invalid params: protocolVersion must be a string",
                    ));
                };
                let version = negotiate(requested);
                self.lifecycle = Lifecycle::Session(version.to_owned());
                return Some(result(
                    &id,
                    json!({
                        "protocolVersion": version,
                        "capabilities": { "tools": {} },
                        "serverInfo": NavigatorServer::implementation(),
                        "instructions": self.server.instructions(),
                    }),
                ));
            }
            (_, "initialize") => {
                return Some(error(
                    &id,
                    -32600,
                    "Invalid Request: the session is already initialized",
                ));
            }
            (Lifecycle::Pending, "ping") => return Some(result(&id, json!({}))),
            (Lifecycle::Session(version), _) => Some(version.clone()),
            (Lifecycle::Pending | Lifecycle::Inline, _) => None,
        };
        // The request's own version (its `_meta`), and whether it must carry
        // the stateless protocol's metadata.
        let meta = params.get("_meta").and_then(Value::as_object);
        let requested = meta
            .and_then(|meta| meta.get(META_VERSION))
            .and_then(Value::as_str);
        if let Some(requested) = requested
            && !SUPPORTED_VERSIONS.contains(&requested)
        {
            return Some(error_with(
                &id,
                -32022,
                "Unsupported protocol version",
                json!({ "requested": requested, "supported": SUPPORTED_VERSIONS }),
            ));
        }
        let inline = session_version.is_none();
        let needs_meta = inline
            || method == "server/discover"
            || requested.is_some_and(|version| version >= STATELESS_VERSION);
        if needs_meta {
            let mut missing = Vec::new();
            if requested.is_none() {
                missing.push(META_VERSION);
            }
            if !meta
                .and_then(|meta| meta.get(META_CAPABILITIES))
                .is_some_and(Value::is_object)
            {
                missing.push(META_CAPABILITIES);
            }
            if !missing.is_empty() {
                return Some(error(
                    &id,
                    -32602,
                    &format!(
                        "request _meta is missing or has malformed required fields: {}",
                        missing.join(", ")
                    ),
                ));
            }
            if self.lifecycle == Lifecycle::Pending && method != "server/discover" {
                self.lifecycle = Lifecycle::Inline;
            }
        }
        let version = requested
            .map(str::to_owned)
            .or(session_version)
            .unwrap_or_else(|| STATELESS_VERSION.to_owned());
        let stateless = version.as_str() >= STATELESS_VERSION;
        let legacy = !needs_meta && !stateless;
        let mut answer = match method {
            "ping" if legacy => json!({}),
            "server/discover" => json!({
                "resultType": "complete",
                "supportedVersions": SUPPORTED_VERSIONS,
                "capabilities": { "tools": {} },
                "instructions": self.server.instructions(),
                "ttlMs": 0,
                "cacheScope": "private",
                "_meta": { "io.modelcontextprotocol/serverInfo": NavigatorServer::implementation() },
            }),
            "tools/list" => {
                let tools: Vec<Value> = self.server.navigator.tools().iter().map(tool).collect();
                cacheable(json!({ "tools": tools }), stateless)
            }
            // Nothing else is offered: empty lists.
            "prompts/list" => cacheable(json!({ "prompts": [] }), stateless),
            "resources/list" => cacheable(json!({ "resources": [] }), stateless),
            "resources/templates/list" => cacheable(json!({ "resourceTemplates": [] }), stateless),
            "tools/call" => return self.call(id, params, stateless, detach).await,
            _ => {
                return Some(error_with(
                    &id,
                    -32601,
                    "Method not found",
                    json!({ "method": method }),
                ));
            }
        };
        if stateless && method != "ping" && method != "server/discover" {
            answer["resultType"] = json!("complete");
        }
        Some(result(&id, answer))
    }

    /// `tools/call`: runs the tool on its own task (when detached), so
    /// other requests and `notifications/cancelled` are read meanwhile.
    async fn call(
        &mut self,
        id: Value,
        params: Map<String, Value>,
        stateless: bool,
        detach: bool,
    ) -> Option<Value> {
        let Some(name) = params
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            return Some(error(&id, -32602, "Invalid params: name must be a string"));
        };
        if !self.server.navigator.tools().iter().any(|t| t.name == name) {
            return Some(error(&id, -32602, &format!("Unknown tool: {name}")));
        }
        let args = match params.get("arguments") {
            None | Some(Value::Null) => Value::Null,
            Some(Value::Object(args)) => Value::Object(args.clone()),
            Some(_) => {
                return Some(error(
                    &id,
                    -32602,
                    "Invalid params: arguments must be an object",
                ));
            }
        };
        let navigator = self.server.navigator.clone();
        let key = id.to_string();
        let run = async move {
            let mut answer = tool_result(navigator.call(&name, args).await);
            if stateless {
                answer["resultType"] = json!("complete");
            }
            result(&id, answer)
        };
        if !detach {
            return Some(run.await);
        }
        let out = self.out.clone();
        let running = self.running.clone();
        let done = key.clone();
        let handle = self.tasks.spawn(async move {
            let answer = run.await;
            running
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&done);
            let _ = out.send(answer).await;
        });
        self.running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key, handle);
        None
    }
}

/// Whether arrays and objects nest deeper than [`MAX_DEPTH`] outside
/// strings.
fn exceeds_depth(bytes: &[u8]) -> bool {
    let (mut depth, mut in_string, mut escaped) = (0usize, false, false);
    for &byte in bytes {
        if in_string {
            match (escaped, byte) {
                (true, _) => escaped = false,
                (false, b'\\') => escaped = true,
                (false, b'"') => in_string = false,
                _ => {}
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'[' | b'{' => {
                depth += 1;
                if depth > MAX_DEPTH {
                    return true;
                }
            }
            b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    false
}

/// One line of at most [`MAX_LINE`] bytes (`Err` for a longer one, which
/// is skipped without being kept); `None` at the end of the stream.
async fn read_line<R: AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
) -> std::io::Result<Option<Result<Vec<u8>, ()>>> {
    let mut line = Vec::new();
    let mut too_long = false;
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return Ok(match (too_long, line.is_empty()) {
                (true, _) => Some(Err(())),
                (false, true) => None,
                (false, false) => Some(Ok(line)),
            });
        }
        let end = available.iter().position(|byte| *byte == b'\n');
        let chunk = &available[..end.unwrap_or(available.len())];
        if !too_long {
            if line.len() + chunk.len() > MAX_LINE {
                too_long = true;
                line = Vec::new();
            } else {
                line.extend_from_slice(chunk);
            }
        }
        let used = end.map_or(available.len(), |end| end + 1);
        reader.consume(used);
        if end.is_some() {
            return Ok(Some(if too_long { Err(()) } else { Ok(line) }));
        }
    }
}

/// MCP request ids are strings or integers.
fn valid_id(id: &Value) -> bool {
    id.is_string() || id.as_i64().is_some() || id.as_u64().is_some()
}

/// The version `initialize` agrees to.
fn negotiate(requested: &str) -> &'static str {
    SUPPORTED_VERSIONS
        .iter()
        .copied()
        .find(|version| *version == requested && *version < STATELESS_VERSION)
        .unwrap_or(LATEST_HANDSHAKE_VERSION)
}

/// A list result with the stateless protocol's cache hints.
fn cacheable(mut list: Value, stateless: bool) -> Value {
    if stateless {
        list["ttlMs"] = json!(0);
        list["cacheScope"] = json!("private");
    }
    list
}

fn result(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn error_with(id: &Value, code: i64, message: &str, data: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message, "data": data } })
}

/// The protocol form of a tool definition.
fn tool(def: &ToolDef) -> Value {
    json!({
        "name": def.name,
        "title": def.title,
        "description": def.description,
        "inputSchema": def.input_schema,
        "annotations": {
            "title": def.title,
            "readOnlyHint": true,
            "destructiveHint": false,
            "idempotentHint": true,
            "openWorldHint": def.open_world,
        },
    })
}

/// A tool's answer as a tool result: JSON as pretty text (objects also as
/// structured content), Markdown as text, errors as error results.
fn tool_result(output: Result<ToolOutput, crate::tools::ToolError>) -> Value {
    let text = |text: String| json!([{ "type": "text", "text": text }]);
    match output {
        Ok(ToolOutput::Json(value)) => {
            let pretty = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
            let mut result = json!({ "content": text(pretty), "isError": false });
            if value.is_object() {
                result["structuredContent"] = value;
            }
            result
        }
        Ok(ToolOutput::Markdown(markdown)) => {
            json!({ "content": text(markdown), "isError": false })
        }
        Err(error) => json!({ "content": text(error.0), "isError": true }),
    }
}

/// Why the server stopped with an error.
#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    /// The client never completed the MCP handshake (no longer returned:
    /// a client that leaves before `initialize` ends the session cleanly).
    #[error("the MCP client did not initialize: {0}")]
    Initialize(String),
    /// The session ended with an error (reading from the client failed).
    #[error("the MCP session failed: {0}")]
    Session(String),
    /// The async runtime could not start.
    #[error("cannot start the async runtime: {0}")]
    Runtime(#[from] std::io::Error),
}

/// Serves `navigator` on `reader` and `writer` (JSON-RPC messages, one per
/// line) until the client closes the stream.
pub async fn serve<R, W>(navigator: Arc<Navigator>, reader: R, writer: W) -> Result<(), ServeError>
where
    R: AsyncRead + Send + Unpin + 'static,
    W: AsyncWrite + Send + Unpin + 'static,
{
    NavigatorServer::new(navigator).serve(reader, writer).await
}

/// `navree mcp`: serves Navigator for the app at `options.path` on stdin and
/// stdout until the client goes away. Nothing else is written to stdout.
pub fn run_stdio(options: NavigatorOptions) -> Result<(), ServeError> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let navigator = Arc::new(Navigator::new(options));
        serve(navigator, tokio::io::stdin(), tokio::io::stdout()).await
    })
}
