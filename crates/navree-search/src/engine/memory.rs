//! The `memory` engine: an in-process index for tests and development.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, PoisonError, RwLock};

use navree_kernel::BoxFuture;
use serde_json::Value;

use crate::engine::{EngineHit, EngineQuery, EngineResults, SearchEngine, compare_sort_values};
use crate::settings::WORKSPACE_FIELD;
use crate::text::{QueryTerms, Token, field_text, highlight, term_match, tokenize};
use crate::{Document, IndexSettings, SearchError};

/// An in-process index, kept in memory and lost on restart.
///
/// It supports every query method, typo tolerance included, and ranks with
/// the index's field boosts, so tests agree with production engines on what
/// matches. A query word matches a document word exactly, as a prefix (the
/// last word of the query) or within a typo budget (one typo from five
/// characters, two from nine); every query word must match.
///
/// Clones share the same indexes.
#[derive(Clone, Default)]
pub struct MemoryEngine {
    indexes: Arc<RwLock<HashMap<String, MemoryIndex>>>,
}

#[derive(Clone, Default)]
struct MemoryIndex {
    docs: BTreeMap<String, StoredDoc>,
}

#[derive(Clone)]
struct StoredDoc {
    /// Filter and sort values, `_workspace` included.
    values: BTreeMap<String, Value>,
    /// The text of each text field and its words.
    texts: Vec<(String, String, Vec<Token>)>,
}

impl MemoryEngine {
    /// An empty engine.
    pub fn new() -> Self {
        Self::default()
    }

    fn read<T>(&self, f: impl FnOnce(&HashMap<String, MemoryIndex>) -> T) -> T {
        f(&self.indexes.read().unwrap_or_else(PoisonError::into_inner))
    }

    fn write<T>(&self, f: impl FnOnce(&mut HashMap<String, MemoryIndex>) -> T) -> T {
        f(&mut self.indexes.write().unwrap_or_else(PoisonError::into_inner))
    }
}

impl std::fmt::Debug for MemoryEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<String> = self.read(|indexes| indexes.keys().cloned().collect());
        f.debug_struct("MemoryEngine")
            .field("indexes", &names)
            .finish()
    }
}

fn stored(settings: &IndexSettings, document: Document) -> StoredDoc {
    let mut values = document.fields().clone();
    if let Some(workspace) = document.workspace_id() {
        values.insert(WORKSPACE_FIELD.to_owned(), Value::from(workspace));
    }
    let texts = settings
        .text_fields()
        .filter_map(|(field, _)| {
            let text = field_text(document.get(field)?)?;
            let tokens = tokenize(&text);
            Some((field.to_owned(), text, tokens))
        })
        .collect();
    StoredDoc { values, texts }
}

/// The score of `doc` for `terms` (0 to 1), or `None` when a term matches
/// no text field.
fn score(doc: &StoredDoc, terms: &QueryTerms, settings: &IndexSettings) -> Option<f64> {
    if terms.is_empty() {
        return Some(0.0);
    }
    let boosts: HashMap<&str, u8> = settings.text_fields().collect();
    let mut total = 0.0;
    for term in &terms.terms {
        let best = doc
            .texts
            .iter()
            .map(|(field, _, tokens)| {
                let boost = f64::from(*boosts.get(field.as_str()).unwrap_or(&1));
                term_match(term, tokens).0 * boost
            })
            .fold(0.0, f64::max);
        if best <= 0.0 {
            return None;
        }
        total += best;
    }
    let max = terms.terms.len() as f64 * f64::from(settings.max_boost());
    Some((total / max).clamp(0.0, 1.0))
}

impl SearchEngine for MemoryEngine {
    fn name(&self) -> &'static str {
        "memory"
    }

    fn prepare<'a>(
        &'a self,
        index: &'a str,
        _settings: &'a IndexSettings,
    ) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            self.write(|indexes| {
                indexes.entry(index.to_owned()).or_default();
            });
            Ok(())
        })
    }

    fn upsert<'a>(
        &'a self,
        index: &'a str,
        settings: &'a IndexSettings,
        documents: Vec<Document>,
    ) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            let docs: Vec<(String, StoredDoc)> = documents
                .into_iter()
                .map(|d| (d.id().to_owned(), stored(settings, d)))
                .collect();
            self.write(|indexes| {
                let target = indexes.entry(index.to_owned()).or_default();
                target.docs.extend(docs);
            });
            Ok(())
        })
    }

    fn delete<'a>(
        &'a self,
        index: &'a str,
        ids: &'a [String],
    ) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            self.write(|indexes| {
                if let Some(target) = indexes.get_mut(index) {
                    for id in ids {
                        target.docs.remove(id);
                    }
                }
            });
            Ok(())
        })
    }

    fn flush<'a>(&'a self, index: &'a str) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            self.write(|indexes| {
                if let Some(target) = indexes.get_mut(index) {
                    target.docs.clear();
                }
            });
            Ok(())
        })
    }

    fn drop_index<'a>(&'a self, index: &'a str) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            self.write(|indexes| indexes.remove(index));
            Ok(())
        })
    }

    fn swap<'a>(
        &'a self,
        index: &'a str,
        staged: &'a str,
    ) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            self.write(|indexes| {
                let staged = indexes.remove(staged).unwrap_or_default();
                indexes.insert(index.to_owned(), staged);
            });
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
            let matches: Vec<(String, f64, StoredDoc)> = self.read(|indexes| {
                let Some(target) = indexes.get(index) else {
                    return Vec::new();
                };
                target
                    .docs
                    .iter()
                    .filter(|(_, doc)| query.filter.matches(&doc.values))
                    .filter_map(|(id, doc)| {
                        score(doc, &terms, settings).map(|s| (id.clone(), s, doc.clone()))
                    })
                    .collect()
            });
            let mut matches = matches;
            matches.sort_by(|(a_id, a_score, a), (b_id, b_score, b)| {
                for key in query.sort {
                    let order = compare_sort_values(
                        a.values.get(&key.field),
                        b.values.get(&key.field),
                        key.descending,
                    );
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
                .map(|(id, score, doc)| {
                    let highlights = match query.highlight {
                        Some(tags) if !terms.is_empty() => doc
                            .texts
                            .iter()
                            .filter_map(|(field, text, _)| {
                                highlight(text, &terms, tags).map(|h| (field.clone(), h))
                            })
                            .collect(),
                        _ => BTreeMap::new(),
                    };
                    let title = query.with_title.then(|| title_of(&doc, settings)).flatten();
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
        Box::pin(async move {
            Ok(self.read(|indexes| indexes.get(index).map_or(0, |i| i.docs.len() as u64)))
        })
    }

    fn contains<'a>(
        &'a self,
        index: &'a str,
        id: &'a str,
    ) -> BoxFuture<'a, Result<bool, SearchError>> {
        Box::pin(async move {
            Ok(self.read(|indexes| indexes.get(index).is_some_and(|i| i.docs.contains_key(id))))
        })
    }
}

fn title_of(doc: &StoredDoc, settings: &IndexSettings) -> Option<String> {
    let field = settings.title_field()?;
    doc.texts
        .iter()
        .find(|(name, _, _)| name == field)
        .map(|(_, text, _)| text.clone())
}
