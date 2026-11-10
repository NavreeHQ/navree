//! The `memory` vector store: collections in this process's memory,
//! searched exhaustively.

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use navree_kernel::BoxFuture;

use super::{Filter, VectorDriver, VectorMatch, VectorRecord, score};
use crate::error::AiError;
use crate::options::Metric;

/// Collections in memory.
pub(crate) struct MemoryStore {
    metric: Metric,
    collections: Mutex<HashMap<String, BTreeMap<String, VectorRecord>>>,
}

impl MemoryStore {
    pub(crate) fn new(metric: Metric) -> Self {
        Self {
            metric,
            collections: Mutex::new(HashMap::new()),
        }
    }

    fn with<T>(
        &self,
        collection: &str,
        f: impl FnOnce(&mut BTreeMap<String, VectorRecord>) -> T,
    ) -> T {
        let mut collections = self
            .collections
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        f(collections.entry(collection.to_owned()).or_default())
    }
}

impl VectorDriver for MemoryStore {
    fn ensure<'a>(&'a self, collection: &'a str) -> BoxFuture<'a, Result<(), AiError>> {
        self.with(collection, |_| ());
        Box::pin(async { Ok(()) })
    }

    fn upsert<'a>(
        &'a self,
        collection: &'a str,
        records: Vec<VectorRecord>,
    ) -> BoxFuture<'a, Result<(), AiError>> {
        self.with(collection, |map| {
            for record in records {
                map.insert(record.id.clone(), record);
            }
        });
        Box::pin(async { Ok(()) })
    }

    fn delete<'a>(
        &'a self,
        collection: &'a str,
        ids: Vec<String>,
    ) -> BoxFuture<'a, Result<(), AiError>> {
        self.with(collection, |map| {
            for id in ids {
                map.remove(&id);
            }
        });
        Box::pin(async { Ok(()) })
    }

    fn delete_where<'a>(
        &'a self,
        collection: &'a str,
        filter: &'a Filter,
    ) -> BoxFuture<'a, Result<(), AiError>> {
        self.with(collection, |map| {
            map.retain(|_, record| !filter.matches(&record.metadata))
        });
        Box::pin(async { Ok(()) })
    }

    fn search<'a>(
        &'a self,
        collection: &'a str,
        vector: &'a [f32],
        filter: Option<&'a Filter>,
        limit: usize,
        min_score: Option<f32>,
    ) -> BoxFuture<'a, Result<Vec<VectorMatch>, AiError>> {
        let metric = self.metric;
        let mut matches: Vec<VectorMatch> = self.with(collection, |map| {
            map.values()
                .filter(|record| filter.is_none_or(|f| f.matches(&record.metadata)))
                .map(|record| VectorMatch {
                    id: record.id.clone(),
                    score: score(metric, vector, &record.embedding),
                    text: record.text.clone(),
                    metadata: record.metadata.clone(),
                })
                .filter(|m| min_score.is_none_or(|min| m.score >= min))
                .collect()
        });
        matches.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
        matches.truncate(limit);
        Box::pin(async move { Ok(matches) })
    }

    fn count<'a>(&'a self, collection: &'a str) -> BoxFuture<'a, Result<u64, AiError>> {
        let count = self.with(collection, |map| map.len() as u64);
        Box::pin(async move { Ok(count) })
    }

    fn get<'a>(
        &'a self,
        collection: &'a str,
        id: &'a str,
    ) -> BoxFuture<'a, Result<Option<VectorRecord>, AiError>> {
        let record = self.with(collection, |map| map.get(id).cloned());
        Box::pin(async move { Ok(record) })
    }

    fn clear<'a>(&'a self, collection: &'a str) -> BoxFuture<'a, Result<(), AiError>> {
        self.with(collection, |map| map.clear());
        Box::pin(async { Ok(()) })
    }
}
