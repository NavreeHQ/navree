//! [`SearchOptions`]: the `search` configuration section, and [`Engine`].

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use navree_config::ConfigSection;
use serde::{Deserialize, Deserializer, Serialize};

use crate::engine::SearchEngine;
use crate::settings::valid_name;

/// When indexes follow database changes (`search.sync`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PostgresOptions {
    /// The text search configuration (default `english`).
    pub language: String,
    /// Typo tolerance with `pg_trgm` (default false).
    pub trigram: bool,
}

impl Default for PostgresOptions {
    fn default() -> Self {
        Self {
            language: "english".to_owned(),
            trigram: false,
        }
    }
}

/// Options of the `tantivy` engine (`search.tantivy`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
        key: Option<String>,
    },
    /// A Typesense server or cluster.
    Typesense {
        /// The first node's URL.
        url: String,
        /// The other nodes of a cluster.
        nodes: Vec<String>,
        /// The API key (required).
        key: Option<String>,
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
                *key = Some(value.into());
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
        let redacted = |key: &Option<String>| key.as_ref().map(|_| "<redacted>");
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
pub enum VectorStoreKind {
    /// The `pgvector` extension in the app's PostgreSQL database.
    Pgvector,
    /// A Qdrant server.
    Qdrant,
    /// In process, for tests.
    Memory,
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
pub struct VectorOptions {
    /// Where vectors live.
    pub store: VectorStoreKind,
    /// The Qdrant server.
    #[serde(default)]
    pub url: Option<String>,
    /// The Qdrant API key.
    #[serde(default)]
    pub key: Option<String>,
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
    /// Synonyms: `{ "acme" = ["acme corp"] }`.
    pub synonyms: BTreeMap<String, Vec<String>>,
    /// Words the index ignores (a list, or a comma-separated string).
    #[serde(deserialize_with = "list_or_csv")]
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
                if key.as_deref().is_none_or(str::is_empty) {
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

impl ConfigSection for SearchOptions {
    const KEY: &'static str = "search";

    fn validate(&self) -> Result<(), String> {
        self.check()
    }
}

impl<'de> Deserialize<'de> for SearchOptions {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = RawSearchOptions::deserialize(deserializer)?;
        raw.into_options().map_err(serde::de::Error::custom)
    }
}

/// The `search` section as written.
#[derive(Deserialize)]
#[serde(default)]
struct RawSearchOptions {
    engine: String,
    url: Option<String>,
    key: Option<String>,
    #[serde(deserialize_with = "list_or_csv")]
    nodes: Vec<String>,
    prefix: String,
    queue: String,
    sync: SyncMode,
    unique_for_secs: u64,
    chunk_size: u64,
    timeout_secs: u64,
    postgres: PostgresOptions,
    tantivy: TantivyOptions,
    vectors: RawVectorOptions,
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

#[derive(Deserialize)]
#[serde(default)]
struct RawVectorOptions {
    store: Option<VectorStoreKind>,
    url: Option<String>,
    key: Option<String>,
    dimensions: usize,
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
        let key = self.key.filter(|k| !k.is_empty());
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
            key: self.vectors.key.filter(|k| !k.is_empty()),
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

/// A list, or one comma-separated string (what an environment variable
/// holds).
fn list_or_csv<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Items {
        List(Vec<String>),
        Joined(String),
    }
    let items = match Items::deserialize(deserializer)? {
        Items::List(items) => items,
        Items::Joined(joined) => joined.split(',').map(str::to_owned).collect(),
    };
    Ok(items
        .into_iter()
        .map(|item| item.trim().to_owned())
        .filter(|item| !item.is_empty())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(value: serde_json::Value) -> Result<SearchOptions, String> {
        let options: SearchOptions =
            serde_json::from_value(value).map_err(|e| e.to_string())?;
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
        assert_eq!(key.as_deref(), Some("secret"));
        assert_eq!(options.index_name("contacts"), "crm_contacts");
        assert!(!format!("{options:?}").contains("secret"), "keys are redacted");

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
            "indexes": {"companies": {"synonyms": {"acme": ["acme corp"]}, "stop_words": "inc,ltd"}}
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
}
