//! [`TableQuery`]: a table's whole state as a URL states it. A bad value is
//! ignored rather than an error: a URL can be old or edited, and a table should
//! still show.
//!
//! The list part of the URL is the grammar every Navree list speaks, the one a
//! JSON endpoint reads (see `navree_http::list`). The table reads it with
//! `ListQuery::parse_lenient` and keeps what its columns allow. The rest is the
//! table's own state: which columns, how wide, how dense, which rows are open.
//!
//! ```text
//! search=sutlej                         the global search
//! sort=-amount,title                    columns, in priority order; `-` is descending; `sort=` is none
//! page=3  per_page=50                   a numbered page, and the page size
//! cursor=...  before=...  limit=50      a cursor table: the page after, the page before, the page size
//! filter[title][contains]=sutlej        a text filter
//! filter[stage]=won                     a select filter; a boolean one is filter[active]=true
//! filter[stage][in]=won,lost            any of these (multi and relation filters)
//! filter[amount][gte]=1&filter[amount][lte]=9   a range; dates too
//! filter[owner][null]=true              is NULL
//! count=1                               count now, for a table that doesn't by itself
//! cols=title,amount                     the columns shown, in order
//! w=title:240,amount:120                widths in pixels
//! fb=stage,amount                       the filters a filter bar shows, in order
//! density=compact                       row height
//! open=12,15                            rows open to their details
//! ```
//!
//! A table that pages by number reads `page` and `per_page` and ignores `cursor`,
//! `before` and `limit`; a cursor table is the other way round. The page size is
//! only one the table offers.

use std::marker::PhantomData;

use http::request::Parts;
use navree_http::extract::{FromRequestParts, urlencoded};
pub(crate) use navree_http::list::Op;
use navree_http::list::{
    Condition, ListQuery, MAX_PARAMS, MAX_QUERY_BYTES, MAX_SORTS, encode_component,
};
use serde::de::DeserializeOwned;

use super::{
    Column, Direction, FilterBar, FilterKind, MAX_PER_PAGE, NoFilter, TableConfig, TableDensity,
    TableRow,
};
use crate::form::encode;

/// The columns and settings a query reads its URL against: a [`TableRow`]'s, or
/// those of a table described at run time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Shape {
    pub(crate) columns: &'static [Column],
    pub(crate) config: TableConfig,
}

impl Shape {
    /// The shape of `R`.
    pub(crate) const fn of<R: TableRow>() -> Self {
        Self {
            columns: R::COLUMNS,
            config: R::CONFIG,
        }
    }
}

/// The longest text a search or text filter keeps, in bytes.
pub const MAX_TEXT: usize = navree_http::list::MAX_TEXT;
/// The most values one multi filter keeps.
pub const MAX_VALUES: usize = navree_http::list::MAX_VALUES;
/// The most rows open at once.
pub const MAX_OPEN_ROWS: usize = 25;
/// The most page-owned filter parameters read.
const MAX_PAGE_FILTER_PARAMS: usize = 64;
/// Widths a URL can set, in pixels.
const WIDTHS: std::ops::RangeInclusive<u32> = 40..=1600;

/// One sort term.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SortTerm {
    /// Which column, as an index of `COLUMNS`.
    pub column: usize,
    /// Which way.
    pub direction: Direction,
}

/// What a filter holds.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FilterValue {
    /// Contains this text.
    Text(String),
    /// Is any of these.
    Values(Vec<String>),
    /// Between these; at least one is set.
    Range {
        /// The lowest value, inclusive.
        min: Option<String>,
        /// The highest value, inclusive.
        max: Option<String>,
    },
    /// Is this.
    Bool(bool),
    /// Is NULL.
    Empty,
}

/// A filter in force.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveFilter {
    /// Which column, as an index of `COLUMNS`.
    pub column: usize,
    /// What it holds.
    pub value: FilterValue,
}

/// A table's state, without the page-owned filters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct State {
    pub(crate) q: Option<String>,
    pub(crate) sort: Vec<SortTerm>,
    pub(crate) filters: Vec<ActiveFilter>,
    pub(crate) page: u32,
    pub(crate) per: u32,
    pub(crate) after: Option<String>,
    pub(crate) before: Option<String>,
    pub(crate) count: bool,
    /// The columns shown, in order: indexes of `COLUMNS`.
    pub(crate) cols: Vec<usize>,
    pub(crate) widths: Vec<(usize, u32)>,
    pub(crate) density: TableDensity,
    pub(crate) open: Vec<String>,
    /// The filter bar's choice of filters, in order; `None` is every filter.
    pub(crate) bar: Option<Vec<usize>>,
}

/// A table's state as the URL states it.
///
/// ```ignore
/// use navree_ui::table::{Direction, NoFilter, TableQuery};
/// use navree_ui::TableRow;
///
/// #[derive(TableRow)]
/// #[table(sort = "-amount")]
/// struct Row {
///     #[col(sortable, filter = text)]
///     title: String,
///     #[col(sortable, numeric)]
///     amount: i64,
/// }
///
/// let query = TableQuery::<Row, NoFilter>::parse("/rows", "sort=title&sort2=x&per_page=99999&page=-1");
/// assert_eq!(query.sorts()[0].direction, Direction::Asc);
/// assert_eq!(query.page(), 1);
/// assert_eq!(query.per_page(), 25, "a page size the table doesn't offer is ignored");
/// ```
#[derive(Clone, Debug)]
pub struct TableQuery<R, F = NoFilter> {
    pub(crate) path: String,
    pub(crate) state: State,
    pub(crate) filter: F,
    pub(crate) search_mode: SearchMode,
    pub(crate) shape: Shape,
    marker: PhantomData<fn() -> R>,
}

/// How the search box matches the columns marked `search`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum SearchMode {
    /// The text anywhere in the value, whatever its case: `LOWER(col) LIKE '%text%'`. It
    /// works on every database with no index, and no index can serve it, so the whole
    /// table is read. That is fine to some tens of thousands of rows; MySQL takes about
    /// eight seconds at a million. The default.
    #[default]
    Contains,
    /// The words of the text, in any order, through the database's full-text index:
    /// PostgreSQL's text search, and `MATCH ... AGAINST` on MySQL and MariaDB over a
    /// `FULLTEXT` index that covers exactly the searched columns. A million rows answer in
    /// milliseconds. It finds words, not fragments (`renew` does not find `renewal`), and
    /// SQLite has no full-text condition, so there it is an error at load. Create the
    /// index in a migration: on MySQL `FULLTEXT (title, notes)` over the same columns, in
    /// the same order, as the `#[col(search)]` fields.
    Words,
}

/// Whether `name` is one of the list query's parameters, which the table reads from
/// the shared grammar and not from its own pairs.
fn is_list_name(name: &str) -> bool {
    matches!(
        name,
        "sort" | "search" | "page" | "per_page" | "limit" | "cursor" | "before" | "filter"
    ) || name.starts_with("filter[")
}

/// The query string cut to the most the grammar reads, at a character.
fn clip_query(query: &str) -> &str {
    let query = query.strip_prefix('?').unwrap_or(query);
    if query.len() <= MAX_QUERY_BYTES {
        return query;
    }
    let mut end = MAX_QUERY_BYTES;
    while !query.is_char_boundary(end) {
        end -= 1;
    }
    &query[..end]
}

/// Pairs written as a query string, in the list query's spelling: the same string a
/// link has.
pub(crate) fn query_of(pairs: &[(String, String)]) -> String {
    let mut out = String::new();
    for (at, (name, value)) in pairs.iter().enumerate() {
        if at > 0 {
            out.push('&');
        }
        out.push_str(&encode_component(name));
        out.push('=');
        out.push_str(&encode_component(value));
    }
    out
}

/// The name of the parameter that carries a filter on `column` with operator `op`:
/// `filter[stage]` for equality, `filter[amount][gte]` for the rest. Forms and
/// links write it, so they never spell it twice.
pub(crate) fn filter_param(column: &str, op: Op) -> String {
    Condition {
        field: column.to_owned(),
        op,
        values: Vec::new(),
    }
    .param()
}

/// The name of the page size parameter: `per_page` for numbered pages, `limit` for
/// cursor pages.
pub(crate) const fn size_param(cursor: bool) -> &'static str {
    if cursor { "limit" } else { "per_page" }
}

/// Whether `value` is one a column's filter takes: one of its options, if it has
/// any, and of its kind.
fn allowed(column: &Column, value: &str) -> bool {
    (column.options.is_empty() || column.options.iter().any(|(option, _)| *option == value))
        && column.kind.accepts(value)
}

/// The filters of a list query that the columns allow, in the order of the columns
/// whatever the URL's order. Anything else is left out.
fn read_filters(columns: &[Column], list: &ListQuery) -> Vec<ActiveFilter> {
    let mut texts: Vec<Option<String>> = vec![None; columns.len()];
    let mut values: Vec<Vec<String>> = vec![Vec::new(); columns.len()];
    let mut mins: Vec<Option<String>> = vec![None; columns.len()];
    let mut maxes: Vec<Option<String>> = vec![None; columns.len()];
    let mut flags: Vec<Option<bool>> = vec![None; columns.len()];
    let mut empties: Vec<bool> = vec![false; columns.len()];
    for condition in &list.filter {
        let Some(at) = columns
            .iter()
            .position(|column| column.name == condition.field)
        else {
            continue;
        };
        let column = &columns[at];
        let value = condition.value();
        match (column.filter, condition.op) {
            (FilterKind::None, _) => {}
            (FilterKind::Text, Op::Contains) => {
                let text = value.trim();
                if texts[at].is_none() && !text.is_empty() {
                    texts[at] = Some(text.to_owned());
                }
            }
            (FilterKind::Boolean, Op::Eq) => {
                if flags[at].is_none() {
                    flags[at] = matches!(value, "true" | "false").then_some(value == "true");
                }
            }
            // One value: the first the column takes.
            (FilterKind::Select, Op::Eq | Op::In) => {
                if values[at].is_empty()
                    && let Some(first) = condition.values.iter().find(|v| allowed(column, v))
                {
                    values[at].push(first.clone());
                }
            }
            (FilterKind::Multi | FilterKind::Relation, Op::Eq | Op::In) => {
                for value in &condition.values {
                    if allowed(column, value)
                        && values[at].len() < MAX_VALUES
                        && !values[at].contains(value)
                    {
                        values[at].push(value.clone());
                    }
                }
            }
            (FilterKind::Range, Op::Gte) => {
                if mins[at].is_none() && column.kind.accepts(value) {
                    mins[at] = Some(value.to_owned());
                }
            }
            (FilterKind::Range, Op::Lte) => {
                if maxes[at].is_none() && column.kind.accepts(value) {
                    maxes[at] = Some(value.to_owned());
                }
            }
            // `null=false` ("is not NULL") is a filter a table does not have.
            (_, Op::Null) if column.nullable && condition.null_flag() == Some(true) => {
                empties[at] = true;
            }
            _ => {}
        }
    }
    let mut filters = Vec::new();
    for at in 0..columns.len() {
        let filter = if empties[at] {
            Some(FilterValue::Empty)
        } else if let Some(text) = texts[at].take() {
            Some(FilterValue::Text(text))
        } else if let Some(flag) = flags[at] {
            Some(FilterValue::Bool(flag))
        } else if !values[at].is_empty() {
            Some(FilterValue::Values(std::mem::take(&mut values[at])))
        } else if mins[at].is_some() || maxes[at].is_some() {
            Some(FilterValue::Range {
                min: mins[at].take(),
                max: maxes[at].take(),
            })
        } else {
            None
        };
        if let Some(value) = filter {
            filters.push(ActiveFilter { column: at, value });
        }
    }
    filters
}

impl State {
    /// Reads a query string against a definition: the list query's part from `list`
    /// (what `ListQuery::parse_lenient` made of the whole string), the table's own
    /// part from `pairs`. The second answer is the parameters that are neither: the
    /// page's own filters.
    fn parse(
        shape: &Shape,
        pairs: &[(String, String)],
        list: &ListQuery,
    ) -> (Self, Vec<(String, String)>) {
        let columns = shape.columns;
        let config = shape.config;
        let index_of = |name: &str| columns.iter().position(|column| column.name == name);
        let default_sort = || -> Vec<SortTerm> {
            config
                .sort
                .iter()
                .filter_map(|(name, direction)| {
                    index_of(name).map(|column| SortTerm {
                        column,
                        direction: *direction,
                    })
                })
                .collect()
        };
        let default_cols = || -> Vec<usize> {
            columns
                .iter()
                .enumerate()
                .filter(|(_, column)| !column.hidden)
                .map(|(at, _)| at)
                .collect()
        };
        let searchable = columns.iter().any(|column| column.search);
        let mut state = Self {
            q: None,
            sort: default_sort(),
            filters: Vec::new(),
            page: 1,
            per: config.per.clamp(1, MAX_PER_PAGE),
            after: None,
            before: None,
            count: false,
            cols: default_cols(),
            widths: Vec::new(),
            density: TableDensity::Default,
            open: Vec::new(),
            bar: None,
        };
        let mut rest = Vec::new();
        let mut sort_seen = false;
        let mut cols: Vec<usize> = Vec::new();
        let mut cols_seen = false;
        for (name, value) in pairs.iter().take(MAX_PARAMS) {
            match name.as_str() {
                // `sort=` says no order, unlike no `sort` at all.
                "sort" => sort_seen = true,
                "count" => state.count = value == "1",
                "density" => state.density = TableDensity::parse(value),
                "cols" => {
                    cols_seen = true;
                    for name in value.split(',') {
                        if let Some(at) = index_of(name)
                            && !cols.contains(&at)
                        {
                            cols.push(at);
                        }
                    }
                }
                "fb" => {
                    let bar = state.bar.get_or_insert_with(Vec::new);
                    for name in value.split(',') {
                        if let Some(at) =
                            index_of(name).filter(|at| columns[*at].filter != FilterKind::None)
                            && !bar.contains(&at)
                        {
                            bar.push(at);
                        }
                    }
                }
                "w" => {
                    for part in value.split(',') {
                        let Some((name, width)) = part.split_once(':') else {
                            continue;
                        };
                        let (Some(at), Ok(width)) = (index_of(name), width.parse::<u32>()) else {
                            continue;
                        };
                        if WIDTHS.contains(&width)
                            && !state.widths.iter().any(|(column, _)| *column == at)
                        {
                            state.widths.push((at, width));
                        }
                    }
                }
                "open" => {
                    for key in value
                        .split(',')
                        .filter(|key| !key.is_empty() && config.key_kind.accepts(key))
                    {
                        if state.open.len() < MAX_OPEN_ROWS
                            && !state.open.iter().any(|open| open == key)
                        {
                            state.open.push(key.to_owned());
                        }
                    }
                }
                // A table with no searchable column leaves `search` to the page.
                "search" if searchable => {}
                "search" => {
                    if !value.is_empty() {
                        rest.push((name.clone(), value.clone()));
                    }
                }
                other if is_list_name(other) => {}
                _ => {
                    if !value.is_empty() {
                        rest.push((name.clone(), value.clone()));
                    }
                }
            }
        }
        if sort_seen {
            state.sort = list
                .sort
                .iter()
                .filter_map(|key| {
                    let column = index_of(&key.field).filter(|at| columns[*at].sortable)?;
                    let direction = if key.descending {
                        Direction::Desc
                    } else {
                        Direction::Asc
                    };
                    Some(SortTerm { column, direction })
                })
                .take(MAX_SORTS)
                .collect();
        }
        if searchable {
            state.q = list.search.clone();
        }
        state.filters = read_filters(columns, list);
        // Only a page size the table offers.
        let offered = |size: u64| -> Option<u32> {
            let size = u32::try_from(size).ok()?;
            (size <= MAX_PER_PAGE && (size == config.per || config.per_pages.contains(&size)))
                .then_some(size)
        };
        if config.cursor {
            // A cursor is the position; a page number beside it means nothing, and the
            // page after wins over the page before.
            state.after = list.cursor.clone();
            if state.after.is_none() {
                state.before = list.before.clone();
            }
            if let Some(size) = list.limit.and_then(offered) {
                state.per = size;
            }
        } else {
            if let Some(page) = list.page {
                state.page = u32::try_from(page).unwrap_or(1).max(1);
            }
            if let Some(size) = list.per_page.and_then(offered) {
                state.per = size;
            }
        }
        if cols_seen {
            // A locked column can't be hidden; none at all would be no table.
            for (at, column) in columns.iter().enumerate() {
                if column.locked && !cols.contains(&at) {
                    cols.insert(at.min(cols.len()), at);
                }
            }
            if !cols.is_empty() {
                state.cols = cols;
            }
        }
        (state, rest)
    }

    pub(crate) fn is_default_sort_in(&self, shape: &Shape) -> bool {
        let config = shape.config.sort;
        self.sort.len() == config.len()
            && self
                .sort
                .iter()
                .zip(config)
                .all(|(term, (name, direction))| {
                    shape.columns[term.column].name == *name && term.direction == *direction
                })
    }

    fn is_default_cols_in(&self, shape: &Shape) -> bool {
        let mut at = 0;
        for (index, column) in shape.columns.iter().enumerate() {
            if !column.hidden {
                if self.cols.get(at) != Some(&index) {
                    return false;
                }
                at += 1;
            }
        }
        at == self.cols.len()
    }

    /// The pairs that say what the filters are, in the list query's spelling.
    pub(crate) fn condition_pairs_in(&self, shape: &Shape, pairs: &mut Vec<(String, String)>) {
        for filter in &self.filters {
            let column = &shape.columns[filter.column];
            let param = |op: Op| filter_param(column.name, op);
            match &filter.value {
                FilterValue::Text(text) => pairs.push((param(Op::Contains), text.clone())),
                // A select holds one value, written plain.
                FilterValue::Values(values) if column.filter == FilterKind::Select => {
                    if let Some(first) = values.first() {
                        pairs.push((param(Op::Eq), first.clone()));
                    }
                }
                FilterValue::Values(values) => pairs.push((param(Op::In), values.join(","))),
                FilterValue::Bool(flag) => pairs.push((param(Op::Eq), flag.to_string())),
                FilterValue::Empty => pairs.push((param(Op::Null), "true".to_owned())),
                FilterValue::Range { min, max } => {
                    if let Some(min) = min {
                        pairs.push((param(Op::Gte), min.clone()));
                    }
                    if let Some(max) = max {
                        pairs.push((param(Op::Lte), max.clone()));
                    }
                }
            }
        }
    }

    /// The pairs that say what is searched for and filtered.
    pub(crate) fn filter_pairs_in(&self, shape: &Shape, pairs: &mut Vec<(String, String)>) {
        self.condition_pairs_in(shape, pairs);
        if let Some(q) = &self.q {
            pairs.push(("search".to_owned(), q.clone()));
        }
    }

    /// [`pairs_in`](Self::pairs_in) for `R`.
    #[cfg(feature = "orm")]
    pub(crate) fn pairs<R: TableRow>(&self) -> Vec<(String, String)> {
        self.pairs_in(&Shape::of::<R>())
    }

    /// Every pair, in one order, leaving out what's at its default. The list query's
    /// come first, in the order a list query is written (filters, sort, search, page,
    /// page size, cursor, before); the table's own state follows.
    pub(crate) fn pairs_in(&self, shape: &Shape) -> Vec<(String, String)> {
        let columns = shape.columns;
        let cursor = shape.config.cursor;
        let default_size = self.per == shape.config.per.clamp(1, MAX_PER_PAGE);
        let mut pairs = Vec::new();
        self.condition_pairs_in(shape, &mut pairs);
        if !self.is_default_sort_in(shape) {
            let mut sort = String::new();
            for (at, term) in self.sort.iter().enumerate() {
                if at > 0 {
                    sort.push(',');
                }
                if term.direction == Direction::Desc {
                    sort.push('-');
                }
                sort.push_str(columns[term.column].name);
            }
            pairs.push(("sort".to_owned(), sort));
        }
        if let Some(q) = &self.q {
            pairs.push(("search".to_owned(), q.clone()));
        }
        if self.page > 1 {
            pairs.push(("page".to_owned(), self.page.to_string()));
        }
        if !cursor && !default_size {
            pairs.push((size_param(false).to_owned(), self.per.to_string()));
        }
        if let Some(after) = &self.after {
            pairs.push(("cursor".to_owned(), after.clone()));
        }
        if let Some(before) = &self.before {
            pairs.push(("before".to_owned(), before.clone()));
        }
        if cursor && !default_size {
            pairs.push((size_param(true).to_owned(), self.per.to_string()));
        }
        if !self.is_default_cols_in(shape) {
            let names: Vec<&str> = self.cols.iter().map(|at| columns[*at].name).collect();
            pairs.push(("cols".to_owned(), names.join(",")));
        }
        if !self.widths.is_empty() {
            let widths: Vec<String> = self
                .widths
                .iter()
                .map(|(at, width)| format!("{}:{width}", columns[*at].name))
                .collect();
            pairs.push(("w".to_owned(), widths.join(",")));
        }
        if let Some(bar) = &self.bar {
            let names: Vec<&str> = bar.iter().map(|at| columns[*at].name).collect();
            let every: Vec<usize> = (0..columns.len())
                .filter(|at| columns[*at].filter != FilterKind::None)
                .collect();
            if *bar != every {
                pairs.push(("fb".to_owned(), names.join(",")));
            }
        }
        if self.density != TableDensity::Default {
            pairs.push(("density".to_owned(), self.density.as_str().to_owned()));
        }
        if !self.open.is_empty() {
            pairs.push(("open".to_owned(), self.open.join(",")));
        }
        if self.count {
            pairs.push(("count".to_owned(), "1".to_owned()));
        }
        pairs
    }
}

/// The page's own filters. A bad value drops that value, not the others: one wrong
/// parameter shouldn't widen every filter there is.
fn page_filter<F: Default + DeserializeOwned>(pairs: &[(String, String)]) -> F {
    let read = |pairs: &[&(String, String)]| -> Option<F> {
        let borrowed: Vec<(&str, &str)> = pairs
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        let encoded = encode(&borrowed);
        F::deserialize(urlencoded::Deserializer::new(encoded.as_bytes())).ok()
    };
    let all: Vec<&(String, String)> = pairs.iter().take(MAX_PAGE_FILTER_PARAMS).collect();
    if let Some(filter) = read(&all) {
        return filter;
    }
    let good: Vec<&(String, String)> = all
        .iter()
        .copied()
        .filter(|pair| read(&[*pair]).is_some())
        .collect();
    read(&good).unwrap_or_default()
}

impl<R: TableRow, F: FilterBar + Default + DeserializeOwned> TableQuery<R, F> {
    /// Reads a query string.
    pub fn parse(path: impl Into<String>, query: &str) -> Self {
        Self::parse_shaped(path, query, Shape::of::<R>())
    }

    /// Reads a query string against `shape`: the columns of a table described at run
    /// time.
    pub(crate) fn parse_shaped(path: impl Into<String>, query: &str, shape: Shape) -> Self {
        let query = clip_query(query);
        let pairs: Vec<(String, String)> = urlencoded::parse(query.as_bytes())
            .take(MAX_PARAMS)
            .map(|(name, value)| (name.into_owned(), value.into_owned()))
            .collect();
        // What is wrong in the list query's part is left out, not reported: a URL can
        // be old or edited, and the table should still show.
        let (list, _problems) = ListQuery::parse_lenient(query);
        let (state, rest) = State::parse(&shape, &pairs, &list);
        let filter = page_filter::<F>(&rest);
        Self {
            path: path.into(),
            state,
            filter,
            search_mode: SearchMode::Contains,
            shape,
            marker: PhantomData,
        }
    }
}

impl<R: TableRow, F: FilterBar + Default + DeserializeOwned> TableQuery<R, F> {
    /// This state, or, when the URL names none, the user's default view.
    #[must_use]
    pub fn or_default_view(self, views: &[super::SavedView]) -> Self {
        if self.query_string().is_empty()
            && let Some(view) = views.iter().find(|view| view.is_default)
        {
            return Self::parse_shaped(self.path, &view.query, self.shape);
        }
        self
    }
}

impl<R: TableRow, F: FilterBar> TableQuery<R, F> {
    /// The page's path.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// How the search box matches. [`SearchMode::Words`] answers a million rows in
    /// milliseconds through a full-text index, where the default reads the whole table.
    ///
    /// ```
    /// # // In navree-ui's own examples the derive names the crate `crate`: this is its path.
    /// # use navree_ui::table;
    /// use navree_ui::table::SearchMode;
    /// use navree_ui::{TableQuery, TableRow};
    ///
    /// #[derive(Clone, TableRow)]
    /// struct Customer {
    ///     #[col(skip)]
    ///     id: i64,
    ///     #[col(search, sortable)]
    ///     name: String,
    /// }
    ///
    /// # fn main() {
    /// let query = TableQuery::<Customer>::parse("/customers", "search=sharma")
    ///     .search_mode(SearchMode::Words);
    /// assert_eq!(query.search_mode_in_use(), SearchMode::Words);
    /// # }
    /// ```
    pub fn search_mode(mut self, mode: SearchMode) -> Self {
        self.search_mode = mode;
        self
    }

    /// The mode the search box matches in.
    pub fn search_mode_in_use(&self) -> SearchMode {
        self.search_mode
    }

    /// The first sort term: its column's name, and which way.
    pub fn sort(&self) -> Option<(&'static str, Direction)> {
        self.state
            .sort
            .first()
            .map(|term| (self.shape.columns[term.column].name, term.direction))
    }

    /// Every sort term, by priority.
    pub fn sorts(&self) -> &[SortTerm] {
        &self.state.sort
    }

    /// The search text (`search` in the URL).
    pub fn search(&self) -> Option<&str> {
        self.state.q.as_deref()
    }

    /// The filters in force.
    pub fn filters(&self) -> &[ActiveFilter] {
        &self.state.filters
    }

    /// The filter in force on column `name`.
    pub fn filter_on(&self, name: &str) -> Option<&FilterValue> {
        self.state
            .filters
            .iter()
            .find(|filter| self.shape.columns[filter.column].name == name)
            .map(|filter| &filter.value)
    }

    /// Whether a search or any filter narrows the rows, page-owned ones too.
    pub fn is_filtered(&self) -> bool {
        self.state.q.is_some() || !self.state.filters.is_empty() || !self.filter.params().is_empty()
    }

    /// The page, from 1.
    pub fn page(&self) -> u32 {
        self.state.page
    }

    /// The rows per page (`per_page` in the URL, `limit` for a cursor table).
    pub fn per_page(&self) -> u32 {
        self.state.per
    }

    /// The rows to skip, for SQL's `OFFSET`.
    pub fn offset(&self) -> u64 {
        u64::from(self.state.page.saturating_sub(1)) * u64::from(self.state.per)
    }

    /// The cursor to read after (`cursor` in the URL).
    pub fn after(&self) -> Option<&str> {
        self.state.after.as_deref()
    }

    /// The cursor to read before.
    pub fn before(&self) -> Option<&str> {
        self.state.before.as_deref()
    }

    /// Whether the user asked for a count of a table that doesn't count by itself.
    pub fn count_asked(&self) -> bool {
        self.state.count
    }

    /// The filters the page owns.
    pub fn filter(&self) -> &F {
        &self.filter
    }

    /// The columns shown, in order: indexes of `COLUMNS`.
    pub fn visible_columns(&self) -> &[usize] {
        &self.state.cols
    }

    /// The rows open to their details, by key.
    pub fn open_rows(&self) -> &[String] {
        &self.state.open
    }

    /// The row density.
    pub fn density(&self) -> TableDensity {
        self.state.density
    }

    fn write_href(&self, state: &State) -> String {
        let pairs = state.pairs_in(&self.shape);
        let own = self.filter.params();
        let mut out = String::with_capacity(self.path.len() + 40);
        out.push_str(&self.path);
        let mut sep = '?';
        // The list query's own spelling, so a link is the same string as the one the
        // browser's clients write for the same state.
        let mut push = |out: &mut String, name: &str, value: &str| {
            out.push(sep);
            sep = '&';
            out.push_str(&encode_component(name));
            out.push('=');
            out.push_str(&encode_component(value));
        };
        for (name, value) in &own {
            push(&mut out, name, value);
        }
        for (name, value) in &pairs {
            push(&mut out, name, value);
        }
        out
    }

    /// This state's URL: one spelling for one state.
    pub fn href(&self) -> String {
        self.write_href(&self.state)
    }

    /// The URL with something changed.
    pub(crate) fn href_with(&self, edit: impl FnOnce(&mut State)) -> String {
        let mut state = self.state.clone();
        edit(&mut state);
        self.write_href(&state)
    }

    /// The query string alone.
    pub fn query_string(&self) -> String {
        let href = self.href();
        href.split_once('?')
            .map_or(String::new(), |(_, query)| query.to_owned())
    }

    /// The search and filters alone: what a "select all matching" belongs to.
    pub fn filter_key(&self) -> String {
        let mut pairs = Vec::new();
        self.state.filter_pairs_in(&self.shape, &mut pairs);
        for (name, value) in self.filter.params() {
            pairs.push((name.to_owned(), value));
        }
        query_of(&pairs)
    }

    /// The column at `index`.
    pub(crate) fn column(&self, index: usize) -> &'static Column {
        &self.shape.columns[index]
    }

    /// Every column, in its first order.
    pub(crate) fn columns(&self) -> &'static [Column] {
        self.shape.columns
    }

    /// The table's settings.
    pub(crate) fn config(&self) -> TableConfig {
        self.shape.config
    }
}

impl<R, F> FromRequestParts for TableQuery<R, F>
where
    R: TableRow + Send + 'static,
    F: FilterBar + Default + DeserializeOwned + Send + 'static,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        Ok(Self::parse(
            parts.uri.path(),
            parts.uri.query().unwrap_or_default(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_querys_names_are_not_the_tables() {
        for name in [
            "sort",
            "search",
            "page",
            "per_page",
            "limit",
            "cursor",
            "before",
            "filter",
            "filter[a]",
            "filter[a][in]",
        ] {
            assert!(is_list_name(name), "{name}");
        }
        for name in [
            "q", "per", "after", "f.title", "cols", "w", "fb", "density", "open", "count", "term",
        ] {
            assert!(!is_list_name(name), "{name}");
        }
    }

    #[test]
    fn a_filter_is_named_the_way_the_grammar_names_it() {
        assert_eq!(filter_param("stage", Op::Eq), "filter[stage]");
        assert_eq!(
            filter_param("title", Op::Contains),
            "filter[title][contains]"
        );
        assert_eq!(filter_param("stage", Op::In), "filter[stage][in]");
        assert_eq!(filter_param("amount", Op::Gte), "filter[amount][gte]");
        assert_eq!(filter_param("amount", Op::Lte), "filter[amount][lte]");
        assert_eq!(filter_param("owner", Op::Null), "filter[owner][null]");
    }

    #[test]
    fn the_page_size_is_per_page_or_limit() {
        assert_eq!(size_param(false), "per_page");
        assert_eq!(size_param(true), "limit");
    }

    #[test]
    fn a_long_query_is_cut_at_a_character() {
        let long = "é".repeat(MAX_QUERY_BYTES);
        let cut = clip_query(&long);
        assert!(cut.len() <= MAX_QUERY_BYTES);
        assert!(cut.chars().all(|c| c == 'é'));
        assert_eq!(clip_query("?a=1"), "a=1");
        assert_eq!(clip_query("a=1"), "a=1");
    }
}
