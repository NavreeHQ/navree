//! Query builders: [`SearchQuery`], [`SimilarQuery`] and [`AcrossQuery`],
//! and their results, [`Hit`] and [`AcrossHit`].

use std::any::TypeId;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::marker::PhantomData;
use std::ops::RangeBounds;
use std::sync::Arc;

use navree_access::{Ability, Accessible, ActionMarker, Subject};
use navree_db::{Page, PageQuery, Uow};
use sea_orm::sea_query::SimpleExpr;
use sea_orm::{ColumnTrait, QueryFilter};
use serde::{Serialize, Serializer};
use serde_json::Value;
use uuid::Uuid;

use crate::engine::{EngineHit, EngineQuery, SortBy};
use crate::filter::{AccessFilter, access_filter};
use crate::searchable::{key_column, key_value, primary_key_string};
use crate::service::Entry;
use crate::settings::WORKSPACE_FIELD;
use crate::text::HighlightTags;
use crate::{Filter, Search, SearchError, Searchable};

/// A record found by a search, with its relevance and highlights.
///
/// Serializes as the record's fields plus `_score` and `_highlights`.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit<T> {
    /// The record (loaded from the database).
    pub record: T,
    /// Relevance from 0 to 1, higher is better.
    pub score: f64,
    /// Text field → its text, HTML-escaped, with the highlight tags around
    /// the matches. Fields without a match are left out.
    pub highlights: BTreeMap<String, String>,
}

impl<T> Hit<T> {
    /// Converts the record (a model into a resource), keeping the score and
    /// highlights.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Hit<U> {
        Hit {
            record: f(self.record),
            score: self.score,
            highlights: self.highlights,
        }
    }
}

impl<T: Serialize> Serialize for Hit<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let record = serde_json::to_value(&self.record).map_err(serde::ser::Error::custom)?;
        let mut object = match record {
            Value::Object(object) => object,
            other => {
                let mut object = serde_json::Map::new();
                object.insert("record".to_owned(), other);
                object
            }
        };
        object.insert("_score".to_owned(), Value::from(self.score));
        object.insert(
            "_highlights".to_owned(),
            serde_json::to_value(&self.highlights).map_err(serde::ser::Error::custom)?,
        );
        object.serialize(serializer)
    }
}

/// An access scope taken from an `Accessible<E, A>` extractor.
#[derive(Clone)]
pub(crate) struct Access {
    ability: Arc<Ability>,
    action: &'static str,
    subject: &'static str,
    condition: SimpleExpr,
}

impl Access {
    fn of<E: Searchable + Subject, A: ActionMarker>(scope: &Accessible<E, A>) -> Self {
        // Kept generic over the action so `include` and `accessible` take
        // any `Accessible<E, A>`.
        Self {
            ability: scope.ability().clone(),
            action: A::NAME,
            subject: E::SUBJECT,
            condition: scope.condition(),
        }
    }
}

/// What a query asks of one index, checked against its settings.
struct Plan {
    entry: Arc<Entry>,
    /// The normalized filter.
    filter: Filter,
    /// Access conditions the engine cannot apply: filter the rows with them.
    rows: Option<SimpleExpr>,
}

impl Plan {
    fn new(
        entry: Arc<Entry>,
        filters: &[Filter],
        sort: &[SortBy],
        access: Option<&Access>,
        workspace: Option<&str>,
    ) -> Result<Plan, SearchError> {
        let settings = &entry.settings;
        for filter in filters {
            for field in filter.fields() {
                if !settings.is_filterable(field) {
                    return Err(SearchError::NotFilterable {
                        index: settings.index().to_owned(),
                        field: field.to_owned(),
                    });
                }
            }
        }
        for key in sort {
            if !settings.is_sortable(&key.field) {
                return Err(SearchError::NotSortable {
                    index: settings.index().to_owned(),
                    field: key.field.clone(),
                });
            }
        }
        let mut all = filters.to_vec();
        if settings.workspace_column().is_some() {
            let workspace = workspace.ok_or_else(|| SearchError::WorkspaceRequired {
                index: settings.index().to_owned(),
            })?;
            all.push(Filter::eq(WORKSPACE_FIELD, workspace));
        }
        let mut rows = None;
        if let Some(access) = access {
            match access_filter(&access.ability, access.action, access.subject, settings) {
                AccessFilter::Exact(filter) => all.push(filter),
                AccessFilter::Rows => rows = Some(access.condition.clone()),
            }
        }
        Ok(Plan {
            entry,
            filter: Filter::All(all).normalized(),
            rows,
        })
    }

    fn matches_nothing(&self) -> bool {
        self.filter == Filter::Const(false)
    }
}

/// A full-text query on one entity's index, built by
/// [`Search::query`](crate::Search::query).
///
/// ```ignore
/// let deals = search
///     .query::<Deal>("renewal")
///     .filter("stage", "negotiation")
///     .filter_in("owner_id", [ada_id, grace_id])
///     .range("amount_cents", 1_000_000..)
///     .sort_desc("amount_cents")
///     .paginate(page)
///     .await?;
/// ```
pub struct SearchQuery<E> {
    search: Search,
    text: String,
    filters: Vec<Filter>,
    sort: Vec<SortBy>,
    access: Option<Access>,
    workspace: Option<String>,
    hybrid: Option<f32>,
    tags: HighlightTags,
    _entity: PhantomData<fn() -> E>,
}

impl<E: Searchable> SearchQuery<E> {
    pub(crate) fn new(search: Search, text: String) -> Self {
        Self {
            search,
            text,
            filters: Vec::new(),
            sort: Vec::new(),
            access: None,
            workspace: None,
            hybrid: None,
            tags: HighlightTags::default(),
            _entity: PhantomData,
        }
    }

    /// Only records whose `field` equals `value`.
    pub fn filter(mut self, field: &str, value: impl Serialize) -> Self {
        self.filters.push(Filter::eq(field, value));
        self
    }

    /// Only records whose `field` does not equal `value` (records without
    /// a value included).
    pub fn filter_not(mut self, field: &str, value: impl Serialize) -> Self {
        self.filters.push(Filter::ne(field, value));
        self
    }

    /// Only records whose `field` equals one of `values`.
    pub fn filter_in<I, V>(mut self, field: &str, values: I) -> Self
    where
        I: IntoIterator<Item = V>,
        V: Serialize,
    {
        self.filters.push(Filter::is_in(field, values));
        self
    }

    /// Only records whose `field` is inside `range` (`10..20`, `10..`,
    /// `..=20`; numbers and dates).
    pub fn range<T: Serialize>(mut self, field: &str, range: impl RangeBounds<T>) -> Self {
        self.filters.push(Filter::range(field, range));
        self
    }

    /// Only records without a value in `field`.
    pub fn filter_null(mut self, field: &str) -> Self {
        self.filters.push(Filter::null(field));
        self
    }

    /// Only records matching `filter` (any combination).
    pub fn filter_by(mut self, filter: Filter) -> Self {
        self.filters.push(filter);
        self
    }

    /// Sorts by `field`, lowest first; relevance breaks ties.
    pub fn sort_asc(mut self, field: &str) -> Self {
        self.sort.push(SortBy {
            field: field.to_owned(),
            descending: false,
        });
        self
    }

    /// Sorts by `field`, highest first; relevance breaks ties.
    pub fn sort_desc(mut self, field: &str) -> Self {
        self.sort.push(SortBy {
            field: field.to_owned(),
            descending: true,
        });
        self
    }

    /// Only records the scope's principal may access: rules on filterable
    /// fields become engine filters, others filter the loaded rows.
    pub fn accessible<A: ActionMarker>(mut self, scope: Accessible<E, A>) -> Self
    where
        E: Subject,
    {
        self.access = Some(Access::of(&scope));
        self
    }

    /// Only records of workspace `id` (required for indexes scoped to
    /// workspaces).
    pub fn workspace(mut self, id: Uuid) -> Self {
        self.workspace = Some(id.to_string());
        self
    }

    /// Blends keyword and vector relevance: 0.0 is keywords only, 1.0
    /// vectors only. Needs `search.vectors`.
    pub fn hybrid(mut self, weight: f32) -> Self {
        self.hybrid = Some(weight.clamp(0.0, 1.0));
        self
    }

    /// The tags around matches in highlights (default `<mark>`, `</mark>`).
    pub fn highlight_tags(mut self, open: &str, close: &str) -> Self {
        self.tags = HighlightTags {
            open: open.to_owned(),
            close: close.to_owned(),
        };
        self
    }

    /// The page `page` of matching records, in relevance (or sort) order.
    pub async fn paginate(self, page: PageQuery) -> Result<Page<E::Model>, SearchError> {
        let page = PageQuery::new(page.page, page.per_page);
        let (hits, total, rows) = self.run(page.offset(), page.per_page, false).await?;
        let models = load_in_order::<E>(&self.search, hits, rows).await?;
        Ok(Page::new(
            models.into_iter().map(|h| h.record).collect(),
            page,
            total,
        ))
    }

    /// Like [`paginate`](SearchQuery::paginate), with each record's score
    /// and highlights.
    pub async fn hits(self, page: PageQuery) -> Result<Page<Hit<E::Model>>, SearchError> {
        let page = PageQuery::new(page.page, page.per_page);
        let (hits, total, rows) = self.run(page.offset(), page.per_page, true).await?;
        let models = load_in_order::<E>(&self.search, hits, rows).await?;
        Ok(Page::new(models, page, total))
    }

    /// The primary keys of the first `limit` matches.
    pub async fn ids(self, limit: u64) -> Result<Vec<String>, SearchError> {
        let (hits, _, rows) = self.run(0, limit, false).await?;
        let ids: Vec<String> = hits.into_iter().map(|h| h.id).collect();
        match rows {
            None => Ok(ids),
            Some(condition) => {
                let entry = self.search.entry::<E>()?;
                let readable = entry
                    .ops
                    .readable_ids(self.search.db()?, &ids, condition)
                    .await?;
                Ok(ids.into_iter().filter(|id| readable.contains(id)).collect())
            }
        }
    }

    /// How many records match (the engine's count; access conditions on
    /// fields the index does not hold are not counted out).
    pub async fn count(self) -> Result<u64, SearchError> {
        Ok(self.run(0, 0, false).await?.1)
    }

    /// Runs the query on the engine: hits `offset..offset + limit`, the
    /// total, and the row condition still to apply.
    async fn run(
        &self,
        offset: u64,
        limit: u64,
        highlight: bool,
    ) -> Result<(Vec<EngineHit>, u64, Option<SimpleExpr>), SearchError> {
        let entry = self.search.entry::<E>()?;
        let plan = Plan::new(
            entry,
            &self.filters,
            &self.sort,
            self.access.as_ref(),
            self.workspace.as_deref(),
        )?;
        if plan.matches_nothing() {
            return Ok((Vec::new(), 0, None));
        }
        let tags = highlight.then_some(&self.tags);
        let (hits, total) = match self.hybrid {
            Some(weight) if weight > 0.0 && !self.text.trim().is_empty() => {
                self.run_hybrid(&plan, weight, offset, limit, tags).await?
            }
            _ => {
                let results = self
                    .search
                    .engine()
                    .search(
                        &plan.entry.index,
                        EngineQuery {
                            settings: &plan.entry.settings,
                            text: &self.text,
                            filter: &plan.filter,
                            sort: &self.sort,
                            offset,
                            limit,
                            highlight: tags,
                            with_title: false,
                        },
                    )
                    .await?;
                (results.hits, results.total)
            }
        };
        Ok((hits, total, plan.rows))
    }

    /// Keyword and vector hits blended: the best candidates of each side
    /// (at least 100, or as many as the page needs) are scored
    /// `(1 - weight) * keyword + weight * vector`.
    async fn run_hybrid(
        &self,
        plan: &Plan,
        weight: f32,
        offset: u64,
        limit: u64,
        tags: Option<&HighlightTags>,
    ) -> Result<(Vec<EngineHit>, u64), SearchError> {
        let vectors = self
            .search
            .inner
            .vectors
            .as_ref()
            .ok_or(SearchError::VectorsOff)?;
        let window = (offset + limit).max(100);
        let weight = f64::from(weight);
        let keyword = if weight < 1.0 {
            self.search
                .engine()
                .search(
                    &plan.entry.index,
                    EngineQuery {
                        settings: &plan.entry.settings,
                        text: &self.text,
                        filter: &plan.filter,
                        sort: &[],
                        offset: 0,
                        limit: window,
                        highlight: tags,
                        with_title: false,
                    },
                )
                .await?
                .hits
        } else {
            Vec::new()
        };
        let vector = vectors.embed_one(&self.text).await?;
        let nearest = vectors
            .nearest(&plan.entry.index, &vector, &plan.filter, window)
            .await?;
        let mut merged: HashMap<String, EngineHit> = HashMap::new();
        for hit in keyword {
            let score = hit.score * (1.0 - weight);
            merged.insert(hit.id.clone(), EngineHit { score, ..hit });
        }
        for found in nearest {
            let entry = merged.entry(found.id.clone()).or_insert_with(|| EngineHit {
                id: found.id.clone(),
                score: 0.0,
                highlights: BTreeMap::new(),
                title: None,
            });
            entry.score += found.score * weight;
        }
        let mut hits: Vec<EngineHit> = merged.into_values().collect();
        hits.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
        let total = hits.len() as u64;
        let hits = hits
            .into_iter()
            .skip(usize::try_from(offset).unwrap_or(usize::MAX))
            .take(usize::try_from(limit).unwrap_or(usize::MAX))
            .collect();
        Ok((hits, total))
    }
}

impl<E> fmt::Debug for SearchQuery<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SearchQuery")
            .field("entity", &std::any::type_name::<E>())
            .field("text", &self.text)
            .field("filters", &self.filters)
            .field("sort", &self.sort)
            .field("workspace", &self.workspace)
            .field("hybrid", &self.hybrid)
            .finish_non_exhaustive()
    }
}

/// Loads the records of `hits` in one query (filtered by `rows` when
/// given), keeping the hits' order and dropping records that are gone.
pub(crate) async fn load_in_order<E: Searchable>(
    search: &Search,
    hits: Vec<EngineHit>,
    rows: Option<SimpleExpr>,
) -> Result<Vec<Hit<E::Model>>, SearchError> {
    if hits.is_empty() {
        return Ok(Vec::new());
    }
    let db = search.db()?;
    let column = key_column::<E>()?;
    let values: Vec<_> = hits
        .iter()
        .filter_map(|h| key_value(&column, &h.id))
        .collect();
    let mut select = E::find().filter(column.is_in(values));
    if let Some(condition) = rows {
        select = select.filter(condition);
    }
    let models = select.all(&db.reader().await).await?;
    let mut by_id: HashMap<String, E::Model> = models
        .into_iter()
        .map(|m| (primary_key_string(&m), m))
        .collect();
    Ok(hits
        .into_iter()
        .filter_map(|hit| {
            by_id.remove(&hit.id).map(|record| Hit {
                record,
                score: hit.score,
                highlights: hit.highlights,
            })
        })
        .collect())
}

enum SimilarSource {
    Text(String),
    Record(String),
}

/// A vector query: the records nearest in meaning to a text or to another
/// record, built by [`Search::similar`](crate::Search::similar) and
/// [`Search::similar_to`](crate::Search::similar_to).
pub struct SimilarQuery<E> {
    search: Search,
    source: SimilarSource,
    filters: Vec<Filter>,
    access: Option<Access>,
    workspace: Option<String>,
    limit: u64,
    _entity: PhantomData<fn() -> E>,
}

impl<E: Searchable> SimilarQuery<E> {
    pub(crate) fn text(search: Search, text: String) -> Self {
        Self::with(search, SimilarSource::Text(text))
    }

    pub(crate) fn record(search: Search, id: String) -> Self {
        Self::with(search, SimilarSource::Record(id))
    }

    fn with(search: Search, source: SimilarSource) -> Self {
        Self {
            search,
            source,
            filters: Vec::new(),
            access: None,
            workspace: None,
            limit: 10,
            _entity: PhantomData,
        }
    }

    /// Only records whose `field` equals `value`.
    pub fn filter(mut self, field: &str, value: impl Serialize) -> Self {
        self.filters.push(Filter::eq(field, value));
        self
    }

    /// Only records whose `field` does not equal `value`.
    pub fn filter_not(mut self, field: &str, value: impl Serialize) -> Self {
        self.filters.push(Filter::ne(field, value));
        self
    }

    /// Only records whose `field` equals one of `values`.
    pub fn filter_in<I, V>(mut self, field: &str, values: I) -> Self
    where
        I: IntoIterator<Item = V>,
        V: Serialize,
    {
        self.filters.push(Filter::is_in(field, values));
        self
    }

    /// Only records whose `field` is inside `range`.
    pub fn range<T: Serialize>(mut self, field: &str, range: impl RangeBounds<T>) -> Self {
        self.filters.push(Filter::range(field, range));
        self
    }

    /// Only records without a value in `field`.
    pub fn filter_null(mut self, field: &str) -> Self {
        self.filters.push(Filter::null(field));
        self
    }

    /// Only records the scope's principal may access.
    pub fn accessible<A: ActionMarker>(mut self, scope: Accessible<E, A>) -> Self
    where
        E: Subject,
    {
        self.access = Some(Access::of(&scope));
        self
    }

    /// Only records of workspace `id`.
    pub fn workspace(mut self, id: Uuid) -> Self {
        self.workspace = Some(id.to_string());
        self
    }

    /// At most `limit` records (default 10).
    pub fn limit(mut self, limit: u64) -> Self {
        self.limit = limit;
        self
    }

    /// The nearest records, nearest first, with their similarity as score
    /// (0 to 1).
    pub async fn run(self) -> Result<Vec<Hit<E::Model>>, SearchError> {
        let vectors = self
            .search
            .inner
            .vectors
            .as_ref()
            .ok_or(SearchError::VectorsOff)?;
        let entry = self.search.entry::<E>()?;
        let plan = Plan::new(
            entry,
            &self.filters,
            &[],
            self.access.as_ref(),
            self.workspace.as_deref(),
        )?;
        if plan.matches_nothing() {
            return Ok(Vec::new());
        }
        let (vector, exclude) = match &self.source {
            SimilarSource::Text(text) => (vectors.embed_one(text).await?, None),
            SimilarSource::Record(id) => match vectors.get(&plan.entry.index, id).await? {
                Some(vector) => (vector, Some(id.clone())),
                None => return Ok(Vec::new()),
            },
        };
        let extra = u64::from(exclude.is_some());
        let found = vectors
            .nearest(&plan.entry.index, &vector, &plan.filter, self.limit + extra)
            .await?;
        let hits: Vec<EngineHit> = found
            .into_iter()
            .filter(|m| exclude.as_deref() != Some(m.id.as_str()))
            .take(usize::try_from(self.limit).unwrap_or(usize::MAX))
            .map(|m| EngineHit {
                id: m.id,
                score: m.score,
                highlights: BTreeMap::new(),
                title: None,
            })
            .collect();
        load_in_order::<E>(&self.search, hits, plan.rows).await
    }
}

/// One result of an [`AcrossQuery`].
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AcrossHit {
    /// The `name` of the record's index (`Contact`).
    pub kind: String,
    /// The record's primary key.
    pub id: String,
    /// The value of the index's first text field.
    pub title: Option<String>,
    /// Relevance from 0 to 1.
    pub score: f64,
    /// Highlighted text fields.
    pub highlights: BTreeMap<String, String>,
}

impl AcrossHit {
    /// The whole record, read in `uow`'s transaction; `None` when the hit
    /// is of another kind or the record is gone.
    pub async fn load<E: Searchable>(&self, uow: &Uow) -> Result<Option<E::Model>, SearchError> {
        if self.kind != E::settings().name() {
            return Ok(None);
        }
        let column = key_column::<E>()?;
        let Some(value) = key_value(&column, &self.id) else {
            return Ok(None);
        };
        let tx = uow.tx().await?;
        Ok(E::find().filter(column.eq(value)).one(&*tx).await?)
    }
}

struct Include {
    entity: TypeId,
    name: &'static str,
    access: Option<Access>,
}

/// One query over several indexes (a command palette), built by
/// [`Search::across`](crate::Search::across): each included index is
/// queried in parallel and the hits are merged by score.
pub struct AcrossQuery {
    search: Search,
    text: String,
    includes: Vec<Include>,
    limit: u64,
    workspace: Option<String>,
    tags: HighlightTags,
}

impl AcrossQuery {
    pub(crate) fn new(search: Search, text: String) -> Self {
        Self {
            search,
            text,
            includes: Vec::new(),
            limit: 10,
            workspace: None,
            tags: HighlightTags::default(),
        }
    }

    /// Searches `E`'s index too, limited to what the scope's principal may
    /// access (`.include::<Contact>(scope)`).
    pub fn include<E>(mut self, scope: Accessible<E, impl ActionMarker>) -> Self
    where
        E: Searchable + Subject,
    {
        self.includes.push(Include {
            entity: TypeId::of::<E>(),
            name: std::any::type_name::<E>(),
            access: Some(Access::of(&scope)),
        });
        self
    }

    /// At most `limit` hits in all (default 10).
    pub fn limit(mut self, limit: u64) -> Self {
        self.limit = limit;
        self
    }

    /// Only records of workspace `id`.
    pub fn workspace(mut self, id: Uuid) -> Self {
        self.workspace = Some(id.to_string());
        self
    }

    /// The tags around matches in highlights.
    pub fn highlight_tags(mut self, open: &str, close: &str) -> Self {
        self.tags = HighlightTags {
            open: open.to_owned(),
            close: close.to_owned(),
        };
        self
    }

    /// The best `limit` hits over every included index, best first.
    pub async fn run(self) -> Result<Vec<AcrossHit>, SearchError> {
        let mut searches = Vec::new();
        for include in &self.includes {
            let entry = self
                .search
                .entry_of_type(include.entity)
                .ok_or(SearchError::NotRegistered(include.name))?;
            let plan = Plan::new(
                entry,
                &[],
                &[],
                include.access.as_ref(),
                self.workspace.as_deref(),
            )?;
            searches.push(self.run_one(plan));
        }
        let mut hits: Vec<AcrossHit> = futures::future::try_join_all(searches)
            .await?
            .into_iter()
            .flatten()
            .collect();
        hits.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
        hits.truncate(usize::try_from(self.limit).unwrap_or(usize::MAX));
        Ok(hits)
    }

    async fn run_one(&self, plan: Plan) -> Result<Vec<AcrossHit>, SearchError> {
        if plan.matches_nothing() {
            return Ok(Vec::new());
        }
        let results = self
            .search
            .engine()
            .search(
                &plan.entry.index,
                EngineQuery {
                    settings: &plan.entry.settings,
                    text: &self.text,
                    filter: &plan.filter,
                    sort: &[],
                    offset: 0,
                    limit: self.limit,
                    highlight: Some(&self.tags),
                    with_title: true,
                },
            )
            .await?;
        let mut hits = results.hits;
        if let Some(condition) = plan.rows {
            let ids: Vec<String> = hits.iter().map(|h| h.id.clone()).collect();
            let readable = plan
                .entry
                .ops
                .readable_ids(self.search.db()?, &ids, condition)
                .await?;
            hits.retain(|h| readable.contains(&h.id));
        }
        let kind = plan.entry.settings.name().to_owned();
        Ok(hits
            .into_iter()
            .map(|hit| AcrossHit {
                kind: kind.clone(),
                id: hit.id,
                title: hit.title,
                score: hit.score,
                highlights: hit.highlights,
            })
            .collect())
    }
}

impl fmt::Debug for AcrossQuery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<&str> = self.includes.iter().map(|i| i.name).collect();
        f.debug_struct("AcrossQuery")
            .field("text", &self.text)
            .field("includes", &names)
            .field("limit", &self.limit)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[derive(Serialize)]
    struct Contact {
        id: u32,
        name: &'static str,
    }

    #[test]
    fn hits_serialize_as_the_record_plus_score_and_highlights() {
        let hit = Hit {
            record: Contact { id: 1, name: "Ada" },
            score: 0.5,
            highlights: BTreeMap::from([("name".to_owned(), "<mark>Ada</mark>".to_owned())]),
        };
        assert_eq!(
            serde_json::to_value(&hit).unwrap(),
            json!({"id": 1, "name": "Ada", "_score": 0.5, "_highlights": {"name": "<mark>Ada</mark>"}})
        );
        let mapped = hit.map(|c| c.name);
        assert_eq!(mapped.record, "Ada");
        assert_eq!(
            serde_json::to_value(&mapped).unwrap()["record"],
            json!("Ada")
        );
    }
}
