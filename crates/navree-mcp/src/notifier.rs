//! [`McpNotifier`]: notifications the server starts, to the sessions open
//! on this process.

use std::collections::BTreeSet;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, Weak};

use dashmap::DashMap;
use serde_json::{Value, json};

use crate::protocol::jsonrpc;
use crate::protocol::lifecycle::META_SUBSCRIPTION_ID;
use crate::protocol::outlet::{NotSent, Outlet};
use crate::session::Session;

/// Sends the notifications the server starts: a resource changed, the
/// lists of tools, resources or prompts changed. Provided and exported by
/// `McpModule`; inject it (`Arc<McpNotifier>`) into any service.
///
/// It reaches the sessions open on this process: a session that
/// subscribed to the resource (`resources/subscribe`, or
/// `subscriptions/listen` on protocol 2026-07-28) and, for list changes,
/// every session. Each method returns how many sessions it notified (a
/// session with no open stream, or one that stopped reading, is not
/// counted).
///
/// ```no_run
/// # async fn moved(notifier: std::sync::Arc<navree_mcp::McpNotifier>, deal_id: uuid::Uuid) {
/// // next to the deal's real-time broadcast:
/// notifier.resource_updated(format!("crm://deals/{deal_id}")).await;
/// # }
/// ```
#[derive(Default)]
pub struct McpNotifier {
    listeners: DashMap<u64, Listener>,
    next: AtomicU64,
}

/// One open session (or one `subscriptions/listen` stream).
struct Listener {
    session: Option<String>,
    channel: Channel,
    lists: Lists,
    resources: Mutex<BTreeSet<String>>,
}

/// Which list-changed notifications a listener takes.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Lists {
    pub(crate) tools: bool,
    pub(crate) resources: bool,
    pub(crate) prompts: bool,
}

impl Lists {
    pub(crate) const ALL: Lists = Lists {
        tools: true,
        resources: true,
        prompts: true,
    };
}

#[derive(Clone)]
enum Channel {
    /// A session of protocol 2025-11-25 or earlier: its stream.
    Session(Weak<Session>),
    /// A `subscriptions/listen` stream (protocol 2026-07-28): the request's
    /// stream and its id, which every notification carries.
    Listen { outlet: Outlet, subscription: Value },
}

#[derive(Clone, Copy)]
enum ListKind {
    Tools,
    Resources,
    Prompts,
}

impl ListKind {
    fn method(self) -> &'static str {
        match self {
            ListKind::Tools => "notifications/tools/list_changed",
            ListKind::Resources => "notifications/resources/list_changed",
            ListKind::Prompts => "notifications/prompts/list_changed",
        }
    }
}

impl McpNotifier {
    /// Tells the sessions subscribed to `uri` to read it again
    /// (`notifications/resources/updated`).
    pub async fn resource_updated(&self, uri: impl Into<String>) -> usize {
        let uri = uri.into();
        let targets: Vec<(u64, Channel)> = self
            .listeners
            .iter()
            .filter(|listener| {
                listener
                    .resources
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .contains(&uri)
            })
            .map(|listener| (*listener.key(), listener.channel.clone()))
            .collect();
        let mut sent = 0;
        for (id, channel) in targets {
            let result = channel.send(
                "notifications/resources/updated",
                Some(json!({ "uri": uri })),
            );
            sent += self.settle(id, &channel, result);
        }
        sent
    }

    /// Tells every session that the list of tools changed
    /// (`notifications/tools/list_changed`), for example after a deploy that
    /// adds tools or a change to who may use them.
    pub async fn tools_changed(&self) -> usize {
        self.list_changed(ListKind::Tools)
    }

    /// Tells every session that the list of resources changed
    /// (`notifications/resources/list_changed`).
    pub async fn resources_changed(&self) -> usize {
        self.list_changed(ListKind::Resources)
    }

    /// Tells every session that the list of prompts changed
    /// (`notifications/prompts/list_changed`).
    pub async fn prompts_changed(&self) -> usize {
        self.list_changed(ListKind::Prompts)
    }

    /// How many sessions (and listen streams) are open on this process.
    pub fn sessions(&self) -> usize {
        self.listeners.len()
    }

    fn list_changed(&self, kind: ListKind) -> usize {
        let targets: Vec<(u64, Channel)> = self
            .listeners
            .iter()
            .filter(|listener| match kind {
                ListKind::Tools => listener.lists.tools,
                ListKind::Resources => listener.lists.resources,
                ListKind::Prompts => listener.lists.prompts,
            })
            .map(|listener| (*listener.key(), listener.channel.clone()))
            .collect();
        let mut sent = 0;
        for (id, channel) in targets {
            let result = channel.send(kind.method(), None);
            sent += self.settle(id, &channel, result);
        }
        sent
    }

    /// 1 when a notification went out; forgets listeners whose stream is
    /// gone for good.
    fn settle(&self, id: u64, channel: &Channel, result: Result<(), NotSent>) -> usize {
        match result {
            Ok(()) => 1,
            Err(error) => {
                if channel.is_gone() {
                    self.listeners.remove(&id);
                } else {
                    tracing::debug!(target: "navree::mcp", %error, "an MCP notification was not sent");
                }
                0
            }
        }
    }

    /// Registers `session` (its stream takes notifications); replaces an
    /// earlier registration of the same session.
    pub(crate) fn open_session(&self, session: &Arc<Session>) -> u64 {
        self.listeners
            .retain(|_, listener| listener.session.as_deref() != Some(session.id.as_str()));
        self.insert(Listener {
            session: Some(session.id.clone()),
            channel: Channel::Session(Arc::downgrade(session)),
            lists: Lists::ALL,
            resources: Mutex::default(),
        })
    }

    /// Registers a `subscriptions/listen` stream (request `subscription` on
    /// `outlet`); returns its id for [`close`](McpNotifier::close).
    pub(crate) fn listen(
        &self,
        outlet: Outlet,
        subscription: Value,
        lists: Lists,
        resources: BTreeSet<String>,
    ) -> u64 {
        self.insert(Listener {
            session: None,
            channel: Channel::Listen {
                outlet,
                subscription,
            },
            lists,
            resources: Mutex::new(resources),
        })
    }

    fn insert(&self, listener: Listener) -> u64 {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        self.listeners.insert(id, listener);
        id
    }

    /// Forgets listener `id`.
    pub(crate) fn close(&self, id: u64) {
        self.listeners.remove(&id);
    }

    /// Forgets session `session`.
    pub(crate) fn close_session(&self, session: &str) {
        self.listeners
            .retain(|_, listener| listener.session.as_deref() != Some(session));
    }

    /// Subscribes session `session` to `uri`; false when the session is not
    /// open here.
    pub(crate) fn subscribe(&self, session: &str, uri: &str, subscribed: bool) -> bool {
        let mut found = false;
        for listener in self.listeners.iter() {
            if listener.session.as_deref() == Some(session) {
                let mut resources = listener
                    .resources
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                if subscribed {
                    resources.insert(uri.to_owned());
                } else {
                    resources.remove(uri);
                }
                found = true;
            }
        }
        found
    }
}

impl fmt::Debug for McpNotifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("McpNotifier")
            .field("sessions", &self.listeners.len())
            .finish()
    }
}

impl Channel {
    /// Queues notification `method` (never waits on a slow client).
    fn send(&self, method: &str, params: Option<Value>) -> Result<(), NotSent> {
        match self {
            Channel::Session(session) => {
                let session = session.upgrade().ok_or(NotSent::Closed)?;
                session.notify(jsonrpc::notification(method, params))
            }
            Channel::Listen {
                outlet,
                subscription,
            } => {
                let mut params = params.unwrap_or_else(|| json!({}));
                params["_meta"] = json!({ META_SUBSCRIPTION_ID: subscription });
                outlet.notify(jsonrpc::notification(method, Some(params)))
            }
        }
    }

    /// Whether the session or stream ended for good.
    fn is_gone(&self) -> bool {
        match self {
            Channel::Session(session) => {
                session.upgrade().is_none_or(|session| session.is_closed())
            }
            Channel::Listen { outlet, .. } => outlet.is_closed(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sessions_and_listen_streams_get_their_notifications() {
        let notifier = McpNotifier::default();
        let session = Session::new("s1".to_owned(), None, "2025-11-25".to_owned(), None);
        let (outlet, mut stream) = Outlet::channel();
        session.set_stream(outlet);
        notifier.open_session(&session);
        assert!(notifier.subscribe("s1", "crm://x", true));
        let (listen, mut listened) = Outlet::channel();
        notifier.listen(
            listen,
            json!(9),
            Lists {
                tools: true,
                ..Lists::default()
            },
            BTreeSet::from(["crm://x".to_owned()]),
        );

        assert_eq!(notifier.resource_updated("crm://x").await, 2);
        assert_eq!(
            stream.recv().await.unwrap()["params"],
            json!({ "uri": "crm://x" })
        );
        assert_eq!(
            listened.recv().await.unwrap()["params"],
            json!({ "uri": "crm://x", "_meta": { META_SUBSCRIPTION_ID: 9 } })
        );
        assert_eq!(
            notifier.prompts_changed().await,
            1,
            "the stream took tools only"
        );
        assert_eq!(notifier.tools_changed().await, 2);

        // A gone stream is forgotten; a closed session too.
        drop(listened);
        session.close();
        assert_eq!(notifier.tools_changed().await, 0);
        assert_eq!(notifier.sessions(), 0);
    }
}
