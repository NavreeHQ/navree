//! [`Session`]: one MCP session (an `initialize` handshake over HTTP, or the
//! stdio connection): who owns it, the agreed protocol version, the client,
//! the stream server-started notifications go to, and its requests in
//! flight (so `notifications/cancelled` can stop them).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::TryRng as _;
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::context::McpClient;
use crate::protocol::outlet::{NotSent, Outlet};

/// No list read yet.
const NEVER_LISTED: i64 = i64::MIN;

/// One session.
#[derive(Debug)]
pub(crate) struct Session {
    pub(crate) id: String,
    /// The user it belongs to (`None` for the stdio process user, who is
    /// the only one on the connection).
    pub(crate) owner: Option<Uuid>,
    /// The protocol version `initialize` agreed to.
    pub(crate) version: String,
    /// The client's name and version from `initialize`.
    pub(crate) client: Option<McpClient>,
    /// Where notifications the server starts go: the HTTP `GET` stream
    /// while one is open, the stdio writer.
    stream: Mutex<Option<Outlet>>,
    closed: AtomicBool,
    /// Whether the notifier knows the session.
    pub(crate) registered: AtomicBool,
    /// The access version of the last list the session read.
    listed_version: AtomicI64,
    /// Requests in flight.
    pub(crate) requests: Arc<Requests>,
    last_seen: Mutex<Instant>,
}

impl Session {
    pub(crate) fn new(
        id: String,
        owner: Option<Uuid>,
        version: String,
        client: Option<McpClient>,
    ) -> Arc<Session> {
        Arc::new(Session {
            id,
            owner,
            version,
            client,
            stream: Mutex::new(None),
            closed: AtomicBool::new(false),
            registered: AtomicBool::new(false),
            listed_version: AtomicI64::new(NEVER_LISTED),
            requests: Arc::default(),
            last_seen: Mutex::new(Instant::now()),
        })
    }

    /// Sends server-started notifications to `outlet` from now on (a new
    /// `GET` stream replaces the previous one, which ends).
    pub(crate) fn set_stream(&self, outlet: Outlet) {
        *self.stream.lock().unwrap_or_else(PoisonError::into_inner) = Some(outlet);
    }

    /// Queues a server-started notification on the session's stream.
    pub(crate) fn notify(&self, message: Value) -> Result<(), NotSent> {
        if self.is_closed() {
            return Err(NotSent::Closed);
        }
        let mut stream = self.stream.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(outlet) = stream.as_ref() else {
            return Err(NotSent::Closed);
        };
        let sent = outlet.notify(message);
        if sent == Err(NotSent::Closed) {
            *stream = None;
        }
        sent
    }

    /// Whether the session ended.
    pub(crate) fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    /// Ends the session: its stream closes and its requests stop.
    pub(crate) fn close(&self) {
        self.closed.store(true, Ordering::Release);
        self.stream
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        self.requests.cancel_all();
    }

    /// Tracks request `id` until the guard drops; `notifications/cancelled`
    /// for it cancels the returned token.
    pub(crate) fn track(&self, id: &Value, token: CancellationToken) -> InFlight {
        self.requests.track(id, token)
    }

    /// Cancels request `id` (`notifications/cancelled`); false when it is
    /// not in flight.
    pub(crate) fn cancel(&self, id: &Value) -> bool {
        self.requests.cancel(id)
    }

    /// Notes activity (requests keep a session alive).
    pub(crate) fn touch(&self) {
        *self
            .last_seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Instant::now();
    }

    /// Whether the session has been idle for `ttl`.
    pub(crate) fn expired(&self, ttl: Duration) -> bool {
        self.last_seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .elapsed()
            >= ttl
    }

    /// Records that a list was read at access `version`; returns whether
    /// lists read earlier were read at another version (the user's access
    /// changed, so the client should read them again).
    pub(crate) fn access_changed(&self, version: i64) -> bool {
        let listed = self.listed_version.load(Ordering::Relaxed);
        listed != NEVER_LISTED
            && listed != version
            && self
                .listed_version
                .compare_exchange(listed, version, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
    }

    /// Notes the access version a list was read at.
    pub(crate) fn listed(&self, version: i64) {
        self.listed_version.store(version, Ordering::Relaxed);
    }
}

/// The requests in flight on a session or connection, by their JSON id
/// (so `7` and `"7"` are different requests).
#[derive(Debug, Default)]
pub(crate) struct Requests {
    in_flight: Mutex<HashMap<String, CancellationToken>>,
}

impl Requests {
    /// Tracks request `id` until the guard drops.
    pub(crate) fn track(self: &Arc<Self>, id: &Value, token: CancellationToken) -> InFlight {
        let key = id.to_string();
        self.in_flight
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key.clone(), token);
        InFlight {
            requests: self.clone(),
            key,
        }
    }

    /// Cancels request `id`; false when it is not in flight.
    pub(crate) fn cancel(&self, id: &Value) -> bool {
        let token = self
            .in_flight
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&id.to_string());
        token.inspect(CancellationToken::cancel).is_some()
    }

    /// Cancels every request in flight.
    pub(crate) fn cancel_all(&self) {
        let in_flight = std::mem::take(
            &mut *self
                .in_flight
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        );
        for token in in_flight.into_values() {
            token.cancel();
        }
    }
}

/// A request in flight; untracked when dropped.
pub(crate) struct InFlight {
    requests: Arc<Requests>,
    key: String,
}

impl Drop for InFlight {
    fn drop(&mut self) {
        self.requests
            .in_flight
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.key);
    }
}

/// A new session id: 256 bits from the operating system's CSPRNG, base64url
/// (43 visible ASCII characters, as the protocol requires).
///
/// # Panics
///
/// If the operating system's random number generator fails (it does not
/// on supported platforms).
pub(crate) fn new_session_id() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::SysRng
        .try_fill_bytes(&mut bytes)
        .expect("the operating system's random number generator failed");
    URL_SAFE_NO_PAD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn session_ids_are_long_random_and_visible_ascii() {
        let ids: Vec<String> = (0..100).map(|_| new_session_id()).collect();
        for id in &ids {
            assert_eq!(id.len(), 43);
            assert!(id.bytes().all(|byte| (0x21..=0x7e).contains(&byte)), "{id}");
        }
        let unique: std::collections::BTreeSet<&String> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len());
    }

    #[tokio::test]
    async fn cancelling_stops_tracked_requests_and_closing_stops_all() {
        let session = Session::new("s".to_owned(), None, "2025-11-25".to_owned(), None);
        let first = CancellationToken::new();
        let guard = session.track(&json!(1), first.clone());
        assert!(session.cancel(&json!(1)));
        assert!(first.is_cancelled());
        assert!(!session.cancel(&json!(1)), "no longer in flight");
        drop(guard);

        let second = CancellationToken::new();
        let _guard = session.track(&json!("2"), second.clone());
        assert!(!session.cancel(&json!(2)), "ids keep their type");
        session.close();
        assert!(second.is_cancelled());
        assert!(session.is_closed());
    }

    #[test]
    fn notifications_need_an_open_stream() {
        let session = Session::new("s".to_owned(), None, "2025-11-25".to_owned(), None);
        assert_eq!(session.notify(json!({})), Err(NotSent::Closed));
        let (outlet, rx) = Outlet::channel();
        session.set_stream(outlet);
        assert_eq!(session.notify(json!({})), Ok(()));
        drop(rx);
        assert_eq!(session.notify(json!({})), Err(NotSent::Closed));
    }

    #[test]
    fn access_changes_are_noticed_once() {
        let session = Session::new("s".to_owned(), None, "2025-11-25".to_owned(), None);
        assert!(!session.access_changed(1), "nothing listed yet");
        session.listed(1);
        assert!(!session.access_changed(1));
        assert!(session.access_changed(2));
        assert!(!session.access_changed(2));
    }
}
