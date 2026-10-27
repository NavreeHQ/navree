//! A small Chrome DevTools Protocol client: commands over one WebSocket
//! to the browser, with flattened sessions for pages.

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

/// One connection to a browser's DevTools endpoint.
pub(crate) struct Cdp {
    outgoing: mpsc::UnboundedSender<Message>,
    pending: Pending,
    next_id: AtomicU64,
    alive: Arc<AtomicBool>,
    tasks: [JoinHandle<()>; 2],
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
        let alive = Arc::new(AtomicBool::new(true));

        let writer = tokio::spawn(async move {
            while let Some(message) = queue.recv().await {
                if sink.send(message).await.is_err() {
                    break;
                }
            }
            let _ = sink.close().await;
        });

        let reader_pending = pending.clone();
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
                // Events have no id; only answers are routed.
                let Some(id) = value.get("id").and_then(Value::as_u64) else {
                    continue;
                };
                let waiter = reader_pending
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
            reader_alive.store(false, Ordering::SeqCst);
            // Fail everything still waiting.
            reader_pending
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clear();
        });

        Ok(Cdp {
            outgoing,
            pending,
            next_id: AtomicU64::new(1),
            alive,
            tasks: [writer, reader],
        })
    }

    /// Whether the connection is still open.
    pub(crate) fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
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
}

impl Drop for Cdp {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}
