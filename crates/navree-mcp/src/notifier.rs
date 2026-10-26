//! [`McpNotifier`]: notifications the server starts, to the sessions open
//! on this process.

use std::collections::BTreeSet;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

use dashmap::DashMap;
use rmcp::model::ResourceUpdatedNotificationParam;
use rmcp::service::SubscriptionSink;
use rmcp::{Peer, RoleServer};

/// Sends the notifications the server starts: a resource changed, the
/// lists of tools, resources or prompts changed. Provided and exported by
/// `McpModule`; inject it (`Arc<McpNotifier>`) into any service.
///
/// It reaches the sessions open on this process: a session that
/// subscribed to the resource (`resources/subscribe`, or
/// `subscriptions/listen` on protocol 2026-07-28) and, for list changes,
/// every session. Each method returns how many sessions it notified.
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

enum Channel {
    /// A session of protocol 2025-11-25 or earlier.
    Peer(Peer<RoleServer>),
    /// A `subscriptions/listen` stream (protocol 2026-07-28).
    Sink(SubscriptionSink),
}

#[derive(Clone, Copy)]
enum ListKind {
    Tools,
    Resources,
    Prompts,
}

impl McpNotifier {
    /// Tells the sessions subscribed to `uri` to read it again
    /// (`notifications/resources/updated`).
    pub async fn resource_updated(&self, uri: impl Into<String>) -> usize {
        let uri = uri.into();
        let targets: Vec<(u64, ChannelRef)> = self
            .listeners
            .iter()
            .filter(|listener| {
                listener
                    .resources
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .contains(&uri)
            })
            .map(|listener| (*listener.key(), listener.channel.reference()))
            .collect();
        let mut sent = 0;
        for (id, channel) in targets {
            let result = match channel {
                ChannelRef::Peer(peer) => peer
                    .notify_resource_updated(ResourceUpdatedNotificationParam::new(uri.clone()))
                    .await
                    .map_err(|error| error.to_string()),
                ChannelRef::Sink(sink) => sink
                    .notify_resource_updated(uri.clone())
                    .await
                    .map_err(|error| error.to_string()),
            };
            sent += self.settle(id, result);
        }
        sent
    }

    /// Tells every session that the list of tools changed
    /// (`notifications/tools/list_changed`), for example after a deploy that
    /// adds tools or a change to who may use them.
    pub async fn tools_changed(&self) -> usize {
        self.list_changed(ListKind::Tools).await
    }

    /// Tells every session that the list of resources changed
    /// (`notifications/resources/list_changed`).
    pub async fn resources_changed(&self) -> usize {
        self.list_changed(ListKind::Resources).await
    }

    /// Tells every session that the list of prompts changed
    /// (`notifications/prompts/list_changed`).
    pub async fn prompts_changed(&self) -> usize {
        self.list_changed(ListKind::Prompts).await
    }

    /// How many sessions (and listen streams) are open on this process.
    pub fn sessions(&self) -> usize {
        self.listeners.len()
    }

    async fn list_changed(&self, kind: ListKind) -> usize {
        let targets: Vec<(u64, ChannelRef)> = self
            .listeners
            .iter()
            .filter(|listener| match kind {
                ListKind::Tools => listener.lists.tools,
                ListKind::Resources => listener.lists.resources,
                ListKind::Prompts => listener.lists.prompts,
            })
            .map(|listener| (*listener.key(), listener.channel.reference()))
            .collect();
        let mut sent = 0;
        for (id, channel) in targets {
            let result = channel.list_changed(kind).await;
            sent += self.settle(id, result);
        }
        sent
    }

    /// 1 when a notification went out; forgets listeners whose transport is
    /// gone.
    fn settle(&self, id: u64, result: Result<(), String>) -> usize {
        match result {
            Ok(()) => 1,
            Err(error) => {
                let closed = self
                    .listeners
                    .get(&id)
                    .is_none_or(|listener| listener.channel.is_closed());
                if closed {
                    self.listeners.remove(&id);
                } else {
                    tracing::debug!(target: "navree::mcp", %error, "an MCP notification was not sent");
                }
                0
            }
        }
    }

    /// Registers the session `session` (its peer takes notifications);
    /// replaces an earlier registration of the same session.
    pub(crate) fn open_session(&self, session: &str, peer: Peer<RoleServer>) -> u64 {
        self.listeners
            .retain(|_, listener| listener.session.as_deref() != Some(session));
        self.insert(Listener {
            session: Some(session.to_owned()),
            channel: Channel::Peer(peer),
            lists: Lists::ALL,
            resources: Mutex::default(),
        })
    }

    /// Registers a `subscriptions/listen` stream; returns its id for
    /// [`close`](McpNotifier::close).
    pub(crate) fn listen(
        &self,
        sink: SubscriptionSink,
        lists: Lists,
        resources: BTreeSet<String>,
    ) -> u64 {
        self.insert(Listener {
            session: None,
            channel: Channel::Sink(sink),
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

/// A clone of a listener's channel, so sends happen without holding the map.
enum ChannelRef {
    Peer(Peer<RoleServer>),
    Sink(SubscriptionSink),
}

impl Channel {
    fn reference(&self) -> ChannelRef {
        match self {
            Channel::Peer(peer) => ChannelRef::Peer(peer.clone()),
            Channel::Sink(sink) => ChannelRef::Sink(sink.clone()),
        }
    }

    fn is_closed(&self) -> bool {
        match self {
            Channel::Peer(peer) => peer.is_transport_closed(),
            Channel::Sink(_) => false,
        }
    }
}

impl ChannelRef {
    async fn list_changed(self, kind: ListKind) -> Result<(), String> {
        match (self, kind) {
            (ChannelRef::Peer(peer), ListKind::Tools) => peer
                .notify_tool_list_changed()
                .await
                .map_err(|e| e.to_string()),
            (ChannelRef::Peer(peer), ListKind::Resources) => peer
                .notify_resource_list_changed()
                .await
                .map_err(|e| e.to_string()),
            (ChannelRef::Peer(peer), ListKind::Prompts) => peer
                .notify_prompt_list_changed()
                .await
                .map_err(|e| e.to_string()),
            (ChannelRef::Sink(sink), ListKind::Tools) => sink
                .notify_tool_list_changed()
                .await
                .map_err(|e| e.to_string()),
            (ChannelRef::Sink(sink), ListKind::Resources) => sink
                .notify_resource_list_changed()
                .await
                .map_err(|e| e.to_string()),
            (ChannelRef::Sink(sink), ListKind::Prompts) => sink
                .notify_prompt_list_changed()
                .await
                .map_err(|e| e.to_string()),
        }
    }
}
