//! The engine driver trait, [`SearchEngine`], and the built-in engines.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use navree_kernel::BoxFuture;
use serde::Serialize;
use serde_json::Value;

use crate::filter::json_cmp;
use crate::text::HighlightTags;
use crate::{Document, Filter, IndexSettings, SearchError};

pub(crate) mod database;
pub(crate) mod http;
pub(crate) mod meilisearch;
pub(crate) mod memory;
#[cfg(feature = "tantivy")]
pub(crate) mod tantivy;
pub(crate) mod typesense;

pub use database::DatabaseEngine;
pub use meilisearch::MeilisearchEngine;
pub use memory::MemoryEngine;
#[cfg(feature = "tantivy")]
pub use tantivy::TantivyEngine;
pub use typesense::TypesenseEngine;

/// One sort key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SortBy {
    /// The field.
    pub field: String,
    /// Highest first.
    pub descending: bool,
}

/// A search as an engine receives it.
#[derive(Clone, Copy, Debug)]
pub struct EngineQuery<'a> {
    /// The index's settings (fields, boosts, synonyms, stop words).
    pub settings: &'a IndexSettings,
    /// The query text; empty matches every document.
    pub text: &'a str,
    /// The filter, [normalized](Filter::normalized) and never
    /// `Const(false)` (`Const(true)` means none).
    pub filter: &'a Filter,
    /// Sort keys, then relevance. Documents without a value sort last.
    pub sort: &'a [SortBy],
    /// Hits to skip.
    pub offset: u64,
    /// Hits to return.
    pub limit: u64,
    /// Highlight the matches of text fields with these tags.
    pub highlight: Option<&'a HighlightTags>,
    /// Return the value of the index's first text field as the title.
    pub with_title: bool,
}

/// One matching document.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EngineHit {
    /// The record's primary key.
    pub id: String,
    /// Relevance from 0 to 1, higher is better.
    pub score: f64,
    /// Text field → HTML-escaped text with the highlight tags around the
    /// matches (fields without a match are left out).
    pub highlights: BTreeMap<String, String>,
    /// The first text field's value, when asked for.
    pub title: Option<String>,
}

/// A page of hits and the number of matching documents.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct EngineResults {
    /// The hits, best first (or in sort order).
    pub hits: Vec<EngineHit>,
    /// How many documents match.
    pub total: u64,
}

/// A search engine driver. Navree ships five (`postgres`, `meilisearch`,
/// `typesense`, `tantivy` and `memory`); an app can plug in its own with
/// [`Engine::custom`](crate::Engine::custom).
///
/// Index names arrive with `search.prefix` applied. Documents arrive with
/// the workspace set and their filter and sort values normalized (dates as
/// Unix milliseconds). Every engine must pass the conformance suite in
/// `crates/navree-search/tests`.
pub trait SearchEngine: Send + Sync + 'static {
    /// The engine name (`memory`, `meilisearch`, ...).
    fn name(&self) -> &'static str;

    /// Creates `index` if it does not exist and applies `settings`.
    fn prepare<'a>(
        &'a self,
        index: &'a str,
        settings: &'a IndexSettings,
    ) -> BoxFuture<'a, Result<(), SearchError>>;

    /// Adds or replaces `documents`.
    fn upsert<'a>(
        &'a self,
        index: &'a str,
        settings: &'a IndexSettings,
        documents: Vec<Document>,
    ) -> BoxFuture<'a, Result<(), SearchError>>;

    /// Removes the documents with these ids (unknown ids are ignored).
    fn delete<'a>(&'a self, index: &'a str, ids: &'a [String])
    -> BoxFuture<'a, Result<(), SearchError>>;

    /// Removes every document.
    fn flush<'a>(&'a self, index: &'a str) -> BoxFuture<'a, Result<(), SearchError>>;

    /// Deletes the index.
    fn drop_index<'a>(&'a self, index: &'a str) -> BoxFuture<'a, Result<(), SearchError>>;

    /// Makes the documents of `staged` (a prepared index) the documents of
    /// `index` in one step and removes `staged`.
    fn swap<'a>(&'a self, index: &'a str, staged: &'a str)
    -> BoxFuture<'a, Result<(), SearchError>>;

    /// Runs a search.
    fn search<'a>(
        &'a self,
        index: &'a str,
        query: EngineQuery<'a>,
    ) -> BoxFuture<'a, Result<EngineResults, SearchError>>;

    /// How many documents `index` holds.
    fn count<'a>(&'a self, index: &'a str) -> BoxFuture<'a, Result<u64, SearchError>>;

    /// Whether `index` holds a document with this id.
    fn contains<'a>(&'a self, index: &'a str, id: &'a str)
    -> BoxFuture<'a, Result<bool, SearchError>>;
}

/// Compares two documents' values of a sort field: values of the same type
/// in their order, documents without a value last (in both directions).
pub(crate) fn compare_sort_values(a: Option<&Value>, b: Option<&Value>, descending: bool) -> Ordering {
    let a = a.filter(|v| !v.is_null());
    let b = b.filter(|v| !v.is_null());
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(a), Some(b)) => {
            let order = json_cmp(a, b).unwrap_or_else(|| type_rank(a).cmp(&type_rank(b)));
            if descending { order.reverse() } else { order }
        }
    }
}

fn type_rank(value: &Value) -> u8 {
    match value {
        Value::Null => 0,
        Value::Bool(_) => 1,
        Value::Number(_) => 2,
        Value::String(_) => 3,
        Value::Array(_) => 4,
        Value::Object(_) => 5,
    }
}
