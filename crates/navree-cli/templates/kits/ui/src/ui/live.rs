//! Live updates: how a change in the app reaches the pages that are open.
//!
//! This is the one place that bridges the app's events to the page. It is a
//! small hub. Code that changes a customer calls [`Hub::nudge`] (the customers
//! service does, after the change is committed), and every browser tab
//! listening to `GET /customers/events` fetches its table, or its figures,
//! again. The page never reads what the stream says: an event only means "ask
//! again", so nothing a stream sends can put markup in the page.
//!
//! When the framework offers a live hub of its own, use that instead of this
//! file. The pages only name the stream's address ([`STREAM`]) and its event
//! ([`EVENT`]), so they stay as they are.

use std::convert::Infallible;
use std::time::Duration;

use http::HeaderMap;
use http::header::CONTENT_TYPE;
use navree::http::response::sse::{Event as Sse, KeepAlive, Sse as SseResponse};
use navree::http::response::{IntoResponse, Response};
use tokio::sync::broadcast;

/// Where the stream is served.
pub const STREAM: &str = "/customers/events";

/// The name of the event the stream sends. The table and the figures listen
/// for this name.
pub const EVENT: &str = "customers";

/// Who is listening, and the way to tell them.
pub struct Hub {
    tx: broadcast::Sender<()>,
}

impl Default for Hub {
    fn default() -> Self {
        Self::new()
    }
}

impl Hub {
    /// A hub nobody listens to yet.
    pub fn new() -> Self {
        Self {
            tx: broadcast::channel(64).0,
        }
    }

    /// Tells every open stream that a customer changed.
    pub fn nudge(&self) {
        // No listener is not a problem: the next page that opens asks anyway.
        let _ = self.tx.send(());
    }

    /// `GET /customers/events`: a stream of server-sent events, one
    /// [`EVENT`] for each nudge.
    pub fn stream(&self, headers: &HeaderMap) -> Response {
        // The page's script asks once what a failed stream answers: say it is a stream.
        if headers.contains_key("x-ui-probe") {
            return ([(CONTENT_TYPE, "text/event-stream")], "").into_response();
        }
        let rx = self.tx.subscribe();
        let events = futures_util::stream::unfold((rx, true), |(mut rx, first)| async move {
            // A first comment goes out at once: Firefox reports a stream open
            // only when its first bytes arrive.
            if first {
                return Some((
                    Ok::<_, Infallible>(Sse::default().comment("ready")),
                    (rx, false),
                ));
            }
            match rx.recv().await {
                Ok(()) | Err(broadcast::error::RecvError::Lagged(_)) => {
                    Some((Ok(Sse::default().event(EVENT).data("changed")), (rx, false)))
                }
                Err(broadcast::error::RecvError::Closed) => None,
            }
        });
        SseResponse::new(events)
            .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
            .into_response()
    }
}
