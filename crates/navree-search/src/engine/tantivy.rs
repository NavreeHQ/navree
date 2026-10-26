//! The `tantivy` engine: an embedded Tantivy index on local disk.
//!
//! Each index lives in `<path>/<index>/`, in a generation directory named by
//! the `CURRENT` file there, so a fresh import builds a new generation and
//! swaps it in by rewriting `CURRENT`. Text is matched by Tantivy (BM25,
//! exact, prefix and fuzzy terms, one text column per boost); filters,
//! sorts and highlights are applied to the stored documents.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use navree_kernel::BoxFuture;
use serde_json::Value;
use tantivy::collector::{Count, TopDocs};
use tantivy::query::{
    AllQuery, BooleanQuery, BoostQuery, FuzzyTermQuery, Occur, PhraseQuery, Query, TermQuery,
};
use tantivy::schema::{
    Field, IndexRecordOption, STORED, STRING, Schema, TEXT, Value as TantivyValue,
};
use tantivy::{Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument, Term};

use crate::engine::{EngineHit, EngineQuery, EngineResults, SearchEngine, compare_sort_values};
use crate::options::TantivyOptions;
use crate::settings::WORKSPACE_FIELD;
use crate::text::{QueryTerms, Term as QueryTerm, field_text, highlight, typo_budget};
use crate::{Document, IndexSettings, SearchError};

/// The fixed schema: the id, ten text columns (one per boost) and the
/// stored document.
#[derive(Clone, Copy)]
struct Fields {
    id: Field,
    text: [Field; 10],
    stored: Field,
}

fn schema() -> (Schema, Fields) {
    let mut builder = Schema::builder();
    let id = builder.add_text_field("id", STRING | STORED);
    let text = std::array::from_fn(|i| builder.add_text_field(&format!("b{}", i + 1), TEXT));
    let stored = builder.add_text_field("doc", STORED);
    (builder.build(), Fields { id, text, stored })
}

/// An open index: its reader and its single writer.
struct Handle {
    reader: IndexReader,
    writer: Mutex<IndexWriter>,
    fields: Fields,
}

/// An embedded Tantivy index per search index, under one directory.
///
/// Only one process may write to a Tantivy directory: run the sync jobs in
/// the server process (`search.sync = "inline"`) or on a single worker.
pub struct TantivyEngine {
    path: PathBuf,
    memory_budget: usize,
    open: Mutex<HashMap<String, Arc<Handle>>>,
}

impl TantivyEngine {
    /// The engine on the directory `options.path` (created if missing).
    pub fn open(options: &TantivyOptions) -> Result<Self, SearchError> {
        std::fs::create_dir_all(&options.path).map_err(|e| io_error(&options.path, &e))?;
        Ok(Self {
            path: options.path.clone(),
            memory_budget: options.memory_budget_mb.max(15) * 1024 * 1024,
            open: Mutex::new(HashMap::new()),
        })
    }

    fn index_dir(&self, index: &str) -> PathBuf {
        self.path.join(index)
    }

    /// The live generation directory of `index`, if it has one.
    fn current(&self, index: &str) -> Option<PathBuf> {
        let dir = self.index_dir(index);
        let name = std::fs::read_to_string(dir.join("CURRENT")).ok()?;
        let name = name.trim();
        (!name.is_empty()).then(|| dir.join(name))
    }

    /// Points `index` at generation `name`.
    fn set_current(&self, index: &str, name: &str) -> Result<(), SearchError> {
        let dir = self.index_dir(index);
        std::fs::create_dir_all(&dir).map_err(|e| io_error(&dir, &e))?;
        let tmp = dir.join("CURRENT.tmp");
        std::fs::write(&tmp, name).map_err(|e| io_error(&tmp, &e))?;
        std::fs::rename(&tmp, dir.join("CURRENT")).map_err(|e| io_error(&dir, &e))
    }

    /// The open handle of `index`, opening (and creating) it when needed.
    fn handle(&self, index: &str) -> Result<Arc<Handle>, SearchError> {
        let mut open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(handle) = open.get(index) {
            return Ok(handle.clone());
        }
        let generation = match self.current(index) {
            Some(dir) => dir,
            None => {
                let name = generation_name();
                self.set_current(index, &name)?;
                self.index_dir(index).join(name)
            }
        };
        std::fs::create_dir_all(&generation).map_err(|e| io_error(&generation, &e))?;
        let (schema, fields) = schema();
        let directory = tantivy::directory::MmapDirectory::open(&generation).map_err(tantivy_error)?;
        let tantivy_index = Index::open_or_create(directory, schema).map_err(tantivy_error)?;
        let writer = tantivy_index
            .writer(self.memory_budget)
            .map_err(tantivy_error)?;
        let reader = tantivy_index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()
            .map_err(tantivy_error)?;
        let handle = Arc::new(Handle {
            reader,
            writer: Mutex::new(writer),
            fields,
        });
        open.insert(index.to_owned(), handle.clone());
        Ok(handle)
    }

    /// Closes `index` (its writer releases the directory lock).
    fn close(&self, index: &str) {
        let handle = self
            .open
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(index);
        drop(handle);
    }

    /// Runs `f` with the writer of `index`, then commits and reloads.
    async fn write<F>(&self, index: &str, f: F) -> Result<(), SearchError>
    where
        F: FnOnce(&IndexWriter, &Fields) -> Result<(), SearchError> + Send + 'static,
    {
        let handle = self.handle(index)?;
        tokio::task::spawn_blocking(move || {
            let mut writer = handle.writer.lock().unwrap_or_else(PoisonError::into_inner);
            f(&writer, &handle.fields)?;
            writer.commit().map_err(tantivy_error)?;
            handle.reader.reload().map_err(tantivy_error)
        })
        .await
        .map_err(|e| SearchError::Other(format!("the tantivy writer task failed: {e}")))?
    }
}

impl std::fmt::Debug for TantivyEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TantivyEngine")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

fn generation_name() -> String {
    format!("g{}", uuid::Uuid::now_v7().simple())
}

fn tantivy_error(error: impl std::fmt::Display) -> SearchError {
    SearchError::Engine {
        engine: "tantivy",
        message: error.to_string(),
    }
}

fn io_error(path: &Path, error: &std::io::Error) -> SearchError {
    SearchError::Engine {
        engine: "tantivy",
        message: format!("{}: {error}", path.display()),
    }
}

/// The stored form of `document`: its fields plus `_workspace`.
fn stored_json(document: &Document) -> String {
    let mut fields = document.fields().clone();
    if let Some(workspace) = document.workspace_id() {
        fields.insert(WORKSPACE_FIELD.to_owned(), Value::from(workspace));
    }
    serde_json::to_string(&fields).unwrap_or_else(|_| "{}".to_owned())
}

/// The Tantivy query for `terms`: every term must match some text column.
fn text_query(terms: &QueryTerms, fields: &Fields, settings: &IndexSettings) -> Box<dyn Query> {
    if terms.is_empty() {
        return Box::new(AllQuery);
    }
    let boosts: Vec<u8> = {
        let mut b: Vec<u8> = settings.text_fields().map(|(_, b)| b.clamp(1, 10)).collect();
        b.sort_unstable();
        b.dedup();
        b
    };
    let clauses = terms
        .terms
        .iter()
        .map(|term| {
            let mut alternatives: Vec<(Occur, Box<dyn Query>)> = Vec::new();
            for boost in &boosts {
                let field = fields.text[usize::from(*boost) - 1];
                for (query, weight) in term_queries(term, field) {
                    alternatives.push((
                        Occur::Should,
                        Box::new(BoostQuery::new(query, weight * f32::from(*boost))),
                    ));
                }
            }
            (
                Occur::Must,
                Box::new(BooleanQuery::new(alternatives)) as Box<dyn Query>,
            )
        })
        .collect();
    Box::new(BooleanQuery::new(clauses))
}

/// The ways one query word matches `field`, with their weights: exactly,
/// as a prefix (the last word), with typos, and through synonyms.
fn term_queries(term: &QueryTerm, field: Field) -> Vec<(Box<dyn Query>, f32)> {
    let exact = Term::from_field_text(field, &term.word);
    let mut out: Vec<(Box<dyn Query>, f32)> = vec![(
        Box::new(TermQuery::new(exact.clone(), IndexRecordOption::WithFreqs)),
        1.0,
    )];
    if term.prefix {
        out.push((Box::new(FuzzyTermQuery::new_prefix(exact.clone(), 0, true)), 0.9));
    }
    let budget = u8::try_from(typo_budget(term.word.chars().count())).unwrap_or(2);
    if budget > 0 {
        out.push((Box::new(FuzzyTermQuery::new(exact.clone(), budget, true)), 0.7));
        if term.prefix {
            out.push((Box::new(FuzzyTermQuery::new_prefix(exact, budget, true)), 0.5));
        }
    }
    for synonym in &term.synonyms {
        let terms: Vec<Term> = synonym
            .iter()
            .map(|w| Term::from_field_text(field, w))
            .collect();
        let query: Box<dyn Query> = if terms.len() == 1 {
            Box::new(TermQuery::new(terms[0].clone(), IndexRecordOption::WithFreqs))
        } else {
            Box::new(PhraseQuery::new(terms))
        };
        out.push((query, 0.95));
    }
    out
}

impl SearchEngine for TantivyEngine {
    fn name(&self) -> &'static str {
        "tantivy"
    }

    fn prepare<'a>(
        &'a self,
        index: &'a str,
        _settings: &'a IndexSettings,
    ) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move { self.handle(index).map(drop) })
    }

    fn upsert<'a>(
        &'a self,
        index: &'a str,
        settings: &'a IndexSettings,
        documents: Vec<Document>,
    ) -> BoxFuture<'a, Result<(), SearchError>> {
        let text_fields: Vec<(String, u8)> = settings
            .text_fields()
            .map(|(f, b)| (f.to_owned(), b))
            .collect();
        Box::pin(async move {
            self.write(index, move |writer, fields| {
                for document in &documents {
                    writer.delete_term(Term::from_field_text(fields.id, document.id()));
                    let mut doc = TantivyDocument::default();
                    doc.add_text(fields.id, document.id());
                    for (field, boost) in &text_fields {
                        if let Some(text) = document.get(field).and_then(field_text) {
                            doc.add_text(fields.text[usize::from((*boost).clamp(1, 10)) - 1], text);
                        }
                    }
                    doc.add_text(fields.stored, stored_json(document));
                    writer.add_document(doc).map_err(tantivy_error)?;
                }
                Ok(())
            })
            .await
        })
    }

    fn delete<'a>(
        &'a self,
        index: &'a str,
        ids: &'a [String],
    ) -> BoxFuture<'a, Result<(), SearchError>> {
        let ids = ids.to_vec();
        Box::pin(async move {
            self.write(index, move |writer, fields| {
                for id in &ids {
                    writer.delete_term(Term::from_field_text(fields.id, id));
                }
                Ok(())
            })
            .await
        })
    }

    fn flush<'a>(&'a self, index: &'a str) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            self.write(index, |writer, _| {
                writer.delete_all_documents().map_err(tantivy_error)?;
                Ok(())
            })
            .await
        })
    }

    fn drop_index<'a>(&'a self, index: &'a str) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            self.close(index);
            let dir = self.index_dir(index);
            if dir.exists() {
                std::fs::remove_dir_all(&dir).map_err(|e| io_error(&dir, &e))?;
            }
            Ok(())
        })
    }

    fn swap<'a>(
        &'a self,
        index: &'a str,
        staged: &'a str,
    ) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            self.close(staged);
            let Some(from) = self.current(staged) else {
                return Err(SearchError::Other(format!(
                    "the tantivy index `{staged}` does not exist"
                )));
            };
            let name = generation_name();
            let target_dir = self.index_dir(index);
            std::fs::create_dir_all(&target_dir).map_err(|e| io_error(&target_dir, &e))?;
            let to = target_dir.join(&name);
            std::fs::rename(&from, &to).map_err(|e| io_error(&from, &e))?;
            let old = self.current(index);
            self.close(index);
            self.set_current(index, &name)?;
            // The old generation goes once nothing maps it (best effort).
            if let Some(old) = old {
                let _ = std::fs::remove_dir_all(old);
            }
            let staged_dir = self.index_dir(staged);
            let _ = std::fs::remove_dir_all(staged_dir);
            Ok(())
        })
    }

    fn search<'a>(
        &'a self,
        index: &'a str,
        query: EngineQuery<'a>,
    ) -> BoxFuture<'a, Result<EngineResults, SearchError>> {
        Box::pin(async move {
            let settings = query.settings;
            let terms = QueryTerms::parse(
                query.text,
                settings.stop_word_list(),
                settings.synonym_map(),
            );
            let handle = self.handle(index)?;
            let searcher = handle.reader.searcher();
            let fields = handle.fields;
            let tantivy_query = text_query(&terms, &fields, settings);
            let limit = usize::try_from(searcher.num_docs()).unwrap_or(usize::MAX).max(1);
            let found = searcher
                .search(&tantivy_query, &TopDocs::with_limit(limit).order_by_score())
                .map_err(tantivy_error)?;
            let mut matches: Vec<(String, f64, BTreeMap<String, Value>)> = Vec::new();
            for (score, address) in found {
                let doc: TantivyDocument = searcher.doc(address).map_err(tantivy_error)?;
                let id = doc
                    .get_first(fields.id)
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default();
                let stored: BTreeMap<String, Value> = doc
                    .get_first(fields.stored)
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or_default();
                if !query.filter.matches(&stored) {
                    continue;
                }
                let score = if terms.is_empty() {
                    0.0
                } else {
                    let s = f64::from(score);
                    s / (s + 1.0)
                };
                matches.push((id, score, stored));
            }
            matches.sort_by(|(a_id, a_score, a), (b_id, b_score, b)| {
                for key in query.sort {
                    let order =
                        compare_sort_values(a.get(&key.field), b.get(&key.field), key.descending);
                    if order.is_ne() {
                        return order;
                    }
                }
                b_score.total_cmp(a_score).then_with(|| a_id.cmp(b_id))
            });
            let total = matches.len() as u64;
            let hits = matches
                .into_iter()
                .skip(usize::try_from(query.offset).unwrap_or(usize::MAX))
                .take(usize::try_from(query.limit).unwrap_or(usize::MAX))
                .map(|(id, score, stored)| {
                    let highlights = match query.highlight {
                        Some(tags) if !terms.is_empty() => settings
                            .text_fields()
                            .filter_map(|(field, _)| {
                                let text = field_text(stored.get(field)?)?;
                                highlight(&text, &terms, tags).map(|h| (field.to_owned(), h))
                            })
                            .collect(),
                        _ => BTreeMap::new(),
                    };
                    let title = query
                        .with_title
                        .then(|| settings.title_field())
                        .flatten()
                        .and_then(|field| field_text(stored.get(field)?));
                    EngineHit {
                        id,
                        score,
                        highlights,
                        title,
                    }
                })
                .collect();
            Ok(EngineResults { hits, total })
        })
    }

    fn count<'a>(&'a self, index: &'a str) -> BoxFuture<'a, Result<u64, SearchError>> {
        Box::pin(async move { Ok(self.handle(index)?.reader.searcher().num_docs()) })
    }

    fn contains<'a>(
        &'a self,
        index: &'a str,
        id: &'a str,
    ) -> BoxFuture<'a, Result<bool, SearchError>> {
        Box::pin(async move {
            let handle = self.handle(index)?;
            let query = TermQuery::new(
                Term::from_field_text(handle.fields.id, id),
                IndexRecordOption::Basic,
            );
            let count = handle
                .reader
                .searcher()
                .search(&query, &Count)
                .map_err(tantivy_error)?;
            Ok(count > 0)
        })
    }
}
