//! Vector search: the [`Embeddings`] provider trait, the [`VectorStore`]
//! driver trait and its `memory`, `qdrant` and `pgvector` stores.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::time::Duration;

use http::Method;
use navree_db::Db;
use navree_http_client::HttpClient;
use navree_kernel::{AppContext, BoxError, BoxFuture};
use sea_orm::{ConnectionTrait, DbBackend, TransactionTrait};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::engine::http::{Api, Auth, Body, encode};
use crate::filter::{CmpOp, Filter};
use crate::options::{Metric, VectorOptions, VectorStoreKind};
use crate::settings::WORKSPACE_FIELD;
use crate::sql::Sql;
use crate::{SearchError, SearchOptions};

/// A record to embed: its id, its workspace, its filter payload and the
/// text to embed.
pub(crate) type EmbedRecord = (String, Option<String>, BTreeMap<String, Value>, String);

/// Turns texts into vectors. Register one in the app as
/// `Arc<dyn Embeddings>` (for example a wrapper around an embedding model's
/// API); vector search uses it to embed records and query texts.
pub trait Embeddings: Send + Sync + 'static {
    /// One vector per text, in order, each of `search.vectors.dimensions`
    /// numbers.
    fn embed<'a>(&'a self, texts: &'a [String]) -> BoxFuture<'a, Result<Vec<Vec<f32>>, BoxError>>;
}

/// One record's vector, with the filter values vector queries filter on.
#[derive(Clone, Debug, PartialEq)]
pub struct VectorRecord {
    /// The record's primary key.
    pub id: String,
    /// The vector.
    pub vector: Vec<f32>,
    /// Filter field values (normalized), `_workspace` included.
    pub payload: BTreeMap<String, Value>,
}

/// A record near the query vector.
#[derive(Clone, Debug, PartialEq)]
pub struct VectorMatch {
    /// The record's primary key.
    pub id: String,
    /// Similarity from 0 to 1, higher is nearer.
    pub score: f64,
}

/// A vector store driver: `pgvector`, `qdrant` or `memory`.
pub trait VectorStore: Send + Sync + 'static {
    /// The store name.
    fn name(&self) -> &'static str;

    /// Creates the vector index `index` if it does not exist.
    fn prepare<'a>(&'a self, index: &'a str) -> BoxFuture<'a, Result<(), SearchError>>;

    /// Adds or replaces records.
    fn upsert<'a>(
        &'a self,
        index: &'a str,
        records: Vec<VectorRecord>,
    ) -> BoxFuture<'a, Result<(), SearchError>>;

    /// Removes records.
    fn delete<'a>(
        &'a self,
        index: &'a str,
        ids: &'a [String],
    ) -> BoxFuture<'a, Result<(), SearchError>>;

    /// Removes every record.
    fn flush<'a>(&'a self, index: &'a str) -> BoxFuture<'a, Result<(), SearchError>>;

    /// The vector of record `id`.
    fn get<'a>(
        &'a self,
        index: &'a str,
        id: &'a str,
    ) -> BoxFuture<'a, Result<Option<Vec<f32>>, SearchError>>;

    /// The `limit` records nearest to `vector` that match `filter`
    /// (normalized), nearest first.
    fn nearest<'a>(
        &'a self,
        index: &'a str,
        vector: &'a [f32],
        filter: &'a Filter,
        limit: u64,
    ) -> BoxFuture<'a, Result<Vec<VectorMatch>, SearchError>>;
}

/// The similarity of `a` and `b` under `metric`, mapped into 0..1.
pub(crate) fn similarity(metric: Metric, a: &[f32], b: &[f32]) -> f64 {
    let dot: f64 = a
        .iter()
        .zip(b)
        .map(|(x, y)| f64::from(*x) * f64::from(*y))
        .sum();
    match metric {
        Metric::Cosine => {
            let na: f64 = a.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>().sqrt();
            let nb: f64 = b.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>().sqrt();
            if na == 0.0 || nb == 0.0 {
                0.0
            } else {
                ((dot / (na * nb)) + 1.0) / 2.0
            }
        }
        Metric::Dot => 1.0 / (1.0 + (-dot).exp()),
        Metric::Euclidean => {
            let d: f64 = a
                .iter()
                .zip(b)
                .map(|(x, y)| (f64::from(*x) - f64::from(*y)).powi(2))
                .sum::<f64>()
                .sqrt();
            1.0 / (1.0 + d)
        }
    }
}

/// Vectors in process, for tests.
#[derive(Default)]
pub struct MemoryVectorStore {
    metric: Metric,
    indexes: RwLock<HashMap<String, BTreeMap<String, VectorRecord>>>,
}

impl MemoryVectorStore {
    /// An empty store comparing with `metric`.
    pub fn new(metric: Metric) -> Self {
        Self {
            metric,
            indexes: RwLock::new(HashMap::new()),
        }
    }
}

impl fmt::Debug for MemoryVectorStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MemoryVectorStore")
            .field("metric", &self.metric)
            .finish_non_exhaustive()
    }
}

impl VectorStore for MemoryVectorStore {
    fn name(&self) -> &'static str {
        "memory"
    }

    fn prepare<'a>(&'a self, index: &'a str) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            self.indexes
                .write()
                .unwrap_or_else(PoisonError::into_inner)
                .entry(index.to_owned())
                .or_default();
            Ok(())
        })
    }

    fn upsert<'a>(
        &'a self,
        index: &'a str,
        records: Vec<VectorRecord>,
    ) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            let mut indexes = self.indexes.write().unwrap_or_else(PoisonError::into_inner);
            let target = indexes.entry(index.to_owned()).or_default();
            for record in records {
                target.insert(record.id.clone(), record);
            }
            Ok(())
        })
    }

    fn delete<'a>(
        &'a self,
        index: &'a str,
        ids: &'a [String],
    ) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            if let Some(target) = self
                .indexes
                .write()
                .unwrap_or_else(PoisonError::into_inner)
                .get_mut(index)
            {
                for id in ids {
                    target.remove(id);
                }
            }
            Ok(())
        })
    }

    fn flush<'a>(&'a self, index: &'a str) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            self.indexes
                .write()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(index);
            Ok(())
        })
    }

    fn get<'a>(
        &'a self,
        index: &'a str,
        id: &'a str,
    ) -> BoxFuture<'a, Result<Option<Vec<f32>>, SearchError>> {
        Box::pin(async move {
            Ok(self
                .indexes
                .read()
                .unwrap_or_else(PoisonError::into_inner)
                .get(index)
                .and_then(|t| t.get(id))
                .map(|r| r.vector.clone()))
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
            let indexes = self.indexes.read().unwrap_or_else(PoisonError::into_inner);
            let Some(target) = indexes.get(index) else {
                return Ok(Vec::new());
            };
            let mut found: Vec<VectorMatch> = target
                .values()
                .filter(|r| filter.matches(&r.payload))
                .map(|r| VectorMatch {
                    id: r.id.clone(),
                    score: similarity(self.metric, vector, &r.vector),
                })
                .collect();
            found.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
            found.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
            Ok(found)
        })
    }
}

/// A Qdrant server: one collection per index, named like it. Point ids
/// are UUIDs derived from the record ids, which travel in the payload
/// (`_id`).
pub struct QdrantStore {
    api: Api,
    dimensions: usize,
    metric: Metric,
}

impl QdrantStore {
    /// The store for the server at `url`.
    pub fn new(
        url: &str,
        key: Option<String>,
        dimensions: usize,
        metric: Metric,
        timeout: Duration,
    ) -> Result<Self, BoxError> {
        Ok(Self {
            api: Api::new(
                "qdrant",
                vec![url.to_owned()],
                Auth::Header("api-key", key),
                timeout,
            )?,
            dimensions,
            metric,
        })
    }

    /// The store on a given HTTP client (a fake in tests).
    pub fn with_client(url: &str, dimensions: usize, metric: Metric, client: HttpClient) -> Self {
        Self {
            api: Api::with_client(
                "qdrant",
                vec![url.to_owned()],
                Auth::Header("api-key", None),
                client,
            ),
            dimensions,
            metric,
        }
    }
}

impl fmt::Debug for QdrantStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QdrantStore")
            .field("dimensions", &self.dimensions)
            .field("metric", &self.metric)
            .finish_non_exhaustive()
    }
}

/// The Qdrant point id of record `id`: a UUID from the first 16 bytes of
/// its SHA-256.
pub(crate) fn point_id(id: &str) -> String {
    let digest = Sha256::digest(id.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    uuid::Builder::from_random_bytes(bytes)
        .into_uuid()
        .hyphenated()
        .to_string()
}

/// A filter (normalized) as a Qdrant filter object.
pub(crate) fn qdrant_filter(filter: &Filter) -> Value {
    match filter {
        Filter::Const(true) => json!({}),
        Filter::Const(false) => json!({ "must": [{ "has_id": [] }] }),
        Filter::All(items) => {
            json!({ "must": items.iter().map(qdrant_condition).collect::<Vec<_>>() })
        }
        Filter::Any(items) => {
            json!({ "should": items.iter().map(qdrant_condition).collect::<Vec<_>>() })
        }
        other => json!({ "must": [qdrant_condition(other)] }),
    }
}

fn qdrant_condition(filter: &Filter) -> Value {
    match filter {
        Filter::All(_) | Filter::Any(_) | Filter::Const(_) => qdrant_filter(filter),
        Filter::Not(inner) => json!({ "must_not": [qdrant_condition(inner)] }),
        Filter::Null(field) => json!({ "is_empty": { "key": field } }),
        Filter::NotNull(field) => json!({ "must_not": [{ "is_empty": { "key": field } }] }),
        Filter::In { field, values } => json!({ "key": field, "match": { "any": values } }),
        Filter::NotIn { field, values } => {
            json!({ "must_not": [{ "key": field, "match": { "any": values } }] })
        }
        Filter::Cmp { field, op, value } => match op {
            CmpOp::Eq => json!({ "key": field, "match": { "value": value } }),
            CmpOp::Ne => json!({ "must_not": [{ "key": field, "match": { "value": value } }] }),
            CmpOp::Lt => json!({ "key": field, "range": { "lt": value } }),
            CmpOp::Lte => json!({ "key": field, "range": { "lte": value } }),
            CmpOp::Gt => json!({ "key": field, "range": { "gt": value } }),
            CmpOp::Gte => json!({ "key": field, "range": { "gte": value } }),
        },
    }
}

impl VectorStore for QdrantStore {
    fn name(&self) -> &'static str {
        "qdrant"
    }

    fn prepare<'a>(&'a self, index: &'a str) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            let path = format!("/collections/{}", encode(index));
            let response = self.api.send(Method::GET, &path, None, &[404]).await?;
            if response.status().as_u16() != 404 {
                return Ok(());
            }
            let distance = match self.metric {
                Metric::Cosine => "Cosine",
                Metric::Dot => "Dot",
                Metric::Euclidean => "Euclid",
            };
            self.api
                .json(
                    Method::PUT,
                    &path,
                    Some(&Body::Json(json!({
                        "vectors": { "size": self.dimensions, "distance": distance }
                    }))),
                )
                .await?;
            Ok(())
        })
    }

    fn upsert<'a>(
        &'a self,
        index: &'a str,
        records: Vec<VectorRecord>,
    ) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            let points: Vec<Value> = records
                .into_iter()
                .map(|r| {
                    let mut payload: serde_json::Map<String, Value> =
                        r.payload.into_iter().collect();
                    payload.insert("_id".to_owned(), Value::from(r.id.as_str()));
                    json!({ "id": point_id(&r.id), "vector": r.vector, "payload": payload })
                })
                .collect();
            self.api
                .json(
                    Method::PUT,
                    &format!("/collections/{}/points?wait=true", encode(index)),
                    Some(&Body::Json(json!({ "points": points }))),
                )
                .await?;
            Ok(())
        })
    }

    fn delete<'a>(
        &'a self,
        index: &'a str,
        ids: &'a [String],
    ) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            let points: Vec<String> = ids.iter().map(|id| point_id(id)).collect();
            self.api
                .send(
                    Method::POST,
                    &format!("/collections/{}/points/delete?wait=true", encode(index)),
                    Some(&Body::Json(json!({ "points": points }))),
                    &[404],
                )
                .await?;
            Ok(())
        })
    }

    fn flush<'a>(&'a self, index: &'a str) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            self.api
                .send(
                    Method::DELETE,
                    &format!("/collections/{}", encode(index)),
                    None,
                    &[404],
                )
                .await?;
            self.prepare(index).await
        })
    }

    fn get<'a>(
        &'a self,
        index: &'a str,
        id: &'a str,
    ) -> BoxFuture<'a, Result<Option<Vec<f32>>, SearchError>> {
        Box::pin(async move {
            let answer = self
                .api
                .json(
                    Method::POST,
                    &format!("/collections/{}/points", encode(index)),
                    Some(&Body::Json(json!({
                        "ids": [point_id(id)], "with_vector": true, "with_payload": false
                    }))),
                )
                .await?;
            Ok(answer
                .pointer("/result/0/vector")
                .and_then(Value::as_array)
                .map(|v| {
                    v.iter()
                        .filter_map(Value::as_f64)
                        .map(|x| x as f32)
                        .collect()
                }))
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
            let mut body = json!({
                "vector": vector,
                "limit": limit,
                "with_payload": ["_id"],
            });
            if *filter != Filter::Const(true) {
                body["filter"] = qdrant_filter(filter);
            }
            let answer = self
                .api
                .json(
                    Method::POST,
                    &format!("/collections/{}/points/search", encode(index)),
                    Some(&Body::Json(body)),
                )
                .await?;
            let metric = self.metric;
            Ok(answer
                .get("result")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .filter_map(|point| {
                    let id = point.pointer("/payload/_id")?.as_str()?.to_owned();
                    let raw = point.get("score")?.as_f64()?;
                    let score = match metric {
                        Metric::Cosine => (raw + 1.0) / 2.0,
                        Metric::Dot => 1.0 / (1.0 + (-raw).exp()),
                        Metric::Euclidean => 1.0 / (1.0 + raw),
                    };
                    Some(VectorMatch { id, score })
                })
                .collect())
        })
    }
}

/// The table of the `pgvector` store.
pub const VECTORS_TABLE: &str = "search_vectors";

/// The `pgvector` extension in the app's PostgreSQL database: the table
/// `search_vectors` (an `embedding vector(<dimensions>)` column with an
/// HNSW index), created when the store is first prepared.
pub struct PgvectorStore {
    db: Db,
    dimensions: usize,
    metric: Metric,
    ready: std::sync::atomic::AtomicBool,
}

impl PgvectorStore {
    /// The store on `db` (PostgreSQL with the `vector` extension).
    pub fn new(db: Db, dimensions: usize, metric: Metric) -> Self {
        Self {
            db,
            dimensions,
            metric,
            ready: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

impl fmt::Debug for PgvectorStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PgvectorStore")
            .field("dimensions", &self.dimensions)
            .field("metric", &self.metric)
            .finish_non_exhaustive()
    }
}

/// The statements creating the `pgvector` table and its HNSW index.
pub(crate) fn pgvector_schema(dimensions: usize, metric: Metric) -> Vec<String> {
    let ops = match metric {
        Metric::Cosine => "vector_cosine_ops",
        Metric::Dot => "vector_ip_ops",
        Metric::Euclidean => "vector_l2_ops",
    };
    vec![
        "CREATE EXTENSION IF NOT EXISTS vector".to_owned(),
        format!(
            "CREATE TABLE IF NOT EXISTS {VECTORS_TABLE} (\
               index_name VARCHAR(191) NOT NULL, \
               id VARCHAR(191) NOT NULL, \
               payload JSONB NOT NULL DEFAULT '{{}}', \
               embedding vector({dimensions}) NOT NULL, \
               PRIMARY KEY (index_name, id))"
        ),
        format!(
            "CREATE INDEX IF NOT EXISTS {VECTORS_TABLE}_embedding ON {VECTORS_TABLE} USING hnsw (embedding {ops})"
        ),
    ]
}

/// A vector as pgvector's text form, `[1,2,3]`.
pub(crate) fn vector_literal(vector: &[f32]) -> String {
    format!(
        "[{}]",
        vector
            .iter()
            .map(f32::to_string)
            .collect::<Vec<_>>()
            .join(",")
    )
}

/// The nearest-neighbour query of the `pgvector` store.
pub(crate) fn pgvector_nearest(
    index: &str,
    vector: &[f32],
    filter: &Filter,
    limit: u64,
    metric: Metric,
) -> Sql {
    let operator = match metric {
        Metric::Cosine => "<=>",
        Metric::Dot => "<#>",
        Metric::Euclidean => "<->",
    };
    let mut sql = Sql::new(DbBackend::Postgres);
    sql.push("SELECT id, (embedding ")
        .push(operator)
        .push(" ")
        .bind(vector_literal(vector))
        .push(&format!(
            "::vector)::float8 AS distance FROM {VECTORS_TABLE} v WHERE index_name = "
        ))
        .bind(index)
        .push(" AND ");
    sql.filter(filter, "v.payload");
    sql.push(" ORDER BY distance ASC LIMIT ")
        .bind(i64::try_from(limit).unwrap_or(i64::MAX));
    sql
}

impl VectorStore for PgvectorStore {
    fn name(&self) -> &'static str {
        "pgvector"
    }

    fn prepare<'a>(&'a self, _index: &'a str) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            if self.ready.load(std::sync::atomic::Ordering::Relaxed) {
                return Ok(());
            }
            for statement in pgvector_schema(self.dimensions, self.metric) {
                self.db.conn().execute_unprepared(&statement).await?;
            }
            self.ready.store(true, std::sync::atomic::Ordering::Relaxed);
            Ok(())
        })
    }

    fn upsert<'a>(
        &'a self,
        index: &'a str,
        records: Vec<VectorRecord>,
    ) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            self.prepare(index).await?;
            let tx = self.db.conn().begin().await?;
            for record in records {
                let mut sql = Sql::new(DbBackend::Postgres);
                sql.push(&format!(
                    "INSERT INTO {VECTORS_TABLE} (index_name, id, payload, embedding) VALUES ("
                ))
                .bind(index)
                .push(", ")
                .bind(record.id.clone())
                .push(", ")
                .bind(serde_json::to_string(&record.payload).unwrap_or_default())
                .push("::jsonb, ")
                .bind(vector_literal(&record.vector))
                .push(
                    "::vector) ON CONFLICT (index_name, id) DO UPDATE SET \
                     payload = EXCLUDED.payload, embedding = EXCLUDED.embedding",
                );
                tx.execute_raw(sql.statement()).await?;
            }
            tx.commit().await?;
            Ok(())
        })
    }

    fn delete<'a>(
        &'a self,
        index: &'a str,
        ids: &'a [String],
    ) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            self.prepare(index).await?;
            let mut sql = Sql::new(DbBackend::Postgres);
            sql.push(&format!("DELETE FROM {VECTORS_TABLE} WHERE index_name = "))
                .bind(index)
                .push(" AND id IN (");
            for (i, id) in ids.iter().enumerate() {
                if i > 0 {
                    sql.push(", ");
                }
                sql.bind(id.clone());
            }
            sql.push(")");
            if ids.is_empty() {
                return Ok(());
            }
            self.db.conn().execute_raw(sql.statement()).await?;
            Ok(())
        })
    }

    fn flush<'a>(&'a self, index: &'a str) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            self.prepare(index).await?;
            let mut sql = Sql::new(DbBackend::Postgres);
            sql.push(&format!("DELETE FROM {VECTORS_TABLE} WHERE index_name = "))
                .bind(index);
            self.db.conn().execute_raw(sql.statement()).await?;
            Ok(())
        })
    }

    fn get<'a>(
        &'a self,
        index: &'a str,
        id: &'a str,
    ) -> BoxFuture<'a, Result<Option<Vec<f32>>, SearchError>> {
        Box::pin(async move {
            self.prepare(index).await?;
            let mut sql = Sql::new(DbBackend::Postgres);
            sql.push(&format!(
                "SELECT embedding::text AS embedding FROM {VECTORS_TABLE} WHERE index_name = "
            ))
            .bind(index)
            .push(" AND id = ")
            .bind(id);
            let row = self.db.conn().query_one_raw(sql.statement()).await?;
            let Some(row) = row else {
                return Ok(None);
            };
            let text: String = row.try_get("", "embedding")?;
            Ok(Some(
                text.trim_matches(['[', ']'])
                    .split(',')
                    .filter_map(|x| x.trim().parse::<f32>().ok())
                    .collect(),
            ))
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
            self.prepare(index).await?;
            let sql = pgvector_nearest(index, vector, filter, limit, self.metric);
            let rows = self.db.conn().query_all_raw(sql.statement()).await?;
            let metric = self.metric;
            rows.iter()
                .map(|row| {
                    let id: String = row.try_get("", "id")?;
                    let distance: f64 = row.try_get("", "distance")?;
                    let score = match metric {
                        // `<=>` is 1 - cosine similarity.
                        Metric::Cosine => (2.0 - distance) / 2.0,
                        // `<#>` is the negative inner product.
                        Metric::Dot => 1.0 / (1.0 + distance.exp()),
                        Metric::Euclidean => 1.0 / (1.0 + distance),
                    };
                    Ok(VectorMatch { id, score })
                })
                .collect()
        })
    }
}

/// The vector side of the search service: the store, the embeddings
/// provider and a cache of embeddings by text.
pub(crate) struct Vectors {
    options: VectorOptions,
    store: Arc<dyn VectorStore>,
    embeddings: Option<Arc<dyn Embeddings>>,
    cache: Mutex<HashMap<String, Arc<Vec<f32>>>>,
    prepared: Mutex<HashSet<String>>,
}

/// The most embeddings kept in the cache.
const CACHE_LIMIT: usize = 10_000;

impl Vectors {
    pub(crate) async fn build(
        app: &AppContext,
        options: &VectorOptions,
        search: &SearchOptions,
        db: Option<&Db>,
    ) -> Result<Vectors, BoxError> {
        let store: Arc<dyn VectorStore> = match options.store {
            VectorStoreKind::Memory => Arc::new(MemoryVectorStore::new(options.metric)),
            VectorStoreKind::Qdrant => Arc::new(QdrantStore::new(
                options.url.as_deref().unwrap_or_default(),
                options.key.clone(),
                options.dimensions,
                options.metric,
                Duration::from_secs(search.timeout_secs),
            )?),
            VectorStoreKind::Pgvector => {
                let db = db.ok_or("the pgvector store needs DatabaseModule in the app")?;
                if db.backend() != DbBackend::Postgres {
                    return Err("the pgvector store needs a PostgreSQL database".into());
                }
                Arc::new(PgvectorStore::new(
                    db.clone(),
                    options.dimensions,
                    options.metric,
                ))
            }
        };
        let embeddings = app
            .try_get::<dyn Embeddings>()
            .await
            .ok_or_else(|| SearchError::NoEmbeddings.to_string())?;
        Ok(Self::with_store(options.clone(), store, Some(embeddings)))
    }

    pub(crate) fn with_store(
        options: VectorOptions,
        store: Arc<dyn VectorStore>,
        embeddings: Option<Arc<dyn Embeddings>>,
    ) -> Vectors {
        Vectors {
            options,
            store,
            embeddings,
            cache: Mutex::new(HashMap::new()),
            prepared: Mutex::new(HashSet::new()),
        }
    }

    pub(crate) async fn prepare(&self, index: &str) -> Result<(), SearchError> {
        if self
            .prepared
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(index)
        {
            return Ok(());
        }
        self.store.prepare(index).await?;
        self.prepared
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(index.to_owned());
        Ok(())
    }

    /// Embeds `texts`, from the cache when it has them.
    pub(crate) async fn embed(&self, texts: &[String]) -> Result<Vec<Arc<Vec<f32>>>, SearchError> {
        let keys: Vec<String> = texts
            .iter()
            .map(|t| hex::encode(Sha256::digest(t.as_bytes())))
            .collect();
        let mut out: Vec<Option<Arc<Vec<f32>>>> = {
            let cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
            keys.iter().map(|k| cache.get(k).cloned()).collect()
        };
        let missing: Vec<usize> = (0..texts.len()).filter(|i| out[*i].is_none()).collect();
        if !missing.is_empty() {
            let embeddings = self.embeddings.as_ref().ok_or(SearchError::NoEmbeddings)?;
            let batch: Vec<String> = missing.iter().map(|i| texts[*i].clone()).collect();
            let vectors = embeddings
                .embed(&batch)
                .await
                .map_err(SearchError::Embeddings)?;
            if vectors.len() != batch.len() {
                return Err(SearchError::Embeddings(
                    format!("{} texts gave {} vectors", batch.len(), vectors.len()).into(),
                ));
            }
            let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
            if cache.len() + vectors.len() > CACHE_LIMIT {
                cache.clear();
            }
            for (i, vector) in missing.into_iter().zip(vectors) {
                if vector.len() != self.options.dimensions {
                    return Err(SearchError::Embeddings(
                        format!(
                            "a vector has {} dimensions; `search.vectors.dimensions` is {}",
                            vector.len(),
                            self.options.dimensions
                        )
                        .into(),
                    ));
                }
                let vector = Arc::new(vector);
                cache.insert(keys[i].clone(), vector.clone());
                out[i] = Some(vector);
            }
        }
        Ok(out
            .into_iter()
            .map(|v| v.expect("every text embedded"))
            .collect())
    }

    pub(crate) async fn embed_one(&self, text: &str) -> Result<Vec<f32>, SearchError> {
        let mut vectors = self.embed(&[text.to_owned()]).await?;
        Ok(vectors.pop().map(|v| (*v).clone()).unwrap_or_default())
    }

    /// Embeds and stores records: (id, workspace, filter payload, text).
    pub(crate) async fn upsert(
        &self,
        index: &str,
        records: Vec<EmbedRecord>,
    ) -> Result<(), SearchError> {
        if records.is_empty() {
            return Ok(());
        }
        self.prepare(index).await?;
        let texts: Vec<String> = records.iter().map(|r| r.3.clone()).collect();
        let vectors = self.embed(&texts).await?;
        let records = records
            .into_iter()
            .zip(vectors)
            .map(|((id, workspace, mut payload, _), vector)| {
                if let Some(workspace) = workspace {
                    payload.insert(WORKSPACE_FIELD.to_owned(), Value::from(workspace));
                }
                VectorRecord {
                    id,
                    vector: (*vector).clone(),
                    payload,
                }
            })
            .collect();
        self.store.upsert(index, records).await
    }

    pub(crate) async fn delete(&self, index: &str, ids: &[String]) -> Result<(), SearchError> {
        self.prepare(index).await?;
        self.store.delete(index, ids).await
    }

    pub(crate) async fn flush(&self, index: &str) -> Result<(), SearchError> {
        self.prepare(index).await?;
        self.store.flush(index).await
    }

    pub(crate) async fn get(&self, index: &str, id: &str) -> Result<Option<Vec<f32>>, SearchError> {
        self.prepare(index).await?;
        self.store.get(index, id).await
    }

    pub(crate) async fn nearest(
        &self,
        index: &str,
        vector: &[f32],
        filter: &Filter,
        limit: u64,
    ) -> Result<Vec<VectorMatch>, SearchError> {
        self.prepare(index).await?;
        self.store.nearest(index, vector, filter, limit).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn similarities_map_into_zero_to_one() {
        let a = [1.0, 0.0];
        assert!((similarity(Metric::Cosine, &a, &[1.0, 0.0]) - 1.0).abs() < 1e-9);
        assert!((similarity(Metric::Cosine, &a, &[-1.0, 0.0])).abs() < 1e-9);
        assert!((similarity(Metric::Cosine, &a, &[0.0, 1.0]) - 0.5).abs() < 1e-9);
        assert!((similarity(Metric::Euclidean, &a, &a) - 1.0).abs() < 1e-9);
        assert!(similarity(Metric::Dot, &a, &a) > 0.5);
    }

    #[test]
    fn point_ids_are_stable_uuids() {
        let id = point_id("0199c1d2-1111");
        assert_eq!(id, point_id("0199c1d2-1111"));
        assert_ne!(id, point_id("other"));
        assert!(uuid::Uuid::parse_str(&id).is_ok());
    }

    #[test]
    fn qdrant_filters_use_must_should_and_must_not() {
        let filter = Filter::All(vec![
            Filter::eq("status", "customer"),
            Filter::ne("owner", "u1"),
            Filter::range("n", 1..),
            Filter::null("closed_at"),
        ])
        .normalized();
        assert_eq!(
            qdrant_filter(&filter),
            json!({"must": [
                {"key": "status", "match": {"value": "customer"}},
                {"must_not": [{"key": "owner", "match": {"value": "u1"}}]},
                {"key": "n", "range": {"gte": 1}},
                {"is_empty": {"key": "closed_at"}},
            ]})
        );
    }

    #[test]
    fn pgvector_sql_orders_by_distance() {
        let filter = Filter::eq("status", "customer").normalized();
        let sql = pgvector_nearest("crm_contacts", &[0.5, 1.0], &filter, 5, Metric::Cosine);
        assert_eq!(
            sql.text(),
            "SELECT id, (embedding <=> $1::vector)::float8 AS distance FROM search_vectors v \
             WHERE index_name = $2 AND v.payload->'status' = $3::jsonb ORDER BY distance ASC LIMIT $4"
        );
        assert_eq!(vector_literal(&[0.5, 1.0]), "[0.5,1]");
        let schema = pgvector_schema(3, Metric::Dot);
        assert!(schema[1].contains("embedding vector(3)"));
        assert!(schema[2].contains("hnsw (embedding vector_ip_ops)"));
    }
}
