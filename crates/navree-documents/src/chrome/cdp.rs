//! A small Chrome DevTools Protocol client: commands over one WebSocket
//! to the browser, with flattened sessions for pages and their events
//! routed to whoever subscribed to the session.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

/// Largest message accepted from the browser (a printed PDF arrives as one
/// base64 message): 512 MiB.
const MAX_MESSAGE: usize = 512 * 1024 * 1024;

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>;
type Subscribers = Arc<Mutex<HashMap<String, mpsc::UnboundedSender<Event>>>>;

/// An event the browser sent for a page session.
#[derive(Clone, Debug)]
pub(crate) struct Event {
    /// `Page.loadEventFired`, `Fetch.requestPaused`, ...
    pub(crate) method: String,
    /// The event's parameters.
    pub(crate) params: Value,
}

/// One connection to a browser's DevTools endpoint.
pub(crate) struct Cdp {
    outgoing: mpsc::UnboundedSender<Message>,
    pending: Pending,
    subscribers: Subscribers,
    next_id: AtomicU64,
    alive: Arc<AtomicBool>,
    reader: JoinHandle<()>,
}

impl Cdp {
    /// Connects to `ws_url` (`ws://host:port/devtools/browser/<id>`).
    pub(crate) async fn connect(ws_url: &str) -> Result<Cdp, String> {
        let config = WebSocketConfig::default()
            .max_message_size(Some(MAX_MESSAGE))
            .max_frame_size(Some(MAX_MESSAGE));
        let (stream, _) = tokio_tungstenite::connect_async_with_config(ws_url, Some(config), true)
            .await
            .map_err(|error| format!("cannot connect to {ws_url}: {error}"))?;
        let (mut sink, mut source) = stream.split();
        let (outgoing, mut queue) = mpsc::unbounded_channel::<Message>();
        let pending: Pending = Arc::default();
        let subscribers: Subscribers = Arc::default();
        let alive = Arc::new(AtomicBool::new(true));

        tokio::spawn(async move {
            while let Some(message) = queue.recv().await {
                if sink.send(message).await.is_err() {
                    break;
                }
            }
            let _ = sink.close().await;
        });

        let reader_pending = pending.clone();
        let reader_subscribers = subscribers.clone();
        let reader_alive = alive.clone();
        let reader = tokio::spawn(async move {
            while let Some(message) = source.next().await {
                let Ok(message) = message else {
                    break;
                };
                let text = match &message {
                    Message::Text(text) => text.as_str(),
                    Message::Close(_) => break,
                    _ => continue,
                };
                let Ok(value) = serde_json::from_str::<Value>(text) else {
                    continue;
                };
                match value.get("id").and_then(Value::as_u64) {
                    Some(id) => route_answer(&reader_pending, id, &value),
                    None => route_event(&reader_subscribers, value),
                }
            }
            reader_alive.store(false, Ordering::SeqCst);
            // Fail everything still waiting, and end every subscription.
            reader_pending
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clear();
            reader_subscribers
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clear();
        });

        Ok(Cdp {
            outgoing,
            pending,
            subscribers,
            next_id: AtomicU64::new(1),
            alive,
            reader,
        })
    }

    /// Whether the connection is still open.
    pub(crate) fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    /// The events of page `session` from now on, until
    /// [`unsubscribe`](Cdp::unsubscribe) or the connection closes.
    pub(crate) fn subscribe(&self, session: &str) -> mpsc::UnboundedReceiver<Event> {
        let (sender, receiver) = mpsc::unbounded_channel();
        if self.is_alive() {
            self.subscribers
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(session.to_owned(), sender);
        }
        receiver
    }

    /// Stops routing the events of `session`.
    pub(crate) fn unsubscribe(&self, session: &str) {
        self.subscribers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(session);
    }

    /// Sends `method` with `params` (to the page `session`, or to the
    /// browser) and waits for the answer.
    pub(crate) async fn call(
        &self,
        method: &str,
        params: Value,
        session: Option<&str>,
    ) -> Result<Value, String> {
        if !self.is_alive() {
            return Err("the DevTools connection is closed".into());
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let mut message = json!({ "id": id, "method": method, "params": params });
        if let Some(session) = session {
            message["sessionId"] = Value::String(session.to_owned());
        }
        let (sender, receiver) = oneshot::channel();
        self.pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id, sender);
        if self
            .outgoing
            .send(Message::text(message.to_string()))
            .is_err()
        {
            self.pending
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&id);
            return Err("the DevTools connection is closed".into());
        }
        match receiver.await {
            Ok(answer) => answer.map_err(|error| format!("{method}: {error}")),
            Err(_) => Err(format!("{method}: the DevTools connection closed")),
        }
    }

    /// Sends `method` without waiting for the answer (for answers nobody
    /// needs, such as continuing a paused request).
    pub(crate) fn send(&self, method: &str, params: Value, session: Option<&str>) {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let mut message = json!({ "id": id, "method": method, "params": params });
        if let Some(session) = session {
            message["sessionId"] = Value::String(session.to_owned());
        }
        let _ = self.outgoing.send(Message::text(message.to_string()));
    }
}

/// Hands an answer to the call waiting for it.
fn route_answer(pending: &Pending, id: u64, value: &Value) {
    let waiter = pending
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&id);
    if let Some(waiter) = waiter {
        let answer = match value.get("error") {
            Some(error) => Err(error
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| error.to_string())),
            None => Ok(value.get("result").cloned().unwrap_or(Value::Null)),
        };
        let _ = waiter.send(answer);
    }
}

/// Hands a session's event to its subscriber; browser-level events and
/// events of sessions nobody subscribed to are dropped.
fn route_event(subscribers: &Subscribers, mut value: Value) {
    let Some(session) = value
        .get("sessionId")
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        return;
    };
    let Some(method) = value
        .get("method")
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        return;
    };
    let event = Event {
        method,
        params: value
            .get_mut("params")
            .map(Value::take)
            .unwrap_or(Value::Null),
    };
    let subscribers = subscribers.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(subscriber) = subscribers.get(&session) {
        let _ = subscriber.send(event);
    }
}

impl Drop for Cdp {
    /// Stops reading. The writer sends what is still queued (such as the
    /// disposal of a browser context), then closes the connection, because
    /// dropping `outgoing` ends its queue.
    fn drop(&mut self) {
        self.reader.abort();
    }
}
