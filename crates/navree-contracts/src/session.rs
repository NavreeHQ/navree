//! [`SessionStore`]: where session data lives.

use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{BoxError, BoxFuture};

/// Loads and saves session records by id (memory, database, ...).
pub trait SessionStore: Send + Sync + 'static {
    /// The record for `id`, if it exists.
    fn load(&self, id: &str) -> BoxFuture<'_, Result<Option<SessionRecord>, BoxError>>;

    /// Creates or replaces the record for `id`.
    fn save(&self, id: &str, record: SessionRecord) -> BoxFuture<'_, Result<(), BoxError>>;

    /// Deletes the record for `id`.
    fn delete(&self, id: &str) -> BoxFuture<'_, Result<(), BoxError>>;
}

/// The stored data of one session.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct SessionRecord {
    /// Session values by key.
    pub data: Map<String, Value>,
    /// When the session expires.
    pub expires_at: Option<DateTime<Utc>>,
}

/// Session values can hold tokens (CSRF, remember-me), so `Debug` lists keys only.
impl fmt::Debug for SessionRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionRecord")
            .field("keys", &self.data.keys().collect::<Vec<_>>())
            .field("expires_at", &self.expires_at)
            .finish()
    }
}
