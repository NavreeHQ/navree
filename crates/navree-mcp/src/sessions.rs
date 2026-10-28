//! The HTTP transport's sessions: open on this process, and kept in the
//! app's cache (owner, protocol version, client) so any process can
//! restore one. A session is used only by the user who opened it, and
//! ends after `session_ttl_secs` without a request.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use navree_cache::Cache;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::context::McpClient;
use crate::notifier::McpNotifier;
use crate::session::Session;

/// How many lookups pass between sweeps of idle sessions.
const SWEEP_EVERY: usize = 64;

/// The cache key of a session.
fn key(id: &str) -> String {
    format!("mcp:session:{id}")
}

/// What the cache keeps of a session.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct Record {
    owner: Uuid,
    version: String,
    client: Option<(String, String)>,
}

/// The sessions of the HTTP transport.
pub(crate) struct Sessions {
    live: DashMap<String, Arc<Session>>,
    /// When each live session's cache entry was last written.
    stored: DashMap<String, Instant>,
    cache: Option<Arc<Cache>>,
    ttl: Duration,
    notifier: Arc<McpNotifier>,
    lookups: AtomicUsize,
}

impl Sessions {
    pub(crate) fn new(
        cache: Option<Arc<Cache>>,
        ttl: Duration,
        notifier: Arc<McpNotifier>,
    ) -> Self {
        Self {
            live: DashMap::new(),
            stored: DashMap::new(),
            cache,
            ttl,
            notifier,
            lookups: AtomicUsize::new(0),
        }
    }

    /// Keeps a new session.
    pub(crate) async fn insert(&self, session: &Arc<Session>) {
        self.live.insert(session.id.clone(), session.clone());
        self.store(session).await;
    }

    /// Session `id`, when it exists (here or in the cache), has not expired
    /// and belongs to `user`. Another user's session is not found, exactly
    /// like a missing one.
    pub(crate) async fn find(&self, id: &str, user: Uuid) -> Option<Arc<Session>> {
        if self.lookups.fetch_add(1, Ordering::Relaxed) % SWEEP_EVERY == 0 {
            self.sweep().await;
        }
        let live = self.live.get(id).map(|entry| entry.value().clone());
        let session = match live {
            Some(session) if session.expired(self.ttl) => {
                self.remove(&session).await;
                return None;
            }
            Some(session) => session,
            None => self.restore(id).await?,
        };
        if session.owner != Some(user) {
            return None;
        }
        session.touch();
        self.keep_alive(&session).await;
        Some(session)
    }

    /// Ends `session` here and in the cache.
    pub(crate) async fn remove(&self, session: &Arc<Session>) {
        self.live.remove(&session.id);
        self.stored.remove(&session.id);
        session.close();
        self.notifier.close_session(&session.id);
        if let Some(cache) = &self.cache {
            let _ = cache.forget(&key(&session.id)).await;
        }
    }

    /// The session from the cache, opened on this process.
    async fn restore(&self, id: &str) -> Option<Arc<Session>> {
        let cache = self.cache.as_ref()?;
        let record = cache.get::<Record>(&key(id)).await.ok().flatten()?;
        let client = record
            .client
            .map(|(name, version)| McpClient { name, version });
        let session = Session::new(id.to_owned(), Some(record.owner), record.version, client);
        let session = self
            .live
            .entry(id.to_owned())
            .or_insert(session)
            .value()
            .clone();
        self.stored.insert(id.to_owned(), Instant::now());
        Some(session)
    }

    /// Writes the session's cache entry.
    async fn store(&self, session: &Session) {
        let Some(cache) = &self.cache else {
            return;
        };
        let Some(owner) = session.owner else {
            return;
        };
        let record = Record {
            owner,
            version: session.version.clone(),
            client: session
                .client
                .as_ref()
                .map(|client| (client.name.clone(), client.version.clone())),
        };
        if let Err(error) = cache.put(&key(&session.id), &record, self.ttl).await {
            tracing::warn!(target: "navree::mcp", %error, "could not store the MCP session");
        }
        self.stored.insert(session.id.clone(), Instant::now());
    }

    /// Refreshes the cache entry once half of its lifetime has passed.
    async fn keep_alive(&self, session: &Session) {
        let due = self
            .stored
            .get(&session.id)
            .is_none_or(|stored| stored.elapsed() >= self.ttl / 2);
        if due {
            self.store(session).await;
        }
    }

    /// Ends the sessions idle for longer than the TTL.
    async fn sweep(&self) {
        let expired: Vec<Arc<Session>> = self
            .live
            .iter()
            .filter(|entry| entry.value().expired(self.ttl))
            .map(|entry| entry.value().clone())
            .collect();
        for session in expired {
            self.remove(&session).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(owner: Uuid) -> Arc<Session> {
        Session::new(
            crate::session::new_session_id(),
            Some(owner),
            "2025-11-25".to_owned(),
            Some(McpClient {
                name: "claude".to_owned(),
                version: "1".to_owned(),
            }),
        )
    }

    #[tokio::test]
    async fn sessions_survive_in_the_cache_for_their_owner_only() {
        let cache = Arc::new(Cache::memory());
        let notifier = Arc::new(McpNotifier::default());
        let ttl = Duration::from_secs(60);
        let (owner, intruder) = (Uuid::now_v7(), Uuid::now_v7());
        let first = Sessions::new(Some(cache.clone()), ttl, notifier.clone());
        let opened = session(owner);
        first.insert(&opened).await;
        assert!(first.find(&opened.id, owner).await.is_some());
        assert!(first.find(&opened.id, intruder).await.is_none());

        // Another process with the same cache.
        let second = Sessions::new(Some(cache.clone()), ttl, notifier.clone());
        let restored = second.find(&opened.id, owner).await.expect("restored");
        assert_eq!(restored.version, "2025-11-25");
        assert_eq!(restored.client.as_ref().unwrap().name, "claude");
        assert!(second.find(&opened.id, intruder).await.is_none());
        second.remove(&restored).await;
        assert!(
            Sessions::new(Some(cache), ttl, notifier.clone())
                .find(&opened.id, owner)
                .await
                .is_none()
        );

        let memory_only = Sessions::new(None, ttl, notifier);
        assert!(memory_only.find("nope", owner).await.is_none());
    }

    #[tokio::test]
    async fn idle_sessions_end() {
        let sessions = Sessions::new(None, Duration::ZERO, Arc::new(McpNotifier::default()));
        let owner = Uuid::now_v7();
        let opened = session(owner);
        sessions.insert(&opened).await;
        assert!(sessions.find(&opened.id, owner).await.is_none());
        assert!(opened.is_closed());
    }
}
