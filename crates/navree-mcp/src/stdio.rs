//! The stdio transport: `mcp:serve --user <email>`. JSON-RPC messages, one
//! per line, on the process's stdin and stdout.

use std::sync::Arc;
use std::time::Duration;

use futures::future::join_all;
use navree_auth::AuthService;
use navree_contracts::{AuthVia, Principal};
use navree_kernel::{AppContext, BoxError, CommandSpec};
use serde_json::{Map, Value, json};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::context::McpClient;
use crate::handler::{Caller, McpHandler, Scope, Shared};
use crate::notifier::McpNotifier;
use crate::options::McpSettings;
use crate::protocol::jsonrpc::{self, Message, Notification, Parsed, Request, RpcError};
use crate::protocol::lifecycle::{self, Prepared};
use crate::protocol::outlet::Outlet;
use crate::protocol::{allows_batches, negotiate};
use crate::registry::McpRegistry;
use crate::session::{Requests, Session};

/// How long requests still running get to finish once the client is gone.
const DRAIN: Duration = Duration::from_secs(5);

/// `mcp:serve --user <email or id>`: the MCP server on stdin and stdout.
pub(crate) fn serve_command() -> CommandSpec {
    CommandSpec::new(
        "mcp:serve",
        "Serve the MCP server on stdin/stdout as a user: mcp:serve --user <email or id>",
        |ctx, args| async move {
            let user = user_argument(&args)?;
            serve_stdio(&ctx, &user, tokio::io::stdin(), tokio::io::stdout()).await
        },
    )
    .stdio()
}

/// The value of `--user` (`--user anu@example.com` or `--user=...`).
fn user_argument(args: &[String]) -> Result<String, BoxError> {
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix("--user=") {
            return Ok(value.to_owned());
        }
        if arg == "--user"
            && let Some(value) = args.next()
        {
            return Ok(value.clone());
        }
    }
    Err("usage: mcp:serve --user <email or user id>".into())
}

/// Serves the app's MCP server on `reader` and `writer` (JSON-RPC messages,
/// one per line) as `user`, an email address or a user id, until the
/// client closes the stream or the app shuts down: what `mcp:serve` runs
/// on stdin and stdout. The process acts as that user, with their roles
/// and permissions.
pub async fn serve_stdio<R, W>(
    ctx: &AppContext,
    user: &str,
    reader: R,
    writer: W,
) -> Result<(), BoxError>
where
    R: AsyncRead + Send + Unpin + 'static,
    W: AsyncWrite + Send + Unpin + 'static,
{
    let principal = Principal {
        user_id: find_user(ctx, user).await?,
        via: AuthVia::Token,
        token_abilities: None,
        session_id: None,
    };
    let settings = ctx.get::<McpSettings>().await?;
    let shared = Shared::new(
        ctx,
        ctx.get::<McpRegistry>().await?,
        &settings,
        ctx.get::<McpNotifier>().await?,
    )
    .await?;
    let connection = Connection {
        handler: McpHandler::new(Arc::new(shared)),
        principal,
        session_id: format!("stdio-{}", Uuid::now_v7()),
        shutdown: ctx.shutdown_token(),
        requests: Arc::default(),
        state: State::Pending,
    };
    connection
        .serve(reader, writer, settings.max_request_bytes)
        .await
}

/// Where the connection's lifecycle stands.
enum State {
    /// Nothing but `ping` and discovery yet.
    Pending,
    /// After `initialize`: a session at the agreed version.
    Session(Arc<Session>),
    /// The stateless protocol: every request carries its own `_meta`.
    Inline,
}

/// One stdio connection.
struct Connection {
    handler: McpHandler,
    principal: Principal,
    session_id: String,
    shutdown: CancellationToken,
    requests: Arc<Requests>,
    state: State,
}

/// One line read from the client.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Line {
    Message(Vec<u8>),
    /// Longer than the limit (skipped up to its end).
    TooLong,
    End,
}

/// Reads one line of at most `max` bytes; a longer one is skipped without
/// being kept in memory.
pub(crate) async fn read_line<R: AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
    max: usize,
) -> std::io::Result<Line> {
    let mut line = Vec::new();
    let mut too_long = false;
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return Ok(if too_long {
                Line::TooLong
            } else if line.is_empty() {
                Line::End
            } else {
                Line::Message(line)
            });
        }
        let (chunk, found) = match available.iter().position(|byte| *byte == b'\n') {
            Some(end) => (&available[..end], Some(end + 1)),
            None => (available, None),
        };
        if !too_long {
            if line.len() + chunk.len() > max {
                too_long = true;
                line = Vec::new();
            } else {
                line.extend_from_slice(chunk);
            }
        }
        let used = found.unwrap_or(available.len());
        reader.consume(used);
        if found.is_some() {
            return Ok(if too_long {
                Line::TooLong
            } else {
                Line::Message(line)
            });
        }
    }
}

impl Connection {
    async fn serve<R, W>(mut self, reader: R, writer: W, max_line: usize) -> Result<(), BoxError>
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
    {
        let (outlet, mut queue) = Outlet::channel();
        let mut writing = tokio::spawn(async move {
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
        let mut tasks = JoinSet::new();
        loop {
            let line = tokio::select! {
                line = read_line(&mut reader, max_line) => line,
                () = self.shutdown.cancelled() => break,
            };
            match line {
                Ok(Line::End) | Err(_) => break,
                Ok(Line::TooLong) => {
                    let error = RpcError::invalid_request(format!(
                        "Invalid Request: a message is longer than {max_line} bytes"
                    ));
                    outlet.deliver(jsonrpc::error(&Value::Null, &error)).await;
                }
                Ok(Line::Message(bytes)) => {
                    if bytes.iter().all(u8::is_ascii_whitespace) {
                        continue;
                    }
                    self.receive(&bytes, &outlet, &mut tasks).await;
                }
            }
            // Finished requests leave the set as they go.
            while tasks.try_join_next().is_some() {}
        }
        // The client is gone (or the app is stopping): stop what still runs.
        self.requests.cancel_all();
        if let State::Session(session) = &self.state {
            session.close();
            self.handler.notifier().close_session(&session.id);
        }
        if tokio::time::timeout(DRAIN, async { while tasks.join_next().await.is_some() {} })
            .await
            .is_err()
        {
            tasks.abort_all();
        }
        drop(outlet);
        self.state = State::Pending;
        if tokio::time::timeout(DRAIN, &mut writing).await.is_err() {
            writing.abort();
        }
        Ok(())
    }

    /// Handles one line: a message or a batch.
    async fn receive(&mut self, bytes: &[u8], outlet: &Outlet, tasks: &mut JoinSet<()>) {
        match jsonrpc::parse(bytes) {
            Err((id, error)) => {
                outlet.deliver(jsonrpc::error(&id, &error)).await;
            }
            Ok(Parsed::Single(message)) => self.message(message, outlet, tasks).await,
            Ok(Parsed::Batch(entries)) => self.batch(entries, outlet, tasks).await,
        }
    }

    async fn message(&mut self, message: Message, outlet: &Outlet, tasks: &mut JoinSet<()>) {
        match message {
            Message::Request(request) => {
                if let Some(answer) = self.lifecycle(&request, outlet) {
                    outlet.deliver(answer).await;
                    return;
                }
                match self.prepare(&request) {
                    Ok(prepared) => {
                        let (scope, guard) = self.scope(&request, outlet.clone());
                        let handler = self.handler.clone();
                        let outlet = outlet.clone();
                        tasks.spawn(async move {
                            let _guard = guard;
                            if let Some(answer) = handler.answer(&scope, &prepared, request).await {
                                outlet.deliver(answer).await;
                            }
                        });
                    }
                    Err(error) => {
                        outlet.deliver(jsonrpc::error(&request.id, &error)).await;
                    }
                }
            }
            Message::Notification(notification) => self.notification(&notification),
            Message::Reply => {}
        }
    }

    /// A batch: allowed only on a `2025-03-26` session. Its answers go out
    /// together, as one array.
    async fn batch(
        &mut self,
        entries: Vec<Result<Message, (Value, RpcError)>>,
        outlet: &Outlet,
        tasks: &mut JoinSet<()>,
    ) {
        let version = match &self.state {
            State::Session(session) if allows_batches(&session.version) => session.version.clone(),
            State::Session(session) => return refuse_batch(outlet, &session.version).await,
            State::Pending | State::Inline => {
                return refuse_batch(outlet, "a connection without a session").await;
            }
        };
        let mut answers = Vec::new();
        let mut pending = Vec::new();
        for entry in entries {
            match entry {
                Err((id, error)) => answers.push(jsonrpc::error(&id, &error)),
                Ok(Message::Notification(notification)) => self.notification(&notification),
                Ok(Message::Reply) => {}
                Ok(Message::Request(request)) if request.method == "initialize" => {
                    answers.push(jsonrpc::error(
                        &request.id,
                        &RpcError::invalid_request("Invalid Request: initialize cannot be batched"),
                    ));
                }
                Ok(Message::Request(request)) => {
                    match lifecycle::prepare(&request, Some(&version), false) {
                        Err(error) => answers.push(jsonrpc::error(&request.id, &error)),
                        Ok(prepared) => {
                            // Each request's progress goes to its own,
                            // discarded stream: a batch answers at once.
                            let (own, mut discarded) = Outlet::channel();
                            let (scope, guard) = self.scope(&request, own);
                            let handler = self.handler.clone();
                            pending.push(async move {
                                let _guard = guard;
                                let answering = handler.answer(&scope, &prepared, request);
                                tokio::pin!(answering);
                                loop {
                                    tokio::select! {
                                        answer = &mut answering => break answer,
                                        message = discarded.recv() => {
                                            if message.is_none() {
                                                break answering.await;
                                            }
                                        }
                                    }
                                }
                            });
                        }
                    }
                }
            }
        }
        let outlet = outlet.clone();
        tasks.spawn(async move {
            answers.extend(join_all(pending).await.into_iter().flatten());
            if !answers.is_empty() {
                outlet.deliver(Value::Array(answers)).await;
            }
        });
    }

    /// Answers the requests the connection's lifecycle handles itself
    /// (`initialize`, and anything before the lifecycle is chosen); `None`
    /// for requests the handler runs.
    fn lifecycle(&mut self, request: &Request, outlet: &Outlet) -> Option<Value> {
        match (&self.state, request.method.as_str()) {
            (State::Pending, "initialize") => Some(self.initialize(request, outlet)),
            (_, "initialize") => Some(jsonrpc::error(
                &request.id,
                &RpcError::invalid_request("Invalid Request: the session is already initialized"),
            )),
            (State::Pending, "ping") => Some(jsonrpc::result(&request.id, json!({}))),
            (State::Pending, method) => {
                let missing = lifecycle::missing_meta(&request.params);
                if !missing.is_empty() {
                    return Some(jsonrpc::error(
                        &request.id,
                        &lifecycle::missing_meta_error(&missing),
                    ));
                }
                // Discovery answers without choosing a lifecycle; any other
                // complete request chooses the stateless one.
                if method != "server/discover" {
                    self.state = State::Inline;
                }
                None
            }
            _ => None,
        }
    }

    /// `initialize`: agrees on a version and opens the session, whose
    /// notifications go to the connection's writer.
    fn initialize(&mut self, request: &Request, outlet: &Outlet) -> Value {
        let Some(requested) = request
            .params
            .get("protocolVersion")
            .and_then(Value::as_str)
        else {
            return jsonrpc::error(
                &request.id,
                &RpcError::invalid_params("Invalid params: protocolVersion must be a string"),
            );
        };
        let version = negotiate(requested);
        let client = lifecycle::client_info(request.params.get("clientInfo"))
            .map(|(name, version)| McpClient { name, version });
        let session = Session::new(self.session_id.clone(), None, version.to_owned(), client);
        session.set_stream(outlet.clone());
        let result = self.handler.info().initialize_result(version);
        self.state = State::Session(session);
        jsonrpc::result(&request.id, result)
    }

    /// The protocol version and lifecycle a request runs at.
    fn prepare(&self, request: &Request) -> Result<Prepared, RpcError> {
        match &self.state {
            State::Session(session) => lifecycle::prepare(request, Some(&session.version), false),
            State::Pending | State::Inline => lifecycle::prepare(request, None, true),
        }
    }

    /// What a request runs with; it is tracked (for cancellation) until the
    /// guard drops.
    fn scope(&self, request: &Request, outlet: Outlet) -> (Scope, crate::session::InFlight) {
        let cancel = self.shutdown.child_token();
        let guard = self.requests.track(&request.id, cancel.clone());
        let session = match &self.state {
            State::Session(session) => Some(session.clone()),
            State::Pending | State::Inline => None,
        };
        let client = session
            .as_ref()
            .and_then(|session| session.client.clone())
            .or_else(|| {
                lifecycle::meta_client(&request.params)
                    .map(|(name, version)| McpClient { name, version })
            });
        let scope = Scope {
            caller: Caller {
                principal: self.principal.clone(),
                request_id: Uuid::now_v7().to_string(),
                session_id: Some(self.session_id.clone()),
            },
            session,
            client,
            outlet,
            cancel,
            shutdown: self.shutdown.clone(),
        };
        (scope, guard)
    }

    fn notification(&self, notification: &Notification) {
        match notification.method.as_str() {
            "notifications/initialized" => {
                if let State::Session(session) = &self.state {
                    self.handler.register(session);
                }
            }
            "notifications/cancelled" => {
                if let Some(id) = cancelled_request(&notification.params) {
                    self.requests.cancel(id);
                }
            }
            _ => {}
        }
    }
}

/// The request a `notifications/cancelled` names.
pub(crate) fn cancelled_request(params: &Map<String, Value>) -> Option<&Value> {
    params
        .get("requestId")
        .filter(|id| id.is_string() || id.is_number())
}

/// The answer to a batch the protocol does not allow.
async fn refuse_batch(outlet: &Outlet, on: &str) {
    let error = RpcError::invalid_request(format!(
        "Invalid Request: JSON-RPC batches are not allowed on {on}"
    ));
    outlet.deliver(jsonrpc::error(&Value::Null, &error)).await;
}

/// The id of `user`: a user id as is (checked when the app has
/// `AuthModule`), or the account with that email address.
async fn find_user(ctx: &AppContext, user: &str) -> Result<Uuid, BoxError> {
    let auth = ctx.try_get::<AuthService>().await;
    if let Ok(id) = Uuid::parse_str(user) {
        if let Some(auth) = auth
            && auth.find_user(id).await?.is_none()
        {
            return Err(format!("no user with id {id}").into());
        }
        return Ok(id);
    }
    let auth =
        auth.ok_or("mcp:serve --user <email> needs AuthModule in the app (or pass a user id)")?;
    let found = auth
        .find_by_email(user)
        .await?
        .ok_or_else(|| format!("no user with email {user}"))?;
    Ok(found.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn the_user_comes_from_the_flag() {
        assert_eq!(
            user_argument(&args(&["--user", "anu@example.com"])).unwrap(),
            "anu@example.com"
        );
        assert_eq!(
            user_argument(&args(&["--user=anu@example.com"])).unwrap(),
            "anu@example.com"
        );
        assert!(user_argument(&args(&[])).is_err());
        assert!(user_argument(&args(&["--user"])).is_err());
    }

    #[test]
    fn the_command_owns_stdout() {
        let command = serve_command();
        assert_eq!(command.name, "mcp:serve");
        assert!(command.stdio);
    }

    #[tokio::test]
    async fn long_lines_are_skipped_without_being_kept() {
        let input = format!("{}\n{{\"a\":1}}\n", "x".repeat(100));
        let mut reader = BufReader::with_capacity(8, input.as_bytes());
        assert!(matches!(
            read_line(&mut reader, 16).await.unwrap(),
            Line::TooLong
        ));
        let Line::Message(line) = read_line(&mut reader, 16).await.unwrap() else {
            panic!("the next line");
        };
        assert_eq!(line, b"{\"a\":1}");
        assert!(matches!(
            read_line(&mut reader, 16).await.unwrap(),
            Line::End
        ));
    }
}
