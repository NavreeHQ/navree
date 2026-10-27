//! Printing one document in its own browser context: the page loads the
//! document from a private origin, its assets come from the template
//! tree, and every other request is blocked unless the network is allowed.

use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};

use super::cdp::{Cdp, Event};
use crate::templates::Templates;

/// The origin documents load from. `.invalid` never resolves, and every
/// request to it is answered from the template tree before it leaves the
/// browser.
pub const DOCUMENT_ORIGIN: &str = "https://documents.navree.invalid";

/// One document to print.
pub(crate) struct PageJob {
    /// The document's path in the template tree (`invoices/show.html`, or
    /// `inline.html`): its URL, against which relative URLs resolve.
    pub(crate) path: String,
    /// The HTML to print.
    pub(crate) html: String,
    /// The locale for `Intl` and `navigator.language`, when given.
    pub(crate) locale: Option<String>,
    /// The `Page.printToPDF` parameters.
    pub(crate) print: Value,
    /// Where the document's assets come from.
    pub(crate) templates: Templates,
    /// Let requests to other origins through (default: blocked).
    pub(crate) allow_network: bool,
}

/// Prints `job` in a fresh browser context and returns the PDF's bytes.
/// The context (and its page) is disposed of when this returns or is
/// dropped half-way, as on a timeout.
pub(crate) async fn print(cdp: &Arc<Cdp>, job: PageJob) -> Result<Vec<u8>, String> {
    let context = cdp
        .call(
            "Target.createBrowserContext",
            json!({ "disposeOnDetach": true }),
            None,
        )
        .await?;
    let context_id = string(&context, "browserContextId")?;
    let _context = ContextGuard {
        cdp: cdp.clone(),
        id: context_id.clone(),
    };
    let target = cdp
        .call(
            "Target.createTarget",
            json!({ "url": "about:blank", "browserContextId": context_id }),
            None,
        )
        .await?;
    let target_id = string(&target, "targetId")?;
    let attached = cdp
        .call(
            "Target.attachToTarget",
            json!({ "targetId": target_id, "flatten": true }),
            None,
        )
        .await?;
    let session = string(&attached, "sessionId")?;
    let events = cdp.subscribe(&session);
    let _subscription = Subscription {
        cdp: cdp.clone(),
        session: session.clone(),
    };
    let on = Some(session.as_str());
    cdp.call(
        "Fetch.enable",
        json!({ "patterns": [{ "urlPattern": "*", "requestStage": "Request" }] }),
        on,
    )
    .await?;
    cdp.call("Page.enable", json!({}), on).await?;
    if let Some(locale) = &job.locale {
        // An unknown locale is not worth failing the document for.
        if let Err(error) = cdp
            .call(
                "Emulation.setLocaleOverride",
                json!({ "locale": locale }),
                on,
            )
            .await
        {
            tracing::debug!(target: "navree_documents::chrome", %error, locale, "locale override refused");
        }
    }

    let (loaded, on_load) = oneshot::channel();
    let url = format!("{DOCUMENT_ORIGIN}/{}", encode_path(&job.path));
    let print_params = job.print.clone();
    let server = tokio::spawn(serve(cdp.clone(), session.clone(), events, job, loaded));
    let _server = AbortOnDrop(server);

    let navigation = cdp.call("Page.navigate", json!({ "url": url }), on).await?;
    if let Some(error) = navigation.get("errorText").and_then(Value::as_str) {
        return Err(format!("loading the document failed: {error}"));
    }
    on_load
        .await
        .map_err(|_| "the page closed before the document loaded".to_owned())?;
    // Web fonts load when text first uses them, which can be after `load`.
    cdp.call(
        "Runtime.evaluate",
        json!({
            "expression": "document.fonts.ready.then(() => true)",
            "awaitPromise": true,
            "returnByValue": true,
        }),
        on,
    )
    .await?;
    let printed = cdp.call("Page.printToPDF", print_params, on).await?;
    let data = printed
        .get("data")
        .and_then(Value::as_str)
        .ok_or("Page.printToPDF returned no data")?;
    BASE64
        .decode(data)
        .map_err(|error| format!("Page.printToPDF returned invalid base64: {error}"))
}

/// Answers the page's requests until the session ends, and reports the
/// document's `load` event.
async fn serve(
    cdp: Arc<Cdp>,
    session: String,
    mut events: mpsc::UnboundedReceiver<Event>,
    job: PageJob,
    loaded: oneshot::Sender<()>,
) {
    let mut loaded = Some(loaded);
    let mut document_served = false;
    while let Some(event) = events.recv().await {
        match event.method.as_str() {
            "Fetch.requestPaused" => {
                let Some(request_id) = event.params.get("requestId").and_then(Value::as_str) else {
                    continue;
                };
                let url = event
                    .params
                    .pointer("/request/url")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let answer = answer(&job, url).await;
                if matches!(answer, Answer::Document) {
                    document_served = true;
                }
                send_answer(&cdp, &session, request_id, url, answer, &job);
            }
            "Page.loadEventFired" if document_served => {
                if let Some(loaded) = loaded.take() {
                    let _ = loaded.send(());
                }
            }
            _ => {}
        }
    }
}

/// How a paused request is answered.
enum Answer {
    /// The document itself.
    Document,
    /// A file of the template tree, with its content type.
    File(bytes::Bytes, String),
    /// A path of the private origin with no file.
    NotFound,
    /// Another origin, with the network allowed.
    Continue,
    /// Another origin, network blocked.
    Block,
}

async fn answer(job: &PageJob, url: &str) -> Answer {
    let Some(rest) = url
        .strip_prefix(DOCUMENT_ORIGIN)
        .and_then(|r| r.strip_prefix('/'))
    else {
        return if job.allow_network {
            Answer::Continue
        } else {
            Answer::Block
        };
    };
    let path = rest.split(['?', '#']).next().unwrap_or_default();
    let path = decode_path(path);
    if path == job.path {
        return Answer::Document;
    }
    if let Some(reserved) = path.strip_prefix(crate::fonts::RESERVED_PREFIX) {
        return match crate::fonts::reserved_file(reserved) {
            Some((bytes, content_type)) => Answer::File(bytes, content_type.to_owned()),
            None => Answer::NotFound,
        };
    }
    let templates = job.templates.clone();
    let lookup = path.clone();
    let read = tokio::task::spawn_blocking(move || templates.read(&lookup)).await;
    match read {
        Ok(Ok(Some(bytes))) => Answer::File(bytes, content_type(&path)),
        Ok(Ok(None)) => Answer::NotFound,
        Ok(Err(error)) => {
            tracing::warn!(target: "navree_documents::chrome", %error, path, "cannot read a document asset");
            Answer::NotFound
        }
        Err(_) => Answer::NotFound,
    }
}

fn send_answer(
    cdp: &Cdp,
    session: &str,
    request_id: &str,
    url: &str,
    answer: Answer,
    job: &PageJob,
) {
    let on = Some(session);
    match answer {
        Answer::Document => fulfill(
            cdp,
            on,
            request_id,
            200,
            "text/html; charset=utf-8",
            job.html.as_bytes(),
        ),
        Answer::File(bytes, content_type) => {
            fulfill(cdp, on, request_id, 200, &content_type, &bytes);
        }
        Answer::NotFound => {
            tracing::debug!(target: "navree_documents::chrome", url, "document asset not found");
            fulfill(cdp, on, request_id, 404, "text/plain; charset=utf-8", b"");
        }
        Answer::Continue => {
            cdp.send(
                "Fetch.continueRequest",
                json!({ "requestId": request_id }),
                on,
            );
        }
        Answer::Block => {
            tracing::debug!(
                target: "navree_documents::chrome",
                url,
                "blocked a request to another origin (documents.chrome.allow_network is off)"
            );
            cdp.send(
                "Fetch.failRequest",
                json!({ "requestId": request_id, "errorReason": "BlockedByClient" }),
                on,
            );
        }
    }
}

fn fulfill(
    cdp: &Cdp,
    session: Option<&str>,
    request_id: &str,
    status: u16,
    content_type: &str,
    body: &[u8],
) {
    cdp.send(
        "Fetch.fulfillRequest",
        json!({
            "requestId": request_id,
            "responseCode": status,
            "responseHeaders": [
                { "name": "Content-Type", "value": content_type },
                { "name": "Cache-Control", "value": "no-store" },
            ],
            "body": BASE64.encode(body),
        }),
        session,
    );
}

/// The content type of a template-tree file, by its extension.
fn content_type(path: &str) -> String {
    let guessed = mime_guess::from_path(path).first_or_octet_stream();
    match guessed.type_() {
        mime_guess::mime::TEXT => format!("{guessed}; charset=utf-8"),
        _ if guessed.subtype() == mime_guess::mime::JAVASCRIPT
            || guessed.subtype() == mime_guess::mime::JSON =>
        {
            format!("{guessed}; charset=utf-8")
        }
        _ => guessed.to_string(),
    }
}

/// `path` with every byte that is not unreserved or `/` percent-encoded.
pub(crate) fn encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// `path` with its percent-escapes decoded (invalid escapes stay as they
/// are; invalid UTF-8 is replaced).
pub(crate) fn decode_path(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(value) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(value);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn string(value: &Value, key: &str) -> Result<String, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("the browser's answer has no `{key}`"))
}

/// Disposes of the browser context (and closes its page) when dropped.
struct ContextGuard {
    cdp: Arc<Cdp>,
    id: String,
}

impl Drop for ContextGuard {
    fn drop(&mut self) {
        self.cdp.send(
            "Target.disposeBrowserContext",
            json!({ "browserContextId": self.id }),
            None,
        );
    }
}

/// Stops routing the session's events when dropped.
struct Subscription {
    cdp: Arc<Cdp>,
    session: String,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.cdp.unsubscribe(&self.session);
    }
}

struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_round_trip_through_percent_encoding() {
        assert_eq!(encode_path("invoices/show.html"), "invoices/show.html");
        assert_eq!(encode_path("a b/ü.html"), "a%20b/%C3%BC.html");
        assert_eq!(decode_path("a%20b/%C3%BC.html"), "a b/ü.html");
        assert_eq!(decode_path("100%"), "100%");
        assert_eq!(decode_path("%zz"), "%zz");
        assert_eq!(decode_path("%4"), "%4");
    }

    #[test]
    fn content_types_carry_a_charset_for_text() {
        assert_eq!(content_type("style.css"), "text/css; charset=utf-8");
        assert_eq!(content_type("logo.png"), "image/png");
        assert_eq!(content_type("fonts/a.woff2"), "font/woff2");
        assert_eq!(content_type("x.unknown-ext"), "application/octet-stream");
    }
}
