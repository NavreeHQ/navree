//! # navree-search
//!
//! `SearchModule`: full-text and vector search over the app's models,
//! kept in sync with the database through the queue.
//!
//! - **Searchable models.** Derive [`Searchable`] next to navree-orm's
//!   `#[derive(Model)]`
//!   (`#[searchable(boost = 3)]`, `#[searchable(filter, sort)]`, ...) or
//!   implement it by hand for documents with related data, and register it
//!   with [`SearchModule::for_feature`].
//! - **Syncing.** Every commit that wrote a searchable model through the
//!   model API queues a `search.sync` job ([`SyncDocuments`]) inside the same
//!   commit; the worker indexes the committed rows. `search.sync` can also
//!   be `inline` or `off`; [`Search::reindex`] and `search:import` cover
//!   writes that bypass the model API (mass updates, raw SQL).
//! - **Queries.** [`Search::query`] returns a [`SearchQuery`]: filters,
//!   ranges and sorts on declared fields, access rules
//!   ([`accessible`](SearchQuery::accessible)), workspaces, then
//!   [`paginate`](SearchQuery::paginate) (a `Page` of models in relevance
//!   order), [`hits`](SearchQuery::hits) (with scores and highlights),
//!   `ids` or `count`. [`Search::across`] searches several indexes at once.
//! - **Engines** ([`SearchEngine`]): `postgres` (PostgreSQL full text; FTS5
//!   on SQLite), `meilisearch` and `typesense` over HTTP, `tantivy`
//!   embedded (the `tantivy` feature) and `memory` for tests.
//! - **Vectors**: `#[searchable(embed)]` fields are embedded by the app's
//!   [`Embeddings`] provider and stored in pgvector, Qdrant or memory
//!   ([`VectorStore`]); [`Search::similar`], [`Search::similar_to`] and
//!   [`SearchQuery::hybrid`] query them. With the `ai` feature, navree-ai's
//!   `Embeddings` is the provider when the app registers none, and
//!   `search.vectors.store = "ai"` keeps the vectors in navree-ai's
//!   `VectorStore` (`AiVectorStore`), configured once in `ai.vectors`.
//! - **Commands**: `search:import <Name>|--all [--fresh]`,
//!   `search:flush <Name>`, `search:settings [<Name>]`, `search:status`.
//!
//! ```no_run
//! use navree_config::{ConfigModule, ConfigSources};
//! use navree_db::DatabaseModule;
//! use navree_kernel::Navree;
//! use navree_queue::QueueModule;
//! use navree_search::SearchModule;
//!
//! # fn main() -> Result<(), navree_kernel::BoxError> {
//! Navree::app()
//!     .module(ConfigModule::for_root(ConfigSources::defaults()))
//!     .module(DatabaseModule::from_config())
//!     .module(QueueModule::from_config())
//!     .module(SearchModule::from_config()) // section `search`
//!     .run()
//! # }
//! ```
#![forbid(unsafe_code)]

#[cfg(feature = "ai")]
mod ai;
mod commands;
mod document;
mod engine;
mod error;
mod filter;
#[cfg(feature = "fuzzing")]
#[doc(hidden)]
pub mod fuzzing;
mod index;
mod migration;
mod module;
mod options;
mod query;
mod searchable;
mod service;
mod settings;
mod sql;
mod sync;
mod text;
mod vector;

#[cfg(feature = "ai")]
pub use ai::AiVectorStore;
pub use document::Document;
#[cfg(feature = "tantivy")]
pub use engine::TantivyEngine;
pub use engine::{
    DatabaseEngine, EngineHit, EngineQuery, EngineResults, MeilisearchEngine, MemoryEngine,
    SearchEngine, SortBy, TypesenseEngine,
};
pub use error::SearchError;
pub use filter::{CmpOp, Filter};
pub use migration::{DOCUMENTS_TABLE, FTS_TABLE, SEARCH_MIGRATIONS};
pub use module::SearchModule;
pub use options::{
    Engine, IndexOverrides, Metric, PostgresOptions, SearchOptions, SyncMode, TantivyOptions,
    VectorOptions, VectorStoreKind,
};
pub use query::{AcrossHit, AcrossQuery, Hit, SearchQuery, SimilarQuery};
pub use searchable::{__private, SearchContext, Searchable};
pub use service::{ImportReport, IndexStatus, Search};
pub use settings::{FieldSettings, IndexSettings, WORKSPACE_FIELD};
pub use sync::{SYNC_JOB, SyncDocuments};
pub use text::HighlightTags;
pub use vector::{
    Embeddings, MemoryVectorStore, PgvectorStore, QdrantStore, VECTORS_TABLE, VectorMatch,
    VectorRecord, VectorStore,
};

/// `#[derive(Searchable)]`: implements [`Searchable`] for a navree-orm
/// model from its fields (`#[model(column = "..")]` names a field's
/// column).
pub use navree_macros::Searchable;
