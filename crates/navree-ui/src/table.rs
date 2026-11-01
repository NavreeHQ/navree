//! Tables from a row struct: [`TableRow`] (derived), [`TableQuery`] (sort,
//! filter and page in the URL, so links are shareable and JavaScript is
//! optional) and [`DataTable`] (a region that morphs in place).

use std::marker::PhantomData;
use std::time::Duration;

use http::request::Parts;
use navree_http::extract::{FromRequestParts, urlencoded};
use serde::de::DeserializeOwned;

use crate::components::{Badge, BadgeVariant, Button, Empty, Input, NativeSelect, Pagination, Skeleton, SkeletonShape, icon};
use crate::form::{encode, encode_into};
use crate::html::Href;
use crate::icons;
use crate::node::{AttrValue, Element, IntoNodes, Node, Uses, el};

/// One column. The derive writes these.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Column {
    /// The field's name: what `?sort=` holds.
    pub name: &'static str,
    /// The heading.
    pub label: &'static str,
    /// Whether the heading is a sort link.
    pub sortable: bool,
    /// Aligned right, in tabular figures.
    pub numeric: bool,
    /// Hidden on phones.
    pub hide_on_mobile: bool,
}

/// A struct that can be a row. `#[derive(TableRow)]` implements it.
pub trait TableRow {
    /// The columns, in order.
    const COLUMNS: &'static [Column];

    /// The row's stable key, so a morph keeps it.
    fn key(&self) -> Option<String>;

    /// The cells, in the order of the columns.
    fn cells(&self) -> Vec<Node>;
}

/// A value as a table cell shows it.
pub trait CellValue {
    /// The cell's content.
    fn to_cell(&self) -> Node;
}

impl CellValue for String {
    #[inline]
    fn to_cell(&self) -> Node {
        Node::Text(self.clone().into())
    }
}

impl CellValue for &'static str {
    #[inline]
    fn to_cell(&self) -> Node {
        Node::from(*self)
    }
}

macro_rules! int_cells {
    ($($type:ty),+) => {
        $(impl CellValue for $type {
            #[inline]
            fn to_cell(&self) -> Node {
                Node::Int(i64::from(*self))
            }
        })+
    };
}

int_cells!(i8, i16, i32, i64, u8, u16, u32);

impl CellValue for crate::form::Money {
    fn to_cell(&self) -> Node {
        Node::Text(self.display().into())
    }
}

impl CellValue for chrono::NaiveDate {
    fn to_cell(&self) -> Node {
        Node::Text(self.format("%Y-%m-%d").to_string().into())
    }
}

impl<T: CellValue> CellValue for Option<T> {
    fn to_cell(&self) -> Node {
        self.as_ref().map_or(Node::Empty, CellValue::to_cell)
    }
}

/// A `<td>`. The derive calls this.
#[inline]
pub fn cell(content: Node, numeric: bool, hide_on_mobile: bool) -> Node {
    Node::Element(el("td").flag("data-numeric", numeric).flag("data-hide-mobile", hide_on_mobile).child(content))
}

/// A link in a cell. The derive calls this.
#[inline]
pub fn link_cell(href: String, content: Node) -> Node {
    Node::Element(el("a").href(href).child(content))
}

/// A badge in a cell. The derive calls this.
#[inline]
pub fn badge_cell(content: Node, variant: BadgeVariant) -> Node {
    Badge::new(content).variant(variant).into()
}

/// A struct of filters. `#[derive(FilterBar)]` implements it.
pub trait FilterBar {
    /// The filters set now, as URL parameters.
    fn params(&self) -> Vec<(&'static str, String)>;

    /// The toolbar's controls.
    fn controls(&self) -> Vec<Node>;
}

/// No filters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize)]
pub struct NoFilter {}

impl FilterBar for NoFilter {
    fn params(&self) -> Vec<(&'static str, String)> {
        Vec::new()
    }

    fn controls(&self) -> Vec<Node> {
        Vec::new()
    }
}

/// A search box in a toolbar. The derive calls this.
pub fn search_control(name: &'static str, label: &'static str, value: Option<String>) -> Node {
    let input = Input::search(name).placeholder(label).attr("aria-label", label);
    match value {
        Some(value) => input.value(value).into(),
        None => input.into(),
    }
}

/// A select in a toolbar. The derive calls this.
pub fn select_control(name: &'static str, label: &'static str, options: &'static [(&'static str, &'static str)], value: Option<String>) -> Node {
    let mut select = NativeSelect::new(name).prompt(label).attr("aria-label", label);
    for (option, text) in options {
        select = select.option(*option, *text);
    }
    if let Some(value) = value {
        select = select.selected(value);
    }
    select.into()
}

/// Which way a column is sorted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Direction {
    /// Smallest first.
    #[default]
    Asc,
    /// Largest first.
    Desc,
}

/// The largest page size a URL can ask for.
pub const MAX_PER_PAGE: u32 = 1000;

/// Sort, filter and page as the URL states them. A bad value is ignored rather
/// than an error: a URL can be old or edited, and a table should still show.
#[derive(Clone, Debug)]
pub struct TableQuery<R, F = NoFilter> {
    path: String,
    sort: Option<(&'static str, Direction)>,
    page: u32,
    per_page: u32,
    filter: F,
    marker: PhantomData<fn() -> R>,
}

impl<R: TableRow, F: FilterBar + Default + DeserializeOwned> TableQuery<R, F> {
    /// Reads a query string.
    pub fn parse(path: impl Into<String>, query: &str) -> Self {
        let mut sort_name = None;
        let mut direction = Direction::Asc;
        let mut page = 1;
        let mut per_page = 50;
        let mut rest: Vec<(String, String)> = Vec::new();
        for (name, value) in urlencoded::parse(query.as_bytes()) {
            match name.as_ref() {
                "sort" => sort_name = Some(value.into_owned()),
                "dir" => {
                    if value == "desc" {
                        direction = Direction::Desc;
                    }
                }
                "page" => page = value.parse().unwrap_or(1).max(1),
                "per" => per_page = value.parse().unwrap_or(50).clamp(1, MAX_PER_PAGE),
                _ if !value.is_empty() => rest.push((name.into_owned(), value.into_owned())),
                _ => {}
            }
        }
        let sort = sort_name
            .and_then(|name| R::COLUMNS.iter().find(|column| column.sortable && column.name == name))
            .map(|column| (column.name, direction));
        let pairs: Vec<(&str, &str)> = rest.iter().map(|(name, value)| (name.as_str(), value.as_str())).collect();
        let encoded = encode(&pairs);
        let filter = F::deserialize(urlencoded::Deserializer::new(encoded.as_bytes())).unwrap_or_default();
        Self { path: path.into(), sort, page, per_page, filter, marker: PhantomData }
    }

    /// The column to sort by, and which way.
    pub fn sort(&self) -> Option<(&'static str, Direction)> {
        self.sort
    }

    /// The page, from 1.
    pub fn page(&self) -> u32 {
        self.page
    }

    /// The rows per page.
    pub fn per_page(&self) -> u32 {
        self.per_page
    }

    /// The rows to skip, for SQL's `OFFSET`.
    pub fn offset(&self) -> u64 {
        u64::from(self.page - 1) * u64::from(self.per_page)
    }

    /// The filters.
    pub fn filter(&self) -> &F {
        &self.filter
    }

    /// The URL with another sort and page.
    fn href(&self, sort: Option<(&str, Direction)>, page: u32) -> String {
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
        if let Some((name, direction)) = sort {
            push(&mut out, "sort", name);
            if direction == Direction::Desc {
                push(&mut out, "dir", "desc");
            }
        }
        for (name, value) in self.filter.params() {
            push(&mut out, name, &value);
        }
        if page > 1 {
            push(&mut out, "page", &page.to_string());
        }
        if self.per_page != 50 {
            push(&mut out, "per", &self.per_page.to_string());
        }
        out
    }
}

impl<R, F> FromRequestParts for TableQuery<R, F>
where
    R: TableRow + Send + 'static,
    F: FilterBar + Default + DeserializeOwned + Send + 'static,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        Ok(Self::parse(parts.uri.path(), parts.uri.query().unwrap_or_default()))
    }
}

/// A table with sort links, a filter bar and pagination, as one region.
///
/// ```ignore
/// DataTable::new("deals", &query, &rows).total(1000)
/// ```
#[must_use]
pub struct DataTable<'a, R, F> {
    id: &'static str,
    query: &'a TableQuery<R, F>,
    rows: &'a [R],
    total: u64,
    empty: Option<Empty>,
    caption: Option<&'static str>,
}

impl<'a, R: TableRow, F: FilterBar + Default + DeserializeOwned> DataTable<'a, R, F> {
    /// The `rows` of this page; `id` names the region.
    pub fn new(id: &'static str, query: &'a TableQuery<R, F>, rows: &'a [R]) -> Self {
        Self { id, query, rows, total: rows.len() as u64, empty: None, caption: None }
    }

    /// Sets how many rows there are across all pages.
    pub fn total(mut self, total: u64) -> Self {
        self.total = total;
        self
    }

    /// Sets what shows when there are no rows.
    pub fn empty(mut self, empty: Empty) -> Self {
        self.empty = Some(empty);
        self
    }

    /// Names the table for assistive technology.
    pub fn caption(mut self, caption: &'static str) -> Self {
        self.caption = Some(caption);
        self
    }

    fn head(&self) -> Element {
        let current = self.query.sort();
        let cells: Vec<Element> = R::COLUMNS
            .iter()
            .map(|column| {
                let mut th = el("th").attr("scope", "col").flag("data-numeric", column.numeric).flag("data-hide-mobile", column.hide_on_mobile);
                let sorted = current.filter(|(name, _)| *name == column.name).map(|(_, direction)| direction);
                if let Some(direction) = sorted {
                    th = th.attr("aria-sort", if direction == Direction::Asc { "ascending" } else { "descending" });
                }
                if column.sortable {
                    // The first click sorts up, the next down.
                    let next = if sorted == Some(Direction::Asc) { Direction::Desc } else { Direction::Asc };
                    let glyph = match sorted {
                        Some(Direction::Asc) => icons::ARROW_UP,
                        Some(Direction::Desc) => icons::ARROW_DOWN,
                        None => icons::ARROW_UP_DOWN,
                    };
                    th.child(
                        el("a")
                            .slot("sort-link")
                            .href(self.query.href(Some((column.name, next)), 1))
                            .flag("data-ui-in-region", true)
                            .children((column.label, icon(glyph))),
                    )
                } else {
                    th.child(column.label)
                }
            })
            .collect();
        el("thead").child(el("tr").child(cells))
    }

    fn into_node(self) -> Node {
        let mut root = el("div").slot("data-table").attr("id", self.id).flag("data-ui-region", true).uses(Uses::CORE);
        let controls = self.query.filter().controls();
        if !controls.is_empty() {
            root = root.child(
                el("form").slot("table-toolbar").attr("role", "search").attr("method", "get").attr("action", self.query.path.clone()).attr("data-ui-target", self.id).children((
                    controls,
                    self.query.sort().map(|(name, _)| el("input").attr("type", "hidden").attr("name", "sort").attr("value", name)),
                    self.query.sort().filter(|(_, direction)| *direction == Direction::Desc).map(|_| el("input").attr("type", "hidden").attr("name", "dir").attr("value", "desc")),
                    // With JavaScript off, a select needs a button.
                    el("noscript").child(Button::outline("Apply").submit()),
                )),
            );
        }
        if self.rows.is_empty() {
            return root.child(self.empty.unwrap_or_else(|| Empty::new("Nothing here yet.").icon(icons::INBOX))).into();
        }
        let mut rows = Vec::with_capacity(self.rows.len());
        for row in self.rows {
            let mut tr = el("tr");
            if let Some(key) = row.key() {
                tr = tr.attr("data-key", key);
            }
            tr.children = row.cells();
            rows.push(Node::Element(tr));
        }
        let table = el("table").slot("table").children((
            self.caption.map(|caption| el("caption").child(caption)),
            self.head(),
            el("tbody").child(rows),
        ));
        let per = u64::from(self.query.per_page());
        let pages = u32::try_from(self.total.div_ceil(per).max(1)).unwrap_or(u32::MAX);
        let sort = self.query.sort();
        let from = self.query.offset() + 1;
        let to = (from + self.rows.len() as u64).saturating_sub(1);
        let footer = el("div").slot("table-footer").children((
            el("span").child(format!("{from}–{to} of {}", self.total)),
            (pages > 1).then(|| in_region(Pagination::new(self.query.page(), pages, |page| self.query.href(sort, page)).into())),
        ));
        root.children((el("div").slot("table-container").child(table), footer)).into()
    }
}

impl<R: TableRow, F: FilterBar + Default + DeserializeOwned> From<DataTable<'_, R, F>> for Node {
    fn from(table: DataTable<'_, R, F>) -> Self {
        table.into_node()
    }
}

/// Marks every link inside as staying in its region.
fn in_region(mut node: Node) -> Node {
    fn walk(node: &mut Node) {
        if let Node::Element(element) = node {
            if element.tag == "a" && element.get("href").is_some() {
                element.attrs.push(crate::node::Attr { name: "data-ui-in-region", value: AttrValue::Flag });
            }
            element.children.iter_mut().for_each(walk);
        }
    }
    walk(&mut node);
    node
}

/// A named part of a page that updates by itself: fetched after paint, polled,
/// or refetched when an op invalidates it. Its answer morphs in.
///
/// ```
/// use navree_ui::{Region, render};
///
/// let html = render(&Region::new("kpis").src("/fragments/kpis").lazy().into());
/// assert!(html.contains(r#"data-ui-src="/fragments/kpis""#));
/// assert!(html.contains("data-ui-lazy"));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Region {
    el: Element,
    placeholder: bool,
}

impl Region {
    /// A region named `id`.
    pub fn new(id: impl Into<AttrValue>) -> Self {
        Self { el: el("div").slot("region").attr("id", id).flag("data-ui-region", true).uses(Uses::CORE), placeholder: false }
    }

    /// Sets where its content comes from: a page holding an element with this
    /// id, or a fragment.
    pub fn src(mut self, src: impl Into<Href>) -> Self {
        self.el = self.el.attr("data-ui-src", src.into());
        self
    }

    /// Fetches it after paint, showing a skeleton until then.
    pub fn lazy(mut self) -> Self {
        self.el = self.el.flag("data-ui-lazy", true).attr("aria-busy", "true");
        self.placeholder = true;
        self
    }

    /// Refetches it this often, while the page is visible.
    pub fn poll(mut self, every: Duration) -> Self {
        self.el = self.el.attr("data-ui-poll", i64::try_from(every.as_millis()).unwrap_or(i64::MAX));
        self
    }

    /// Adds its content.
    pub fn children(mut self, children: impl IntoNodes) -> Self {
        self.el = self.el.child(children);
        self.placeholder = false;
        self
    }

    fn into_node(self) -> Node {
        if self.placeholder {
            return self.el.children((Skeleton::new(SkeletonShape::Line), Skeleton::new(SkeletonShape::Short))).into();
        }
        self.el.into()
    }
}

impl From<Region> for Node {
    fn from(region: Region) -> Self {
        region.into_node()
    }
}
