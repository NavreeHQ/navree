//! A Chrome DevTools Protocol connection: JSON commands and events over
//! the browser's WebSocket, with flattened page sessions.
//!
//! The connection runs on the background runtime ([`crate::runtime`]).
//! Commands are sent from any runtime through a channel; each page's
//! events are folded into its [`PageLog`] as they arrive.

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

use crate::{BrowserError, runtime};

/// The most lines a page's console log keeps.
const MAX_LINES: usize = 5_000;
/// The most network requests a page's log keeps.
const MAX_REQUESTS: usize = 2_000;

/// An error answered by the browser, or a closed connection (`code` 0).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CdpError {
    pub(crate) code: i64,
    pub(crate) message: String,
}

impl CdpError {
    fn closed() -> CdpError {
        CdpError {
            code: 0,
            message: "the connection to the browser is closed".to_owned(),
        }
    }

    /// The page navigated or reloaded while a script ran: try again.
    pub(crate) fn is_context_lost(&self) -> bool {
        [
            "Execution context was destroyed",
            "Cannot find context with specified id",
            "Cannot find default execution context",
            "Inspected target navigated or closed",
            "uniqueContextId not found",
            "Execution context with given id not found",
            "Not attached to an active page",
        ]
        .iter()
        .any(|needle| self.message.contains(needle))
    }

    /// The page is switching documents and cannot answer this moment.
    pub(crate) fn is_between_documents(&self) -> bool {
        self.message.contains("Not attached to an active page")
    }

    /// The page's session is gone (the page or its browser closed).
    pub(crate) fn is_session_gone(&self) -> bool {
        self.code == 0
            || [
                "Session with given id not found",
                "No session with given id",
                "Target closed",
                "No target with given id",
            ]
            .iter()
            .any(|needle| self.message.contains(needle))
    }
}

impl fmt::Display for CdpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

/// A command for the connection task.
struct Outgoing {
    session: Option<String>,
    method: String,
    params: Value,
    reply: Option<oneshot::Sender<Result<Value, CdpError>>>,
}

type Logs = Arc<Mutex<HashMap<String, Arc<Mutex<PageLog>>>>>;

/// A DevTools connection to one browser process.
pub(crate) struct Connection {
    tx: mpsc::UnboundedSender<Outgoing>,
    logs: Logs,
    closed: Arc<AtomicBool>,
}

impl Connection {
    /// Connects to the browser's DevTools WebSocket `url`.
    pub(crate) async fn connect(url: String) -> Result<Connection, BrowserError> {
        let (tx, rx) = mpsc::unbounded_channel();
        let logs: Logs = Arc::default();
        let closed = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = oneshot::channel();
        runtime::handle().spawn(run(url, rx, logs.clone(), closed.clone(), ready_tx));
        match ready_rx.await {
            Ok(Ok(())) => Ok(Connection { tx, logs, closed }),
            Ok(Err(message)) => Err(BrowserError::Connection(message)),
            Err(_) => Err(BrowserError::Connection(
                "the connection task stopped".to_owned(),
            )),
        }
    }

    /// Sends `method` with `params` (to the page `session`, or the browser)
    /// and waits for the answer.
    pub(crate) async fn call(
        &self,
        session: Option<&str>,
        method: &str,
        params: Value,
    ) -> Result<Value, CdpError> {
        let (reply, answer) = oneshot::channel();
        self.tx
            .send(Outgoing {
                session: session.map(str::to_owned),
                method: method.to_owned(),
                params,
                reply: Some(reply),
            })
            .map_err(|_| CdpError::closed())?;
        answer.await.unwrap_or_else(|_| Err(CdpError::closed()))
    }

    /// Sends `method` without waiting for the answer.
    pub(crate) fn fire(&self, session: Option<&str>, method: &str, params: Value) {
        let _ = self.tx.send(Outgoing {
            session: session.map(str::to_owned),
            method: method.to_owned(),
            params,
            reply: None,
        });
    }

    /// The log of the page `session` (created on first use).
    pub(crate) fn log(&self, session: &str) -> Arc<Mutex<PageLog>> {
        log_of(&self.logs, session)
    }

    /// Stops keeping the log of `session`.
    pub(crate) fn forget(&self, session: &str) {
        lock(&self.logs).remove(session);
    }

    /// Whether the connection has closed.
    pub(crate) fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

fn log_of(logs: &Logs, session: &str) -> Arc<Mutex<PageLog>> {
    lock(logs).entry(session.to_owned()).or_default().clone()
}

/// The connection task: sends commands, routes answers and events.
async fn run(
    url: String,
    mut rx: mpsc::UnboundedReceiver<Outgoing>,
    logs: Logs,
    closed: Arc<AtomicBool>,
    ready: oneshot::Sender<Result<(), String>>,
) {
    let config = WebSocketConfig::default()
        .max_message_size(Some(1 << 30))
        .max_frame_size(Some(1 << 30));
    let ws = match tokio_tungstenite::connect_async_with_config(&url, Some(config), false).await {
        Ok((ws, _)) => ws,
        Err(error) => {
            closed.store(true, Ordering::SeqCst);
            let _ = ready.send(Err(format!("cannot connect to {url}: {error}")));
            return;
        }
    };
    let _ = ready.send(Ok(()));
    let (mut sink, mut stream) = ws.split();
    let mut pending: HashMap<u64, oneshot::Sender<Result<Value, CdpError>>> = HashMap::new();
    let mut next_id: u64 = 1;
    loop {
        let mut outgoing = Vec::new();
        tokio::select! {
            command = rx.recv() => match command {
                Some(command) => outgoing.push(command),
                None => break,
            },
            message = stream.next() => match message {
                Some(Ok(Message::Text(text))) => {
                    outgoing = receive(text.as_str(), &mut pending, &logs);
                }
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                Some(Ok(_)) => {}
            },
        }
        let mut failed = false;
        for command in outgoing {
            let id = next_id;
            next_id += 1;
            let mut message =
                json!({ "id": id, "method": command.method, "params": command.params });
            if let Some(session) = command.session {
                message["sessionId"] = Value::String(session);
            }
            if let Some(reply) = command.reply {
                pending.insert(id, reply);
            }
            if sink.send(Message::text(message.to_string())).await.is_err() {
                failed = true;
                break;
            }
        }
        if failed {
            break;
        }
    }
    closed.store(true, Ordering::SeqCst);
    for (_, reply) in pending.drain() {
        let _ = reply.send(Err(CdpError::closed()));
    }
    for log in lock(&logs).values() {
        let mut log = lock(log);
        log.gone
            .get_or_insert_with(|| "the browser closed".to_owned());
    }
    let _ = sink.close().await;
}

/// Handles one message from the browser; returns commands to send in
/// response (accepting dialogs).
fn receive(
    text: &str,
    pending: &mut HashMap<u64, oneshot::Sender<Result<Value, CdpError>>>,
    logs: &Logs,
) -> Vec<Outgoing> {
    let Ok(mut message) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    if let Some(id) = message["id"].as_u64() {
        if let Some(reply) = pending.remove(&id) {
            let result = match message.get_mut("error") {
                Some(error) => {
                    let mut text = error["message"].as_str().unwrap_or("error").to_owned();
                    if let Some(data) = error["data"].as_str() {
                        text.push_str(": ");
                        text.push_str(data);
                    }
                    Err(CdpError {
                        code: error["code"].as_i64().unwrap_or(-1),
                        message: text,
                    })
                }
                None => Ok(message
                    .get_mut("result")
                    .map(Value::take)
                    .unwrap_or(Value::Null)),
            };
            let _ = reply.send(result);
        }
        return Vec::new();
    }
    let Some(method) = message["method"].as_str() else {
        return Vec::new();
    };
    let params = &message["params"];
    match message["sessionId"].as_str() {
        Some(session) => {
            let log = log_of(logs, session);
            let mut log = lock(&log);
            match log.event(method, params) {
                Some((method, params)) => vec![Outgoing {
                    session: Some(session.to_owned()),
                    method: method.to_owned(),
                    params,
                    reply: None,
                }],
                None => Vec::new(),
            }
        }
        None => {
            if method == "Target.detachedFromTarget"
                && let Some(session) = params["sessionId"].as_str()
                && let Some(log) = lock(logs).get(session)
            {
                lock(log)
                    .gone
                    .get_or_insert_with(|| "the page was closed".to_owned());
            }
            Vec::new()
        }
    }
}

/// A network request the page made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Request {
    pub(crate) id: String,
    pub(crate) method: String,
    pub(crate) url: String,
    pub(crate) status: Option<u16>,
    pub(crate) failure: Option<String>,
    /// The response arrived (or the request failed).
    pub(crate) done: bool,
    /// A `wait_for_request` already returned it.
    pub(crate) seen: bool,
}

/// What a page reported: console lines, errors, requests, navigation.
#[derive(Debug, Default)]
pub(crate) struct PageLog {
    /// Console messages, exceptions, dialogs and failed requests, in order.
    pub(crate) lines: Vec<String>,
    /// `console.error` calls and uncaught exceptions.
    pub(crate) errors: Vec<String>,
    pub(crate) requests: Vec<Request>,
    /// The main frame's URL.
    pub(crate) url: String,
    /// The main frame's id (the page target's id).
    pub(crate) main_frame: Option<String>,
    /// The main frame's current document.
    pub(crate) loader: Option<String>,
    /// Documents whose `load` event fired (most recent last).
    pub(crate) loaded: Vec<String>,
    /// Why the page can no longer be used.
    pub(crate) gone: Option<String>,
    /// A native drag the browser intercepted (`Input.dragIntercepted`).
    pub(crate) drag: Option<Value>,
}

impl PageLog {
    fn line(&mut self, line: String) {
        if self.lines.len() >= MAX_LINES {
            self.lines.drain(..MAX_LINES / 10);
        }
        self.lines.push(line);
    }

    fn is_main(&self, frame: &Value) -> bool {
        match (&self.main_frame, frame.as_str()) {
            (Some(main), Some(frame)) => main == frame,
            _ => true,
        }
    }

    /// Folds one page event in; returns a command to answer it with.
    pub(crate) fn event(&mut self, method: &str, p: &Value) -> Option<(&'static str, Value)> {
        match method {
            "Runtime.consoleAPICalled" => {
                let kind = p["type"].as_str().unwrap_or("log");
                let text = p["args"]
                    .as_array()
                    .map(|args| args.iter().map(remote_text).collect::<Vec<_>>().join(" "))
                    .unwrap_or_default();
                self.line(format!("[{kind}] {text}"));
                if matches!(kind, "error" | "assert") {
                    self.errors.push(format!("console.{kind}: {text}"));
                }
            }
            "Runtime.exceptionThrown" => {
                let details = &p["exceptionDetails"];
                let text = details["exception"]["description"]
                    .as_str()
                    .or_else(|| details["text"].as_str())
                    .unwrap_or("an exception")
                    .to_owned();
                self.line(format!("[exception] {text}"));
                self.errors.push(format!("uncaught {text}"));
            }
            "Log.entryAdded" => {
                let entry = &p["entry"];
                // Failed requests are logged from the Network events.
                if entry["source"] != "network" {
                    self.line(format!(
                        "[{} {}] {}",
                        entry["source"].as_str().unwrap_or("log"),
                        entry["level"].as_str().unwrap_or("info"),
                        entry["text"].as_str().unwrap_or_default()
                    ));
                }
            }
            "Network.requestWillBeSent" => {
                let id = p["requestId"].as_str().unwrap_or_default().to_owned();
                let url = p["request"]["url"].as_str().unwrap_or_default().to_owned();
                let method = p["request"]["method"].as_str().unwrap_or("GET").to_owned();
                if let Some(existing) = self.requests.iter_mut().find(|r| r.id == id) {
                    // A redirect keeps the request id.
                    existing.url = url;
                    existing.method = method;
                    existing.status = None;
                } else {
                    if self.requests.len() >= MAX_REQUESTS {
                        self.requests.drain(..MAX_REQUESTS / 10);
                    }
                    self.requests.push(Request {
                        id,
                        method,
                        url,
                        status: None,
                        failure: None,
                        done: false,
                        seen: false,
                    });
                }
            }
            "Network.responseReceived" => {
                if let Some(request) = self.request(&p["requestId"]) {
                    request.status = p["response"]["status"]
                        .as_u64()
                        .and_then(|s| u16::try_from(s).ok());
                }
            }
            "Network.loadingFinished" => {
                if let Some(request) = self.request(&p["requestId"]) {
                    request.done = true;
                    if let Some(status) = request.status.filter(|s| *s >= 400) {
                        let line =
                            format!("[network] {} {} -> {status}", request.method, request.url);
                        self.line(line);
                    }
                }
            }
            "Network.loadingFailed" => {
                if let Some(request) = self.request(&p["requestId"]) {
                    let error = p["errorText"].as_str().unwrap_or("failed").to_owned();
                    request.done = true;
                    request.failure = Some(error.clone());
                    let line = format!(
                        "[network] {} {} failed: {error}",
                        request.method, request.url
                    );
                    self.line(line);
                }
            }
            "Page.frameNavigated" => {
                let frame = &p["frame"];
                if frame.get("parentId").is_none() {
                    self.main_frame = frame["id"].as_str().map(str::to_owned);
                    self.url = frame["url"].as_str().unwrap_or_default().to_owned();
                    self.loader = frame["loaderId"].as_str().map(str::to_owned);
                }
            }
            "Page.navigatedWithinDocument" => {
                if self.is_main(&p["frameId"]) {
                    self.url = p["url"].as_str().unwrap_or_default().to_owned();
                }
            }
            "Page.lifecycleEvent" => {
                if p["name"] == "load"
                    && self.is_main(&p["frameId"])
                    && let Some(loader) = p["loaderId"].as_str()
                {
                    if self.loaded.len() >= 50 {
                        self.loaded.remove(0);
                    }
                    self.loaded.push(loader.to_owned());
                }
            }
            "Page.javascriptDialogOpening" => {
                let kind = p["type"].as_str().unwrap_or("alert");
                let text = p["message"].as_str().unwrap_or_default();
                self.line(format!("[dialog] {kind}({text:?}) accepted"));
                let mut answer = json!({ "accept": true });
                if kind == "prompt" {
                    answer["promptText"] = p["defaultPrompt"].clone();
                }
                return Some(("Page.handleJavaScriptDialog", answer));
            }
            "Input.dragIntercepted" => self.drag = Some(p["data"].clone()),
            "Inspector.targetCrashed" => self.gone = Some("the page crashed".to_owned()),
            "Inspector.detached" => {
                self.gone
                    .get_or_insert_with(|| "the page was closed".to_owned());
            }
            _ => {}
        }
        None
    }

    fn request(&mut self, id: &Value) -> Option<&mut Request> {
        let id = id.as_str()?;
        self.requests.iter_mut().rev().find(|r| r.id == id)
    }
}

/// A console argument as text.
fn remote_text(object: &Value) -> String {
    match object.get("value") {
        Some(Value::String(text)) => text.clone(),
        Some(value) if !value.is_null() => value.to_string(),
        _ => object["description"]
            .as_str()
            .or_else(|| object["unserializableValue"].as_str())
            .or_else(|| object["type"].as_str())
            .unwrap_or_default()
            .to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn console_errors_and_exceptions_are_counted_other_messages_only_logged() {
        let mut log = PageLog::default();
        log.event(
            "Runtime.consoleAPICalled",
            &json!({ "type": "log", "args": [{ "type": "string", "value": "hello" }, { "type": "number", "value": 3 }] }),
        );
        log.event(
            "Runtime.consoleAPICalled",
            &json!({ "type": "error", "args": [{ "type": "object", "description": "Error: boom" }] }),
        );
        log.event(
            "Runtime.exceptionThrown",
            &json!({ "exceptionDetails": { "text": "Uncaught", "exception": { "description": "TypeError: x is undefined" } } }),
        );
        log.event(
            "Log.entryAdded",
            &json!({ "entry": { "source": "network", "level": "error", "text": "Failed to load resource" } }),
        );
        log.event(
            "Log.entryAdded",
            &json!({ "entry": { "source": "security", "level": "error", "text": "CSP" } }),
        );
        assert_eq!(
            log.lines,
            [
                "[log] hello 3",
                "[error] Error: boom",
                "[exception] TypeError: x is undefined",
                "[security error] CSP",
            ]
        );
        assert_eq!(
            log.errors,
            [
                "console.error: Error: boom",
                "uncaught TypeError: x is undefined"
            ]
        );
    }

    #[test]
    fn requests_are_tracked_through_redirects_responses_and_failures() {
        let mut log = PageLog::default();
        log.event(
            "Network.requestWillBeSent",
            &json!({ "requestId": "1", "request": { "url": "http://x/a", "method": "POST" } }),
        );
        log.event(
            "Network.requestWillBeSent",
            &json!({ "requestId": "1", "request": { "url": "http://x/b", "method": "GET" } }),
        );
        log.event(
            "Network.responseReceived",
            &json!({ "requestId": "1", "response": { "status": 404 } }),
        );
        log.event("Network.loadingFinished", &json!({ "requestId": "1" }));
        log.event(
            "Network.requestWillBeSent",
            &json!({ "requestId": "2", "request": { "url": "http://x/c", "method": "GET" } }),
        );
        log.event(
            "Network.loadingFailed",
            &json!({ "requestId": "2", "errorText": "net::ERR_CONNECTION_REFUSED" }),
        );
        assert_eq!(log.requests.len(), 2);
        assert_eq!(log.requests[0].url, "http://x/b");
        assert_eq!(log.requests[0].status, Some(404));
        assert!(log.requests[0].done);
        assert_eq!(
            log.requests[1].failure.as_deref(),
            Some("net::ERR_CONNECTION_REFUSED")
        );
        assert_eq!(
            log.lines,
            [
                "[network] GET http://x/b -> 404",
                "[network] GET http://x/c failed: net::ERR_CONNECTION_REFUSED"
            ]
        );
        assert!(
            log.errors.is_empty(),
            "failed requests are not console errors"
        );
    }

    #[test]
    fn the_main_frame_url_and_loads_are_followed() {
        let mut log = PageLog {
            main_frame: Some("F".into()),
            ..PageLog::default()
        };
        log.event(
            "Page.frameNavigated",
            &json!({ "frame": { "id": "F", "url": "http://x/login", "loaderId": "L1" } }),
        );
        log.event(
            "Page.frameNavigated",
            &json!({ "frame": { "id": "C", "parentId": "F", "url": "http://ads/", "loaderId": "L9" } }),
        );
        log.event(
            "Page.lifecycleEvent",
            &json!({ "frameId": "C", "loaderId": "L9", "name": "load" }),
        );
        log.event(
            "Page.lifecycleEvent",
            &json!({ "frameId": "F", "loaderId": "L1", "name": "DOMContentLoaded" }),
        );
        log.event(
            "Page.lifecycleEvent",
            &json!({ "frameId": "F", "loaderId": "L1", "name": "load" }),
        );
        log.event(
            "Page.navigatedWithinDocument",
            &json!({ "frameId": "F", "url": "http://x/deals" }),
        );
        assert_eq!(log.url, "http://x/deals");
        assert_eq!(log.loader.as_deref(), Some("L1"));
        assert_eq!(log.loaded, ["L1"]);
    }

    #[test]
    fn dialogs_are_accepted_and_logged() {
        let mut log = PageLog::default();
        let answer = log.event(
            "Page.javascriptDialogOpening",
            &json!({ "type": "confirm", "message": "Delete the deal?" }),
        );
        assert_eq!(
            answer,
            Some(("Page.handleJavaScriptDialog", json!({ "accept": true })))
        );
        let answer = log.event(
            "Page.javascriptDialogOpening",
            &json!({ "type": "prompt", "message": "Name?", "defaultPrompt": "Anu" }),
        );
        assert_eq!(answer.unwrap().1["promptText"], "Anu");
        assert_eq!(
            log.lines[0],
            "[dialog] confirm(\"Delete the deal?\") accepted"
        );
    }

    #[test]
    fn a_page_between_documents_is_retried_not_failed() {
        let error = CdpError {
            code: -32000,
            message: "Not attached to an active page".to_owned(),
        };
        assert!(error.is_between_documents());
        assert!(error.is_context_lost());
        assert!(!error.is_session_gone());
    }

    #[test]
    fn crashes_and_detaches_make_the_page_gone() {
        let mut log = PageLog::default();
        log.event("Inspector.targetCrashed", &json!({}));
        assert_eq!(log.gone.as_deref(), Some("the page crashed"));
    }

    #[test]
    fn answers_reach_their_callers_and_errors_carry_the_message() {
        let logs: Logs = Arc::default();
        let mut pending = HashMap::new();
        let (tx, mut rx) = oneshot::channel();
        pending.insert(7, tx);
        receive(
            r#"{"id":7,"error":{"code":-32000,"message":"Cannot find context with specified id"}}"#,
            &mut pending,
            &logs,
        );
        let error = rx.try_recv().unwrap().unwrap_err();
        assert!(error.is_context_lost());
        assert!(!error.is_session_gone());
        let (tx, mut rx) = oneshot::channel();
        pending.insert(8, tx);
        receive(r#"{"id":8,"result":{"value":1}}"#, &mut pending, &logs);
        assert_eq!(rx.try_recv().unwrap().unwrap(), json!({ "value": 1 }));

        let answers = receive(
            r#"{"method":"Page.javascriptDialogOpening","sessionId":"S","params":{"type":"alert","message":"hi"}}"#,
            &mut pending,
            &logs,
        );
        assert_eq!(answers.len(), 1);
        assert_eq!(answers[0].session.as_deref(), Some("S"));
        assert_eq!(lock(&log_of(&logs, "S")).lines.len(), 1);

        receive(
            r#"{"method":"Target.detachedFromTarget","params":{"sessionId":"S"}}"#,
            &mut pending,
            &logs,
        );
        assert_eq!(
            lock(&log_of(&logs, "S")).gone.as_deref(),
            Some("the page was closed")
        );
        assert!(CdpError::closed().is_session_gone());
    }
}
