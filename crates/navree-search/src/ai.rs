//! Vector search on navree-ai (crate feature `ai`): navree-ai's
//! [`Embeddings`](navree_ai::Embeddings) embed records and query texts,
//! and with `search.vectors.store = "ai"` its
//! [`VectorStore`](navree_ai::VectorStore) keeps the vectors, so an app
//! with AI and search has one embedding model, one store (memory, pgvector
//! or Qdrant), one size and one metric: those of `ai.vectors`.
//!
//! Each search index is the AI collection `search_<index>` (lower case),
//! next to the app's own collections. A record's filter values are the
//! collection record's metadata.

use std::collections::BTreeMap;
use std::fmt;

use navree_ai::{AiError, Collection, Embedding, VectorStore as AiStore};
use navree_kernel::{BoxError, BoxFuture};
use serde_json::Value;

use crate::SearchError;
use crate::filter::{CmpOp, Filter};
use crate::vector::{Embeddings, VectorMatch, VectorRecord, VectorStore};

/// The collection that keeps the vectors of search index `index`.
pub(crate) fn collection_name(index: &str) -> String {
    format!("search_{}", index.to_ascii_lowercase())
}

/// navree-ai's embeddings as search's provider: what an app with the `ai`
/// feature uses when it registers no `Arc<dyn Embeddings>` of its own.
impl Embeddings for navree_ai::Embeddings {
    fn embed<'a>(&'a self, texts: &'a [String]) -> BoxFuture<'a, Result<Vec<Vec<f32>>, BoxError>> {
        Box::pin(async move {
            let vectors = self.embed_many(texts).await?;
            Ok(vectors.into_iter().map(Embedding::into_vec).collect())
        })
    }
}

/// Search's [`VectorStore`] on navree-ai's: `search.vectors.store = "ai"`.
///
/// Scores are mapped into 0..1 as search's other stores give them: a
/// cosine similarity `s` scores `(s + 1) / 2`, a dot product `p` scores
/// `1 / (1 + e^-p)`, and a Euclidean distance keeps navree-ai's
/// `1 / (1 + d)`. A filter navree-ai cannot run as search does (a null
/// test, a null operand, a range on booleans, a field with a `.`) is run
/// on the closest records' metadata after a wider search.
#[derive(Clone)]
pub struct AiVectorStore {
    store: AiStore,
}

/// How many records a search fetches for each one asked for when part of
/// its filter runs on the results.
const WIDEN: u64 = 10;
/// The fewest records such a search fetches.
const WIDEN_MIN: u64 = 100;

impl AiVectorStore {
    /// Search's vectors in `store`.
    pub fn new(store: AiStore) -> Self {
        Self { store }
    }

    /// The navree-ai store.
    pub fn store(&self) -> &AiStore {
        &self.store
    }

    fn collection(&self, index: &str) -> Collection {
        self.store.collection(collection_name(index))
    }

    /// A similarity of navree-ai's in 0..1.
    fn score(&self, score: f32) -> f64 {
        let score = f64::from(score);
        match self.store.metric() {
            navree_ai::Metric::Cosine => ((score + 1.0) / 2.0).clamp(0.0, 1.0),
            navree_ai::Metric::Dot => 1.0 / (1.0 + (-score).exp()),
            navree_ai::Metric::Euclidean => score.clamp(0.0, 1.0),
        }
    }
}

impl fmt::Debug for AiVectorStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AiVectorStore")
            .field("store", &self.store)
            .finish()
    }
}

/// A navree-ai error as search's.
fn engine(error: AiError) -> SearchError {
    SearchError::Engine {
        engine: "ai",
        message: error.to_string(),
    }
}

/// Whether navree-ai's filters read `field` as search does: a top-level
/// key of letters, digits and `_`.
fn plain_field(field: &str) -> bool {
    !field.is_empty() && field.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Whether every navree-ai store compares `value` as search does: a
/// string, a boolean (for equality), an integer an `f64` holds exactly, or
/// a float in a range (Qdrant's `match` takes no floats, so equality on
/// one is left to the results).
fn plain_value(value: &Value, ordered: bool) -> bool {
    match value {
        Value::String(_) => true,
        Value::Bool(_) => !ordered,
        Value::Number(number) => match number.as_i64() {
            Some(n) => n.unsigned_abs() <= 1 << 53,
            None => ordered && number.is_f64(),
        },
        _ => false,
    }
}

/// `filter` (normalized) as navree-ai's filter, and whether that one is
/// the same filter (`false`: it lets through more records, and `filter`
/// runs on the results). `None` lets every record through.
pub(crate) fn translate(filter: &Filter) -> (Option<navree_ai::Filter>, bool) {
    match filter {
        Filter::Const(true) => (None, true),
        Filter::Const(false) => (None, false),
        Filter::All(items) => {
            let mut exact = true;
            let mut parts = Vec::new();
            for item in items {
                let (part, same) = translate(item);
                exact &= same;
                parts.extend(part);
            }
            let filter = match parts.len() {
                0 => None,
                1 => parts.pop(),
                _ => Some(navree_ai::Filter::And(parts)),
            };
            (filter, exact)
        }
        Filter::Any(items) => {
            let mut parts = Vec::new();
            for item in items {
                match translate(item) {
                    (Some(part), true) => parts.push(part),
                    // One side that lets everything through lets the whole
                    // `Any` through.
                    _ => return (None, false),
                }
            }
            match parts.len() {
                0 => (None, false),
                1 => (parts.pop(), true),
                _ => (Some(navree_ai::Filter::Or(parts)), true),
            }
        }
        Filter::Not(inner) => match translate(inner) {
            (Some(part), true) => (Some(navree_ai::Filter::not(part)), true),
            _ => (None, false),
        },
        Filter::Cmp { field, op, value } => {
            let ordered = !matches!(op, CmpOp::Eq | CmpOp::Ne);
            if !plain_field(field) || !plain_value(value, ordered) {
                return (None, false);
            }
            let field = field.clone();
            let value = value.clone();
            let filter = match op {
                CmpOp::Eq => navree_ai::Filter::Eq(field, value),
                CmpOp::Ne => navree_ai::Filter::not(navree_ai::Filter::Eq(field, value)),
                CmpOp::Lt => navree_ai::Filter::Lt(field, value),
                CmpOp::Lte => navree_ai::Filter::Lte(field, value),
                CmpOp::Gt => navree_ai::Filter::Gt(field, value),
                CmpOp::Gte => navree_ai::Filter::Gte(field, value),
            };
            (Some(filter), true)
        }
        Filter::In { field, values } => {
            if !plain_field(field) || !values.iter().all(|v| plain_value(v, false)) {
                return (None, false);
            }
            (
                Some(navree_ai::Filter::AnyOf(field.clone(), values.clone())),
                true,
            )
        }
        Filter::NotIn { field, values } => {
            if !plain_field(field) || !values.iter().all(|v| plain_value(v, false)) {
                return (None, false);
            }
            (
                Some(navree_ai::Filter::not(navree_ai::Filter::AnyOf(
                    field.clone(),
                    values.clone(),
                ))),
                true,
            )
        }
        Filter::Null(_) | Filter::NotNull(_) => (None, false),
    }
}

/// A record's filter values as navree-ai metadata.
fn metadata(payload: BTreeMap<String, Value>) -> Value {
    Value::Object(payload.into_iter().collect())
}

/// navree-ai metadata as a record's filter values.
fn payload(metadata: &Value) -> BTreeMap<String, Value> {
    metadata
        .as_object()
        .map(|object| {
            object
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        })
        .unwrap_or_default()
}

impl VectorStore for AiVectorStore {
    fn name(&self) -> &'static str {
        "ai"
    }

    fn filters_exactly(&self, filter: &Filter) -> bool {
        translate(filter).1
    }

    fn prepare<'a>(&'a self, index: &'a str) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            self.store
                .ensure_collection(&collection_name(index))
                .await
                .map_err(engine)
        })
    }

    fn upsert<'a>(
        &'a self,
        index: &'a str,
        records: Vec<VectorRecord>,
    ) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            let records = records
                .into_iter()
                .map(|record| {
                    navree_ai::VectorRecord::new(record.id, record.vector)
                        .metadata(metadata(record.payload))
                })
                .collect();
            self.collection(index)
                .upsert_many(records)
                .await
                .map_err(engine)
        })
    }

    fn delete<'a>(
        &'a self,
        index: &'a str,
        ids: &'a [String],
    ) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            self.collection(index)
                .delete_many(ids.iter().cloned())
                .await
                .map_err(engine)
        })
    }

    fn flush<'a>(&'a self, index: &'a str) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move { self.collection(index).clear().await.map_err(engine) })
    }

    fn get<'a>(
        &'a self,
        index: &'a str,
        id: &'a str,
    ) -> BoxFuture<'a, Result<Option<Vec<f32>>, SearchError>> {
        Box::pin(async move {
            Ok(self
                .collection(index)
                .get(id)
                .await
                .map_err(engine)?
                .map(|record| record.embedding.into_vec()))
        })
    }

    fn nearest<'a>(
        &'a self,
        index: &'a str,
        vector: &'a [f32],
        filter: &'a Filter,
        limit: u64,
    ) -> BoxFuture<'a, Result<Vec<VectorMatch>, SearchError>> {
        Box::pin(async move {
            if *filter == Filter::Const(false) || limit == 0 {
                return Ok(Vec::new());
            }
            let (translated, exact) = translate(filter);
            let fetch = if exact {
                limit
            } else {
                limit.saturating_mul(WIDEN).max(WIDEN_MIN)
            };
            let mut search = self
                .collection(index)
                .search(Embedding::new(vector.to_vec()))
                .limit(usize::try_from(fetch).unwrap_or(usize::MAX));
            if let Some(translated) = translated {
                search = search.filter(translated);
            }
            let found = search.await.map_err(engine)?;
            let mut matches: Vec<VectorMatch> = found
                .into_iter()
                .filter(|found| exact || filter.matches(&payload(&found.metadata)))
                .map(|found| VectorMatch {
                    score: self.score(found.score),
                    id: found.id,
                })
                .collect();
            matches.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
            matches.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
            Ok(matches)
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn ai(filter: Filter) -> (Option<navree_ai::Filter>, bool) {
        translate(&filter.normalized())
    }

    #[test]
    fn indexes_are_collections_of_their_own() {
        assert_eq!(collection_name("crm_contacts"), "search_crm_contacts");
        assert_eq!(collection_name("Deals"), "search_deals");
    }

    #[test]
    fn comparisons_translate_exactly() {
        assert_eq!(
            ai(Filter::eq("stage", "won")),
            (Some(navree_ai::Filter::eq("stage", "won")), true)
        );
        assert_eq!(
            ai(Filter::ne("stage", "won")),
            (
                Some(navree_ai::Filter::not(navree_ai::Filter::eq(
                    "stage", "won"
                ))),
                true
            )
        );
        assert_eq!(
            ai(Filter::is_in("owner", ["u1", "u2"])),
            (Some(navree_ai::Filter::any_of("owner", ["u1", "u2"])), true)
        );
        let (both, exact) = ai(Filter::All(vec![
            Filter::eq("stage", "won"),
            Filter::range("amount", 100..),
        ]));
        assert!(exact);
        assert_eq!(
            both,
            Some(navree_ai::Filter::And(vec![
                navree_ai::Filter::eq("stage", "won"),
                navree_ai::Filter::gte("amount", 100),
            ]))
        );
        assert_eq!(ai(Filter::Const(true)), (None, true));
    }

    #[test]
    fn what_navree_ai_cannot_run_is_left_to_the_results() {
        // A null test lets everything through and is not exact.
        assert_eq!(ai(Filter::null("closed_at")), (None, false));
        // In an `All`, the other parts still narrow the search.
        let (narrowed, exact) = ai(Filter::All(vec![
            Filter::eq("stage", "won"),
            Filter::null("closed_at"),
        ]));
        assert_eq!(narrowed, Some(navree_ai::Filter::eq("stage", "won")));
        assert!(!exact);
        // In an `Any`, nothing can be narrowed.
        assert_eq!(
            ai(Filter::Any(vec![
                Filter::eq("stage", "won"),
                Filter::null("closed_at"),
            ])),
            (None, false)
        );
        // Ranges on booleans, dotted fields and huge integers.
        assert!(!ai(Filter::range("flag", true..)).1);
        assert!(!ai(Filter::eq("owner.id", "u1")).1);
        assert!(!ai(Filter::eq("big", 9_007_199_254_740_993_i64)).1);
        assert!(ai(Filter::eq("flag", true)).1);
        // Floats: in ranges, not in equality.
        assert!(ai(Filter::range("score", 1.5..)).1);
        assert!(!ai(Filter::eq("score", 2.25)).1);
        assert!(!ai(Filter::is_in("score", [1.5])).1);
    }

    #[test]
    fn filter_values_are_metadata() {
        let values = BTreeMap::from([
            ("stage".to_owned(), json!("won")),
            ("_workspace".to_owned(), json!("w1")),
        ]);
        let metadata = metadata(values.clone());
        assert_eq!(metadata, json!({ "stage": "won", "_workspace": "w1" }));
        assert_eq!(payload(&metadata), values);
        assert!(payload(&json!(null)).is_empty());
    }
}
