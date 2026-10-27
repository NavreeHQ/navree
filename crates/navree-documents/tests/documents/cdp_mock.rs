//! A stand-in for Chrome's DevTools endpoint: it answers the commands the
//! Chrome engine sends, requests the document and its assets through the
//! `Fetch` domain like a browser would, and "prints" the document as a PDF
//! of its text. It records everything for assertions.

#![allow(dead_code)]

use std::collections::{BTreeSet, HashMap};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use futures_util::{SinkExt, StreamExt};
use navree_documents::Pdf;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

/// How the mock behaves.
#[derive(Clone, Debug, Default)]
pub struct Behavior {
    /// How long `Page.printToPDF` takes.
    pub print_delay: Duration,
    /// Drop the connection at the first `Page.printToPDF`, as a crashing
    /// browser would.
    pub crash_on_first_print: bool,
}

/// What the mock saw.
#[derive(Debug, Default)]
pub struct Seen {
    /// Every command: method, params, session.
    pub calls: Vec<(String, Value, Option<String>)>,
    /// The HTML of every document the engine served.
    pub documents: Vec<String>,
    /// Asset URLs fulfilled, with their status and body.
    pub fulfilled: Vec<(String, u16, Vec<u8>)>,
    /// Asset URLs the engine blocked.
    pub blocked: Vec<String>,
    /// Asset URLs the engine let through.
    pub continued: Vec<String>,
    /// WebSocket connections accepted.
    pub connections: usize,
    /// Browser contexts created and not disposed of.
    pub open_contexts: BTreeSet<String>,
    /// Pages printing now, and the most at once.
    pub printing: usize,
    pub max_printing: usize,
    /// `GET /json/version` requests.
    pub version_requests: usize,
    crashed: bool,
}

/// A running mock endpoint.
pub struct MockChrome {
    pub addr: SocketAddr,
    seen: Arc<Mutex<Seen>>,
    task: tokio::task::JoinHandle<()>,
}

impl MockChrome {
    pub async fn start(behavior: Behavior) -> MockChrome {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Seen::default()));
        let state = seen.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let state = state.clone();
                let behavior = behavior.clone();
                tokio::spawn(async move {
                    connection(stream, addr, state, behavior).await;
                });
            }
        });
        MockChrome { addr, seen, task }
    }

    /// The browser WebSocket URL.
    pub fn ws_url(&self) -> String {
        format!("ws://{}/devtools/browser/mock", self.addr)
    }

    /// The DevTools HTTP address.
    pub fn http_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn seen(&self) -> MutexGuard<'_, Seen> {
        self.seen.lock().unwrap()
    }

    /// The methods called, in order.
    pub fn methods(&self) -> Vec<String> {
        self.seen().calls.iter().map(|(m, _, _)| m.clone()).collect()
    }

    /// The params of every call of `method`.
    pub fn params_of(&self, method: &str) -> Vec<Value> {
        self.seen()
            .calls
            .iter()
            .filter(|(m, _, _)| m == method)
            .map(|(_, p, _)| p.clone())
            .collect()
    }

    /// Waits until every browser context is disposed of (up to 2 s).
    pub async fn wait_for_contexts_disposed(&self) {
        for _ in 0..200 {
            if self.seen().open_contexts.is_empty() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("contexts still open: {:?}", self.seen().open_contexts);
    }
}

impl Drop for MockChrome {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn connection(stream: TcpStream, addr: SocketAddr, seen: Arc<Mutex<Seen>>, behavior: Behavior) {
    let mut head = [0u8; 64];
    let Ok(n) = stream.peek(&mut head).await else {
        return;
    };
    if head[..n].starts_with(b"GET /json/version") {
        version(stream, addr, &seen).await;
        return;
    }
    let Ok(socket) = tokio_tungstenite::accept_async(stream).await else {
        return;
    };
    seen.lock().unwrap().connections += 1;
    let (mut sink, mut source) = socket.split();
    let (out, mut outgoing) = mpsc::unbounded_channel::<Option<Value>>();
    let writer = tokio::spawn(async move {
        while let Some(message) = outgoing.recv().await {
            match message {
                Some(value) => {
                    if sink.send(Message::text(value.to_string())).await.is_err() {
                        break;
                    }
                }
                None => {
                    let _ = sink.close().await;
                    break;
                }
            }
        }
    });
    let mut browser = Browser {
        out,
        seen: seen.clone(),
        behavior,
        next: 0,
        navigations: HashMap::new(),
        requests: HashMap::new(),
        contexts: Vec::new(),
    };
    while let Some(Ok(message)) = source.next().await {
        let Message::Text(text) = message else {
            continue;
        };
        let value: Value = serde_json::from_str(text.as_str()).unwrap();
        if !browser.handle(value).await {
            break;
        }
    }
    // Contexts made with `disposeOnDetach` go when their connection does.
    {
        let mut seen = seen.lock().unwrap();
        for context in &browser.contexts {
            seen.open_contexts.remove(context);
        }
    }
    drop(browser);
    let _ = writer.await;
}

async fn version(mut stream: TcpStream, addr: SocketAddr, seen: &Mutex<Seen>) {
    let mut buffer = [0u8; 1024];
    let _ = stream.read(&mut buffer).await;
    seen.lock().unwrap().version_requests += 1;
    // Chrome reports its own idea of its host.
    let body = json!({
        "Browser": "HeadlessChrome/0.0 (mock)",
        "webSocketDebuggerUrl": format!("ws://localhost:1/devtools/browser/mock-{}", addr.port()),
    })
    .to_string();
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
}

/// A navigation waiting for its document and assets.
struct Navigation {
    /// The id of the `Page.navigate` command to answer.
    command: u64,
    url: String,
    /// Asset requests not answered yet.
    waiting: usize,
}

struct Browser {
    out: mpsc::UnboundedSender<Option<Value>>,
    seen: Arc<Mutex<Seen>>,
    behavior: Behavior,
    next: u64,
    /// By session.
    navigations: HashMap<String, Navigation>,
    /// Paused request id → (session, url, is the document).
    requests: HashMap<String, (String, String, bool)>,
    /// Contexts this connection created.
    contexts: Vec<String>,
}

impl Browser {
    fn id(&mut self, prefix: &str) -> String {
        self.next += 1;
        format!("{prefix}-{}", self.next)
    }

    fn answer(&self, id: u64, session: Option<&str>, result: Value) {
        let mut message = json!({ "id": id, "result": result });
        if let Some(session) = session {
            message["sessionId"] = json!(session);
        }
        let _ = self.out.send(Some(message));
    }

    fn fail(&self, id: u64, session: Option<&str>, text: &str) {
        let mut message = json!({ "id": id, "error": { "code": -32000, "message": text } });
        if let Some(session) = session {
            message["sessionId"] = json!(session);
        }
        let _ = self.out.send(Some(message));
    }

    fn event(&self, session: &str, method: &str, params: Value) {
        let _ = self.out.send(Some(
            json!({ "method": method, "params": params, "sessionId": session }),
        ));
    }

    fn pause(&mut self, session: &str, url: &str, document: bool) {
        let request = self.id("request");
        self.requests
            .insert(request.clone(), (session.to_owned(), url.to_owned(), document));
        self.event(
            session,
            "Fetch.requestPaused",
            json!({ "requestId": request, "request": { "url": url, "method": "GET" } }),
        );
    }

    /// Handles one command; false ends the connection.
    async fn handle(&mut self, value: Value) -> bool {
        let id = value["id"].as_u64().unwrap();
        let method = value["method"].as_str().unwrap().to_owned();
        let params = value.get("params").cloned().unwrap_or(Value::Null);
        let session = value
            .get("sessionId")
            .and_then(Value::as_str)
            .map(str::to_owned);
        self.seen
            .lock()
            .unwrap()
            .calls
            .push((method.clone(), params.clone(), session.clone()));
        let on = session.as_deref();
        match method.as_str() {
            "Target.createBrowserContext" => {
                let context = self.id("context");
                self.seen.lock().unwrap().open_contexts.insert(context.clone());
                self.contexts.push(context.clone());
                self.answer(id, on, json!({ "browserContextId": context }));
            }
            "Target.disposeBrowserContext" => {
                let context = params["browserContextId"].as_str().unwrap_or_default();
                self.seen.lock().unwrap().open_contexts.remove(context);
                self.answer(id, on, json!({}));
            }
            "Target.createTarget" => {
                let target = self.id("target");
                self.answer(id, on, json!({ "targetId": target }));
            }
            "Target.attachToTarget" => {
                let session = self.id("session");
                self.answer(id, on, json!({ "sessionId": session }));
            }
            "Page.navigate" => {
                let session = session.clone().unwrap();
                let url = params["url"].as_str().unwrap().to_owned();
                self.navigations.insert(
                    session.clone(),
                    Navigation {
                        command: id,
                        url: url.clone(),
                        waiting: 0,
                    },
                );
                self.pause(&session, &url, true);
            }
            "Fetch.fulfillRequest" | "Fetch.failRequest" | "Fetch.continueRequest" => {
                self.answer(id, on, json!({}));
                let request = params["requestId"].as_str().unwrap();
                let Some((session, url, document)) = self.requests.remove(request) else {
                    return true;
                };
                let body = params
                    .get("body")
                    .and_then(Value::as_str)
                    .map(|b| BASE64.decode(b).unwrap())
                    .unwrap_or_default();
                {
                    let mut seen = self.seen.lock().unwrap();
                    match method.as_str() {
                        "Fetch.fulfillRequest" if document => {
                            seen.documents.push(String::from_utf8(body.clone()).unwrap());
                        }
                        "Fetch.fulfillRequest" => {
                            let status = params["responseCode"].as_u64().unwrap() as u16;
                            seen.fulfilled.push((url.clone(), status, body.clone()));
                        }
                        "Fetch.failRequest" => seen.blocked.push(url.clone()),
                        _ => seen.continued.push(url.clone()),
                    }
                }
                if document {
                    let html = String::from_utf8_lossy(&body).into_owned();
                    let assets = asset_urls(&html, &url);
                    self.navigations.get_mut(&session).unwrap().waiting = assets.len();
                    for asset in assets {
                        self.pause(&session, &asset, false);
                    }
                }
                let Some(navigation) = self.navigations.get_mut(&session) else {
                    return true;
                };
                if !document {
                    navigation.waiting -= 1;
                }
                let done = navigation.waiting == 0;
                if done && !self.requests.values().any(|r| r.0 == session) {
                    let navigation = self.navigations.remove(&session).unwrap();
                    self.answer(
                        navigation.command,
                        Some(&session),
                        json!({ "frameId": "frame", "loaderId": "loader" }),
                    );
                    self.event(&session, "Page.loadEventFired", json!({ "timestamp": 1.0 }));
                }
            }
            "Runtime.evaluate" => {
                self.answer(id, on, json!({ "result": { "type": "boolean", "value": true } }));
            }
            "Page.printToPDF" => {
                {
                    let mut seen = self.seen.lock().unwrap();
                    if self.behavior.crash_on_first_print && !seen.crashed {
                        seen.crashed = true;
                        drop(seen);
                        let _ = self.out.send(None);
                        return false;
                    }
                    seen.printing += 1;
                    seen.max_printing = seen.max_printing.max(seen.printing);
                }
                let html = self.seen.lock().unwrap().documents.last().cloned().unwrap_or_default();
                let delay = self.behavior.print_delay;
                let out = self.out.clone();
                let seen = self.seen.clone();
                let session = session.clone();
                // Print concurrently, like a browser with several pages.
                tokio::spawn(async move {
                    tokio::time::sleep(delay).await;
                    let pdf = Pdf::from_lines(html_text(&html));
                    let mut message =
                        json!({ "id": id, "result": { "data": BASE64.encode(pdf.bytes()) } });
                    if let Some(session) = session {
                        message["sessionId"] = json!(session);
                    }
                    seen.lock().unwrap().printing -= 1;
                    let _ = out.send(Some(message));
                });
            }
            "Browser.close" => {
                self.answer(id, on, json!({}));
                let _ = self.out.send(None);
                return false;
            }
            "Fetch.enable" | "Page.enable" | "Emulation.setLocaleOverride" => {
                self.answer(id, on, json!({}));
            }
            other => self.fail(id, on, &format!("'{other}' wasn't found")),
        }
        true
    }
}

/// The URLs of `src` and `href` attributes, resolved against `base`.
fn asset_urls(html: &str, base: &str) -> Vec<String> {
    let origin = {
        let scheme_end = base.find("://").map(|i| i + 3).unwrap_or(0);
        let path_start = base[scheme_end..]
            .find('/')
            .map(|i| i + scheme_end)
            .unwrap_or(base.len());
        &base[..path_start]
    };
    let directory = &base[..base.rfind('/').map(|i| i + 1).unwrap_or(base.len())];
    let mut urls = Vec::new();
    for attribute in ["src=\"", "href=\""] {
        let mut rest = html;
        while let Some(start) = rest.find(attribute) {
            rest = &rest[start + attribute.len()..];
            let Some(end) = rest.find('"') else {
                break;
            };
            let value = &rest[..end];
            rest = &rest[end..];
            let url = if value.starts_with("http://") || value.starts_with("https://") {
                value.to_owned()
            } else if value.starts_with("data:") || value.starts_with('#') {
                continue;
            } else if let Some(path) = value.strip_prefix('/') {
                format!("{origin}/{path}")
            } else {
                format!("{directory}{value}")
            };
            urls.push(url);
        }
    }
    urls
}

/// The visible text of `html`, one line per block, entities decoded.
pub fn html_text(html: &str) -> Vec<String> {
    let mut text = html.to_owned();
    for (open, close) in [("<head", "</head>"), ("<style", "</style>"), ("<script", "</script>")] {
        while let Some(start) = text.find(open) {
            let end = text[start..]
                .find(close)
                .map(|e| start + e + close.len())
                .unwrap_or(text.len());
            text.replace_range(start..end, "");
        }
    }
    let mut out = String::new();
    let mut in_tag = false;
    for c in text.chars() {
        match c {
            '<' => {
                in_tag = true;
                out.push('\n');
            }
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.lines()
        .map(|line| {
            line.trim()
                .replace("&amp;", "&")
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&quot;", "\"")
                .replace("&#x27;", "'")
                .replace("&#39;", "'")
        })
        .filter(|line| !line.is_empty())
        .collect()
}
