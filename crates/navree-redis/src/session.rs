//! [`RedisSessionStore`]: navree-session's records on Redis.

use std::fmt;
use std::sync::Arc;

use navree_contracts::{BoxError, BoxFuture, Clock, SessionRecord, SessionStore, SystemClock};
use redis::AsyncCommands;

use crate::RedisConnection;
use crate::cache::millis;

/// Session records on Redis (`session.store = "redis"`): each one a JSON
/// string under `<prefix>session:<id>` that expires with the session, so
/// Redis removes idle sessions itself. Every process using the server
/// shares the sessions.
///
/// Expiry follows the app's `dyn Clock` (the system clock unless a test
/// overrides it): a record past its `expires_at` is never loaded.
pub struct RedisSessionStore {
    connection: RedisConnection,
    namespace: String,
    clock: Arc<dyn Clock>,
}

impl RedisSessionStore {
    /// The store on `connection`, with records under `<prefix>session:`.
    pub fn new(connection: RedisConnection, prefix: &str) -> Self {
        Self {
            connection,
            namespace: format!("{prefix}session:"),
            clock: Arc::new(SystemClock),
        }
    }

    /// The same store, deciding what is expired with `clock`.
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// Where the record of session `id` is stored.
    pub fn redis_key(&self, id: &str) -> String {
        format!("{}{id}", self.namespace)
    }

    fn fail(&self, source: redis::RedisError) -> BoxError {
        Box::new(self.connection.error(source))
    }
}

impl fmt::Debug for RedisSessionStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedisSessionStore")
            .field("connection", &self.connection.name())
            .field("namespace", &self.namespace)
            .finish_non_exhaustive()
    }
}

impl SessionStore for RedisSessionStore {
    fn load(&self, id: &str) -> BoxFuture<'_, Result<Option<SessionRecord>, BoxError>> {
        let key = self.redis_key(id);
        Box::pin(async move {
            let stored: Option<Vec<u8>> = self
                .connection
                .manager()
                .get(&key)
                .await
                .map_err(|e| self.fail(e))?;
            let Some(stored) = stored else {
                return Ok(None);
            };
            let record: SessionRecord = serde_json::from_slice(&stored)
                .map_err(|error| format!("the session record under `{key}` is not valid: {error}"))?;
            if record
                .expires_at
                .is_some_and(|at| at <= self.clock.now())
            {
                let _: u64 = self
                    .connection
                    .manager()
                    .del(&key)
                    .await
                    .map_err(|e| self.fail(e))?;
                return Ok(None);
            }
            Ok(Some(record))
        })
    }

    fn save(&self, id: &str, record: SessionRecord) -> BoxFuture<'_, Result<(), BoxError>> {
        let key = self.redis_key(id);
        Box::pin(async move {
            let ttl = match record.expires_at {
                None => None,
                Some(at) => match (at - self.clock.now()).to_std() {
                    Ok(ttl) if !ttl.is_zero() => Some(ttl),
                    // Already expired: nothing to keep.
                    _ => {
                        let _: u64 = self
                            .connection
                            .manager()
                            .del(&key)
                            .await
                            .map_err(|e| self.fail(e))?;
                        return Ok(());
                    }
                },
            };
            let json = serde_json::to_vec(&record)?;
            let mut command = redis::cmd("SET");
            command.arg(&key).arg(json);
            if let Some(ttl) = ttl {
                command.arg("PX").arg(millis(ttl));
            }
            command
                .query_async::<()>(&mut self.connection.manager())
                .await
                .map_err(|e| self.fail(e))
        })
    }

    fn delete(&self, id: &str) -> BoxFuture<'_, Result<(), BoxError>> {
        let key = self.redis_key(id);
        Box::pin(async move {
            let _: u64 = self
                .connection
                .manager()
                .del(&key)
                .await
                .map_err(|e| self.fail(e))?;
            Ok(())
        })
    }
}
