//! [`MemoryStore`]: session records in the process.

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use navis_contracts::{BoxError, BoxFuture, Clock, SessionRecord, SessionStore, SystemClock};

use crate::PRUNE_EVERY;

/// Keeps session records in a map in the process: fast, but lost on
/// restart and not shared between instances (use
/// [`DatabaseStore`](crate::DatabaseStore) for those).
///
/// Expired records are never loaded, and are removed on every hundredth
/// save or by [`prune_expired`](MemoryStore::prune_expired).
pub struct MemoryStore {
    records: Mutex<HashMap<String, SessionRecord>>,
    clock: Arc<dyn Clock>,
    saves: AtomicU64,
}

impl Default for MemoryStore {
    fn default() -> Self {
        MemoryStore {
            records: Mutex::default(),
            clock: Arc::new(SystemClock),
            saves: AtomicU64::new(0),
        }
    }
}

impl MemoryStore {
    /// An empty store.
    pub fn new() -> MemoryStore {
        MemoryStore::default()
    }

    /// The same store, deciding what is expired with `clock`.
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> MemoryStore {
        self.clock = clock;
        self
    }

    /// Removes every expired record; returns how many.
    pub fn prune_expired(&self) -> usize {
        let now = self.clock.now();
        let mut records = self.records();
        let before = records.len();
        records.retain(|_, record| record.expires_at.is_none_or(|at| at > now));
        before - records.len()
    }

    /// The number of records (expired ones included until pruned).
    pub fn len(&self) -> usize {
        self.records().len()
    }

    /// Whether the store holds no record.
    pub fn is_empty(&self) -> bool {
        self.records().is_empty()
    }

    fn records(&self) -> MutexGuard<'_, HashMap<String, SessionRecord>> {
        self.records.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl SessionStore for MemoryStore {
    fn load(&self, id: &str) -> BoxFuture<'_, Result<Option<SessionRecord>, BoxError>> {
        let now = self.clock.now();
        let mut records = self.records();
        let record = match records.get(id) {
            Some(record) if record.expires_at.is_some_and(|at| at <= now) => {
                records.remove(id);
                None
            }
            record => record.cloned(),
        };
        Box::pin(async move { Ok(record) })
    }

    fn save(&self, id: &str, record: SessionRecord) -> BoxFuture<'_, Result<(), BoxError>> {
        self.records().insert(id.to_owned(), record);
        if self.saves.fetch_add(1, Ordering::Relaxed) % PRUNE_EVERY == PRUNE_EVERY - 1 {
            self.prune_expired();
        }
        Box::pin(async { Ok(()) })
    }

    fn delete(&self, id: &str) -> BoxFuture<'_, Result<(), BoxError>> {
        self.records().remove(id);
        Box::pin(async { Ok(()) })
    }
}

/// Shows the number of records only.
impl fmt::Debug for MemoryStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MemoryStore")
            .field("records", &self.records().len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use chrono::{TimeDelta, Utc};
    use navis_contracts::FixedClock;
    use serde_json::json;

    use super::*;

    fn record(expires_at: Option<chrono::DateTime<Utc>>) -> SessionRecord {
        let mut record = SessionRecord {
            expires_at,
            ..SessionRecord::default()
        };
        record.data.insert("k".into(), json!(1));
        record
    }

    #[tokio::test]
    async fn save_load_delete() {
        let store = MemoryStore::new();
        assert!(store.load("a").await.unwrap().is_none());
        store.save("a", record(None)).await.unwrap();
        assert_eq!(store.load("a").await.unwrap().unwrap().data["k"], json!(1));
        store.delete("a").await.unwrap();
        assert!(store.load("a").await.unwrap().is_none());
        store.delete("a").await.unwrap();
        assert_eq!(format!("{store:?}"), "MemoryStore { records: 0 }");
    }

    #[tokio::test]
    async fn expired_records_are_pruned() {
        let now = Utc::now();
        let clock = Arc::new(FixedClock::new(now));
        let store = MemoryStore::new().with_clock(clock.clone());
        store
            .save("short", record(Some(now + TimeDelta::minutes(1))))
            .await
            .unwrap();
        store
            .save("long", record(Some(now + TimeDelta::minutes(10))))
            .await
            .unwrap();
        store.save("forever", record(None)).await.unwrap();
        assert_eq!(store.prune_expired(), 0);

        clock.advance(Duration::from_secs(120));
        assert!(
            store.load("short").await.unwrap().is_none(),
            "an expired record is not loaded"
        );
        assert_eq!(store.prune_expired(), 0, "loading removed it already");
        clock.advance(Duration::from_secs(600));
        assert_eq!(store.prune_expired(), 1);
        assert!(store.load("forever").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn saving_prunes_now_and_then() {
        let now = Utc::now();
        let clock = Arc::new(FixedClock::new(now));
        let store = MemoryStore::new().with_clock(clock.clone());
        for i in 0..10 {
            store
                .save(
                    &format!("old{i}"),
                    record(Some(now + TimeDelta::seconds(1))),
                )
                .await
                .unwrap();
        }
        clock.advance(Duration::from_secs(5));
        for i in 0..PRUNE_EVERY {
            store.save(&format!("new{i}"), record(None)).await.unwrap();
        }
        assert_eq!(store.len(), usize::try_from(PRUNE_EVERY).unwrap());
    }
}
