//! The streamable HTTP transport, mounted on the app's own router: `POST`,
//! `GET` and `DELETE` at `mcp.path`, the OAuth protected resource metadata
//! and the `WWW-Authenticate` challenge of 401s.
//!
//! - `POST` carries one JSON-RPC message (or, on a `2025-03-26` session, a
//!   batch). An answer that is ready before anything else is sent as JSON;
//!   when the server has notifications for the request first (progress, a
//!   `subscriptions/listen` stream) the answer is an SSE stream.
//! - `initialize` opens a session: its id (256 random bits) comes back in
//!   `Mcp-Session-Id`, and the session belongs to the user who opened it.
//! - `GET` opens the session's stream of notifications the server starts;
//!   `DELETE` ends the session.
//! - On the stateless protocol (2026-07-28) there are no sessions: every
//!   request carries `_meta` and the standard `Mcp-Method` / `Mcp-Name`
//!   headers, checked against the body.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::Request;
use axum::middleware::{Next, from_fn};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use bytes::Bytes;
use futures::future::join_all;
use http::header::{ACCEPT, ALLOW, CACHE_CONTROL, CONTENT_TYPE, ORIGIN, WWW_AUTHENTICATE};
use http::{HeaderMap, HeaderValue, Method, StatusCode};
use navree_cache::Cache;
use navree_contracts::{AuthState, Principal, RequestId};
use navree_http::{
    ApiMeta, AppError, Controller, GlobalLayer, Json, LayerSlot, RouteDef, RouteMeta,
};
use navree_kernel::{AppContext, BoxFuture, Injectable, KernelError, Resolver};
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_util::sync::{CancellationToken, DropGuard};

use crate::context::McpClient;
use crate::handler::{Caller, McpHandler, Scope, Shared};
use crate::notifier::McpNotifier;
use crate::options::{McpAuth, McpSettings};
use crate::protocol::jsonrpc::{
    self, HEADER_MISMATCH, INVALID_PARAMS, METHOD_NOT_FOUND, Message, Notification, Parsed,
    Request as RpcRequest, RpcError, UNSUPPORTED_PROTOCOL_VERSION,
};
use crate::protocol::lifecycle;
use crate::protocol::outlet::Outlet;
use crate::protocol::{STATELESS_VERSION, allows_batches, has_handshake, is_supported, negotiate};
use crate::registry::McpRegistry;
use crate::session::{Session, new_session_id};
use crate::sessions::Sessions;
use crate::stdio::cancelled_request;

/// The session header.
const SESSION_HEADER: &str = "mcp-session-id";
/// The protocol version header.
const VERSION_HEADER: &str = "mcp-protocol-version";
/// The standard method header (stateless protocol).
const METHOD_HEADER: &str = "mcp-method";
/// The standard name header (stateless protocol).
const NAME_HEADER: &str = "mcp-name";
/// The prefix of the standard argument headers (stateless protocol).
const PARAM_HEADER_PREFIX: &str = "mcp-param-";
/// The version a request without a version header is taken to speak.
const DEFAULT_VERSION: &str = "2025-03-26";
/// How often an idle SSE stream sends a comment, so proxies keep it open.
const KEEP_ALIVE: Duration = Duration::from_secs(15);

/// Serves the HTTP transport (registered by `McpModule`).
pub(crate) struct McpController {
    transport: Arc<Transport>,
}

/// The transport's state.
struct Transport {
    settings: Arc<McpSettings>,
    handler: McpHandler,
    sessions: Sessions,
    shutdown: CancellationToken,
}

impl Injectable for McpController {
    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            let settings = r.get::<McpSettings>().await?;
            let registry = r.get::<McpRegistry>().await?;
            let notifier = r.get::<McpNotifier>().await?;
            let app = r.app();
            let shared = Shared::new(&app, registry, &settings, notifier.clone())
                .await
                .map_err(|error| KernelError::Construction {
                    provider: std::any::type_name::<McpController>().to_owned(),
                    source: error.into(),
                })?;
            let cache = app.try_get::<Cache>().await;
            let ttl = Duration::from_secs(settings.session_ttl_secs);
            Ok(McpController {
                transport: Arc::new(Transport {
                    settings,
                    handler: McpHandler::new(Arc::new(shared)),
                    sessions: Sessions::new(cache, ttl, notifier),
                    shutdown: app.shutdown_token(),
                }),
            })
        })
    }
}

/// A response, or the error response that ends the request early.
type Checked<T> = Result<T, Response>;

impl Transport {
    /// One request of the transport: the Origin check, then the method.
    async fn handle(&self, request: Request) -> Response {
        // DNS rebinding: a browser page may only call from an allowed
        // origin. Clients that are not browsers send no Origin.
        if let Some(origin) = request.headers().get(ORIGIN) {
            let allowed = origin
                .to_str()
                .is_ok_and(|origin| self.settings.origin_allowed(origin));
            if !allowed {
                return AppError::forbidden()
                    .with_detail("This origin may not call the MCP endpoint")
                    .into_response();
            }
        }
        let principal = request
            .extensions()
            .get::<AuthState>()
            .and_then(|state| state.0.clone());
        let Some(principal) = principal else {
            return AppError::unauthenticated().into_response();
        };
        let result = match *request.method() {
            Method::POST => self.post(request, principal).await,
            Method::GET => self.get(request, principal).await,
            Method::DELETE => self.delete(request, principal).await,
            _ => Ok(method_not_allowed("GET, POST, DELETE")),
        };
        result.unwrap_or_else(|response| response)
    }

    async fn post(&self, request: Request, principal: Principal) -> Checked<Response> {
        let (parts, body) = request.into_parts();
        let accepts = header(&parts.headers, ACCEPT.as_str())?.is_some_and(|accept| {
            accept.contains("application/json") && accept.contains("text/event-stream")
        });
        if !accepts {
            return Err(status_error(
                StatusCode::NOT_ACCEPTABLE,
                "The client must accept both application/json and text/event-stream",
            ));
        }
        let json = header(&parts.headers, CONTENT_TYPE.as_str())?
            .is_some_and(|value| value.starts_with("application/json"));
        if !json {
            return Err(status_error(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "The body must be application/json",
            ));
        }
        let limit = self.settings.max_request_bytes;
        let bytes = axum::body::to_bytes(body, limit)
            .await
            .map_err(|_| AppError::payload_too_large(limit).into_response())?;
        let caller = Caller {
            request_id: parts
                .extensions
                .get::<RequestId>()
                .map(|id| id.as_str().to_owned())
                .unwrap_or_else(|| uuid::Uuid::now_v7().to_string()),
            principal,
            session_id: None,
        };
        let headers = &parts.headers;
        match jsonrpc::parse(&bytes) {
            Err((id, error)) => Err(rpc_response(StatusCode::BAD_REQUEST, &id, &error)),
            Ok(Parsed::Batch(entries)) => self.batch(headers, caller, entries).await,
            Ok(Parsed::Single(message)) => self.message(headers, caller, message).await,
        }
    }

    /// One message: through a session (the handshake protocols) or on its
    /// own (the stateless protocol, and discovery).
    async fn message(
        &self,
        headers: &HeaderMap,
        mut caller: Caller,
        message: Message,
    ) -> Checked<Response> {
        if !uses_session(&message, headers)? {
            check_standard_headers(headers, &message, |name| self.tool_schema(name))?;
            return match message {
                Message::Request(request) => self.direct(headers, caller, request).await,
                Message::Notification(_) => Ok(accepted()),
                Message::Reply => Err(rpc_response(
                    StatusCode::BAD_REQUEST,
                    &Value::Null,
                    &RpcError::invalid_request(
                        "Invalid Request: the stateless protocol takes no responses",
                    ),
                )),
            };
        }
        let Some(id) = header(headers, SESSION_HEADER)? else {
            if is_request(&message, "server/discover") {
                check_standard_headers(headers, &message, |name| self.tool_schema(name))?;
                let Message::Request(request) = message else {
                    unreachable!("checked above");
                };
                return self.direct(headers, caller, request).await;
            }
            check_standard_headers(headers, &message, |name| self.tool_schema(name))?;
            return match message {
                Message::Request(request) if request.method == "initialize" => {
                    self.initialize(headers, &caller.principal, &request).await
                }
                _ => Err(status_error(
                    StatusCode::BAD_REQUEST,
                    "A session id is required: send initialize first",
                )),
            };
        };
        let session = self.session(id, &caller.principal).await?;
        check_version_header(headers, message_meta_version(&message).is_some())?;
        check_meta_matches_header(headers, &message)?;
        check_standard_headers(headers, &message, |name| self.tool_schema(name))?;
        caller.session_id = Some(session.id.clone());
        match message {
            Message::Request(request) if request.method == "initialize" => Ok(json_response(
                StatusCode::OK,
                &jsonrpc::error(
                    &request.id,
                    &RpcError::invalid_request(
                        "Invalid Request: the session is already initialized",
                    ),
                ),
            )),
            Message::Request(request) => Ok(self.in_session(&session, caller, request).await),
            Message::Notification(notification) => {
                self.session_notification(&session, &notification);
                Ok(accepted())
            }
            Message::Reply => Ok(accepted()),
        }
    }

    /// `initialize`: agrees on a version, opens a session for the caller and
    /// answers with its id.
    async fn initialize(
        &self,
        headers: &HeaderMap,
        principal: &Principal,
        request: &RpcRequest,
    ) -> Checked<Response> {
        let Some(requested) = request
            .params
            .get("protocolVersion")
            .and_then(Value::as_str)
        else {
            return Ok(json_response(
                StatusCode::OK,
                &jsonrpc::error(
                    &request.id,
                    &RpcError::invalid_params("Invalid params: protocolVersion must be a string"),
                ),
            ));
        };
        // A version header must name the version the body asks for.
        if let Some(named) = header(headers, VERSION_HEADER)?
            && named != requested
        {
            return Err(rpc_response(
                StatusCode::BAD_REQUEST,
                &request.id,
                &RpcError::invalid_request(format!(
                    "Invalid Request: MCP-Protocol-Version header ({named}) does not match initialize params.protocolVersion ({requested})"
                )),
            ));
        }
        let version = negotiate(requested);
        let client = lifecycle::client_info(request.params.get("clientInfo"))
            .map(|(name, version)| McpClient { name, version });
        let session = Session::new(
            new_session_id(),
            Some(principal.user_id),
            version.to_owned(),
            client,
        );
        self.sessions.insert(&session).await;
        let answer = jsonrpc::result(&request.id, self.handler.info().initialize_result(version));
        let mut response = json_response(StatusCode::OK, &answer);
        if let Ok(value) = HeaderValue::from_str(&session.id) {
            response.headers_mut().insert(SESSION_HEADER, value);
        }
        Ok(response)
    }

    /// The caller's session `id`, or the 404 of a session that does not
    /// exist (or is someone else's).
    async fn session(&self, id: &str, principal: &Principal) -> Checked<Arc<Session>> {
        self.sessions
            .find(id, principal.user_id)
            .await
            .ok_or_else(|| {
                AppError::not_found()
                    .with_detail("Session not found")
                    .into_response()
            })
    }

    /// A request in a session. Disconnecting does not cancel it (the
    /// handshake protocols cancel with `notifications/cancelled`).
    async fn in_session(
        &self,
        session: &Arc<Session>,
        caller: Caller,
        request: RpcRequest,
    ) -> Response {
        let prepared = match lifecycle::prepare(&request, Some(&session.version), false) {
            Ok(prepared) => prepared,
            Err(error) => {
                return json_response(StatusCode::OK, &jsonrpc::error(&request.id, &error));
            }
        };
        let cancel = self.shutdown.child_token();
        let guard = session.track(&request.id, cancel.clone());
        let client = session.client.clone().or_else(|| meta_client(&request));
        let (outlet, queue) = Outlet::channel();
        let scope = Scope {
            caller,
            session: Some(session.clone()),
            client,
            outlet: outlet.clone(),
            cancel,
            shutdown: self.shutdown.clone(),
        };
        let handler = self.handler.clone();
        tokio::spawn(async move {
            let _guard = guard;
            if let Some(answer) = handler.answer(&scope, &prepared, request).await {
                scope.outlet.deliver(answer).await;
            }
        });
        drop(outlet);
        self.reply(queue, None, |_| StatusCode::OK).await
    }

    /// A request outside any session (the stateless protocol, discovery):
    /// it runs at the version its `_meta` (or header) names, and a client
    /// that disconnects cancels it.
    async fn direct(
        &self,
        headers: &HeaderMap,
        caller: Caller,
        request: RpcRequest,
    ) -> Checked<Response> {
        let version = header(headers, VERSION_HEADER)?.unwrap_or(DEFAULT_VERSION);
        let malformed = !lifecycle::has_complete_meta(&request.params);
        let prepared = match lifecycle::prepare(&request, Some(version), false) {
            Ok(prepared) => prepared,
            Err(error) => {
                let status = stateless_status(&error, malformed);
                return Ok(json_response(status, &jsonrpc::error(&request.id, &error)));
            }
        };
        let cancel = self.shutdown.child_token();
        let client = meta_client(&request);
        let (outlet, queue) = Outlet::channel();
        let scope = Scope {
            caller,
            session: None,
            client,
            outlet: outlet.clone(),
            cancel: cancel.clone(),
            shutdown: self.shutdown.clone(),
        };
        let handler = self.handler.clone();
        tokio::spawn(async move {
            if let Some(answer) = handler.answer(&scope, &prepared, request).await {
                scope.outlet.deliver(answer).await;
            }
        });
        drop(outlet);
        Ok(self
            .reply(queue, Some(cancel), |answer| {
                answer
                    .get("error")
                    .map(|error| {
                        let code = error["code"].as_i64().unwrap_or_default();
                        stateless_status(&RpcError::new(code, "", None), malformed)
                    })
                    .unwrap_or(StatusCode::OK)
            })
            .await)
    }

    /// The HTTP answer to a request: JSON when its answer comes first, an
    /// SSE stream when notifications come before it. With `cancel`, the
    /// request is cancelled when the client goes away before it is done.
    async fn reply(
        &self,
        mut queue: mpsc::Receiver<Value>,
        cancel: Option<CancellationToken>,
        status: impl Fn(&Value) -> StatusCode,
    ) -> Response {
        // Dropped (the client went away) while waiting: cancel the request.
        let waiting = cancel.clone().map(CancellationToken::drop_guard);
        let first = queue.recv().await;
        if let Some(guard) = waiting {
            guard.disarm();
        }
        let Some(first) = first else {
            // Cancelled before it answered: nothing to say.
            return sse_response(SseStream::new(None, None, self.shutdown.clone(), None));
        };
        if jsonrpc::is_answer(&first) {
            return json_response(status(&first), &first);
        }
        sse_response(SseStream::new(
            Some(first),
            Some(queue),
            self.shutdown.clone(),
            cancel.map(CancellationToken::drop_guard),
        ))
    }

    /// A notification in a session.
    fn session_notification(&self, session: &Arc<Session>, notification: &Notification) {
        match notification.method.as_str() {
            "notifications/initialized" => self.handler.register(session),
            "notifications/cancelled" => {
                if let Some(id) = cancelled_request(&notification.params) {
                    session.cancel(id);
                }
            }
            _ => {}
        }
    }

    /// A batch: allowed only on a `2025-03-26` session; the answers come
    /// back together as a JSON array (progress is not streamed).
    async fn batch(
        &self,
        headers: &HeaderMap,
        mut caller: Caller,
        entries: Vec<Result<Message, (Value, RpcError)>>,
    ) -> Checked<Response> {
        let session = match header(headers, SESSION_HEADER)? {
            Some(id) => Some(self.session(id, &caller.principal).await?),
            None => None,
        };
        let Some(session) = session.filter(|session| allows_batches(&session.version)) else {
            return Err(rpc_response(
                StatusCode::BAD_REQUEST,
                &Value::Null,
                &RpcError::invalid_request(
                    "Invalid Request: JSON-RPC batches are allowed only on protocol 2025-03-26",
                ),
            ));
        };
        check_version_header(headers, false)?;
        caller.session_id = Some(session.id.clone());
        let mut answers = Vec::new();
        let mut pending = Vec::new();
        for entry in entries {
            match entry {
                Err((id, error)) => answers.push(jsonrpc::error(&id, &error)),
                Ok(Message::Notification(notification)) => {
                    self.session_notification(&session, &notification);
                }
                Ok(Message::Reply) => {}
                Ok(Message::Request(request)) if request.method == "initialize" => {
                    answers.push(jsonrpc::error(
                        &request.id,
                        &RpcError::invalid_request("Invalid Request: initialize cannot be batched"),
                    ));
                }
                Ok(Message::Request(request)) => {
                    let caller = caller.clone();
                    let session = session.clone();
                    pending.push(async move {
                        let response = self.in_session(&session, caller, request).await;
                        answer_of(response).await
                    });
                }
            }
        }
        answers.extend(join_all(pending).await.into_iter().flatten());
        if answers.is_empty() {
            return Ok(accepted());
        }
        Ok(json_response(StatusCode::OK, &Value::Array(answers)))
    }

    /// `GET`: the session's stream of notifications the server starts.
    async fn get(&self, request: Request, principal: Principal) -> Checked<Response> {
        let headers = request.headers();
        let accepts = header(headers, ACCEPT.as_str())?
            .is_some_and(|accept| accept.contains("text/event-stream"));
        if !accepts {
            return Err(status_error(
                StatusCode::NOT_ACCEPTABLE,
                "The client must accept text/event-stream",
            ));
        }
        check_version_header(headers, false)?;
        if header(headers, VERSION_HEADER)?.is_some_and(|version| !has_handshake(version)) {
            // The stateless protocol has no session stream.
            return Ok(method_not_allowed("POST"));
        }
        let Some(id) = header(headers, SESSION_HEADER)? else {
            return Err(status_error(
                StatusCode::BAD_REQUEST,
                "A session id is required: send initialize first",
            ));
        };
        let session = self.session(id, &principal).await?;
        if headers.contains_key("last-event-id") {
            // Streams are not replayed: an empty stream ends the resumption.
            return Ok(sse_response(SseStream::new(
                None,
                None,
                self.shutdown.clone(),
                None,
            )));
        }
        let (outlet, queue) = Outlet::channel();
        session.set_stream(outlet);
        self.handler.register(&session);
        Ok(sse_response(SseStream::new(
            None,
            Some(queue),
            self.shutdown.clone(),
            None,
        )))
    }

    /// `DELETE`: ends the session.
    async fn delete(&self, request: Request, principal: Principal) -> Checked<Response> {
        let headers = request.headers();
        check_version_header(headers, false)?;
        if header(headers, VERSION_HEADER)?.is_some_and(|version| !has_handshake(version)) {
            return Ok(method_not_allowed("POST"));
        }
        let Some(id) = header(headers, SESSION_HEADER)? else {
            return Err(status_error(
                StatusCode::BAD_REQUEST,
                "A session id is required",
            ));
        };
        let session = self.session(id, &principal).await?;
        self.sessions.remove(&session).await;
        Ok(accepted())
    }

    /// The input schema of tool `name`, for the `Mcp-Param-*` checks.
    fn tool_schema(&self, name: &str) -> Option<Value> {
        self.handler.tool_schema(name)
    }
}

/// The answer message inside a response this transport built.
async fn answer_of(response: Response) -> Option<Value> {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .ok()?;
    if let Ok(answer) = serde_json::from_slice::<Value>(&bytes) {
        return Some(answer);
    }
    // An SSE reply: its answer is the last message.
    String::from_utf8_lossy(&bytes)
        .split("\n\n")
        .filter_map(|event| {
            event
                .lines()
                .find_map(|line| line.strip_prefix("data: "))
                .and_then(|data| serde_json::from_str::<Value>(data).ok())
        })
        .find(jsonrpc::is_answer)
}

/// Whether a message goes through a session (the handshake protocols): an
/// `initialize`, or a message whose version (its `_meta`'s, else the
/// header's, else `2025-03-26`) has a handshake and that does not carry the
/// stateless protocol's complete `_meta`. Checks the version header and the
/// `_meta` against it on the way.
fn uses_session(message: &Message, headers: &HeaderMap) -> Checked<bool> {
    let meta_version = message_meta_version(message);
    check_version_header(headers, meta_version.is_some())?;
    if is_request(message, "initialize") {
        return Ok(true);
    }
    check_meta_matches_header(headers, message)?;
    let complete = match message {
        Message::Request(request) => lifecycle::has_complete_meta(&request.params),
        _ => false,
    };
    let version = meta_version
        .or(header(headers, VERSION_HEADER)?)
        .unwrap_or(DEFAULT_VERSION);
    Ok(!complete && has_handshake(version))
}

/// The protocol version a request's `_meta` names.
fn message_meta_version(message: &Message) -> Option<&str> {
    match message {
        Message::Request(request) => lifecycle::meta_version(&request.params),
        _ => None,
    }
}

fn is_request(message: &Message, method: &str) -> bool {
    matches!(message, Message::Request(request) if request.method == method)
}

/// The client in a request's `_meta`.
fn meta_client(request: &RpcRequest) -> Option<McpClient> {
    lifecycle::meta_client(&request.params).map(|(name, version)| McpClient { name, version })
}

/// The `MCP-Protocol-Version` header must name a version the server speaks
/// (any version when the request's `_meta` names its own, which the
/// lifecycle checks).
fn check_version_header(headers: &HeaderMap, meta_names_version: bool) -> Checked<()> {
    if let Some(version) = header(headers, VERSION_HEADER)?
        && !meta_names_version
        && !is_supported(version)
    {
        return Err(status_error(
            StatusCode::BAD_REQUEST,
            &format!("Unsupported MCP-Protocol-Version: {version}"),
        ));
    }
    Ok(())
}

/// A request's `_meta` protocol version and the `MCP-Protocol-Version`
/// header must agree; a request on the stateless protocol needs its
/// `_meta`.
fn check_meta_matches_header(headers: &HeaderMap, message: &Message) -> Checked<()> {
    let Message::Request(request) = message else {
        return Ok(());
    };
    if request.method == "initialize" {
        return Ok(());
    }
    let named = header(headers, VERSION_HEADER)?;
    let Some(meta_version) = lifecycle::meta_version(&request.params) else {
        let needs_meta = request.method == "server/discover"
            || named.is_some_and(|version| version >= STATELESS_VERSION);
        if needs_meta {
            let missing = lifecycle::missing_meta(&request.params);
            return Err(rpc_response(
                StatusCode::BAD_REQUEST,
                &request.id,
                &lifecycle::missing_meta_error(&missing),
            ));
        }
        return Ok(());
    };
    match named {
        None => Err(rpc_response(
            StatusCode::BAD_REQUEST,
            &request.id,
            &RpcError::new(
                HEADER_MISMATCH,
                "request _meta protocolVersion requires the MCP-Protocol-Version header",
                None,
            ),
        )),
        Some(named) if named != meta_version => Err(rpc_response(
            StatusCode::BAD_REQUEST,
            &request.id,
            &RpcError::new(
                HEADER_MISMATCH,
                format!(
                    "MCP-Protocol-Version header ({named}) does not match request _meta protocolVersion ({meta_version})"
                ),
                None,
            ),
        )),
        Some(_) => Ok(()),
    }
}

/// On the stateless protocol, the standard headers must match the body:
/// `Mcp-Method` the method, `Mcp-Name` the tool, prompt or resource, and
/// `Mcp-Param-<name>` the arguments a tool's schema marks with
/// `x-mcp-header` (values may be `=?base64?...?=`). Intermediaries route on
/// them without reading the body.
fn check_standard_headers(
    headers: &HeaderMap,
    message: &Message,
    tool_schema: impl Fn(&str) -> Option<Value>,
) -> Checked<()> {
    let applies =
        header(headers, VERSION_HEADER)?.is_some_and(|version| version >= STATELESS_VERSION);
    if !applies {
        return Ok(());
    }
    let (id, method, params) = match message {
        Message::Request(request) => (request.id.clone(), &request.method, &request.params),
        Message::Notification(notification) => {
            (Value::Null, &notification.method, &notification.params)
        }
        Message::Reply => return Ok(()),
    };
    let mismatch = |reason: String| {
        rpc_response(
            StatusCode::BAD_REQUEST,
            &id,
            &RpcError::new(HEADER_MISMATCH, reason, None),
        )
    };
    let single = |name: &str| -> Checked<Option<&str>> {
        let mut values = headers.get_all(name).iter();
        let Some(first) = values.next() else {
            return Ok(None);
        };
        if values.next().is_some() {
            return Err(mismatch(format!("duplicate {name} header")));
        }
        first
            .to_str()
            .map(Some)
            .map_err(|_| mismatch(format!("{name} header is not valid text")))
    };
    if method == "initialize" {
        // The handshake may leave the header out, but not contradict it.
        return match single(METHOD_HEADER)? {
            Some(value) if value != "initialize" => Err(mismatch(format!(
                "Mcp-Method header `{value}` does not match body method `initialize`"
            ))),
            _ => Ok(()),
        };
    }
    match single(METHOD_HEADER)? {
        None => return Err(mismatch("missing required Mcp-Method header".to_owned())),
        Some(value) if value != method => {
            return Err(mismatch(format!(
                "Mcp-Method header `{value}` does not match body method `{method}`"
            )));
        }
        Some(_) => {}
    }
    let name_key = match method.as_str() {
        "tools/call" | "prompts/get" => Some("name"),
        "resources/read" | "resources/subscribe" | "resources/unsubscribe" => Some("uri"),
        _ => None,
    };
    if let Some(expected) = name_key
        .and_then(|key| params.get(key))
        .and_then(Value::as_str)
    {
        let Some(raw) = single(NAME_HEADER)? else {
            return Err(mismatch(format!(
                "missing required Mcp-Name header for `{method}`"
            )));
        };
        let decoded = decode_header(raw)
            .ok_or_else(|| mismatch("Mcp-Name header is not valid Base64".to_owned()))?;
        if decoded != expected {
            return Err(mismatch(format!(
                "Mcp-Name header `{decoded}` does not match body value `{expected}`"
            )));
        }
    }
    if method == "tools/call"
        && let Some(schema) = params
            .get("name")
            .and_then(Value::as_str)
            .and_then(tool_schema)
    {
        let arguments = params.get("arguments");
        for (path, name) in param_headers(&schema) {
            let full = format!("{PARAM_HEADER_PREFIX}{name}");
            let sent = single(&full)?;
            let value = path
                .iter()
                .try_fold(arguments, |value, key| value?.get(key).map(Some))
                .flatten()
                .filter(|value| !value.is_null())
                .and_then(primitive_text);
            match (sent, value) {
                (None, None) => {}
                (Some(_), None) => {
                    return Err(mismatch(format!("unexpected {full} header")));
                }
                (None, Some(_)) => return Err(mismatch(format!("missing {full} header"))),
                (Some(raw), Some(expected)) => {
                    let decoded = decode_header(raw)
                        .ok_or_else(|| mismatch(format!("{full} header is not valid Base64")))?;
                    if decoded != expected {
                        return Err(mismatch(format!(
                            "{full} header `{decoded}` does not match the argument"
                        )));
                    }
                }
            }
        }
    }
    Ok(())
}

/// The `x-mcp-header` annotations of a tool schema: the property path and
/// the header name.
fn param_headers(schema: &Value) -> Vec<(Vec<String>, String)> {
    fn walk(
        properties: Option<&Value>,
        path: &mut Vec<String>,
        out: &mut Vec<(Vec<String>, String)>,
    ) {
        let Some(Value::Object(properties)) = properties else {
            return;
        };
        for (name, property) in properties {
            path.push(name.clone());
            if let Some(Value::String(header)) = property.get("x-mcp-header")
                && !header.is_empty()
            {
                out.push((path.clone(), header.to_ascii_lowercase()));
            }
            walk(property.get("properties"), path, out);
            path.pop();
        }
    }
    let mut out = Vec::new();
    walk(schema.get("properties"), &mut Vec::new(), &mut out);
    out
}

/// A JSON primitive as header text.
fn primitive_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Bool(flag) => Some(flag.to_string()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

/// A standard header's value: as sent, or decoded from `=?base64?...?=`.
fn decode_header(value: &str) -> Option<String> {
    match value
        .strip_prefix("=?base64?")
        .and_then(|inner| inner.strip_suffix("?="))
    {
        Some(inner) => String::from_utf8(STANDARD.decode(inner).ok()?).ok(),
        None => Some(value.to_owned()),
    }
}

/// A header's value as text; the 400 of a header that is not text.
fn header<'a>(headers: &'a HeaderMap, name: &str) -> Checked<Option<&'a str>> {
    match headers.get(name) {
        None => Ok(None),
        Some(value) => value.to_str().map(Some).map_err(|_| {
            status_error(
                StatusCode::BAD_REQUEST,
                &format!("The {name} header is not valid text"),
            )
        }),
    }
}

/// The HTTP status of a stateless request's JSON-RPC error: a refused
/// version, header or capability is a 400, as is a request missing its
/// `_meta`; an unknown method a 404; other errors travel in a 200.
fn stateless_status(error: &RpcError, meta_malformed: bool) -> StatusCode {
    match error.code {
        UNSUPPORTED_PROTOCOL_VERSION | HEADER_MISMATCH | -32021 => StatusCode::BAD_REQUEST,
        INVALID_PARAMS if meta_malformed => StatusCode::BAD_REQUEST,
        METHOD_NOT_FOUND => StatusCode::NOT_FOUND,
        _ => StatusCode::OK,
    }
}

/// A JSON response.
fn json_response(status: StatusCode, body: &Value) -> Response {
    let mut response = Response::new(Body::from(body.to_string()));
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    response
}

/// A JSON-RPC error answer with an HTTP status.
fn rpc_response(status: StatusCode, id: &Value, error: &RpcError) -> Response {
    json_response(status, &jsonrpc::error(id, error))
}

/// Navree's error body with `status`.
fn status_error(status: StatusCode, detail: &str) -> Response {
    AppError::from_status(status)
        .with_detail(detail)
        .into_response()
}

/// `202 Accepted`, for notifications.
fn accepted() -> Response {
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::ACCEPTED;
    response
}

/// `405 Method Not Allowed` with the methods that are.
fn method_not_allowed(allow: &'static str) -> Response {
    let mut response = status_error(
        StatusCode::METHOD_NOT_ALLOWED,
        "This method is not allowed here",
    );
    response
        .headers_mut()
        .insert(ALLOW, HeaderValue::from_static(allow));
    response
}

/// An SSE stream of JSON-RPC messages: an optional first message, then
/// what arrives on the queue, with keep-alive comments while idle. It ends
/// when the queue closes or the app shuts down. With a guard, a client
/// that goes away before the end cancels the request.
struct SseStream {
    first: Option<Value>,
    queue: Option<mpsc::Receiver<Value>>,
    shutdown: CancellationToken,
    guard: Option<DropGuard>,
    ticker: tokio::time::Interval,
}

impl SseStream {
    fn new(
        first: Option<Value>,
        queue: Option<mpsc::Receiver<Value>>,
        shutdown: CancellationToken,
        guard: Option<DropGuard>,
    ) -> Self {
        let mut ticker =
            tokio::time::interval_at(tokio::time::Instant::now() + KEEP_ALIVE, KEEP_ALIVE);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        Self {
            first,
            queue,
            shutdown,
            guard,
            ticker,
        }
    }

    /// The next chunk, or `None` at the end.
    async fn next(&mut self) -> Option<Bytes> {
        if let Some(message) = self.first.take() {
            return Some(event(&message));
        }
        let queue = self.queue.as_mut()?;
        tokio::select! {
            biased;
            message = queue.recv() => match message {
                Some(message) => Some(event(&message)),
                None => {
                    // Done: the request answered, nothing to cancel.
                    if let Some(guard) = self.guard.take() {
                        guard.disarm();
                    }
                    None
                }
            },
            () = self.shutdown.cancelled() => None,
            _ = self.ticker.tick() => Some(Bytes::from_static(b": keep-alive\n\n")),
        }
    }
}

/// One SSE event carrying a message.
fn event(message: &Value) -> Bytes {
    Bytes::from(format!("event: message\ndata: {message}\n\n"))
}

/// The response of an SSE stream.
fn sse_response(stream: SseStream) -> Response {
    let body = futures::stream::unfold(stream, |mut stream| async move {
        stream
            .next()
            .await
            .map(|chunk| (Ok::<_, Infallible>(chunk), stream))
    });
    let mut response = Response::new(Body::from_stream(body));
    let headers = response.headers_mut();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    response
}

impl Controller for McpController {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let settings = &self.transport.settings;
        let path = format!("!{}", settings.path);
        let body_limit = settings.max_request_bytes;
        // One name per route: navree-api turns names into operation ids.
        let transport_routes = [
            (
                Method::POST,
                "mcp.message",
                "Send an MCP JSON-RPC message (JSON or SSE answer)",
            ),
            (
                Method::GET,
                "mcp.stream",
                "Open the SSE stream of server-sent MCP messages",
            ),
            (Method::DELETE, "mcp.close", "End the MCP session"),
        ];
        let mut routes: Vec<RouteDef> = transport_routes
            .into_iter()
            .map(|(method, name, summary)| {
                let transport = self.transport.clone();
                RouteDef::on(method, path.clone(), move |request: Request| {
                    let transport = transport.clone();
                    async move { transport.handle(request).await }
                })
                .authenticated()
                .body_limit(body_limit)
                .name(name)
                .api(api_meta(summary))
            })
            .collect();
        if let Some(oauth) = &settings.oauth {
            let document = oauth.metadata(&settings.title);
            let metadata_routes = [
                (
                    "!/.well-known/oauth-protected-resource".to_owned(),
                    "mcp.metadata",
                ),
                (
                    format!("!/.well-known/oauth-protected-resource{}", settings.path),
                    "mcp.metadata.endpoint",
                ),
            ];
            for (path, name) in metadata_routes {
                let document = document.clone();
                routes.push(
                    RouteDef::on(Method::GET, path, move || {
                        let document = document.clone();
                        async move { Json(document) }
                    })
                    .public()
                    .name(name)
                    .api(api_meta(
                        "The MCP endpoint's OAuth protected resource metadata (RFC 9728)",
                    )),
                );
            }
        }
        routes
    }
}

/// The OpenAPI metadata of an MCP route: tag `MCP` and `summary`.
fn api_meta(summary: &str) -> ApiMeta {
    ApiMeta {
        summary: Some(summary.to_owned()),
        tags: vec!["MCP".to_owned()],
        ..ApiMeta::default()
    }
}

/// The layer that adds the `WWW-Authenticate` challenge to the endpoint's
/// 401s: `Bearer` with `auth = "bearer"`, `Bearer resource_metadata="..."`
/// with `auth = "oauth"` (keeping an authenticator's `error` parameters).
pub(crate) fn challenge_layer() -> GlobalLayer {
    GlobalLayer::new(
        LayerSlot::Session,
        "mcp-challenge",
        |_ctx, router: Router| Ok(router.layer(from_fn(challenge))),
    )
}

async fn challenge(request: Request, next: Next) -> Response {
    let settings = match request.extensions().get::<AppContext>() {
        Some(ctx) => ctx.get::<McpSettings>().await.ok(),
        None => None,
    };
    let Some(settings) = settings.filter(|settings| request.uri().path() == settings.path) else {
        return next.run(request).await;
    };
    let response = next.run(request).await;
    if response.status() != StatusCode::UNAUTHORIZED {
        return response;
    }
    let existing = response
        .headers()
        .get(WWW_AUTHENTICATE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let value = match (settings.auth, &settings.oauth) {
        (McpAuth::Oauth, Some(oauth)) => {
            let metadata = format!("resource_metadata=\"{}\"", oauth.metadata_url());
            match existing.as_deref().and_then(|e| e.strip_prefix("Bearer")) {
                Some(params) if !params.trim().is_empty() => {
                    format!(
                        "Bearer {metadata}, {}",
                        params.trim().trim_start_matches(',')
                    )
                }
                _ => format!("Bearer {metadata}"),
            }
        }
        // An authenticator's own challenge (`error="invalid_token"`) stays.
        _ if existing.is_some() => return response,
        _ => "Bearer".to_owned(),
    };
    let Ok(value) = HeaderValue::from_str(&value) else {
        return response;
    };
    rebuild(response, value)
}

/// The 401 as a fresh response with the challenge. The rendered error body
/// and headers are kept; the pending error in the extensions is dropped, so
/// the error layer does not render it again over the new header. The
/// route's metadata stays for request metrics.
fn rebuild(response: Response, challenge: HeaderValue) -> Response {
    let (parts, body) = response.into_parts();
    let mut rebuilt = Response::new(body);
    *rebuilt.status_mut() = parts.status;
    *rebuilt.headers_mut() = parts.headers;
    rebuilt.headers_mut().insert(WWW_AUTHENTICATE, challenge);
    if let Some(meta) = parts.extensions.get::<Arc<RouteMeta>>() {
        rebuilt.extensions_mut().insert(meta.clone());
    }
    rebuilt
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn standard_header_values_may_be_base64() {
        assert_eq!(decode_header("list_deals").as_deref(), Some("list_deals"));
        assert_eq!(
            decode_header("=?base64?Y2Fmw6k=?=").as_deref(),
            Some("café")
        );
        assert_eq!(decode_header("=?base64?not base64?="), None);
    }

    #[test]
    fn annotated_arguments_become_param_headers() {
        let schema = json!({ "properties": {
            "region": { "type": "string", "x-mcp-header": "Region" },
            "filter": { "properties": { "tenant": { "type": "string", "x-mcp-header": "Tenant" } } },
            "plain": { "type": "string" },
        }});
        let mut headers = param_headers(&schema);
        headers.sort();
        assert_eq!(
            headers,
            [
                (
                    vec!["filter".to_owned(), "tenant".to_owned()],
                    "tenant".to_owned()
                ),
                (vec!["region".to_owned()], "region".to_owned()),
            ]
        );
    }

    #[test]
    fn stateless_errors_map_to_statuses() {
        let error = |code| RpcError::new(code, "", None);
        assert_eq!(
            stateless_status(&error(-32022), false),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            stateless_status(&error(-32020), false),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            stateless_status(&error(-32602), true),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(stateless_status(&error(-32602), false), StatusCode::OK);
        assert_eq!(
            stateless_status(&error(-32601), false),
            StatusCode::NOT_FOUND
        );
        assert_eq!(stateless_status(&error(-32603), false), StatusCode::OK);
    }

    #[tokio::test]
    async fn sse_streams_end_with_their_queue() {
        let (tx, rx) = mpsc::channel(4);
        let mut stream = SseStream::new(
            Some(json!({ "first": true })),
            Some(rx),
            CancellationToken::new(),
            None,
        );
        assert_eq!(
            stream.next().await.unwrap(),
            Bytes::from("event: message\ndata: {\"first\":true}\n\n")
        );
        tx.send(json!(2)).await.unwrap();
        drop(tx);
        assert_eq!(
            stream.next().await.unwrap(),
            Bytes::from("event: message\ndata: 2\n\n")
        );
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn a_dropped_stream_cancels_its_request() {
        let cancel = CancellationToken::new();
        let (_tx, rx) = mpsc::channel::<Value>(1);
        let stream = SseStream::new(
            None,
            Some(rx),
            CancellationToken::new(),
            Some(cancel.clone().drop_guard()),
        );
        drop(stream);
        assert!(cancel.is_cancelled());
    }
}
