//! The data table. One definition, `#[derive(TableRow)]`, drives everything:
//!
//! - [`TableQuery`] reads the state from the URL (search, filters, sort, page or
//!   cursor, page size, columns, widths, density, open rows), through an
//!   allow-list, so links are shareable and JavaScript is optional.
//! - With the `orm` feature, `TableQuery::load` runs it through `navree-orm` on
//!   PostgreSQL, MySQL, SQLite and MongoDB, and `TableQuery::export_csv` writes the
//!   same query as a file.
//! - [`DataTable`] draws it: a region that morphs in place.
//! - [`BulkRequest`] reads a bulk action back and resolves the selection against
//!   the same definition.
//!
//! ```ignore
//! #[derive(Clone, Model, TableRow, serde::Serialize)]
//! #[table(sort = "-updated_at", per = 25)]
//! struct Deal {
//!     #[col(skip)]
//!     id: i64,
//!     #[col(sortable, search, sticky, filter = text, link = "/deals/{id}")]
//!     title: String,
//!     #[col(sortable, filter = multi)]
//!     stage: Stage,
//!     #[col(sortable, numeric, filter = range)]
//!     amount: Option<i64>,
//!     #[col(sortable, filter = date)]
//!     updated_at: chrono::NaiveDate,
//! }
//!
//! async fn index(shell: AppShell, uow: Uow, query: TableQuery<Deal>) -> Result<AppPage> {
//!     let page = query.load(Deal::query(), &uow).await?;
//!     Ok(shell.page("Deals").body(
//!         DataTable::new("deals", &query, &page)
//!             .caption("Deals")
//!             .bulk("/deals/bulk", [BulkAction::new("archive", "Archive")]),
//!     ))
//! }
//! ```

use std::fmt::Write as _;

use crate::components::{BadgeVariant, Input, NativeSelect};
use crate::html::{Href, escape_into};
use crate::node::{Node, Uses};

mod csv;
mod edit;
#[cfg(feature = "orm")]
mod orm;
mod query;
mod region;
mod saved;
mod selection;
mod view;

pub use csv::write_csv_field;
pub use edit::EditCell;
pub use query::{
    ActiveFilter, FilterValue, MAX_OPEN_ROWS, MAX_TEXT, MAX_VALUES, SortTerm, TableQuery,
};
pub use region::Region;
pub use saved::{SavedView, ViewOp, ViewPost};
pub use selection::{BulkMode, BulkRefusal, BulkRequest, MAX_SELECTED_IDS};
pub use view::{
    BulkAction, DataTable, FilterMode, FilterOption, RowAction, TableData, TableDensity,
    TableHeight, TableLabels, TablePage, Total,
};

#[cfg(feature = "orm")]
pub use orm::{AsyncChunk, MAX_EXPORT_ROWS, SavedViewModel, delete_view, list_views, save_view};

/// How a column's values are typed, so a value from a URL binds as what the
/// column holds. A value that doesn't parse as its kind is dropped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ValueKind {
    /// Text.
    #[default]
    Text,
    /// A whole number.
    Int,
    /// Minor units of money: `12.50` in a URL binds as `1250`.
    Money,
    /// A float.
    Float,
    /// An exact decimal.
    Decimal,
    /// A date.
    Date,
    /// An instant in UTC. A range's bounds are whole days.
    DateTime,
    /// True or false.
    Bool,
    /// A UUID.
    Uuid,
}

impl ValueKind {
    /// Whether `text` parses as this kind.
    pub fn accepts(self, text: &str) -> bool {
        match self {
            Self::Text => true,
            Self::Int => text.parse::<i64>().is_ok(),
            Self::Money => crate::form::Money::parse(text).is_some(),
            Self::Float | Self::Decimal => text.parse::<f64>().is_ok_and(f64::is_finite),
            Self::Date | Self::DateTime => {
                chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").is_ok()
            }
            Self::Bool => matches!(text, "true" | "false"),
            Self::Uuid => uuid::Uuid::parse_str(text).is_ok(),
        }
    }

    /// The type of `<input>` that takes this kind in a range.
    pub(crate) const fn input_type(self) -> &'static str {
        match self {
            Self::Date | Self::DateTime => "date",
            Self::Int | Self::Float | Self::Decimal => "number",
            _ => "text",
        }
    }
}

/// The kind of a field's values. Every type a cell can show has one.
pub trait ColumnValue {
    /// How values bind.
    const KIND: ValueKind;
    /// Whether the column can hold NULL.
    const NULLABLE: bool = false;
}

macro_rules! column_values {
    ($($kind:ident: $($type:ty),+;)+) => {
        $($(impl ColumnValue for $type {
            const KIND: ValueKind = ValueKind::$kind;
        })+)+
    };
}

column_values! {
    Text: String, &'static str;
    Int: i8, i16, i32, i64, u8, u16, u32, u64;
    Money: crate::form::Money;
    Float: f32, f64;
    Date: chrono::NaiveDate;
    DateTime: chrono::DateTime<chrono::Utc>, chrono::NaiveDateTime;
    Bool: bool;
    Uuid: uuid::Uuid;
}

impl<T: ColumnValue> ColumnValue for Option<T> {
    const KIND: ValueKind = T::KIND;
    const NULLABLE: bool = true;
}

/// Which filter a column offers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FilterKind {
    /// None.
    #[default]
    None,
    /// Contains this text, ignoring case: `f.title=sutlej`.
    Text,
    /// Between two values, each optional: `f.amount.min=1&f.amount.max=9`.
    Range,
    /// Is this one value: `f.stage=won`.
    Select,
    /// Is any of these: `f.stage=won&f.stage=lost`.
    Multi,
    /// Yes or no: `f.active=true`.
    Boolean,
    /// Is any of these keys of another model, named by options the page passes.
    Relation,
}

/// Where a column's cells line up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    /// The reading direction's start.
    #[default]
    Start,
    /// The middle.
    Center,
    /// The reading direction's end.
    End,
}

/// Whether a column stays in view while the table scrolls sideways.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sticky {
    /// It scrolls.
    #[default]
    No,
    /// At the start.
    Start,
    /// At the end.
    End,
}

/// One column. The derive writes these.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Column {
    /// The field's name: what the URL holds.
    pub name: &'static str,
    /// The heading.
    pub label: &'static str,
    /// The column queried, when it isn't the name: `companies.name`.
    pub source: &'static str,
    /// How values bind.
    pub kind: ValueKind,
    /// Whether it can hold NULL.
    pub nullable: bool,
    /// Whether the heading is a sort link.
    pub sortable: bool,
    /// Aligned right, in tabular figures.
    pub numeric: bool,
    /// Hidden on phones.
    pub hide_on_mobile: bool,
    /// Part of the global search.
    pub search: bool,
    /// Heavier text.
    pub strong: bool,
    /// Off until the user shows it.
    pub hidden: bool,
    /// Cut to one line with the full text as a tooltip.
    pub truncate: bool,
    /// Wraps instead of staying on one line.
    pub wrap: bool,
    /// Can't be hidden.
    pub locked: bool,
    /// Stays in view.
    pub sticky: Sticky,
    /// Where cells line up.
    pub align: Align,
    /// Its width in pixels, until the user resizes it.
    pub width: Option<u32>,
    /// The group heading above it.
    pub group: Option<&'static str>,
    /// Its filter.
    pub filter: FilterKind,
    /// The fixed choices of a select or multi filter: `(value, label)`.
    pub options: &'static [(&'static str, &'static str)],
}

/// How a table counts its rows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Count {
    /// An exact total: one more query for each page.
    #[default]
    Exact,
    /// At most this many are counted, so the query is bounded: "10,000+".
    Capped(u64),
    /// No count: Previous and Next only.
    None,
}

/// A table's settings. The derive writes these from `#[table(...)]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableConfig {
    /// How the key binds.
    pub key_kind: ValueKind,
    /// The order when the URL names none.
    pub sort: &'static [(&'static str, Direction)],
    /// The rows per page when the URL names none.
    pub per: u32,
    /// The page sizes on offer. The URL can't ask for another.
    pub per_pages: &'static [u32],
    /// Keyset pages instead of numbered ones, for huge tables.
    pub cursor: bool,
    /// A column that changes when a row does, compared before a bulk action.
    pub version: Option<&'static str>,
    /// How rows are counted.
    pub count: Count,
}

impl TableConfig {
    /// What a table has unless it says otherwise.
    pub const DEFAULT: Self = Self {
        key_kind: ValueKind::Int,
        sort: &[],
        per: 25,
        per_pages: &[10, 25, 50, 100],
        cursor: false,
        version: None,
        count: Count::Exact,
    };
}

/// A struct that can be a row. `#[derive(TableRow)]` implements it.
pub trait TableRow {
    /// The columns, in their first order.
    const COLUMNS: &'static [Column];

    /// The table's settings.
    const CONFIG: TableConfig = TableConfig::DEFAULT;

    /// The row's stable key, so a morph keeps it and a bulk action can name it.
    fn key(&self) -> Option<String>;

    /// Writes column `column`'s cell as HTML, escaped.
    fn write_cell(&self, column: usize, out: &mut CellWriter<'_>);

    /// Writes column `column`'s cell as plain text, for a file or a label.
    fn write_text(&self, column: usize, out: &mut String);
}

/// Where a cell writes. It can only escape: there is no way to write markup through
/// it, so a custom [`CellValue`] can't put raw HTML on the page.
///
/// ```compile_fail
/// use navree_ui::table::CellWriter;
///
/// fn raw(out: &mut CellWriter<'_>) {
///     out.out.push_str("<img src=x onerror=alert(1)>");
/// }
/// ```
pub struct CellWriter<'a> {
    out: &'a mut String,
    uses: Uses,
}

impl<'a> CellWriter<'a> {
    pub(crate) fn new(out: &'a mut String) -> Self {
        Self {
            out,
            uses: Uses::NONE,
        }
    }

    /// Writes a component: a badge, an avatar, a list of tags. It goes through the
    /// same renderer as the rest of a page, so its text is escaped.
    pub fn node(&mut self, node: &Node) {
        self.uses = self.uses.with(crate::render::measure(node).uses);
        crate::render::render_into(self.out, node);
    }

    /// The modules what was written needs.
    pub(crate) fn uses(&self) -> Uses {
        self.uses
    }

    /// Writes text, escaped, with the direction of its own.
    pub fn text(&mut self, text: &str) {
        self.out.push_str("<bdi>");
        escape_into(self.out, text);
        self.out.push_str("</bdi>");
    }

    /// Writes text, escaped, in the cell's direction: for numbers, dates and labels.
    pub fn plain(&mut self, text: &str) {
        escape_into(self.out, text);
    }

    /// Writes a value through `Display`, escaped.
    pub fn display(&mut self, value: &impl std::fmt::Display) {
        struct Escaped<'b>(&'b mut String);
        impl std::fmt::Write for Escaped<'_> {
            fn write_str(&mut self, text: &str) -> std::fmt::Result {
                escape_into(self.0, text);
                Ok(())
            }
        }
        let _ = write!(Escaped(self.out), "{value}");
    }
}

/// A value as a table cell shows it, written straight to the page.
pub trait CellValue {
    /// Writes the cell's content, through the one escaping path.
    fn write_cell(&self, out: &mut CellWriter<'_>);

    /// Writes it as plain text.
    fn write_text(&self, out: &mut String);
}

impl CellValue for String {
    #[inline]
    fn write_cell(&self, out: &mut CellWriter<'_>) {
        out.text(self);
    }

    #[inline]
    fn write_text(&self, out: &mut String) {
        out.push_str(self);
    }
}

impl CellValue for &str {
    /// Text is the user's: `<bdi>` gives it its own direction, so an Arabic name
    /// in an English table, or the reverse, reads and clips from its own start.
    #[inline]
    fn write_cell(&self, out: &mut CellWriter<'_>) {
        out.text(self);
    }

    #[inline]
    fn write_text(&self, out: &mut String) {
        out.push_str(self);
    }
}

macro_rules! display_cells {
    ($($type:ty),+) => {
        $(impl CellValue for $type {
            #[inline]
            fn write_cell(&self, out: &mut CellWriter<'_>) {
                out.display(self);
            }

            #[inline]
            fn write_text(&self, out: &mut String) {
                let _ = write!(out, "{self}");
            }
        })+
    };
}

display_cells!(i8, i16, i32, i64, u8, u16, u32, u64, f32, f64, uuid::Uuid);

impl CellValue for bool {
    fn write_cell(&self, out: &mut CellWriter<'_>) {
        out.plain(if *self { "Yes" } else { "No" });
    }

    fn write_text(&self, out: &mut String) {
        out.push_str(if *self { "Yes" } else { "No" });
    }
}

impl CellValue for crate::form::Money {
    fn write_cell(&self, out: &mut CellWriter<'_>) {
        out.plain(&self.display());
    }

    fn write_text(&self, out: &mut String) {
        out.push_str(&self.display());
    }
}

macro_rules! formatted_cells {
    ($($type:ty => $format:literal),+) => {
        $(impl CellValue for $type {
            fn write_cell(&self, out: &mut CellWriter<'_>) {
                out.display(&self.format($format));
            }

            fn write_text(&self, out: &mut String) {
                let _ = write!(out, "{}", self.format($format));
            }
        })+
    };
}

formatted_cells!(chrono::NaiveDate => "%Y-%m-%d", chrono::NaiveDateTime => "%Y-%m-%d %H:%M", chrono::DateTime<chrono::Utc> => "%Y-%m-%d %H:%M");

impl<T: CellValue> CellValue for Option<T> {
    #[inline]
    fn write_cell(&self, out: &mut CellWriter<'_>) {
        if let Some(value) = self {
            value.write_cell(out);
        }
    }

    #[inline]
    fn write_text(&self, out: &mut String) {
        if let Some(value) = self {
            value.write_text(out);
        }
    }
}

/// The label of an option's value. The derive calls this.
pub fn option_label<'a>(options: &[(&'a str, &'a str)], value: &'a str) -> &'a str {
    options
        .iter()
        .find(|(option, _)| *option == value)
        .map_or(value, |(_, label)| *label)
}

/// A stored value shown as its option's label: the derive wraps a field with
/// `options = ...` in this.
pub struct OptionCell<'a, T> {
    /// The stored value.
    pub value: &'a T,
    /// `(value, label)` for each choice.
    pub options: &'static [(&'static str, &'static str)],
}

/// A stored text value, present or not.
pub trait OptionText {
    /// The text, when there is one.
    fn option_text(&self) -> Option<&str>;
}

impl OptionText for String {
    fn option_text(&self) -> Option<&str> {
        Some(self)
    }
}

impl OptionText for &str {
    fn option_text(&self) -> Option<&str> {
        Some(self)
    }
}

impl<T: OptionText> OptionText for Option<T> {
    fn option_text(&self) -> Option<&str> {
        self.as_ref().and_then(OptionText::option_text)
    }
}

impl<T: OptionText> CellValue for OptionCell<'_, T> {
    fn write_cell(&self, out: &mut CellWriter<'_>) {
        if let Some(value) = self.value.option_text() {
            out.plain(option_label_of(self.options, value));
        }
    }

    fn write_text(&self, out: &mut String) {
        if let Some(value) = self.value.option_text() {
            out.push_str(option_label_of(self.options, value));
        }
    }
}

fn option_label_of<'a>(options: &'a [(&'a str, &'a str)], value: &'a str) -> &'a str {
    options
        .iter()
        .find(|(option, _)| *option == value)
        .map_or(value, |(_, label)| *label)
}

/// One value in a URL's path, percent-encoded, so a stored `../../logout` or
/// `//evil.example` stays one harmless segment. The derive calls this for every
/// field in `link = "..."`. `.` and `..` can't be encoded away, since browsers read
/// `%2e%2e` as `..` too, so they become `_`.
pub fn path_segment(value: &impl std::fmt::Display) -> String {
    let text = value.to_string();
    if text == "." || text == ".." {
        return "_".to_owned();
    }
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

/// Writes a link in a cell. The derive calls this.
pub fn write_link(out: &mut CellWriter<'_>, href: String, content: &impl CellValue) {
    out.out.push_str("<a href=\"");
    escape_into(out.out, Href::from(href).as_str());
    out.out.push_str("\">");
    content.write_cell(out);
    out.out.push_str("</a>");
}

/// Writes a badge in a cell. The derive calls this.
pub fn write_badge(out: &mut CellWriter<'_>, content: &impl CellValue, variant: BadgeVariant) {
    out.out.push_str(match variant {
        BadgeVariant::Outline => "<span data-slot=\"badge\" data-variant=\"outline\">",
        BadgeVariant::Success => "<span data-slot=\"badge\" data-variant=\"success\">",
        BadgeVariant::Warning => "<span data-slot=\"badge\" data-variant=\"warning\">",
        BadgeVariant::Danger => "<span data-slot=\"badge\" data-variant=\"danger\">",
        _ => "<span data-slot=\"badge\">",
    });
    content.write_cell(out);
    out.out.push_str("</span>");
}

/// A column with no value of its own: what it shows comes from `#[col(cell = ...)]`
/// or `DataTable::cell`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Computed;

impl CellValue for Computed {
    fn write_cell(&self, _: &mut CellWriter<'_>) {}

    fn write_text(&self, _: &mut String) {}
}

impl ColumnValue for Computed {
    const KIND: ValueKind = ValueKind::Text;
}

/// Where a table's rows come from, when that isn't `navree-orm`. Rows can always be
/// handed straight to [`DataTable`]; this is the shape to give a reusable source, such
/// as another service. `TableQuery` has already checked the state against the
/// definition, so a source reads only allowed columns and bound values through its
/// accessors. `navree-orm` is the path built for this: see `TableQuery::load`.
pub trait TableSource<R: TableRow> {
    /// What can go wrong.
    type Error;

    /// One page for this state.
    fn page<F: FilterBar + Sync>(
        &self,
        query: &TableQuery<R, F>,
    ) -> impl std::future::Future<Output = Result<TablePage<R>, Self::Error>> + Send;
}

/// A struct of filters a page owns: controls in the toolbar and parameters in the
/// URL that the table keeps but doesn't query. `#[derive(FilterBar)]` implements it.
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
    let input = Input::search(name)
        .placeholder(label)
        .attr("aria-label", label);
    match value {
        Some(value) => input.value(value).into(),
        None => input.into(),
    }
}

/// A select in a toolbar. The derive calls this.
pub fn select_control(
    name: &'static str,
    label: &'static str,
    options: &'static [(&'static str, &'static str)],
    value: Option<String>,
) -> Node {
    let mut select = NativeSelect::new(name)
        .prompt(label)
        .attr("aria-label", label);
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

/// The largest page size a URL can ask for, whatever a definition offers.
pub const MAX_PER_PAGE: u32 = 1000;
