//! The table's backend: a [`TableQuery`] runs through `navree-orm`, on PostgreSQL,
//! MySQL, SQLite and MongoDB. Pass the app's scoped query as the builder: every
//! read, export and bulk action starts from it, so workspace scoping and row-level
//! security hold on all of them.
//!
//! - **Order.** NULL is the smallest value on every store: ascending NULLs first,
//!   descending NULLs last. MySQL, SQLite and MongoDB sort that way by themselves;
//!   PostgreSQL gets it spelled out, and one index on `(col NULLS FIRST, key)` then
//!   serves both directions. The key is always the last term, in the direction of
//!   the term before it, so ties have one order and the index is walked one way.
//! - **Cursor pages.** `col > v OR (col = v AND next...)`, with the first term also
//!   as a plain bound so the database starts in the right place. Where NULLs sit
//!   ahead of the position they're a second query, not an `OR` that would make the
//!   database scan from the start of the index.
//! - **Deep numbered pages** read from whichever end of the index is nearer.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use bytes::Bytes;
use http::header::{CONTENT_DISPOSITION, CONTENT_TYPE};
use http::{HeaderValue, StatusCode};
use navree_http::body::Body;
use navree_http::response::{IntoResponse, Response};
use navree_orm::ir::Order;
use navree_orm::{
    Access, Builder, Dialect, Executor, Filter, Model, Record, Result, Value, col, col_as,
};
use serde_json::{Value as Json, json};

use super::query::{FilterValue, SearchMode, SortTerm};
use super::{
    BulkMode, BulkRefusal, BulkRequest, Column, Count, Direction, FilterBar, FilterKind, SavedView,
    TablePage, TableQuery, TableRow, Total, ValueKind, write_csv_field,
};

/// The most rows one export can ever write, whatever [`TableQuery::export_limit`] asks.
/// A larger result is cut, and the response says so in `x-navree-truncated`.
pub const MAX_EXPORT_ROWS: u64 = 2_000_000;
/// The rows an export writes unless [`TableQuery::export_limit`] raises it, up to
/// [`MAX_EXPORT_ROWS`]. A larger result is cut, and the response says so in
/// `x-navree-truncated`.
pub const DEFAULT_EXPORT_ROWS: u64 = 100_000;
/// Rows read from the database at a time while exporting.
const EXPORT_CHUNK: u32 = 2_000;
/// Bytes an export keeps in memory before it spills to a temporary file.
const EXPORT_MEMORY: usize = 8 * 1024 * 1024;

/// How deep a page has to be before its keys are found first.
const DEFERRED_FROM: u64 = 5_000;

/// A value from a URL, bound as what the column holds.
fn bind(kind: ValueKind, text: &str) -> Option<Value> {
    match kind {
        ValueKind::Text => Some(Value::String(text.to_owned())),
        ValueKind::Int => text.parse().ok().map(Value::I64),
        ValueKind::Money => crate::form::Money::parse(text).map(|money| Value::I64(money.0)),
        ValueKind::Float => text
            .parse()
            .ok()
            .filter(|value: &f64| value.is_finite())
            .map(Value::F64),
        ValueKind::Decimal => text.parse().ok().map(Value::Decimal),
        ValueKind::Date => chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d")
            .ok()
            .map(Value::Date),
        ValueKind::DateTime => {
            let day = chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").ok()?;
            Some(Value::Timestamp(day.and_hms_opt(0, 0, 0)?.and_utc()))
        }
        ValueKind::Bool => matches!(text, "true" | "false").then(|| Value::Bool(text == "true")),
        ValueKind::Uuid => uuid::Uuid::parse_str(text).ok().map(Value::Uuid),
    }
}

/// The filter of one column.
fn filter_of(column: &'static Column, value: &FilterValue) -> Option<Filter> {
    let source = col(column.source);
    match value {
        FilterValue::Empty => Some(source.is_null()),
        FilterValue::Bool(flag) => Some(source.eq(Value::Bool(*flag))),
        FilterValue::Text(text) => {
            Some(col_as::<String>(column.source).contains_ignore_case(text.clone()))
        }
        FilterValue::Values(values) => {
            let bound: Vec<Value> = values
                .iter()
                .filter_map(|value| bind(column.kind, value))
                .collect();
            if bound.is_empty() {
                return None;
            }
            Some(if column.filter == FilterKind::Select && bound.len() == 1 {
                source.eq(bound.into_iter().next()?)
            } else {
                source.is_in(bound)
            })
        }
        FilterValue::Range { min, max } => {
            let mut parts = Vec::with_capacity(2);
            if let Some(low) = min.as_deref().and_then(|text| bind(column.kind, text)) {
                parts.push(source.gte(low));
            }
            if let Some(text) = max.as_deref() {
                // The last day of an instant's range runs to its end.
                if column.kind == ValueKind::DateTime {
                    let next = chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d")
                        .ok()
                        .and_then(|day| day.succ_opt())
                        .and_then(|day| day.and_hms_opt(0, 0, 0));
                    if let Some(next) = next {
                        parts.push(source.lt(Value::Timestamp(next.and_utc())));
                    }
                } else if let Some(high) = bind(column.kind, text) {
                    parts.push(source.lte(high));
                }
            }
            if parts.is_empty() {
                None
            } else {
                Some(Filter::all(parts))
            }
        }
    }
}

/// A sort value in a cursor.
fn tag(value: &Value) -> Option<Json> {
    Some(match value {
        Value::Null(_) => json!(["n"]),
        Value::Bool(flag) => json!(["b", flag]),
        Value::I8(number) => json!(["i", number]),
        Value::I16(number) => json!(["i", number]),
        Value::I32(number) => json!(["i", number]),
        Value::I64(number) => json!(["i", number]),
        Value::U8(number) => json!(["i", number]),
        Value::U16(number) => json!(["i", number]),
        Value::U32(number) => json!(["i", number]),
        Value::U64(number) => json!(["i", number]),
        Value::F32(number) => json!(["f", number]),
        Value::F64(number) => json!(["f", number]),
        Value::Decimal(number) => json!(["e", number.to_string()]),
        Value::String(text) => json!(["s", text]),
        Value::Uuid(id) => json!(["u", id.to_string()]),
        Value::Date(day) => json!(["d", day.to_string()]),
        Value::DateTime(at) => json!(["l", at.format("%Y-%m-%dT%H:%M:%S%.f").to_string()]),
        Value::Timestamp(at) => json!(["t", at.to_rfc3339()]),
        Value::TimestampTz(at) => json!(["t", at.to_rfc3339()]),
        _ => return None,
    })
}

/// Reads a sort value back, but only as the kind its column holds: a cursor is the
/// user's to edit, and a value of the wrong type shouldn't reach the database.
fn untag(tagged: &Json, kind: ValueKind, nullable: bool) -> Option<Value> {
    let parts = tagged.as_array()?;
    let name = parts.first()?.as_str()?;
    let body = parts.get(1);
    let text = || body.and_then(Json::as_str);
    let ok = match kind {
        ValueKind::Text => name == "s",
        ValueKind::Int | ValueKind::Money => name == "i",
        ValueKind::Float => name == "f" || name == "i",
        ValueKind::Decimal => name == "e",
        ValueKind::Date => name == "d",
        ValueKind::DateTime => matches!(name, "t" | "l"),
        ValueKind::Bool => name == "b",
        ValueKind::Uuid => name == "u",
    };
    if name == "n" {
        return nullable.then_some(Value::Null(navree_orm::Kind::String));
    }
    if !ok {
        return None;
    }
    match name {
        "b" => body?.as_bool().map(Value::Bool),
        "i" => body?.as_i64().map(Value::I64),
        "f" => body?.as_f64().map(Value::F64),
        "e" => text()?.parse().ok().map(Value::Decimal),
        "s" => text().map(|text| Value::String(text.to_owned())),
        "u" => uuid::Uuid::parse_str(text()?).ok().map(Value::Uuid),
        "d" => chrono::NaiveDate::parse_from_str(text()?, "%Y-%m-%d")
            .ok()
            .map(Value::Date),
        "l" => chrono::NaiveDateTime::parse_from_str(text()?, "%Y-%m-%dT%H:%M:%S%.f")
            .ok()
            .map(Value::DateTime),
        "t" => chrono::DateTime::parse_from_rfc3339(text()?)
            .ok()
            .map(|at| Value::Timestamp(at.to_utc())),
        _ => None,
    }
}

/// One term of the order, as queried.
#[derive(Clone, Copy)]
struct Term {
    source: &'static str,
    /// The name a model or an extra holds it under.
    field: &'static str,
    kind: ValueKind,
    nullable: bool,
    direction: Direction,
}

impl Term {
    fn flipped(self) -> Self {
        Self {
            direction: if self.direction == Direction::Asc {
                Direction::Desc
            } else {
                Direction::Asc
            },
            ..self
        }
    }

    fn order(self, dialect: Option<Dialect>) -> Order {
        let source = col(self.source);
        let order = if self.direction == Direction::Asc {
            source.asc()
        } else {
            source.desc()
        };
        // The other stores already sort NULL as the smallest value. A column that
        // can't hold NULL says nothing, so it still matches a plain index.
        match (dialect, self.direction, self.nullable) {
            (Some(Dialect::Postgres), Direction::Asc, true) => order.nulls_first(),
            (Some(Dialect::Postgres), Direction::Desc, true) => order.nulls_last(),
            _ => order,
        }
    }

    fn is_null(value: &Value) -> bool {
        matches!(value, Value::Null(_))
    }

    /// Where this term equals `value`.
    fn equal(self, value: &Value) -> Filter {
        if Self::is_null(value) {
            col(self.source).is_null()
        } else {
            col(self.source).eq(value.clone())
        }
    }

    /// Where this term is strictly past `value`, in order.
    fn past(self, value: &Value) -> Option<Filter> {
        let source = col(self.source);
        match (self.direction, Self::is_null(value)) {
            (Direction::Asc, true) => Some(source.is_not_null()),
            (Direction::Asc, false) => Some(source.gt(value.clone())),
            (Direction::Desc, true) => None,
            (Direction::Desc, false) if self.nullable => {
                Some(Filter::any([source.lt(value.clone()), source.is_null()]))
            }
            (Direction::Desc, false) => Some(source.lt(value.clone())),
        }
    }
}

/// `t1 > v1 OR (t1 = v1 AND t2 > v2) ...` from `at` on.
fn past_from(terms: &[Term], values: &[Value], at: usize) -> Option<Filter> {
    let term = *terms.get(at)?;
    let value = values.get(at)?;
    let tail = past_from(terms, values, at + 1).map(|tail| Filter::all([term.equal(value), tail]));
    match (term.past(value), tail) {
        (Some(here), Some(tail)) => Some(Filter::any([here, tail])),
        (Some(here), None) => Some(here),
        (None, Some(tail)) => Some(tail),
        (None, None) => None,
    }
}

/// The rows past a position: a first condition, and, when NULLs of the first term
/// lie ahead, a second one to read after it runs out.
fn keyset(terms: &[Term], values: &[Value]) -> (Option<Filter>, Option<Filter>) {
    let (Some(first), Some(value)) = (terms.first().copied(), values.first()) else {
        return (None, None);
    };
    let source = col(first.source);
    let tail = past_from(terms, values, 1);
    let null = Term::is_null(value);
    match (first.direction, null) {
        (Direction::Asc, true) => {
            // Through the NULLs, then every value.
            let here = tail.map_or_else(
                || source.is_null(),
                |tail| Filter::all([source.is_null(), tail]),
            );
            (Some(here), Some(source.is_not_null()))
        }
        (Direction::Desc, true) => (tail.map(|tail| Filter::all([source.is_null(), tail])), None),
        (direction, false) => {
            let bound = if direction == Direction::Asc {
                source.gte(value.clone())
            } else {
                source.lte(value.clone())
            };
            let strict = if direction == Direction::Asc {
                source.gt(value.clone())
            } else {
                source.lt(value.clone())
            };
            let within = tail.map_or_else(
                || strict.clone(),
                |tail| {
                    Filter::any([
                        strict.clone(),
                        Filter::all([source.eq(value.clone()), tail]),
                    ])
                },
            );
            let after_nulls =
                (direction == Direction::Desc && first.nullable).then(|| source.is_null());
            (Some(Filter::all([bound, within])), after_nulls)
        }
    }
}

fn encode_cursor(values: &[Value]) -> Option<String> {
    let tagged: Vec<Json> = values.iter().map(tag).collect::<Option<_>>()?;
    Some(URL_SAFE_NO_PAD.encode(Json::Array(tagged).to_string()))
}

fn decode_cursor(text: &str, terms: &[Term]) -> Option<Vec<Value>> {
    let bytes = URL_SAFE_NO_PAD.decode(text).ok()?;
    let parsed: Json = serde_json::from_slice(&bytes).ok()?;
    let items = parsed.as_array()?;
    if items.len() != terms.len() {
        return None;
    }
    items
        .iter()
        .zip(terms)
        .map(|(item, term)| untag(item, term.kind, term.nullable))
        .collect()
}

/// The value of `term` in `record`.
fn value_of<M: Model>(record: &Record<M>, term: &Term) -> Option<Value> {
    if let Ok(Some(value)) = record.value(term.field) {
        return Some(value);
    }
    record
        .extras()
        .iter()
        .find(|(name, _)| name == term.field)
        .map(|(_, value)| value.clone())
}

fn text_of(value: &Value) -> Option<String> {
    tag(value).map(|tagged| tagged.to_string())
}

impl<R: TableRow, F: FilterBar> TableQuery<R, F> {
    /// The search and filters, on `builder`.
    pub fn apply<M: Model>(&self, mut builder: Builder<M>) -> Builder<M> {
        if let Some(q) = &self.state.q {
            // Only the columns marked `search` that this person may use.
            let searched = R::COLUMNS
                .iter()
                .enumerate()
                .filter(|(at, column)| column.search && !self.is_withdrawn(*at))
                .map(|(_, column)| column);
            match self.search_mode {
                SearchMode::Contains => {
                    let searches: Vec<Filter> = searched
                        .map(|column| {
                            col_as::<String>(column.source).contains_ignore_case(q.clone())
                        })
                        .collect();
                    if !searches.is_empty() {
                        builder = builder.where_(Filter::any(searches));
                    }
                }
                SearchMode::Words => {
                    let sources: Vec<&'static str> = searched.map(|column| column.source).collect();
                    if !sources.is_empty() {
                        builder = builder.where_full_text(sources, q.clone());
                    }
                }
            }
        }
        for active in &self.state.filters {
            if self.is_withdrawn(active.column) {
                continue;
            }
            if let Some(filter) = filter_of(&R::COLUMNS[active.column], &active.value) {
                builder = builder.where_(filter);
            }
        }
        builder
    }

    /// The key as a term of the order.
    fn key_term<M: Model>(direction: Direction) -> Term {
        let key = M::meta().key_column();
        Term {
            source: key,
            field: key,
            kind: R::CONFIG.key_kind,
            nullable: false,
            direction,
        }
    }

    /// The order's terms, the key last.
    fn terms<M: Model>(&self) -> Vec<Term> {
        let mut terms: Vec<Term> = self
            .state
            .sort
            .iter()
            .filter(|term| !self.is_withdrawn(term.column))
            .map(|SortTerm { column, direction }| {
                let column = &R::COLUMNS[*column];
                Term {
                    source: column.source,
                    field: column.source.rsplit('.').next().unwrap_or(column.source),
                    kind: column.kind,
                    nullable: column.nullable,
                    direction: *direction,
                }
            })
            .collect();
        let key = M::meta().key_column();
        let last = terms.last().map_or(Direction::Asc, |term| term.direction);
        if !terms.iter().any(|term| term.field == key) {
            terms.push(Self::key_term::<M>(last));
        }
        terms
    }

    fn ordered<M: Model>(
        terms: &[Term],
        dialect: Option<Dialect>,
        mut builder: Builder<M>,
    ) -> Builder<M> {
        builder = builder.reorder();
        for term in terms {
            builder = builder.order(term.order(dialect));
        }
        builder
    }

    /// One page of models, the query's rows being the rows.
    pub async fn load<E>(&self, builder: Builder<R>, exec: &E) -> Result<TablePage<R>>
    where
        R: Model,
        E: Executor + ?Sized,
    {
        self.load_as(builder, exec, Record::into_inner).await
    }

    /// One page, each record made into a row by `map`: for rows that aren't models,
    /// such as a model with joined or related names.
    pub async fn load_as<M, E>(
        &self,
        builder: Builder<M>,
        exec: &E,
        map: impl Fn(Record<M>) -> R,
    ) -> Result<TablePage<R>>
    where
        M: Model,
        E: Executor + ?Sized,
    {
        let dialect = exec.target(Access::Read).await?.dialect();
        let filtered = self.apply(builder);
        let terms = self.terms::<M>();
        let total = self.count_rows(filtered.clone(), exec).await?;
        let version = self.version_of(filtered.clone(), exec).await?;
        let mut page = if R::CONFIG.cursor {
            self.cursor_page(filtered, exec, dialect, &terms, total, map)
                .await?
        } else {
            self.number_page(filtered, exec, dialect, &terms, total, map)
                .await?
        };
        page.version = version;
        Ok(page)
    }

    async fn count_rows<M: Model, E: Executor + ?Sized>(
        &self,
        filtered: Builder<M>,
        exec: &E,
    ) -> Result<Total> {
        let count = if self.state.count {
            Count::Exact
        } else {
            R::CONFIG.count
        };
        Ok(match count {
            Count::Exact => Total::Exact(filtered.count(exec).await?),
            Count::None => Total::Unknown,
            Count::Capped(cap) => {
                let key = M::meta().key_column();
                let bounded = filtered
                    .reorder()
                    .select([key])
                    .limit(cap.saturating_add(1));
                let counted = navree_orm::TableBuilder::from_sub(bounded, "counted")
                    .count(exec)
                    .await?;
                if counted > cap {
                    Total::AtLeast(cap)
                } else {
                    Total::Exact(counted)
                }
            }
        })
    }

    async fn version_of<M: Model, E: Executor + ?Sized>(
        &self,
        filtered: Builder<M>,
        exec: &E,
    ) -> Result<Option<String>> {
        let Some(column) = R::CONFIG.version else {
            return Ok(None);
        };
        let value: Option<Value> = filtered.reorder().max(exec, col(column)).await?;
        Ok(value.as_ref().and_then(text_of))
    }

    async fn number_page<M: Model, E: Executor + ?Sized>(
        &self,
        filtered: Builder<M>,
        exec: &E,
        dialect: Option<Dialect>,
        terms: &[Term],
        total: Total,
        map: impl Fn(Record<M>) -> R,
    ) -> Result<TablePage<R>> {
        let per = u64::from(self.state.per);
        let mut page = self.state.page;
        if let Total::Exact(total) = total {
            // A page past the end, after rows went away, is the last page.
            let pages = u32::try_from(total.div_ceil(per).max(1)).unwrap_or(u32::MAX);
            page = page.min(pages);
            let offset = u64::from(page - 1) * per;
            if total == 0 {
                return Ok(TablePage {
                    rows: Vec::new(),
                    total: Total::Exact(0),
                    page: 1,
                    has_more: false,
                    next: None,
                    prev: None,
                    version: None,
                });
            }
            // Past halfway, read from the far end of the index.
            let far = offset > total / 2;
            let rows = if far {
                let flipped: Vec<Term> = terms.iter().map(|term| term.flipped()).collect();
                let take = per.min(total - offset);
                let skip = total - offset - take;
                let mut rows = Self::read_at(filtered, exec, dialect, &flipped, skip, take).await?;
                rows.reverse();
                rows
            } else {
                Self::read_at(filtered, exec, dialect, terms, offset, per).await?
            };
            let has_more = offset + (rows.len() as u64) < total;
            return Ok(TablePage {
                rows: rows.into_iter().map(map).collect(),
                total: Total::Exact(total),
                page,
                has_more,
                next: None,
                prev: None,
                version: None,
            });
        }
        // No exact count: one row more says whether a next page has rows.
        let ordered = Self::ordered(terms, dialect, filtered);
        let mut offset = u64::from(page - 1) * per;
        let mut rows = ordered
            .clone()
            .offset(offset)
            .limit(per + 1)
            .get(exec)
            .await?
            .into_vec();
        if rows.is_empty() && page > 1 {
            page = 1;
            offset = 0;
            rows = ordered
                .offset(offset)
                .limit(per + 1)
                .get(exec)
                .await?
                .into_vec();
        }
        let has_more = rows.len() as u64 > per;
        rows.truncate(self.state.per as usize);
        Ok(TablePage {
            rows: rows.into_iter().map(map).collect(),
            total,
            page,
            has_more,
            next: None,
            prev: None,
            version: None,
        })
    }

    /// `take` records after skipping `skip`. A deep page finds its keys first, where
    /// the database can stay in the index, and only then reads those rows. MySQL has
    /// no `LIMIT` in an `IN` subquery, and MongoDB no cheap equivalent.
    async fn read_at<M: Model, E: Executor + ?Sized>(
        filtered: Builder<M>,
        exec: &E,
        dialect: Option<Dialect>,
        terms: &[Term],
        skip: u64,
        take: u64,
    ) -> Result<Vec<Record<M>>> {
        let ordered = Self::ordered(terms, dialect, filtered.clone());
        if skip > DEFERRED_FROM
            && matches!(dialect, Some(Dialect::Postgres | Dialect::Sqlite))
            && terms.iter().all(|term| !term.source.contains('.'))
        {
            let key = M::meta().key_column();
            let keys = ordered.clone().select([key]).offset(skip).limit(take);
            return Ok(Self::ordered(terms, dialect, filtered)
                .where_(col(key).in_subquery(keys))
                .get(exec)
                .await?
                .into_vec());
        }
        Ok(ordered.offset(skip).limit(take).get(exec).await?.into_vec())
    }

    /// Up to `want` records past `position`, in `terms`' order.
    async fn read_past<M: Model, E: Executor + ?Sized>(
        filtered: &Builder<M>,
        exec: &E,
        dialect: Option<Dialect>,
        terms: &[Term],
        position: Option<&[Value]>,
        want: u64,
    ) -> Result<Vec<Record<M>>> {
        let ordered = Self::ordered(terms, dialect, filtered.clone());
        let Some(values) = position else {
            return Ok(ordered.limit(want).get(exec).await?.into_vec());
        };
        let (first, second) = keyset(terms, values);
        let mut rows = match first {
            Some(filter) => ordered
                .clone()
                .where_(filter)
                .limit(want)
                .get(exec)
                .await?
                .into_vec(),
            None => Vec::new(),
        };
        let left = want.saturating_sub(rows.len() as u64);
        if left > 0
            && let Some(filter) = second
        {
            rows.extend(
                ordered
                    .where_(filter)
                    .limit(left)
                    .get(exec)
                    .await?
                    .into_vec(),
            );
        }
        Ok(rows)
    }

    async fn cursor_page<M: Model, E: Executor + ?Sized>(
        &self,
        filtered: Builder<M>,
        exec: &E,
        dialect: Option<Dialect>,
        terms: &[Term],
        total: Total,
        map: impl Fn(Record<M>) -> R,
    ) -> Result<TablePage<R>> {
        let per = u64::from(self.state.per);
        // Nothing matches: no need to read the table a second time to find that out.
        if total == Total::Exact(0) {
            return Ok(TablePage {
                rows: Vec::new(),
                total,
                page: 1,
                has_more: false,
                next: None,
                prev: None,
                version: None,
            });
        }
        // A cursor that doesn't fit this order is an old link: the first page.
        let after = self
            .state
            .after
            .as_deref()
            .and_then(|text| decode_cursor(text, terms));
        let before = if after.is_none() {
            self.state
                .before
                .as_deref()
                .and_then(|text| decode_cursor(text, terms))
        } else {
            None
        };
        let values_of = |record: &Record<M>| -> Option<Vec<Value>> {
            terms.iter().map(|term| value_of(record, term)).collect()
        };
        let (mut records, has_prev, has_next) = if let Some(position) = &before {
            let flipped: Vec<Term> = terms.iter().map(|term| term.flipped()).collect();
            let mut rows =
                Self::read_past(&filtered, exec, dialect, &flipped, Some(position), per + 1)
                    .await?;
            let more = rows.len() as u64 > per;
            rows.truncate(self.state.per as usize);
            rows.reverse();
            (rows, more, true)
        } else {
            let mut rows =
                Self::read_past(&filtered, exec, dialect, terms, after.as_deref(), per + 1).await?;
            let more = rows.len() as u64 > per;
            rows.truncate(self.state.per as usize);
            (rows, after.is_some(), more)
        };
        // Going back past the start lands on a short page: show the first page whole.
        if before.is_some() && !has_prev && (records.len() as u64) < per {
            records = Self::read_past(&filtered, exec, dialect, terms, None, per).await?;
        }
        let next = if has_next {
            records
                .last()
                .and_then(values_of)
                .and_then(|values| encode_cursor(&values))
        } else {
            None
        };
        let prev = if has_prev {
            records
                .first()
                .and_then(values_of)
                .and_then(|values| encode_cursor(&values))
        } else {
            None
        };
        Ok(TablePage {
            rows: records.into_iter().map(map).collect(),
            total,
            page: 1,
            has_more: has_next,
            next,
            prev,
            version: None,
        })
    }

    /// How many rows an export may write: [`DEFAULT_EXPORT_ROWS`] unless `rows` raises
    /// it, and never more than [`MAX_EXPORT_ROWS`]. [`export_csv`](Self::export_csv)
    /// stops there and says so in `x-navree-truncated` when the result has more rows.
    /// An export reads the table once, a chunk at a time, in a request of its own: raise
    /// the limit for a route that is throttled and that only those who must may reach.
    ///
    /// ```ignore
    /// let response = query
    ///     .export_limit(500_000)
    ///     .export_csv(Deal::query(), &uow, "deals.csv", Record::into_inner)
    ///     .await?;
    /// ```
    #[must_use]
    pub fn export_limit(mut self, rows: u64) -> Self {
        self.export_cap = Some(rows.clamp(1, MAX_EXPORT_ROWS));
        self
    }

    /// The most rows an export of this query writes.
    fn export_rows_allowed(&self) -> u64 {
        self.export_cap
            .unwrap_or(DEFAULT_EXPORT_ROWS)
            .clamp(1, MAX_EXPORT_ROWS)
    }

    /// Walks the rows of this state, a chunk at a time, to hand them to a file writer:
    /// `each` takes the rows of one chunk. The columns that matter are
    /// [`TableQuery::visible_columns`]. Answers how many rows were read, stopping at
    /// `limit` (never past [`MAX_EXPORT_ROWS`]): an answer of `limit` may mean that more
    /// rows were left.
    ///
    /// The rows come in the state's sort order when each term of it is a column of the
    /// table being read. When one is a column of a joined table (`source =
    /// "companies.name"`) they come in the order of the key instead, since a position in
    /// such an order can't be found again without reading from the start.
    pub async fn export_rows<M, E>(
        &self,
        builder: Builder<M>,
        exec: &E,
        limit: u64,
        map: impl Fn(Record<M>) -> R,
        each: impl AsyncChunk<R>,
    ) -> Result<u64>
    where
        M: Model,
        E: Executor + ?Sized,
    {
        self.walk(builder, exec, limit.min(MAX_EXPORT_ROWS), map, each)
            .await
    }

    /// [`export_rows`](Self::export_rows) without the ceiling.
    async fn walk<M, E>(
        &self,
        builder: Builder<M>,
        exec: &E,
        limit: u64,
        map: impl Fn(Record<M>) -> R,
        mut each: impl AsyncChunk<R>,
    ) -> Result<u64>
    where
        M: Model,
        E: Executor + ?Sized,
    {
        let dialect = exec.target(Access::Read).await?.dialect();
        let filtered = self.apply(builder);
        let terms = walk_order(self.terms::<M>(), Self::key_term::<M>(Direction::Asc));
        let mut written: u64 = 0;
        let mut position: Option<Vec<Value>> = None;
        while written < limit {
            let chunk = (limit - written).min(u64::from(EXPORT_CHUNK));
            // Always by position, never by `OFFSET`: a read costs the same at the
            // millionth row as at the first.
            let rows = Self::read_past(&filtered, exec, dialect, &terms, position.as_deref(), chunk)
                .await?;
            let got = rows.len() as u64;
            if let Some(last) = rows.last() {
                position = terms.iter().map(|term| value_of(last, term)).collect();
            }
            if got == chunk && position.is_none() && written + got < limit {
                return Err(navree_orm::OrmError::InvalidQuery(
                    "an export can't go on: a row has no value for a column it is ordered by"
                        .to_owned(),
                ));
            }
            let rows: Vec<R> = rows.into_iter().map(&map).collect();
            each.chunk(&rows).await?;
            written += got;
            if got < chunk {
                break;
            }
        }
        Ok(written)
    }

    /// The rows as a CSV file: the current search, filters, sort and columns. It's
    /// written inside the handler, so it runs in the request's unit of work under
    /// the same row-level security, reading a chunk at a time and spilling to a
    /// temporary file past 8 MiB. Nothing holds the whole result in memory.
    ///
    /// It writes at most [`DEFAULT_EXPORT_ROWS`] rows, or as many as
    /// [`export_limit`](Self::export_limit) allows up to [`MAX_EXPORT_ROWS`]. A result
    /// with more rows is cut there, and the response says so in `x-navree-truncated`.
    /// The rows are in the sort order of the page, except that a sort by a column of a
    /// joined table writes them in the order of the key (see
    /// [`export_rows`](Self::export_rows)).
    ///
    /// Each export holds a unit of work, a read position and a temporary file for as
    /// long as it takes. Throttle the route that serves it.
    ///
    /// Formula-looking cells are neutralised, see [`write_csv_field`].
    pub async fn export_csv<M, E>(
        &self,
        builder: Builder<M>,
        exec: &E,
        file: &str,
        map: impl Fn(Record<M>) -> R,
    ) -> Result<Response>
    where
        M: Model,
        E: Executor + ?Sized,
    {
        let mut line = String::with_capacity(4096);
        line.push('\u{feff}');
        for (at, column) in self.state.cols.iter().enumerate() {
            if at > 0 {
                line.push(',');
            }
            write_csv_field(&mut line, R::COLUMNS[*column].label);
        }
        line.push_str("\r\n");
        let limit = self.export_rows_allowed();
        let mut sink = CsvSink {
            cols: self.state.cols.clone(),
            spool: Spool::new(),
            line,
            cell: String::new(),
            room: limit,
        };
        // One row more than is written says that the result goes on.
        let read = self
            .walk(builder, exec, limit + 1, map, &mut sink)
            .await?;
        Ok(sink.spool.into_response(file, read > limit).await)
    }
}

/// The order an export walks the rows in: the terms of the query's order while a
/// position in it can be found again by the values of its columns, and the key alone
/// when a term is a column of a joined table. Such a position can't be found again
/// without reading from the start, which is what paging by `OFFSET` does.
fn walk_order(terms: Vec<Term>, key: Term) -> Vec<Term> {
    if terms.iter().any(|term| term.source.contains('.')) {
        vec![key]
    } else {
        terms
    }
}

/// What takes the rows of an export, a chunk at a time.
pub trait AsyncChunk<R> {
    /// Takes one chunk.
    fn chunk(&mut self, rows: &[R]) -> impl std::future::Future<Output = Result<()>>;
}

struct CsvSink {
    cols: Vec<usize>,
    spool: Spool,
    line: String,
    cell: String,
    /// How many more rows the file takes.
    room: u64,
}

impl<R: TableRow> AsyncChunk<R> for &mut CsvSink {
    async fn chunk(&mut self, rows: &[R]) -> Result<()> {
        let take = usize::try_from(self.room).unwrap_or(usize::MAX);
        self.room = self.room.saturating_sub(rows.len() as u64);
        for row in rows.iter().take(take) {
            for (at, column) in self.cols.iter().enumerate() {
                if at > 0 {
                    self.line.push(',');
                }
                self.cell.clear();
                row.write_text(*column, &mut self.cell);
                write_csv_field(&mut self.line, &self.cell);
            }
            self.line.push_str("\r\n");
        }
        self.spool
            .write(self.line.as_bytes())
            .await
            .map_err(|error| navree_orm::OrmError::Driver {
                driver: "csv".into(),
                message: format!("writing an export: {error}"),
            })?;
        self.line.clear();
        Ok(())
    }
}

/// Collects an export's rows in memory: for a file format that's built whole, such
/// as XLSX, with `limit` as the bound on what it may hold.
impl<R: Clone> AsyncChunk<R> for &mut Vec<R> {
    async fn chunk(&mut self, rows: &[R]) -> Result<()> {
        self.extend_from_slice(rows);
        Ok(())
    }
}

impl<R: TableRow, F: FilterBar> BulkRequest<R, F> {
    /// The query of exactly the rows this action selects, from the app's scoped
    /// `builder`. Keys that no longer exist or aren't in scope simply don't match.
    ///
    /// For "all that match", the rows matching now are counted, inside this unit of
    /// work, against what the user saw: a different count, or a different version
    /// when the table has a version column, is an `Err`, and nothing should change.
    pub async fn resolve<M, E>(
        &self,
        builder: Builder<M>,
        exec: &E,
    ) -> Result<std::result::Result<Builder<M>, BulkRefusal>>
    where
        M: Model,
        E: Executor + ?Sized,
    {
        let key = M::meta().key_column();
        let kind = R::CONFIG.key_kind;
        let values = |list: &[String]| -> Vec<Value> {
            list.iter().filter_map(|id| bind(kind, id)).collect()
        };
        if *self.mode() == BulkMode::Ids {
            return Ok(Ok(builder.where_(col(key).is_in(values(self.ids())))));
        }
        let matching = self.query().apply(builder);
        let version = match R::CONFIG.version {
            Some(column) => {
                let value: Option<Value> =
                    matching.clone().reorder().max(exec, col(column)).await?;
                value.as_ref().and_then(text_of)
            }
            None => None,
        };
        let selected = if self.excluded().is_empty() {
            matching
        } else {
            matching.where_(col(key).not_in(values(self.excluded())))
        };
        let now = selected.clone().count(exec).await?;
        let shown = self.expected().unwrap_or(0);
        let version_moved = R::CONFIG.version.is_some()
            && self.version().is_some()
            && self.version() != version.as_deref();
        if now != shown || version_moved {
            return Ok(Err(BulkRefusal { shown, now }));
        }
        Ok(Ok(selected))
    }
}

/// An export on its way out: in memory, then in a temporary file.
struct Spool {
    memory: Vec<u8>,
    file: Option<(tokio::fs::File, TempPath)>,
}

/// A temporary file's path, removed when it's dropped.
struct TempPath(std::path::PathBuf);

impl Drop for TempPath {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

impl Spool {
    fn new() -> Self {
        Self {
            memory: Vec::new(),
            file: None,
        }
    }

    async fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        use tokio::io::AsyncWriteExt as _;
        if self.file.is_none() && self.memory.len() + bytes.len() <= EXPORT_MEMORY {
            self.memory.extend_from_slice(bytes);
            return Ok(());
        }
        if self.file.is_none() {
            let path =
                std::env::temp_dir().join(format!("navree-export-{}.csv", uuid::Uuid::new_v4()));
            let file = tokio::fs::OpenOptions::new()
                .create_new(true)
                .read(true)
                .write(true)
                .open(&path)
                .await?;
            self.file = Some((file, TempPath(path)));
        }
        if let Some((file, _)) = &mut self.file {
            if !self.memory.is_empty() {
                file.write_all(&self.memory).await?;
                self.memory = Vec::new();
            }
            file.write_all(bytes).await?;
        }
        Ok(())
    }

    async fn into_response(self, name: &str, cut: bool) -> Response {
        let body = match self.file {
            None => Body::from(Bytes::from(self.memory)),
            Some((mut file, path)) => {
                use tokio::io::{AsyncReadExt as _, AsyncSeekExt as _};
                if file.seek(std::io::SeekFrom::Start(0)).await.is_err() {
                    return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                }
                let stream = futures_util::stream::unfold(
                    (file, path, vec![0_u8; 64 * 1024]),
                    |(mut file, path, mut buffer)| async move {
                        match file.read(&mut buffer).await {
                            Ok(0) => None,
                            Ok(count) => {
                                let chunk = Bytes::copy_from_slice(&buffer[..count]);
                                Some((Ok::<_, std::io::Error>(chunk), (file, path, buffer)))
                            }
                            Err(error) => Some((Err(error), (file, path, buffer))),
                        }
                    },
                );
                Body::from_stream(stream)
            }
        };
        let mut response = Response::new(body);
        let headers = response.headers_mut();
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("text/csv; charset=utf-8"),
        );
        let safe: String = name
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
            .collect();
        if let Ok(value) = HeaderValue::from_str(&format!("attachment; filename=\"{safe}\"")) {
            headers.insert(CONTENT_DISPOSITION, value);
        }
        if cut {
            headers.insert("x-navree-truncated", HeaderValue::from_static("1"));
        }
        response
    }
}

/// The most views one owner can keep of one table. [`save_view`] refuses another with
/// [`TooManyViews`]. [`list_views`] shows at most 200.
pub const MAX_SAVED_VIEWS: u64 = 50;

/// Why [`save_view`] saved nothing: its owner already keeps [`MAX_SAVED_VIEWS`] views of
/// that table. It comes in a [`navree_orm::OrmError`] ([`TooManyViews::of`] finds it there), so an
/// app can answer the form with its words and a 4xx status, as the docs show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TooManyViews {
    /// The most views one owner keeps of a table.
    pub limit: u64,
}

impl TooManyViews {
    /// The refusal inside `error`, when that is what it is.
    pub fn of(error: &navree_orm::OrmError) -> Option<&Self> {
        std::error::Error::source(error).and_then(|source| source.downcast_ref::<Self>())
    }
}

impl std::fmt::Display for TooManyViews {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "A table keeps at most {} saved views for one person. Delete one to save another.",
            self.limit
        )
    }
}

impl std::error::Error for TooManyViews {}

/// A saved view, stored in an SQL database. The table is the app's to create; see the crate's README.
#[derive(navree_orm::Model, Clone, Debug, Default, serde::Serialize)]
#[model(table = "navree_ui_table_views")]
pub struct SavedViewModel {
    /// Its key.
    pub id: String,
    /// Whose it is: a user's id, or anything else the app scopes by.
    pub owner: String,
    /// Which table.
    pub table_id: String,
    /// What it's called.
    pub name: String,
    /// The state, as a query string.
    pub query: String,
    /// Whether the table opens with it.
    pub is_default: bool,
}

impl From<SavedViewModel> for SavedView {
    fn from(view: SavedViewModel) -> Self {
        Self {
            id: view.id,
            name: view.name,
            query: view.query,
            is_default: view.is_default,
        }
    }
}

/// A saved view, stored in a document store (MongoDB): the collection `navree_ui_table_views`, a
/// document for each view with the same fields as [`SavedViewModel`]'s table, its key in `_id`.
/// MongoDB makes the collection when the first view is saved; an index on `(owner, table_id)`
/// keeps the list of a user's views quick when there are many.
#[derive(navree_orm::Model, Clone, Debug, Default, serde::Serialize)]
#[model(
    collection = "navree_ui_table_views",
    driver = "mongodb",
    key_type = "manual"
)]
pub struct SavedViewDocument {
    /// Its key.
    pub id: String,
    /// Whose it is: a user's id, or anything else the app scopes by.
    pub owner: String,
    /// Which table.
    pub table_id: String,
    /// What it's called.
    pub name: String,
    /// The state, as a query string.
    pub query: String,
    /// Whether the table opens with it.
    pub is_default: bool,
}

impl From<SavedViewDocument> for SavedView {
    fn from(view: SavedViewDocument) -> Self {
        Self {
            id: view.id,
            name: view.name,
            query: view.query,
            is_default: view.is_default,
        }
    }
}

/// A stored view, whichever model holds it: a row of the SQL table or a document.
trait StoredView: Model + Into<SavedView> {
    /// The model for `owner`'s `view` of table `table`.
    fn from_view(view: SavedView, owner: &str, table: &str) -> Self;
}

macro_rules! stored_view {
    ($model:ident) => {
        impl StoredView for $model {
            fn from_view(view: SavedView, owner: &str, table: &str) -> Self {
                Self {
                    id: view.id,
                    owner: owner.to_owned(),
                    table_id: table.to_owned(),
                    name: view.name,
                    query: view.query,
                    is_default: view.is_default,
                }
            }
        }
    };
}

stored_view!(SavedViewModel);
stored_view!(SavedViewDocument);

/// Whether `exec` runs on a document store, where views are documents, and not on an SQL database.
async fn on_documents<E: Executor + ?Sized>(exec: &E) -> Result<bool> {
    Ok(exec.target(Access::Read).await?.dialect().is_none())
}

/// `owner`'s views of `table`, in model `M`.
fn views_of<M: StoredView>(owner: &str, table: &str) -> Builder<M> {
    M::query()
        .where_(col("owner").eq(Value::String(owner.to_owned())))
        .where_(col("table_id").eq(Value::String(table.to_owned())))
}

async fn list_in<M: StoredView, E: Executor + ?Sized>(
    exec: &E,
    owner: &str,
    table: &str,
) -> Result<Vec<SavedView>> {
    let rows = views_of::<M>(owner, table)
        .order_by("name")
        .order_by(M::meta().key_column())
        .limit(200)
        .get(exec)
        .await?;
    Ok(rows
        .into_vec()
        .into_iter()
        .map(|record| record.into_inner().into())
        .collect())
}

/// How many views `owner` keeps of `table`, in model `M`.
async fn count_in<M: StoredView, E: Executor + ?Sized>(
    exec: &E,
    owner: &str,
    table: &str,
) -> Result<u64> {
    views_of::<M>(owner, table).count(exec).await
}

async fn create_in<M: StoredView, E: Executor + ?Sized>(
    exec: &E,
    view: SavedView,
    owner: &str,
    table: &str,
) -> Result<SavedView> {
    let stored = M::create(exec, M::from_view(view, owner, table)).await?;
    Ok(stored.into_inner().into())
}

async fn clear_default_in<M: StoredView, E: Executor + ?Sized>(
    exec: &E,
    owner: &str,
    table: &str,
) -> Result<()> {
    views_of::<M>(owner, table)
        .update(exec, [col("is_default").set(Value::Bool(false))])
        .await?;
    Ok(())
}

async fn delete_in<M: StoredView, E: Executor + ?Sized>(
    exec: &E,
    owner: &str,
    id: &str,
) -> Result<u64> {
    M::query()
        .where_(col("owner").eq(Value::String(owner.to_owned())))
        .where_(col(M::meta().key_column()).eq(Value::String(id.to_owned())))
        .delete(exec)
        .await
}

/// The views of `owner` on table `table`, by name. On an SQL database they are the rows of
/// `navree_ui_table_views` ([`SavedViewModel`]); on MongoDB, its documents ([`SavedViewDocument`]).
pub async fn list_views<E: Executor + ?Sized>(
    exec: &E,
    owner: &str,
    table: &str,
) -> Result<Vec<SavedView>> {
    if on_documents(exec).await? {
        list_in::<SavedViewDocument, E>(exec, owner, table).await
    } else {
        list_in::<SavedViewModel, E>(exec, owner, table).await
    }
}

/// Saves `owner`'s view of table `table`, on the store `exec` runs on (see [`list_views`]). The state
/// is parsed through the table's definition first, so what's stored is only what the definition allows.
///
/// `owner` and `table` are what a person's views are found by, so an app with workspaces puts the
/// workspace into the owner (`"{workspace_id}:{user_id}"`), or one workspace's people could see each
/// other's views, and a person in two workspaces would have one set. An owner keeps at most
/// [`MAX_SAVED_VIEWS`] views of a table: past that, nothing is saved (not even a default's change)
/// and the error is [`TooManyViews`]. The count is read before the view is written, so views saved
/// in the same instant by one owner can overshoot it by a few.
///
/// # Errors
///
/// [`TooManyViews`], in a [`navree_orm::OrmError`] ([`TooManyViews::of`]); the store's.
pub async fn save_view<R, F, E>(
    exec: &E,
    owner: &str,
    table: &str,
    name: &str,
    query: &TableQuery<R, F>,
    is_default: bool,
) -> Result<SavedView>
where
    R: TableRow,
    F: FilterBar,
    E: Executor + ?Sized,
{
    let documents = on_documents(exec).await?;
    // Before anything is written: a refusal changes nothing, a default included.
    let kept = if documents {
        count_in::<SavedViewDocument, E>(exec, owner, table).await?
    } else {
        count_in::<SavedViewModel, E>(exec, owner, table).await?
    };
    if kept >= MAX_SAVED_VIEWS {
        return Err(navree_orm::OrmError::refused(
            "SavedView",
            TooManyViews {
                limit: MAX_SAVED_VIEWS,
            },
        ));
    }
    if is_default {
        if documents {
            clear_default_in::<SavedViewDocument, E>(exec, owner, table).await?;
        } else {
            clear_default_in::<SavedViewModel, E>(exec, owner, table).await?;
        }
    }
    let mut name = name.trim().to_owned();
    name.truncate(name.char_indices().nth(80).map_or(name.len(), |(at, _)| at));
    let mut state = query.state.clone();
    // Whatever the page owns isn't this table's to store.
    // A view is how to look, not where one was.
    state.page = 1;
    state.after = None;
    state.before = None;
    state.count = false;
    state.open.clear();
    // Written the way the table writes its own links, so the view that matches the page
    // is found by comparing strings.
    let view = SavedView {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        query: super::query::query_of(&state.pairs::<R>()),
        is_default,
    };
    if documents {
        create_in::<SavedViewDocument, E>(exec, view, owner, table).await
    } else {
        create_in::<SavedViewModel, E>(exec, view, owner, table).await
    }
}

/// Deletes `owner`'s view `id`. Someone else's view doesn't match.
pub async fn delete_view<E: Executor + ?Sized>(exec: &E, owner: &str, id: &str) -> Result<u64> {
    if on_documents(exec).await? {
        delete_in::<SavedViewDocument, E>(exec, owner, id).await
    } else {
        delete_in::<SavedViewModel, E>(exec, owner, id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_view_is_a_row_on_sql_and_a_document_on_mongodb() {
        assert_eq!(SavedViewModel::meta().table(), "navree_ui_table_views");
        assert_eq!(SavedViewModel::meta().driver_name(), None);
        assert_eq!(SavedViewModel::meta().key_column(), "id");
        assert_eq!(SavedViewDocument::meta().table(), "navree_ui_table_views");
        assert_eq!(SavedViewDocument::meta().driver_name(), Some("mongodb"));
        assert_eq!(SavedViewDocument::meta().key_column(), "_id");
    }

    #[test]
    fn an_export_walks_in_key_order_when_a_sort_column_is_of_a_joined_table() {
        let term = |source: &'static str, direction| Term {
            source,
            field: "name",
            kind: ValueKind::Text,
            nullable: false,
            direction,
        };
        let key = Term {
            source: "id",
            field: "id",
            kind: ValueKind::Int,
            nullable: false,
            direction: Direction::Asc,
        };
        let own = walk_order(vec![term("title", Direction::Desc), key], key);
        let sources: Vec<&str> = own.iter().map(|term| term.source).collect();
        assert_eq!(sources, ["title", "id"], "the page's own order is kept");
        let joined = walk_order(vec![term("companies.name", Direction::Desc), key], key);
        assert_eq!(joined.len(), 1);
        assert_eq!(joined[0].source, "id");
        assert_eq!(joined[0].direction, Direction::Asc);
    }

    #[test]
    fn either_model_holds_the_same_view() {
        let view = SavedView {
            id: "v1".to_owned(),
            name: "Won".to_owned(),
            query: "filter%5Bstage%5D=won".to_owned(),
            is_default: true,
        };
        let row = SavedViewModel::from_view(view.clone(), "me", "deals");
        let document = SavedViewDocument::from_view(view.clone(), "me", "deals");
        assert_eq!((row.owner.as_str(), row.table_id.as_str()), ("me", "deals"));
        assert_eq!(
            (document.owner.as_str(), document.table_id.as_str()),
            ("me", "deals")
        );
        assert_eq!(SavedView::from(row), view);
        assert_eq!(SavedView::from(document), view);
    }
}
