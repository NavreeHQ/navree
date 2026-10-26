//! [`VectorStore`]: collections of vectors with their text and metadata,
//! searched by meaning, in memory, pgvector or Qdrant.

use std::collections::HashSet;
use std::fmt;
use std::future::{Future, IntoFuture};
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use navree_kernel::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::embeddings::{Embedding, Embeddings};
use crate::error::AiError;
use crate::options::{Metric, VectorStoreKind, VectorsOptions};

pub(crate) mod memory;
pub(crate) mod pgvector;
pub(crate) mod qdrant;

/// A collection declaration, for `m.vector_collection(..)`
/// ([`ModuleBuilderAiExt`](crate::ModuleBuilderAiExt)): with pgvector or
/// Qdrant, the collection is created at boot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VectorCollection {
    name: String,
}

impl VectorCollection {
    /// A collection named `name` (lower-case letters, digits and `_`).
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }

    /// Its name.
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// One record: an id (your key), its vector, the text it came from and
/// metadata to filter on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VectorRecord {
    /// Your key for the record (`<activity id>:<chunk>`).
    pub id: String,
    /// The vector.
    pub embedding: Embedding,
    /// The passage the vector was computed from.
    pub text: String,
    /// A JSON object to filter on.
    pub metadata: Value,
}

impl VectorRecord {
    /// A record with no text and empty metadata.
    pub fn new(id: impl Into<String>, embedding: impl Into<Embedding>) -> Self {
        Self {
            id: id.into(),
            embedding: embedding.into(),
            text: String::new(),
            metadata: json!({}),
        }
    }

    /// Sets the text.
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.text = text.into();
        self
    }

    /// Sets the metadata (a JSON object).
    pub fn metadata(mut self, metadata: Value) -> Self {
        self.metadata = metadata;
        self
    }
}

/// A search result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VectorMatch {
    /// The record's id.
    pub id: String,
    /// How close it is: higher is closer, whatever the metric (a
    /// reranker's score from 0 to 1 after [`Ai::rerank`](crate::Ai::rerank)).
    pub score: f32,
    /// The record's text.
    pub text: String,
    /// The record's metadata.
    pub metadata: Value,
}

/// A metadata filter, applied inside the vector search.
///
/// ```
/// use navree_ai::Filter;
///
/// let filter = Filter::eq("kind", "call")
///     .and(Filter::gte("created_at", "2026-10-01"))
///     .and(Filter::not(Filter::any_of("owner_id", ["a", "b"])));
/// ```
#[derive(Clone, Debug, PartialEq)]
pub enum Filter {
    /// `field == value`.
    Eq(String, Value),
    /// `field` equals one of the values.
    AnyOf(String, Vec<Value>),
    /// `field > value` (numbers, or strings such as RFC 3339 dates).
    Gt(String, Value),
    /// `field >= value`.
    Gte(String, Value),
    /// `field < value`.
    Lt(String, Value),
    /// `field <= value`.
    Lte(String, Value),
    /// Not the filter.
    Not(Box<Filter>),
    /// Every filter.
    And(Vec<Filter>),
    /// Any filter.
    Or(Vec<Filter>),
}

fn to_value(value: impl Serialize) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

impl Filter {
    /// `field == value` (a dotted path reaches nested fields).
    pub fn eq(field: impl Into<String>, value: impl Serialize) -> Self {
        Filter::Eq(field.into(), to_value(value))
    }

    /// `field` equals one of `values`.
    pub fn any_of<I, V>(field: impl Into<String>, values: I) -> Self
    where
        I: IntoIterator<Item = V>,
        V: Serialize,
    {
        Filter::AnyOf(field.into(), values.into_iter().map(to_value).collect())
    }

    /// `field > value`.
    pub fn gt(field: impl Into<String>, value: impl Serialize) -> Self {
        Filter::Gt(field.into(), to_value(value))
    }

    /// `field >= value`.
    pub fn gte(field: impl Into<String>, value: impl Serialize) -> Self {
        Filter::Gte(field.into(), to_value(value))
    }

    /// `field < value`.
    pub fn lt(field: impl Into<String>, value: impl Serialize) -> Self {
        Filter::Lt(field.into(), to_value(value))
    }

    /// `field <= value`.
    pub fn lte(field: impl Into<String>, value: impl Serialize) -> Self {
        Filter::Lte(field.into(), to_value(value))
    }

    /// Not `filter`.
    #[allow(clippy::should_implement_trait)]
    pub fn not(filter: Filter) -> Self {
        Filter::Not(Box::new(filter))
    }

    /// This filter and `other`.
    pub fn and(self, other: Filter) -> Self {
        match self {
            Filter::And(mut filters) => {
                filters.push(other);
                Filter::And(filters)
            }
            this => Filter::And(vec![this, other]),
        }
    }

    /// This filter or `other`.
    pub fn or(self, other: Filter) -> Self {
        match self {
            Filter::Or(mut filters) => {
                filters.push(other);
                Filter::Or(filters)
            }
            this => Filter::Or(vec![this, other]),
        }
    }

    /// Whether `metadata` passes.
    pub fn matches(&self, metadata: &Value) -> bool {
        use std::cmp::Ordering;
        let compare = |field: &str, value: &Value, accept: fn(Ordering) -> bool| {
            lookup(metadata, field)
                .and_then(|found| order(found, value))
                .is_some_and(accept)
        };
        match self {
            Filter::Eq(field, value) => {
                lookup(metadata, field).is_some_and(|found| same(found, value))
            }
            Filter::AnyOf(field, values) => lookup(metadata, field)
                .is_some_and(|found| values.iter().any(|value| same(found, value))),
            Filter::Gt(field, value) => compare(field, value, Ordering::is_gt),
            Filter::Gte(field, value) => compare(field, value, Ordering::is_ge),
            Filter::Lt(field, value) => compare(field, value, Ordering::is_lt),
            Filter::Lte(field, value) => compare(field, value, Ordering::is_le),
            Filter::Not(filter) => !filter.matches(metadata),
            Filter::And(filters) => filters.iter().all(|f| f.matches(metadata)),
            Filter::Or(filters) => filters.iter().any(|f| f.matches(metadata)),
        }
    }

    /// Every field the filter names.
    pub(crate) fn fields(&self) -> Vec<&str> {
        match self {
            Filter::Eq(f, _)
            | Filter::AnyOf(f, _)
            | Filter::Gt(f, _)
            | Filter::Gte(f, _)
            | Filter::Lt(f, _)
            | Filter::Lte(f, _) => vec![f.as_str()],
            Filter::Not(filter) => filter.fields(),
            Filter::And(filters) | Filter::Or(filters) => {
                filters.iter().flat_map(Filter::fields).collect()
            }
        }
    }

    /// Fails for a field that is not a dotted path of letters, digits and
    /// `_` (stores put field names into their queries).
    pub(crate) fn check_fields(&self) -> Result<(), AiError> {
        for field in self.fields() {
            let valid = !field.is_empty()
                && field.split('.').all(|part| {
                    !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                });
            if !valid {
                return Err(AiError::config(format!(
                    "invalid filter field `{field}` (use letters, digits, `_` and `.`)"
                )));
            }
        }
        Ok(())
    }
}

/// The value at dotted `path` of `metadata`.
fn lookup<'a>(metadata: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.')
        .try_fold(metadata, |value, key| value.get(key))
}

/// JSON equality, numbers compared as numbers.
fn same(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) if a.is_number() && b.is_number() => x == y,
        _ => a == b,
    }
}

/// The order of two numbers or two strings.
fn order(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64()?.partial_cmp(&y.as_f64()?),
        (Value::String(x), Value::String(y)) => Some(x.cmp(y)),
        _ => None,
    }
}

/// What a search looks for: a text (embedded with
/// [`Embeddings`], cached) or a vector.
#[derive(Clone, Debug, PartialEq)]
pub enum SearchQuery {
    /// A text to embed.
    Text(String),
    /// A vector.
    Embedding(Embedding),
}

impl From<&str> for SearchQuery {
    fn from(text: &str) -> Self {
        SearchQuery::Text(text.to_owned())
    }
}

impl From<String> for SearchQuery {
    fn from(text: String) -> Self {
        SearchQuery::Text(text)
    }
}

impl From<&String> for SearchQuery {
    fn from(text: &String) -> Self {
        SearchQuery::Text(text.clone())
    }
}

impl From<Embedding> for SearchQuery {
    fn from(embedding: Embedding) -> Self {
        SearchQuery::Embedding(embedding)
    }
}

/// A store's operations on named collections.
pub(crate) trait VectorDriver: Send + Sync {
    /// Creates the collection when it is missing; fails when it exists with
    /// another size.
    fn ensure<'a>(&'a self, collection: &'a str) -> BoxFuture<'a, Result<(), AiError>>;
    fn upsert<'a>(
        &'a self,
        collection: &'a str,
        records: Vec<VectorRecord>,
    ) -> BoxFuture<'a, Result<(), AiError>>;
    fn delete<'a>(
        &'a self,
        collection: &'a str,
        ids: Vec<String>,
    ) -> BoxFuture<'a, Result<(), AiError>>;
    fn delete_where<'a>(
        &'a self,
        collection: &'a str,
        filter: &'a Filter,
    ) -> BoxFuture<'a, Result<(), AiError>>;
    fn search<'a>(
        &'a self,
        collection: &'a str,
        vector: &'a [f32],
        filter: Option<&'a Filter>,
        limit: usize,
        min_score: Option<f32>,
    ) -> BoxFuture<'a, Result<Vec<VectorMatch>, AiError>>;
    fn count<'a>(&'a self, collection: &'a str) -> BoxFuture<'a, Result<u64, AiError>>;
}

/// The shared state of a [`VectorStore`].
struct StoreInner {
    driver: Arc<dyn VectorDriver>,
    embeddings: Embeddings,
    dimensions: usize,
    kind: VectorStoreKind,
    ensured: Mutex<HashSet<String>>,
}

/// Vectors in named collections. Inject `Arc<VectorStore>` and open one
/// with [`collection`](Self::collection).
///
/// ```
/// use navree_ai::{AiFake, Embeddings, Filter, VectorRecord, VectorStore};
/// use serde_json::json;
///
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() -> Result<(), navree_ai::AiError> {
/// let embeddings = Embeddings::new(AiFake::new().ai());
/// let vectors = VectorStore::memory(embeddings.clone());
/// let notes = vectors.collection("activity_notes");
///
/// let text = "Pricing call with Dana. She asked for a 12-month term.";
/// notes.upsert(
///     VectorRecord::new("a1:0", embeddings.embed(text).await?)
///         .text(text)
///         .metadata(json!({ "subject_id": "deal-1" })),
/// ).await?;
///
/// let matches = notes.search("what term did Dana ask for").filter(Filter::eq("subject_id", "deal-1")).limit(8).await?;
/// assert_eq!(matches[0].id, "a1:0");
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct VectorStore {
    inner: Arc<StoreInner>,
}

impl VectorStore {
    /// A store in this process's memory, searched exhaustively, with the
    /// sizes and metric of the embeddings' `ai.vectors` options.
    pub fn memory(embeddings: Embeddings) -> Self {
        let metric = embeddings_options(&embeddings).metric;
        Self::with_driver(
            Arc::new(memory::MemoryStore::new(metric)),
            embeddings,
            VectorStoreKind::Memory,
        )
    }

    pub(crate) fn with_driver(
        driver: Arc<dyn VectorDriver>,
        embeddings: Embeddings,
        kind: VectorStoreKind,
    ) -> Self {
        let dimensions = embeddings.dimensions();
        Self {
            inner: Arc::new(StoreInner {
                driver,
                embeddings,
                dimensions,
                kind,
                ensured: Mutex::new(HashSet::new()),
            }),
        }
    }

    /// The store configured by `options`: memory, pgvector on `db`, or
    /// Qdrant over HTTP.
    pub(crate) fn from_options(
        options: &VectorsOptions,
        embeddings: Embeddings,
        db: Option<Arc<navree_db::Db>>,
        http: navree_http_client::HttpClient,
    ) -> Result<Self, AiError> {
        let driver: Arc<dyn VectorDriver> = match options.store {
            VectorStoreKind::Memory => Arc::new(memory::MemoryStore::new(options.metric)),
            VectorStoreKind::Pgvector => {
                let db = db.ok_or_else(|| {
                    AiError::config(
                        "ai.vectors.store = \"pgvector\" needs DatabaseModule in the app",
                    )
                })?;
                Arc::new(pgvector::PgVector::new(
                    db,
                    options.dimensions,
                    options.metric,
                ))
            }
            VectorStoreKind::Qdrant => Arc::new(qdrant::Qdrant::new(
                options
                    .url
                    .clone()
                    .ok_or_else(|| AiError::config("ai.vectors.url is required for Qdrant"))?,
                options.key.clone(),
                options.dimensions,
                options.metric,
                http,
            )),
        };
        Ok(Self::with_driver(driver, embeddings, options.store))
    }

    /// The collection `name`.
    pub fn collection(&self, name: impl Into<String>) -> Collection {
        Collection {
            store: self.inner.clone(),
            name: name.into(),
        }
    }

    /// The store kind.
    pub fn kind(&self) -> VectorStoreKind {
        self.inner.kind
    }

    /// Creates `name` when it is missing (done on first use; at boot for
    /// declared collections).
    pub async fn ensure_collection(&self, name: &str) -> Result<(), AiError> {
        self.collection(name).ensure().await
    }
}

fn embeddings_options(embeddings: &Embeddings) -> VectorsOptions {
    VectorsOptions {
        dimensions: embeddings.dimensions(),
        metric: embeddings.metric(),
        ..VectorsOptions::default()
    }
}

impl fmt::Debug for VectorStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VectorStore")
            .field("kind", &self.inner.kind)
            .field("dimensions", &self.inner.dimensions)
            .finish_non_exhaustive()
    }
}

/// One collection of a [`VectorStore`].
#[derive(Clone)]
pub struct Collection {
    store: Arc<StoreInner>,
    name: String,
}

impl Collection {
    /// Its name.
    pub fn name(&self) -> &str {
        &self.name
    }

    async fn ensure(&self) -> Result<(), AiError> {
        check_name(&self.name)?;
        if lock(&self.store.ensured).contains(&self.name) {
            return Ok(());
        }
        self.store.driver.ensure(&self.name).await?;
        lock(&self.store.ensured).insert(self.name.clone());
        Ok(())
    }

    /// Inserts `record`, or replaces the record with its id.
    pub async fn upsert(&self, record: VectorRecord) -> Result<(), AiError> {
        self.upsert_many(vec![record]).await
    }

    /// Inserts or replaces each record by id.
    pub async fn upsert_many(&self, records: Vec<VectorRecord>) -> Result<(), AiError> {
        if records.is_empty() {
            return Ok(());
        }
        for record in &records {
            if record.embedding.dimensions() != self.store.dimensions {
                return Err(AiError::config(format!(
                    "record `{}` has {} dimensions; collection `{}` takes {} (ai.vectors.dimensions)",
                    record.id,
                    record.embedding.dimensions(),
                    self.name,
                    self.store.dimensions
                )));
            }
            if !record.metadata.is_object() {
                return Err(AiError::config(format!(
                    "record `{}`: metadata must be a JSON object",
                    record.id
                )));
            }
        }
        self.ensure().await?;
        self.store.driver.upsert(&self.name, records).await
    }

    /// Removes the record `id`.
    pub async fn delete(&self, id: impl Into<String>) -> Result<(), AiError> {
        self.ensure().await?;
        self.store.driver.delete(&self.name, vec![id.into()]).await
    }

    /// Removes every record that matches `filter` (all chunks of one
    /// activity, say).
    pub async fn delete_where(&self, filter: Filter) -> Result<(), AiError> {
        filter.check_fields()?;
        self.ensure().await?;
        self.store.driver.delete_where(&self.name, &filter).await
    }

    /// A search for the records closest to `query` (a text, embedded, or
    /// an [`Embedding`]): `.filter(f)`, `.limit(n)` (default 10),
    /// `.min_score(s)`, then await. Best first.
    pub fn search(&self, query: impl Into<SearchQuery>) -> SearchRequest {
        SearchRequest {
            collection: self.clone(),
            query: query.into(),
            filter: None,
            limit: 10,
            min_score: None,
        }
    }

    /// The number of records.
    pub async fn count(&self) -> Result<u64, AiError> {
        self.ensure().await?;
        self.store.driver.count(&self.name).await
    }
}

impl fmt::Debug for Collection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Collection")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// A search being built ([`Collection::search`]); awaiting it returns the
/// matches, best first.
#[must_use = "a search does nothing until it is awaited"]
pub struct SearchRequest {
    collection: Collection,
    query: SearchQuery,
    filter: Option<Filter>,
    limit: usize,
    min_score: Option<f32>,
}

impl SearchRequest {
    /// Only records whose metadata matches `filter` (applied inside the
    /// search, so `limit(8)` returns eight matching records).
    pub fn filter(mut self, filter: Filter) -> Self {
        self.filter = Some(match self.filter.take() {
            Some(existing) => existing.and(filter),
            None => filter,
        });
        self
    }

    /// At most `limit` matches (default 10).
    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }

    /// Only matches scoring at least `score`.
    pub fn min_score(mut self, score: f32) -> Self {
        self.min_score = Some(score);
        self
    }
}

impl IntoFuture for SearchRequest {
    type Output = Result<Vec<VectorMatch>, AiError>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            if let Some(filter) = &self.filter {
                filter.check_fields()?;
            }
            let collection = self.collection;
            let vector = match self.query {
                SearchQuery::Embedding(embedding) => embedding,
                SearchQuery::Text(text) => collection.store.embeddings.embed(&text).await?,
            };
            if vector.dimensions() != collection.store.dimensions {
                return Err(AiError::config(format!(
                    "the query vector has {} dimensions; collection `{}` takes {}",
                    vector.dimensions(),
                    collection.name,
                    collection.store.dimensions
                )));
            }
            if self.limit == 0 {
                return Ok(Vec::new());
            }
            collection.ensure().await?;
            collection
                .store
                .driver
                .search(
                    &collection.name,
                    &vector,
                    self.filter.as_ref(),
                    self.limit,
                    self.min_score,
                )
                .await
        })
    }
}

/// Collection names are lower-case letters, digits and `_` (they become
/// table and collection names).
fn check_name(name: &str) -> Result<(), AiError> {
    let valid = !name.is_empty()
        && name.len() <= 48
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if valid {
        Ok(())
    } else {
        Err(AiError::config(format!(
            "invalid vector collection name `{name}`: use lower-case letters, digits and `_` (at most 48)"
        )))
    }
}

/// The score of a distance or similarity under `metric`: higher is closer.
pub(crate) fn score(metric: Metric, a: &[f32], b: &[f32]) -> f32 {
    match metric {
        Metric::Cosine => crate::embeddings::cosine(a, b),
        Metric::Dot => a.iter().zip(b).map(|(x, y)| x * y).sum(),
        Metric::Euclidean => {
            let distance = a
                .iter()
                .zip(b)
                .map(|(x, y)| (x - y) * (x - y))
                .sum::<f32>()
                .sqrt();
            1.0 / (1.0 + distance)
        }
    }
}

/// `{"a": {"b": value}}` for path `a.b`.
pub(crate) fn nested(path: &str, value: Value) -> Value {
    path.rsplit('.').fold(value, |inner, key| {
        let mut map = Map::new();
        map.insert(key.to_owned(), inner);
        Value::Object(map)
    })
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_match_metadata() {
        let metadata = json!({
            "kind": "call", "subject_id": "d1", "count": 3,
            "created_at": "2026-10-09T10:00:00Z", "owner": { "id": "u1" }
        });
        assert!(Filter::eq("kind", "call").matches(&metadata));
        assert!(Filter::eq("count", 3.0).matches(&metadata));
        assert!(!Filter::eq("kind", "note").matches(&metadata));
        assert!(Filter::any_of("subject_id", ["d0", "d1"]).matches(&metadata));
        assert!(Filter::gte("created_at", "2026-10-01").matches(&metadata));
        assert!(!Filter::lt("count", 3).matches(&metadata));
        assert!(Filter::lte("count", 3).matches(&metadata));
        assert!(Filter::gt("count", 2).matches(&metadata));
        assert!(Filter::eq("owner.id", "u1").matches(&metadata));
        assert!(Filter::not(Filter::eq("kind", "note")).matches(&metadata));
        assert!(
            Filter::eq("kind", "note")
                .or(Filter::eq("count", 3))
                .matches(&metadata)
        );
        assert!(
            !Filter::eq("kind", "call")
                .and(Filter::eq("count", 4))
                .matches(&metadata)
        );
        assert!(!Filter::gt("missing", 1).matches(&metadata));
        // Mixed types never compare.
        assert!(!Filter::gt("kind", 1).matches(&metadata));
    }

    #[test]
    fn filter_fields_are_checked() {
        assert!(Filter::eq("owner.id", 1).check_fields().is_ok());
        assert!(Filter::eq("a'; drop", 1).check_fields().is_err());
        assert!(Filter::not(Filter::eq("", 1)).check_fields().is_err());
    }

    #[test]
    fn collection_names_are_checked() {
        assert!(check_name("activity_notes").is_ok());
        assert!(check_name("Activity").is_err());
        assert!(check_name("a-b").is_err());
        assert!(check_name("").is_err());
    }

    #[test]
    fn scores_are_higher_when_closer() {
        let a = [1.0, 0.0];
        let near = [0.9, 0.1];
        let far = [-1.0, 0.0];
        for metric in [Metric::Cosine, Metric::Dot, Metric::Euclidean] {
            assert!(
                score(metric, &a, &near) > score(metric, &a, &far),
                "{metric:?}"
            );
        }
        assert_eq!(nested("a.b", json!(1)), json!({ "a": { "b": 1 } }));
    }
}
