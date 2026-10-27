//! Sessions of the HTTP transport: who owns each one, and the session
//! state kept in the app's cache so any process can restore a session.

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use dashmap::DashMap;
use navree_cache::Cache;
use rmcp::transport::streamable_http_server::session::{
    SessionState, SessionStore, SessionStoreError,
};
use uuid::Uuid;

/// The cache key of a session's state.
fn state_key(id: &str) -> String {
    format!("mcp:session:{id}")
}

/// The cache key of a session's owner.
fn owner_key(id: &str) -> String {
    format!("mcp:session:{id}:owner")
}

/// Which user each session belongs to: a session is used only by the user
/// who opened it. Kept in memory and, with a cache, in the cache, for
/// `session_ttl_secs` after the session's last request.
pub(crate) struct Sessions {
    owners: DashMap<String, (Uuid, Instant)>,
    cache: Option<Arc<Cache>>,
    ttl: Duration,
}

impl Sessions {
    pub(crate) fn new(cache: Option<Arc<Cache>>, ttl: Duration) -> Self {
        Self {
            owners: DashMap::new(),
            cache,
            ttl,
        }
    }

    /// The owner of session `id`, when it is known here or in the cache.
    pub(crate) async fn owner(&self, id: &str) -> Option<Uuid> {
        if let Some(entry) = self.owners.get(id) {
            return Some(entry.0);
        }
        let cache = self.cache.as_ref()?;
        let owner = cache.get::<Uuid>(&owner_key(id)).await.ok().flatten()?;
        self.owners.insert(id.to_owned(), (owner, Instant::now()));
        Some(owner)
    }

    /// Records `user` as the owner of the new session `id`.
    pub(crate) async fn claim(&self, id: &str, user: Uuid) {
        self.owners.insert(id.to_owned(), (user, Instant::now()));
        if let Some(cache) = &self.cache
            && let Err(error) = cache.put(&owner_key(id), &user, self.ttl).await
        {
            tracing::warn!(target: "navree::mcp", %error, "could not store the MCP session owner");
        }
    }

    /// Keeps the cached entries of session `id` alive: refreshed when half
    /// of their lifetime has passed.
    pub(crate) async fn touch(&self, id: &str) {
        let Some(cache) = &self.cache else {
            return;
        };
        let due = match self.owners.get_mut(id) {
            Some(mut entry) if entry.1.elapsed() >= self.ttl / 2 => {
                entry.1 = Instant::now();
                Some(entry.0)
            }
            _ => None,
        };
        let Some(owner) = due else {
            return;
        };
        let _ = cache.put(&owner_key(id), &owner, self.ttl).await;
        if let Ok(Some(state)) = cache.get::<SessionState>(&state_key(id)).await {
            let _ = cache.put(&state_key(id), &state, self.ttl).await;
        }
    }

    /// Forgets session `id` (it was closed).
    pub(crate) async fn forget(&self, id: &str) {
        self.owners.remove(id);
        if let Some(cache) = &self.cache {
            let _ = cache.forget(&owner_key(id)).await;
            let _ = cache.forget(&state_key(id)).await;
        }
    }
}

/// rmcp's session store on the app's cache: the client's `initialize`
/// parameters, so a process that did not open a session can restore it.
pub(crate) struct CacheSessionStore {
    cache: Arc<Cache>,
    ttl: Duration,
}

impl CacheSessionStore {
    pub(crate) fn new(cache: Arc<Cache>, ttl: Duration) -> Self {
        Self { cache, ttl }
    }
}

#[async_trait]
impl SessionStore for CacheSessionStore {
    async fn load(&self, session_id: &str) -> Result<Option<SessionState>, SessionStoreError> {
        Ok(self.cache.get(&state_key(session_id)).await?)
    }

    async fn store(&self, session_id: &str, state: &SessionState) -> Result<(), SessionStoreError> {
        Ok(self
            .cache
            .put(&state_key(session_id), state, self.ttl)
            .await?)
    }

    async fn delete(&self, session_id: &str) -> Result<(), SessionStoreError> {
        self.cache.forget(&state_key(session_id)).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn owners_survive_in_the_cache() {
        let cache = Arc::new(Cache::memory());
        let user = Uuid::now_v7();
        let first = Sessions::new(Some(cache.clone()), Duration::from_secs(60));
        first.claim("s1", user).await;
        assert_eq!(first.owner("s1").await, Some(user));

        // Another process with the same cache.
        let second = Sessions::new(Some(cache.clone()), Duration::from_secs(60));
        assert_eq!(second.owner("s1").await, Some(user));
        second.forget("s1").await;
        assert_eq!(
            Sessions::new(Some(cache), Duration::from_secs(60))
                .owner("s1")
                .await,
            None
        );

        let memory_only = Sessions::new(None, Duration::from_secs(60));
        assert_eq!(memory_only.owner("s2").await, None);
        memory_only.claim("s2", user).await;
        assert_eq!(memory_only.owner("s2").await, Some(user));
    }
}
