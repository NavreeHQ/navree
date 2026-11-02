//! [`TableQuery`]: a table's whole state as a URL states it. A bad value is
//! ignored rather than an error: a URL can be old or edited, and a table should
//! still show.
//!
//! ```text
//! q=ada                      the global search
//! sort=-amount,title         columns, in priority order; `-` is descending; `sort=` is none
//! page=3  per=50             a numbered page, and the page size
//! after=...  before=...      a cursor, in cursor mode
//! count=1                    count now, for a table that doesn't by itself
//! f.title=ada                a filter: text, select, boolean, relation
//! f.stage=won&f.stage=lost   any of these
//! f.amount.min=1  .max=9     a range; dates too
//! f.owner.empty=1            is NULL
//! cols=title,amount          the columns shown, in order
//! w=title:240,amount:120     widths in pixels
//! fb=stage,amount             the filters a filter bar shows, in order
//! density=compact            row height
//! open=12,15                 rows open to their details
//! ```

use std::marker::PhantomData;

use http::request::Parts;
use navree_http::extract::{FromRequestParts, urlencoded};
use serde::de::DeserializeOwned;

use super::{
    Column, Direction, FilterBar, FilterKind, MAX_PER_PAGE, NoFilter, TableDensity, TableRow,
};
use crate::form::{encode, encode_into};

/// The longest text a search or text filter keeps, in bytes.
pub const MAX_TEXT: usize = 256;
/// The most values one multi filter keeps.
pub const MAX_VALUES: usize = 200;
/// The most rows open at once.
pub const MAX_OPEN_ROWS: usize = 25;
/// The most query parameters read from one URL.
const MAX_PARAMS: usize = 1000;
/// The most page-owned filter parameters read.
const MAX_PAGE_FILTER_PARAMS: usize = 64;
/// The most sort terms kept.
const MAX_SORTS: usize = 4;
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
/// let query = TableQuery::<Row, NoFilter>::parse("/rows", "sort=title&sort2=x&per=99999&page=-1");
/// assert_eq!(query.sorts()[0].direction, Direction::Asc);
/// assert_eq!(query.page(), 1);
/// assert_eq!(query.per_page(), 25, "a page size the table doesn't offer is ignored");
/// ```
#[derive(Clone, Debug)]
pub struct TableQuery<R, F = NoFilter> {
    pub(crate) path: String,
    pub(crate) state: State,
    pub(crate) filter: F,
    marker: PhantomData<fn() -> R>,
}

fn clip(mut text: String) -> String {
    if text.len() > MAX_TEXT {
        let mut end = MAX_TEXT;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    text
}

impl State {
    /// Reads a query string against a definition.
    fn parse<R: TableRow>(pairs: &[(String, String)]) -> (Self, Vec<(String, String)>) {
        let columns = R::COLUMNS;
        let config = R::CONFIG;
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
        // Filters, in the order of the columns whatever the URL's order.
        let mut texts: Vec<Option<String>> = vec![None; columns.len()];
        let mut values: Vec<Vec<String>> = vec![Vec::new(); columns.len()];
        let mut mins: Vec<Option<String>> = vec![None; columns.len()];
        let mut maxes: Vec<Option<String>> = vec![None; columns.len()];
        let mut flags: Vec<Option<bool>> = vec![None; columns.len()];
        let mut empties: Vec<bool> = vec![false; columns.len()];
        for (name, value) in pairs.iter().take(MAX_PARAMS) {
            match name.as_str() {
                "sort" => {
                    // `sort=` says no order, unlike no `sort` at all.
                    if !sort_seen {
                        state.sort.clear();
                        sort_seen = true;
                    }
                    for term in value.split(',') {
                        let (name, direction) = term
                            .strip_prefix('-')
                            .map_or((term, Direction::Asc), |name| (name, Direction::Desc));
                        let Some(column) = index_of(name).filter(|at| columns[*at].sortable) else {
                            continue;
                        };
                        if state.sort.len() < MAX_SORTS
                            && !state.sort.iter().any(|term| term.column == column)
                        {
                            state.sort.push(SortTerm { column, direction });
                        }
                    }
                }
                "q" if columns.iter().any(|column| column.search) => {
                    let value = value.trim();
                    if !value.is_empty() {
                        state.q = Some(clip(value.to_owned()));
                    }
                }
                "page" => state.page = value.parse::<u32>().unwrap_or(1).max(1),
                "per" => {
                    if let Ok(per) = value.parse::<u32>()
                        && per <= MAX_PER_PAGE
                        && (per == config.per || config.per_pages.contains(&per))
                    {
                        state.per = per;
                    }
                }
                "after" if !value.is_empty() => state.after = Some(clip(value.clone())),
                "before" if !value.is_empty() => state.before = Some(clip(value.clone())),
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
                other => match other.strip_prefix("f.") {
                    Some(rest) if !value.is_empty() => {
                        let (column_name, part) = rest
                            .split_once('.')
                            .map_or((rest, ""), |(name, part)| (name, part));
                        let Some(at) = index_of(column_name) else {
                            continue;
                        };
                        let column = &columns[at];
                        match (column.filter, part) {
                            (FilterKind::Text, "") => {
                                texts[at] = Some(clip(value.trim().to_owned()))
                                    .filter(|text| !text.is_empty())
                            }
                            (FilterKind::Boolean, "") => {
                                flags[at] = matches!(value.as_str(), "true" | "false")
                                    .then_some(value == "true")
                            }
                            (FilterKind::Select | FilterKind::Multi | FilterKind::Relation, "") => {
                                let allowed = column.options.is_empty()
                                    || column.options.iter().any(|(option, _)| *option == value);
                                if allowed
                                    && column.kind.accepts(value)
                                    && values[at].len() < MAX_VALUES
                                    && !values[at].contains(value)
                                {
                                    if column.filter == FilterKind::Select {
                                        values[at].clear();
                                    }
                                    values[at].push(clip(value.clone()));
                                }
                            }
                            (FilterKind::Range, "min" | "from") if column.kind.accepts(value) => {
                                mins[at] = Some(value.clone())
                            }
                            (FilterKind::Range, "max" | "to") if column.kind.accepts(value) => {
                                maxes[at] = Some(value.clone())
                            }
                            (FilterKind::None, _) => {}
                            (_, "empty") if column.nullable => empties[at] = value == "1",
                            _ => {}
                        }
                    }
                    Some(_) => {}
                    None => {
                        if !value.is_empty() {
                            rest.push((name.clone(), value.clone()));
                        }
                    }
                },
            }
        }
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
                state.filters.push(ActiveFilter { column: at, value });
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
        // A cursor is the position; a page number beside it means nothing.
        if config.cursor {
            state.page = 1;
            if state.after.is_some() {
                state.before = None;
            }
        } else {
            state.after = None;
            state.before = None;
        }
        (state, rest)
    }

    pub(crate) fn is_default_sort<R: TableRow>(&self) -> bool {
        let config = R::CONFIG.sort;
        self.sort.len() == config.len()
            && self
                .sort
                .iter()
                .zip(config)
                .all(|(term, (name, direction))| {
                    R::COLUMNS[term.column].name == *name && term.direction == *direction
                })
    }

    fn is_default_cols<R: TableRow>(&self) -> bool {
        let mut at = 0;
        for (index, column) in R::COLUMNS.iter().enumerate() {
            if !column.hidden {
                if self.cols.get(at) != Some(&index) {
                    return false;
                }
                at += 1;
            }
        }
        at == self.cols.len()
    }

    /// The pairs that say what the filters are.
    pub(crate) fn filter_pairs<R: TableRow>(&self, pairs: &mut Vec<(String, String)>) {
        if let Some(q) = &self.q {
            pairs.push(("q".to_owned(), q.clone()));
        }
        for filter in &self.filters {
            let name = R::COLUMNS[filter.column].name;
            match &filter.value {
                FilterValue::Text(text) => pairs.push((format!("f.{name}"), text.clone())),
                FilterValue::Values(values) => pairs.extend(
                    values
                        .iter()
                        .map(|value| (format!("f.{name}"), value.clone())),
                ),
                FilterValue::Bool(flag) => pairs.push((format!("f.{name}"), flag.to_string())),
                FilterValue::Empty => pairs.push((format!("f.{name}.empty"), "1".to_owned())),
                FilterValue::Range { min, max } => {
                    if let Some(min) = min {
                        pairs.push((format!("f.{name}.min"), min.clone()));
                    }
                    if let Some(max) = max {
                        pairs.push((format!("f.{name}.max"), max.clone()));
                    }
                }
            }
        }
    }

    /// Every pair, in one order, leaving out what's at its default.
    pub(crate) fn pairs<R: TableRow>(&self) -> Vec<(String, String)> {
        let mut pairs = Vec::new();
        self.filter_pairs::<R>(&mut pairs);
        if !self.is_default_sort::<R>() {
            let mut sort = String::new();
            for (at, term) in self.sort.iter().enumerate() {
                if at > 0 {
                    sort.push(',');
                }
                if term.direction == Direction::Desc {
                    sort.push('-');
                }
                sort.push_str(R::COLUMNS[term.column].name);
            }
            pairs.push(("sort".to_owned(), sort));
        }
        if self.per != R::CONFIG.per.clamp(1, MAX_PER_PAGE) {
            pairs.push(("per".to_owned(), self.per.to_string()));
        }
        if !self.is_default_cols::<R>() {
            let names: Vec<&str> = self.cols.iter().map(|at| R::COLUMNS[*at].name).collect();
            pairs.push(("cols".to_owned(), names.join(",")));
        }
        if !self.widths.is_empty() {
            let widths: Vec<String> = self
                .widths
                .iter()
                .map(|(at, width)| format!("{}:{width}", R::COLUMNS[*at].name))
                .collect();
            pairs.push(("w".to_owned(), widths.join(",")));
        }
        if let Some(bar) = &self.bar {
            let names: Vec<&str> = bar.iter().map(|at| R::COLUMNS[*at].name).collect();
            let every: Vec<usize> = (0..R::COLUMNS.len())
                .filter(|at| R::COLUMNS[*at].filter != FilterKind::None)
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
        if self.page > 1 {
            pairs.push(("page".to_owned(), self.page.to_string()));
        }
        if let Some(after) = &self.after {
            pairs.push(("after".to_owned(), after.clone()));
        }
        if let Some(before) = &self.before {
            pairs.push(("before".to_owned(), before.clone()));
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
        let pairs: Vec<(String, String)> = urlencoded::parse(query.as_bytes())
            .map(|(name, value)| (name.into_owned(), value.into_owned()))
            .collect();
        let (state, rest) = State::parse::<R>(&pairs);
        let filter = page_filter::<F>(&rest);
        Self {
            path: path.into(),
            state,
            filter,
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
            return Self::parse(self.path, &view.query);
        }
        self
    }
}

impl<R: TableRow, F: FilterBar> TableQuery<R, F> {
    /// The page's path.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The first sort term: its column's name, and which way.
    pub fn sort(&self) -> Option<(&'static str, Direction)> {
        self.state
            .sort
            .first()
            .map(|term| (R::COLUMNS[term.column].name, term.direction))
    }

    /// Every sort term, by priority.
    pub fn sorts(&self) -> &[SortTerm] {
        &self.state.sort
    }

    /// The search text.
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
            .find(|filter| R::COLUMNS[filter.column].name == name)
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

    /// The rows per page.
    pub fn per_page(&self) -> u32 {
        self.state.per
    }

    /// The rows to skip, for SQL's `OFFSET`.
    pub fn offset(&self) -> u64 {
        u64::from(self.state.page.saturating_sub(1)) * u64::from(self.state.per)
    }

    /// The cursor to read after.
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
        let pairs = state.pairs::<R>();
        let own = self.filter.params();
        let mut out = String::with_capacity(self.path.len() + 40);
        out.push_str(&self.path);
        let mut sep = '?';
        let mut push = |out: &mut String, name: &str, value: &str| {
            out.push(sep);
            sep = '&';
            encode_into(out, name);
            out.push('=');
            encode_into(out, value);
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
        self.state.filter_pairs::<R>(&mut pairs);
        for (name, value) in self.filter.params() {
            pairs.push((name.to_owned(), value));
        }
        let borrowed: Vec<(&str, &str)> = pairs
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        encode(&borrowed)
    }

    /// The column at `index`.
    pub(crate) const fn column(index: usize) -> &'static Column {
        &R::COLUMNS[index]
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
