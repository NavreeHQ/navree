//! [`SearchOptions`]: the `search` configuration section, and [`Engine`].

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use navree_config::{ConfigSection, ConfigTable, SectionSchema};
use navree_contracts::Secret;
use serde::{Deserialize, Deserializer, Serialize};

use crate::engine::SearchEngine;
use crate::settings::valid_name;

/// When indexes follow database changes (`search.sync`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SyncMode {
    /// `search.sync` jobs on `search.queue`, run by `queue:work`.
    #[default]
    Queued,
    /// The documents are written after the commit, in a background task;
    /// the response does not wait for it.
    Inline,
    /// Nothing is synced.
    Off,
}

/// Options of the `postgres` engine (`search.postgres`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ConfigTable)]
#[serde(default)]
pub struct PostgresOptions {
    /// The text search configuration (default `english`).
    pub language: String,
    /// Typo tolerance with `pg_trgm` (default false).
    pub trigram: bool,
    /// The `pg_trgm.word_similarity_threshold` of a query with `trigram`:
    /// how close a query word must be to a word of the text to match, above
    /// 0 and at most 1 (default 0.4). PostgreSQL's own default of 0.6 misses
    /// a one-letter typo in a longer word ("lakotia" scores 0.55 against
    /// "Lakhotia"), so the engine sets this value per query.
    pub typo_threshold: f64,
}

/// The default `search.postgres.typo_threshold`.
pub const DEFAULT_TYPO_THRESHOLD: f64 = 0.4;

impl Default for PostgresOptions {
    fn default() -> Self {
        Self {
            language: "english".to_owned(),
            trigram: false,
            typo_threshold: DEFAULT_TYPO_THRESHOLD,
        }
    }
}

/// Options of the `tantivy` engine (`search.tantivy`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ConfigTable)]
#[serde(default)]
pub struct TantivyOptions {
    /// The directory holding one directory per index (default
    /// `storage/search`).
    pub path: PathBuf,
    /// The index writer's buffer in MB (default 64, at least 15).
    pub memory_budget_mb: usize,
}

impl Default for TantivyOptions {
    fn default() -> Self {
        Self {
            path: PathBuf::from("storage/search"),
            memory_budget_mb: 64,
        }
    }
}

/// The engine behind the indexes.
#[derive(Clone)]
#[non_exhaustive]
pub enum Engine {
    /// Full-text search in the app's database: PostgreSQL (`tsvector`, GIN
    /// index, optional trigram typo tolerance), or SQLite FTS5 when the app
    /// runs on SQLite.
    Postgres(PostgresOptions),
    /// A Meilisearch server.
    Meilisearch {
        /// The server URL.
        url: String,
        /// The API key.
        key: Option<Secret<String>>,
    },
    /// A Typesense server or cluster.
    Typesense {
        /// The first node's URL.
        url: String,
        /// The other nodes of a cluster.
        nodes: Vec<String>,
        /// The API key (required).
        key: Option<Secret<String>>,
    },
    /// An embedded Tantivy index on local disk (the `tantivy` feature).
    Tantivy(TantivyOptions),
    /// An in-process index, for tests.
    Memory,
    /// An engine of the app's own.
    Custom(Arc<dyn SearchEngine>),
}

impl Engine {
    /// The `postgres` engine with its default options.
    pub fn postgres() -> Engine {
        Engine::Postgres(PostgresOptions::default())
    }

    /// A Meilisearch server at `url`.
    pub fn meilisearch(url: impl Into<String>) -> Engine {
        Engine::Meilisearch {
            url: url.into(),
            key: None,
        }
    }

    /// A Typesense server at `url` (add the key with [`key`](Engine::key)).
    pub fn typesense(url: impl Into<String>) -> Engine {
        Engine::Typesense {
            url: url.into(),
            nodes: Vec::new(),
            key: None,
        }
    }

    /// A Tantivy index in the directory `path`.
    pub fn tantivy(path: impl Into<PathBuf>) -> Engine {
        Engine::Tantivy(TantivyOptions {
            path: path.into(),
            ..TantivyOptions::default()
        })
    }

    /// The in-process engine.
    pub fn memory() -> Engine {
        Engine::Memory
    }

    /// An engine of the app's own.
    pub fn custom(engine: Arc<dyn SearchEngine>) -> Engine {
        Engine::Custom(engine)
    }

    /// Sets the API key of a server engine (no effect on the others).
    pub fn key(mut self, value: impl Into<String>) -> Engine {
        match &mut self {
            Engine::Meilisearch { key, .. } | Engine::Typesense { key, .. } => {
                *key = Some(Secret::new(value.into()));
            }
            _ => {}
        }
        self
    }

    /// Sets the other nodes of a Typesense cluster (no effect on the
    /// others).
    pub fn nodes<I, S>(mut self, values: I) -> Engine
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        if let Engine::Typesense { nodes, .. } = &mut self {
            *nodes = values.into_iter().map(Into::into).collect();
        }
        self
    }

    /// `postgres`, `meilisearch`, `typesense`, `tantivy`, `memory` or the
    /// custom engine's name.
    pub fn name(&self) -> &'static str {
        match self {
            Engine::Postgres(_) => "postgres",
            Engine::Meilisearch { .. } => "meilisearch",
            Engine::Typesense { .. } => "typesense",
            Engine::Tantivy(_) => "tantivy",
            Engine::Memory => "memory",
            Engine::Custom(engine) => engine.name(),
        }
    }
}

impl Default for Engine {
    fn default() -> Self {
        Engine::postgres()
    }
}

impl fmt::Debug for Engine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let redacted = |key: &Option<Secret<String>>| key.as_ref().map(|_| "<redacted>");
        match self {
            Engine::Postgres(options) => f.debug_tuple("Postgres").field(options).finish(),
            Engine::Meilisearch { url, key } => f
                .debug_struct("Meilisearch")
                .field("url", url)
                .field("key", &redacted(key))
                .finish(),
            Engine::Typesense { url, nodes, key } => f
                .debug_struct("Typesense")
                .field("url", url)
                .field("nodes", nodes)
                .field("key", &redacted(key))
                .finish(),
            Engine::Tantivy(options) => f.debug_tuple("Tantivy").field(options).finish(),
            Engine::Memory => f.write_str("Memory"),
            Engine::Custom(engine) => f.debug_tuple("Custom").field(&engine.name()).finish(),
        }
    }
}

/// Where vectors are stored (`search.vectors.store`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum VectorStoreKind {
    /// The `pgvector` extension in the app's PostgreSQL database.
    Pgvector,
    /// A Qdrant server.
    Qdrant,
    /// In process, for tests.
    Memory,
    /// navree-ai's `VectorStore` (crate feature `ai`): the store, size and
    /// metric of `ai.vectors`, shared with the app's AI collections.
    Ai,
}

/// How vectors are compared (`search.vectors.metric`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Metric {
    /// Cosine similarity (the default).
    #[default]
    Cosine,
    /// Dot product.
    Dot,
    /// Euclidean distance.
    Euclidean,
}

impl Metric {
    /// `cosine`, `dot` or `euclidean`.
    pub fn as_str(self) -> &'static str {
        match self {
            Metric::Cosine => "cosine",
            Metric::Dot => "dot",
            Metric::Euclidean => "euclidean",
        }
    }
}

/// Vector search settings (`search.vectors`); off without a store.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct VectorOptions {
    /// Where vectors live.
    pub store: VectorStoreKind,
    /// The Qdrant server.
    #[serde(default)]
    pub url: Option<String>,
    /// The Qdrant API key. Never serialized.
    #[serde(default, skip_serializing)]
    pub key: Option<Secret<String>>,
    /// The vector size; must match the embedding model (default 1536).
    #[serde(default = "default_dimensions")]
    pub dimensions: usize,
    /// How vectors are compared.
    #[serde(default)]
    pub metric: Metric,
}

fn default_dimensions() -> usize {
    1536
}

impl VectorOptions {
    /// Vectors in `store` with the default size (1536) and metric (cosine).
    pub fn new(store: VectorStoreKind) -> Self {
        Self {
            store,
            url: None,
            key: None,
            dimensions: default_dimensions(),
            metric: Metric::Cosine,
        }
    }
}

impl fmt::Debug for VectorOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VectorOptions")
            .field("store", &self.store)
            .field("url", &self.url)
            .field("key", &self.key.as_ref().map(|_| "<redacted>"))
            .field("dimensions", &self.dimensions)
            .field("metric", &self.metric)
            .finish()
    }
}

/// Settings of one index from configuration
/// (`search.indexes.<index>`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct IndexOverrides {
    /// Synonyms: `{ "agra" = ["agra corp"] }`.
    pub synonyms: BTreeMap<String, Vec<String>>,
    /// Words the index ignores (a list, or a comma-separated string).
    #[serde(deserialize_with = "navree_config::comma_list")]
    pub stop_words: Vec<String>,
}

/// Options of the search module (configuration section `search`).
///
/// ```toml
/// [search]
/// engine = "meilisearch"            # postgres | meilisearch | typesense | tantivy | memory
/// url = "http://127.0.0.1:7700"     # meilisearch and typesense
/// # key: APP_SEARCH__KEY
/// prefix = "crm_"
/// queue = "search"
/// sync = "queued"                   # queued | inline | off
/// chunk_size = 500
/// ```
///
/// In code, build it with [`Engine`] and `..SearchOptions::default()`, or
/// convert an `Engine` (`SearchModule::for_root(Engine::memory())`).
#[derive(Clone, Debug)]
pub struct SearchOptions {
    /// The engine (default `postgres`).
    pub engine: Engine,
    /// Put in front of every index name (default empty).
    pub prefix: String,
    /// The queue of `search.sync` jobs (default `default`).
    pub queue: String,
    /// When indexes follow changes (default queued).
    pub sync: SyncMode,
    /// The window in which sync jobs for the same records collapse, in
    /// seconds (default 5).
    pub unique_for_secs: u64,
    /// Rows per batch for imports (default 500).
    pub chunk_size: u64,
    /// Time allowed for one engine request, in seconds (default 5).
    pub timeout_secs: u64,
    /// Vector search; off when `None`.
    pub vectors: Option<VectorOptions>,
    /// Synonyms and stop words per index (keyed by the index name before
    /// the prefix).
    pub indexes: BTreeMap<String, IndexOverrides>,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            engine: Engine::default(),
            prefix: String::new(),
            queue: "default".to_owned(),
            sync: SyncMode::Queued,
            unique_for_secs: 5,
            chunk_size: 500,
            timeout_secs: 5,
            vectors: None,
            indexes: BTreeMap::new(),
        }
    }
}

impl From<Engine> for SearchOptions {
    fn from(engine: Engine) -> Self {
        Self {
            engine,
            ..SearchOptions::default()
        }
    }
}

impl SearchOptions {
    /// Checks the options; the message names the key.
    pub fn check(&self) -> Result<(), String> {
        if !self.prefix.is_empty() && !valid_name(&self.prefix) {
            return Err("`search.prefix` must be letters, digits and `_`".to_owned());
        }
        if self.queue.trim().is_empty() {
            return Err("`search.queue` must not be empty".to_owned());
        }
        if self.chunk_size == 0 {
            return Err("`search.chunk_size` must be at least 1".to_owned());
        }
        if self.timeout_secs == 0 {
            return Err("`search.timeout_secs` must be at least 1".to_owned());
        }
        match &self.engine {
            Engine::Meilisearch { url, .. } => check_url("search.url", url, "meilisearch")?,
            Engine::Typesense { url, nodes, key } => {
                check_url("search.url", url, "typesense")?;
                for node in nodes {
                    check_url("search.nodes", node, "typesense")?;
                }
                if key.as_ref().is_none_or(|key| key.expose().is_empty()) {
                    return Err(
                        "`search.key` is required when `search.engine` is typesense".to_owned()
                    );
                }
            }
            Engine::Tantivy(options) => {
                if options.memory_budget_mb < 15 {
                    return Err("`search.tantivy.memory_budget_mb` must be at least 15".to_owned());
                }
            }
            Engine::Postgres(options) => {
                if !valid_name(&options.language) {
                    return Err(
                        "`search.postgres.language` must be a text search configuration name"
                            .to_owned(),
                    );
                }
                if !(options.typo_threshold > 0.0 && options.typo_threshold <= 1.0) {
                    return Err(
                        "`search.postgres.typo_threshold` must be above 0 and at most 1".to_owned(),
                    );
                }
            }
            Engine::Memory | Engine::Custom(_) => {}
        }
        if let Some(vectors) = &self.vectors {
            if vectors.dimensions == 0 {
                return Err("`search.vectors.dimensions` must be at least 1".to_owned());
            }
            if vectors.store == VectorStoreKind::Qdrant {
                check_url(
                    "search.vectors.url",
                    vectors.url.as_deref().unwrap_or(""),
                    "qdrant",
                )?;
            }
        }
        for name in self.indexes.keys() {
            if !valid_name(name) {
                return Err(format!("`search.indexes.{name}` is not a valid index name"));
            }
        }
        Ok(())
    }

    /// `prefix` + `index`.
    pub fn index_name(&self, index: &str) -> String {
        format!("{}{index}", self.prefix)
    }
}

fn check_url(key: &str, url: &str, engine: &str) -> Result<(), String> {
    if url.trim().is_empty() {
        return Err(format!("`{key}` is required for the {engine} engine"));
    }
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(format!("`{key}` must be an http:// or https:// URL"));
    }
    Ok(())
}

/// The section is written by hand: a file holds `RawSearchOptions` (the engine
/// is a name there, and its address and key sit beside it), so the
/// description comes from that struct.
impl ConfigSection for SearchOptions {
    const KEY: &'static str = "search";

    fn validate(&self) -> Result<(), String> {
        self.check()
    }

    fn defaults() -> Option<serde_json::Value> {
        Some(ConfigTable::to_json(&RawSearchOptions::default()))
    }

    fn schema() -> Option<SectionSchema> {
        Some(
            SectionSchema::new(
                Self::KEY,
                "Options of the search module (configuration section `search`).",
                <RawSearchOptions as ConfigTable>::fields(),
            )
            .with_docs("/docs/packages/search/"),
        )
    }
}

impl<'de> Deserialize<'de> for SearchOptions {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = RawSearchOptions::deserialize(deserializer)?;
        raw.into_options().map_err(serde::de::Error::custom)
    }
}

/// The `search` section as written.
#[derive(Deserialize, ConfigTable)]
#[serde(default)]
struct RawSearchOptions {
    /// The engine: `postgres` (default; `database` is the same), `meilisearch`,
    /// `typesense`, `tantivy` or `memory`.
    #[config(one_of = "postgres, meilisearch, typesense, tantivy, memory")]
    engine: String,
    /// The server URL (`meilisearch` and `typesense`).
    url: Option<String>,
    /// The API key of the server (`meilisearch`; required by `typesense`).
    key: Option<Secret<String>>,
    /// The other nodes of a Typesense cluster (a list, or a comma-separated
    /// string).
    #[serde(deserialize_with = "navree_config::comma_list")]
    nodes: Vec<String>,
    /// Put in front of every index name (default empty).
    prefix: String,
    /// The queue of `search.sync` jobs (default `default`).
    queue: String,
    /// When indexes follow changes (default `queued`).
    #[config(one_of = "queued, inline, off")]
    sync: SyncMode,
    /// The window in which sync jobs for the same records collapse, in
    /// seconds (default 5).
    unique_for_secs: u64,
    /// Rows per batch for imports (default 500).
    chunk_size: u64,
    /// Time allowed for one engine request, in seconds (default 5).
    timeout_secs: u64,
    /// The `postgres` engine.
    #[config(table)]
    postgres: PostgresOptions,
    /// The `tantivy` engine.
    #[config(table)]
    tantivy: TantivyOptions,
    /// Vector search; off without `vectors.store`.
    #[config(table)]
    vectors: RawVectorOptions,
    /// Synonyms and stop words per index (keyed by the index name before
    /// the prefix).
    indexes: BTreeMap<String, IndexOverrides>,
}

impl Default for RawSearchOptions {
    fn default() -> Self {
        let defaults = SearchOptions::default();
        Self {
            engine: "postgres".to_owned(),
            url: None,
            key: None,
            nodes: Vec::new(),
            prefix: defaults.prefix,
            queue: defaults.queue,
            sync: defaults.sync,
            unique_for_secs: defaults.unique_for_secs,
            chunk_size: defaults.chunk_size,
            timeout_secs: defaults.timeout_secs,
            postgres: PostgresOptions::default(),
            tantivy: TantivyOptions::default(),
            vectors: RawVectorOptions::default(),
            indexes: BTreeMap::new(),
        }
    }
}

/// `search.vectors` as written.
#[derive(Deserialize, ConfigTable)]
#[serde(default)]
struct RawVectorOptions {
    /// Where vectors live (unset: no vector search). `ai` keeps them in
    /// navree-ai's store, with its `ai.vectors` size and metric.
    #[config(one_of = "pgvector, qdrant, memory, ai")]
    store: Option<VectorStoreKind>,
    /// The Qdrant server.
    url: Option<String>,
    /// The Qdrant API key.
    key: Option<Secret<String>>,
    /// The vector size; must match the embedding model (default 1536).
    dimensions: usize,
    /// How vectors are compared (default `cosine`).
    #[config(one_of = "cosine, dot, euclidean")]
    metric: Metric,
}

impl Default for RawVectorOptions {
    fn default() -> Self {
        Self {
            store: None,
            url: None,
            key: None,
            dimensions: default_dimensions(),
            metric: Metric::Cosine,
        }
    }
}

impl RawSearchOptions {
    fn into_options(self) -> Result<SearchOptions, String> {
        let url = self.url.unwrap_or_default();
        let key = self.key.filter(|k| !k.expose().is_empty());
        let engine = match self.engine.trim().to_ascii_lowercase().as_str() {
            "postgres" | "database" => Engine::Postgres(self.postgres),
            "meilisearch" => Engine::Meilisearch { url, key },
            "typesense" => Engine::Typesense {
                url,
                nodes: self.nodes,
                key,
            },
            "tantivy" => Engine::Tantivy(self.tantivy),
            "memory" => Engine::Memory,
            other => {
                return Err(format!(
                    "`search.engine` must be postgres, meilisearch, typesense, tantivy or memory, not `{other}`"
                ));
            }
        };
        let vectors = self.vectors.store.map(|store| VectorOptions {
            store,
            url: self.vectors.url,
            key: self.vectors.key.filter(|k| !k.expose().is_empty()),
            dimensions: self.vectors.dimensions,
            metric: self.vectors.metric,
        });
        Ok(SearchOptions {
            engine,
            prefix: self.prefix,
            queue: self.queue,
            sync: self.sync,
            unique_for_secs: self.unique_for_secs,
            chunk_size: self.chunk_size,
            timeout_secs: self.timeout_secs,
            vectors,
            indexes: self.indexes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(value: serde_json::Value) -> Result<SearchOptions, String> {
        let options: SearchOptions = serde_json::from_value(value).map_err(|e| e.to_string())?;
        options.check()?;
        Ok(options)
    }

    #[test]
    fn defaults_match_the_reference() {
        let options = parse(json!({})).unwrap();
        assert_eq!(options.engine.name(), "postgres");
        assert_eq!(options.queue, "default");
        assert_eq!(options.sync, SyncMode::Queued);
        assert_eq!(options.unique_for_secs, 5);
        assert_eq!(options.chunk_size, 500);
        assert_eq!(options.timeout_secs, 5);
        assert!(options.vectors.is_none());
        let Engine::Postgres(pg) = &options.engine else {
            panic!()
        };
        assert_eq!((pg.language.as_str(), pg.trigram), ("english", false));
        assert_eq!(pg.typo_threshold, 0.4);
    }

    #[test]
    fn the_typo_threshold_is_configurable_and_checked() {
        let options = parse(json!({"postgres": {"trigram": true, "typo_threshold": 0.3}})).unwrap();
        let Engine::Postgres(pg) = &options.engine else {
            panic!()
        };
        assert_eq!(pg.typo_threshold, 0.3);
        for bad in [0.0, -0.1, 1.5] {
            let err = parse(json!({"postgres": {"typo_threshold": bad}})).unwrap_err();
            assert!(err.contains("typo_threshold"), "{err}");
        }
    }

    #[test]
    fn engines_read_their_keys() {
        let options = parse(json!({
            "engine": "typesense",
            "url": "https://ts-1:8108",
            "nodes": "https://ts-2:8108, https://ts-3:8108",
            "key": "secret",
            "prefix": "crm_"
        }))
        .unwrap();
        let Engine::Typesense { url, nodes, key } = &options.engine else {
            panic!()
        };
        assert_eq!(url, "https://ts-1:8108");
        assert_eq!(nodes, &["https://ts-2:8108", "https://ts-3:8108"]);
        assert_eq!(
            key.as_ref().map(|key| key.expose().as_str()),
            Some("secret")
        );
        assert_eq!(options.index_name("contacts"), "crm_contacts");
        assert!(
            !format!("{options:?}").contains("secret"),
            "keys are redacted"
        );

        let options = parse(json!({"engine": "tantivy", "tantivy": {"path": "/var/idx"}})).unwrap();
        let Engine::Tantivy(t) = &options.engine else {
            panic!()
        };
        assert_eq!(t.path, PathBuf::from("/var/idx"));
        assert_eq!(t.memory_budget_mb, 64);
    }

    #[test]
    fn invalid_options_name_the_key() {
        let error = parse(json!({"engine": "elastic"})).unwrap_err();
        assert!(error.contains("`search.engine`"), "{error}");
        let error = parse(json!({"engine": "meilisearch"})).unwrap_err();
        assert!(error.contains("`search.url`"), "{error}");
        let error = parse(json!({"engine": "typesense", "url": "http://ts:8108"})).unwrap_err();
        assert!(error.contains("`search.key`"), "{error}");
        let error = parse(json!({"chunk_size": 0})).unwrap_err();
        assert!(error.contains("`search.chunk_size`"), "{error}");
        let error = parse(json!({"vectors": {"store": "qdrant"}})).unwrap_err();
        assert!(error.contains("`search.vectors.url`"), "{error}");
    }

    #[test]
    fn vectors_and_index_overrides_are_read() {
        let options = parse(json!({
            "engine": "memory",
            "vectors": {"store": "pgvector", "dimensions": 3, "metric": "dot"},
            "indexes": {"companies": {"synonyms": {"agra": ["agra corp"]}, "stop_words": "inc,ltd"}}
        }))
        .unwrap();
        let vectors = options.vectors.unwrap();
        assert_eq!(vectors.store, VectorStoreKind::Pgvector);
        assert_eq!((vectors.dimensions, vectors.metric), (3, Metric::Dot));
        assert_eq!(options.indexes["companies"].stop_words, ["inc", "ltd"]);
    }

    #[test]
    fn engines_build_in_code() {
        let options: SearchOptions = Engine::meilisearch("http://127.0.0.1:7700")
            .key("masterKey")
            .into();
        options.check().unwrap();
        assert_eq!(options.engine.name(), "meilisearch");
        let typesense = Engine::typesense("http://a:8108").nodes(["http://b:8108"]);
        let options: SearchOptions = typesense.into();
        assert!(options.check().unwrap_err().contains("search.key"));
    }

    #[test]
    fn the_section_never_shows_a_key_in_debug_output() {
        let mut vectors = VectorOptions::new(VectorStoreKind::Qdrant);
        vectors.url = Some("http://127.0.0.1:6333".to_owned());
        vectors.key = Some(Secret::new("qdrant-api-s3cret".to_owned()));
        let options = SearchOptions {
            vectors: Some(vectors),
            ..SearchOptions::from(
                Engine::meilisearch("http://127.0.0.1:7700").key("meili-master-s3cret"),
            )
        };
        let typesense = SearchOptions::from(
            Engine::typesense("http://127.0.0.1:8108").key("typesense-api-s3cret"),
        );
        let printed = format!("{options:?} {typesense:?}");
        for secret in [
            "qdrant-api-s3cret",
            "meili-master-s3cret",
            "typesense-api-s3cret",
        ] {
            assert!(!printed.contains(secret), "{secret} in {printed}");
        }
        assert!(printed.contains("http://127.0.0.1:7700"), "{printed}");
    }

    #[test]
    fn serializing_vector_options_never_writes_the_key() {
        let mut vectors = VectorOptions::new(VectorStoreKind::Qdrant);
        vectors.url = Some("http://127.0.0.1:6333".to_owned());
        vectors.key = Some(Secret::new("qdrant-api-s3cret".to_owned()));
        let json = serde_json::to_string(&vectors).unwrap();
        assert!(!json.contains("qdrant-api-s3cret"), "{json}");
        assert!(json.contains("6333"), "{json}");
    }

    #[test]
    fn keys_are_read_from_configuration_as_plain_text() {
        let options = parse(json!({
            "engine": "meilisearch",
            "url": "http://127.0.0.1:7700",
            "key": "meili-key",
            "vectors": {"store": "qdrant", "url": "http://127.0.0.1:6333", "key": "qdrant-key"}
        }))
        .unwrap();
        let Engine::Meilisearch { key, .. } = &options.engine else {
            panic!()
        };
        assert_eq!(key.as_ref().unwrap().expose(), "meili-key");
        let vectors = options.vectors.unwrap();
        assert_eq!(vectors.key.as_ref().unwrap().expose(), "qdrant-key");
    }

    #[test]
    fn the_section_describes_the_keys_a_file_holds() {
        let schema = SearchOptions::schema().expect("schema");
        assert_eq!(schema.key, "search");
        assert_eq!(schema.docs.as_deref(), Some("/docs/packages/search/"));
        assert!(
            schema.unmarked_secrets().is_empty(),
            "{:?}",
            schema.unmarked_secrets()
        );
        for name in [
            "engine", "url", "key", "nodes", "prefix", "queue", "sync", "postgres", "tantivy",
            "vectors", "indexes",
        ] {
            assert!(schema.fields.iter().any(|f| f.name == name), "{name}");
        }
        assert_eq!(schema.secret_paths(), ["search.key", "search.vectors.key"]);
        let engine = schema.fields.iter().find(|f| f.name == "engine").unwrap();
        assert_eq!(
            engine.ty,
            navree_config::FieldType::choice(&[
                "postgres",
                "meilisearch",
                "typesense",
                "tantivy",
                "memory"
            ])
        );
        let defaults = SearchOptions::defaults().expect("defaults");
        assert_eq!(defaults["engine"], "postgres");
        assert_eq!(defaults["queue"], "default");
        assert_eq!(defaults["key"], serde_json::Value::Null);
        assert_eq!(defaults["postgres"]["language"], "english");
    }
}
