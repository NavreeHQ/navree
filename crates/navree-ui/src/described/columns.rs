//! [`DescribedTable`]: a table's columns and settings, given at run time.

use std::borrow::Cow;

use http::StatusCode;
use serde::{Deserialize, Deserializer};

use super::{keep_options, keep_slice, keep_str};
use crate::node::{Node, el};
use crate::table::{
    Align, BulkRequest, CellWriter, Column, Count, Direction, FilterBar, FilterKind, Shape, Sticky,
    TableConfig, TableQuery, TableRow, ValueKind,
};

/// One column of a [`DescribedTable`]: what `#[col(...)]` says of a field, said at run
/// time. The name is what the URL holds; the label is the heading.
///
/// ```
/// use navree_ui::described::ColumnSpec;
/// use navree_ui::table::{FilterKind, ValueKind};
///
/// let amount = ColumnSpec::new("amount", "Amount")
///     .kind(ValueKind::Money)
///     .sortable()
///     .numeric()
///     .filter(FilterKind::Range);
/// assert_eq!(amount.column().name, "amount");
/// ```
#[derive(Clone, Copy, Debug)]
#[must_use]
pub struct ColumnSpec {
    column: Column,
}

impl ColumnSpec {
    /// The column `name`, headed `label`: text, not sorted, filtered or searched.
    pub fn new(name: &str, label: &str) -> Self {
        let name = keep_str(name);
        Self {
            column: Column {
                name,
                label: keep_str(label),
                source: name,
                kind: ValueKind::Text,
                nullable: false,
                sortable: false,
                numeric: false,
                hide_on_mobile: false,
                search: false,
                strong: false,
                hidden: false,
                truncate: false,
                wrap: false,
                locked: false,
                sticky: Sticky::No,
                align: Align::Start,
                width: None,
                group: None,
                filter: FilterKind::None,
                options: &[],
            },
        }
    }

    /// The column queried, when it isn't the name: `companies.name`.
    pub fn source(mut self, source: &str) -> Self {
        self.column.source = keep_str(source);
        self
    }

    /// How the column's values bind, from the URL's filters and the keys of its rows.
    pub fn kind(mut self, kind: ValueKind) -> Self {
        self.column.kind = kind;
        self
    }

    /// The column can hold NULL: its filter offers "is empty".
    pub fn nullable(mut self) -> Self {
        self.column.nullable = true;
        self
    }

    /// Its heading is a sort link.
    pub fn sortable(mut self) -> Self {
        self.column.sortable = true;
        self
    }

    /// Aligned to the end, in tabular figures.
    pub fn numeric(mut self) -> Self {
        self.column.numeric = true;
        self
    }

    /// Hidden on phones.
    pub fn hide_on_mobile(mut self) -> Self {
        self.column.hide_on_mobile = true;
        self
    }

    /// Part of the global search: the table has a search box when a column is.
    pub fn search(mut self) -> Self {
        self.column.search = true;
        self
    }

    /// Heavier text.
    pub fn strong(mut self) -> Self {
        self.column.strong = true;
        self
    }

    /// Off until the user shows it.
    pub fn hidden(mut self) -> Self {
        self.column.hidden = true;
        self
    }

    /// Cut to one line, with the whole text as a tooltip.
    pub fn truncate(mut self) -> Self {
        self.column.truncate = true;
        self
    }

    /// Wraps instead of staying on one line.
    pub fn wrap(mut self) -> Self {
        self.column.wrap = true;
        self
    }

    /// Can't be hidden.
    pub fn locked(mut self) -> Self {
        self.column.locked = true;
        self
    }

    /// Stays in view while the table scrolls sideways.
    pub fn sticky(mut self, sticky: Sticky) -> Self {
        self.column.sticky = sticky;
        self
    }

    /// Where the cells line up.
    pub fn align(mut self, align: Align) -> Self {
        self.column.align = align;
        self
    }

    /// Its width in pixels, until the user resizes it.
    pub fn width(mut self, pixels: u32) -> Self {
        self.column.width = Some(pixels);
        self
    }

    /// The group heading above it.
    pub fn group(mut self, group: &str) -> Self {
        self.column.group = Some(keep_str(group));
        self
    }

    /// Its filter.
    pub fn filter(mut self, filter: FilterKind) -> Self {
        self.column.filter = filter;
        self
    }

    /// The fixed choices of a select or multi filter: `(value, label)`. Choices the page
    /// looks up, such as a relation's rows, go to
    /// [`DataTable::options`](crate::DataTable::options).
    pub fn options<I, V, L>(mut self, options: I) -> Self
    where
        I: IntoIterator<Item = (V, L)>,
        V: AsRef<str>,
        L: AsRef<str>,
    {
        self.column.options = keep_options(options);
        self
    }

    /// The column as the table reads it.
    pub fn column(&self) -> &Column {
        &self.column
    }
}

/// A table's columns and settings, given at run time: what `#[derive(TableRow)]` and
/// `#[table(...)]` say of a struct. [`query`](Self::query) reads a URL against them,
/// and the [`DataTable`](crate::DataTable) of that query draws [`DescribedRow`]s exactly
/// as it draws a derived row.
///
/// Parameters of the page's own, such as a parent's key, are dropped from the table's
/// links unless they are [`kept`](Self::keep).
#[derive(Clone, Debug)]
#[must_use]
pub struct DescribedTable {
    columns: Vec<Column>,
    sort: Vec<(&'static str, Direction)>,
    config: TableConfig,
    kept: Vec<&'static str>,
}

impl Default for DescribedTable {
    fn default() -> Self {
        Self::new()
    }
}

impl DescribedTable {
    /// A table with no columns yet, and what a table has unless it says otherwise:
    /// 25 rows a page of 10, 25, 50 or 100, an exact count, whole-number keys.
    pub fn new() -> Self {
        Self {
            columns: Vec::new(),
            sort: Vec::new(),
            config: TableConfig::DEFAULT,
            kept: Vec::new(),
        }
    }

    /// Adds a column, after the others.
    pub fn column(mut self, column: ColumnSpec) -> Self {
        self.columns.push(column.column);
        self
    }

    /// Adds `name` to the order the URL gets when it names none, after any before it.
    pub fn sort(mut self, name: &str, direction: Direction) -> Self {
        self.sort.push((keep_str(name), direction));
        self
    }

    /// Sets the rows per page when the URL names none.
    pub fn per(mut self, rows: u32) -> Self {
        self.config.per = rows;
        self
    }

    /// Sets the page sizes on offer: the URL can't ask for another.
    pub fn per_pages(mut self, sizes: impl IntoIterator<Item = u32>) -> Self {
        self.config.per_pages = keep_slice(sizes.into_iter().collect());
        self
    }

    /// Pages by keyset instead of by number, for huge tables.
    pub fn cursor(mut self) -> Self {
        self.config.cursor = true;
        self
    }

    /// Sets how rows are counted.
    pub fn count(mut self, count: Count) -> Self {
        self.config.count = count;
        self
    }

    /// Sets how a row's key binds: [`ValueKind::Int`] unless said, [`ValueKind::Text`]
    /// for keys of any kind. A bulk action naming a key that isn't one is refused.
    pub fn key_kind(mut self, kind: ValueKind) -> Self {
        self.config.key_kind = kind;
        self
    }

    /// Sets the column that changes when a row does, compared before a bulk action.
    pub fn version(mut self, column: &str) -> Self {
        self.config.version = Some(keep_str(column));
        self
    }

    /// Keeps the page's own parameter `name` (`range`, `via[company_id]`) in every link
    /// and form of the table, as a page-owned filter is kept.
    pub fn keep(mut self, name: &str) -> Self {
        let name = keep_str(name);
        if !self.kept.contains(&name) {
            self.kept.push(name);
        }
        self
    }

    /// The columns, in their first order.
    pub fn columns(&self) -> &[Column] {
        &self.columns
    }

    /// The index of the column `name`: what a [`SortTerm`](crate::table::SortTerm) and
    /// an [`ActiveFilter`](crate::table::ActiveFilter) name a column by.
    pub fn position(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|column| column.name == name)
    }

    /// What the table's query and draw read.
    fn shape(&self) -> Shape {
        let mut config = self.config;
        config.sort = keep_slice(self.sort.clone());
        Shape {
            columns: keep_slice(self.columns.clone()),
            config,
        }
    }

    /// The table's state as `query` states it, at `path`: what
    /// [`TableQuery::parse`] reads for a derived row. A bad value is left out.
    pub fn query(
        &self,
        path: impl Into<String>,
        query: &str,
    ) -> TableQuery<DescribedRow, KeptParams> {
        let mut parsed =
            TableQuery::<DescribedRow, KeptParams>::parse_shaped(path, query, self.shape());
        parsed.filter.keep(&self.kept);
        parsed
    }

    /// A bulk action's post, read against these columns: what
    /// [`BulkRequest::parse`] reads for a derived row.
    pub fn bulk(&self, body: &[u8]) -> Result<BulkRequest<DescribedRow, KeptParams>, StatusCode> {
        let mut request =
            BulkRequest::<DescribedRow, KeptParams>::parse_shaped(body, self.shape())?;
        request.query.filter.keep(&self.kept);
        Ok(request)
    }
}

/// The parameters of a page that a [`DescribedTable`] keeps in its links: those it
/// was told to [`keep`](DescribedTable::keep), and no others.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeptParams {
    pairs: Vec<(Cow<'static, str>, String)>,
}

impl KeptParams {
    /// Keeps the pairs named in `names`, in their order in the URL; drops the rest.
    fn keep(&mut self, names: &[&'static str]) {
        self.pairs = std::mem::take(&mut self.pairs)
            .into_iter()
            .filter_map(|(name, value)| {
                names
                    .iter()
                    .find(|kept| **kept == name)
                    .filter(|_| !value.is_empty())
                    .map(|kept| (Cow::Borrowed(*kept), value))
            })
            .collect();
    }

    /// The value of the kept parameter `name`.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.pairs
            .iter()
            .find(|(kept, _)| kept == name)
            .map(|(_, value)| value.as_str())
    }
}

impl<'de> Deserialize<'de> for KeptParams {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let pairs = Vec::<(String, String)>::deserialize(deserializer)?;
        Ok(Self {
            pairs: pairs
                .into_iter()
                .map(|(name, value)| (Cow::Owned(name), value))
                .collect(),
        })
    }
}

impl FilterBar for KeptParams {
    fn params(&self) -> Vec<(&'static str, String)> {
        self.pairs
            .iter()
            .filter_map(|(name, value)| match name {
                Cow::Borrowed(name) => Some((*name, value.clone())),
                Cow::Owned(_) => None,
            })
            .collect()
    }

    /// The kept parameters as hidden fields, so the toolbar's form keeps them too.
    fn controls(&self) -> Vec<Node> {
        self.params()
            .into_iter()
            .map(|(name, value)| {
                el("input")
                    .attr("type", "hidden")
                    .attr("name", name)
                    .attr("value", value)
                    .into()
            })
            .collect()
    }
}

/// What one cell of a [`DescribedRow`] shows, and its plain text for a file or a label.
#[derive(Clone, Debug, Default)]
pub struct DescribedCell {
    content: Content,
}

/// What a cell holds.
#[derive(Clone, Debug, Default)]
enum Content {
    #[default]
    Empty,
    /// The user's text, with a direction of its own.
    Text(String),
    /// A number, a date or a label, in the cell's direction.
    Plain(String),
    /// A component, and the text it stands for.
    Node(Node, String),
}

impl DescribedCell {
    /// Nothing.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Text a user wrote: a name, a title. It reads and clips from its own start, in
    /// whatever direction its language has.
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: Content::Text(text.into()),
        }
    }

    /// A number, a date or a label: text in the cell's direction.
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            content: Content::Plain(text.into()),
        }
    }

    /// A component, such as a badge or a link, drawn through the page's one renderer,
    /// and `text`, what it says for a file or a label.
    pub fn node(node: impl Into<Node>, text: impl Into<String>) -> Self {
        Self {
            content: Content::Node(node.into(), text.into()),
        }
    }

    /// The cell's plain text.
    pub fn as_text(&self) -> &str {
        match &self.content {
            Content::Empty => "",
            Content::Text(text) | Content::Plain(text) | Content::Node(_, text) => text,
        }
    }
}

/// A row of a [`DescribedTable`]: its key and one cell for each column, in the columns'
/// order. A row with fewer cells than columns leaves the rest empty.
#[derive(Clone, Debug, Default)]
pub struct DescribedRow {
    key: Option<String>,
    cells: Vec<DescribedCell>,
}

impl DescribedRow {
    /// The row with the key `key`, which a morph keeps it by and a bulk action names it
    /// by.
    pub fn new(key: impl Into<String>, cells: Vec<DescribedCell>) -> Self {
        Self {
            key: Some(key.into()),
            cells,
        }
    }

    /// A row with no key: it can't be selected or opened.
    pub fn keyless(cells: Vec<DescribedCell>) -> Self {
        Self { key: None, cells }
    }

    /// Its key.
    pub fn id(&self) -> Option<&str> {
        self.key.as_deref()
    }

    /// Its cells.
    pub fn cells(&self) -> &[DescribedCell] {
        &self.cells
    }
}

/// A described row has no columns of its own: the table reads them from its query, which
/// [`DescribedTable::query`] gave the description's.
impl TableRow for DescribedRow {
    const COLUMNS: &'static [Column] = &[];

    fn key(&self) -> Option<String> {
        self.key.clone()
    }

    fn write_cell(&self, column: usize, out: &mut CellWriter<'_>) {
        match self.cells.get(column).map(|cell| &cell.content) {
            Some(Content::Text(text)) => out.text(text),
            Some(Content::Plain(text)) => out.plain(text),
            Some(Content::Node(node, _)) => out.node(node),
            Some(Content::Empty) | None => {}
        }
    }

    fn write_text(&self, column: usize, out: &mut String) {
        if let Some(cell) = self.cells.get(column) {
            out.push_str(cell.as_text());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> DescribedTable {
        DescribedTable::new()
            .column(ColumnSpec::new("title", "Title").sortable().search())
            .column(
                ColumnSpec::new("stage", "Stage")
                    .filter(FilterKind::Multi)
                    .options([("lead", "Lead"), ("won", "Won")]),
            )
            .key_kind(ValueKind::Text)
            .sort("title", Direction::Desc)
            .keep("via[company_id]")
    }

    #[test]
    fn the_url_is_read_against_the_described_columns() {
        let query = table().query(
            "/deals",
            "sort=title&filter[stage][in]=won,lost&filter[nope]=1&search=agra",
        );
        assert_eq!(query.sort(), Some(("title", Direction::Asc)));
        assert_eq!(query.search(), Some("agra"));
        // `lost` is not a choice, and `nope` not a column.
        assert_eq!(query.filters().len(), 1);
        assert_eq!(query.filters()[0].column, 1);
        // With nothing said, the description's order.
        let plain = table().query("/deals", "");
        assert_eq!(plain.sort(), Some(("title", Direction::Desc)));
        assert_eq!(plain.href(), "/deals");
    }

    #[test]
    fn only_the_kept_parameters_stay_in_the_links() {
        let query = table().query("/deals", "via[company_id]=7&utm_source=mail&page=2");
        assert_eq!(query.filter().get("via[company_id]"), Some("7"));
        assert_eq!(query.filter().get("utm_source"), None);
        let href = query.href();
        assert!(href.contains("via%5Bcompany_id%5D=7"), "{href}");
        assert!(!href.contains("utm_source"), "{href}");
        assert!(href.contains("page=2"), "{href}");
    }

    #[test]
    fn a_row_writes_its_cells_by_column() {
        let row = DescribedRow::new(
            "a1",
            vec![
                DescribedCell::text("<b>Agra</b>"),
                DescribedCell::plain("Won"),
            ],
        );
        assert_eq!(TableRow::key(&row), Some("a1".to_owned()));
        let mut html = String::new();
        row.write_cell(0, &mut CellWriter::new(&mut html));
        assert_eq!(html, "<bdi>&lt;b&gt;Agra&lt;/b&gt;</bdi>");
        let mut text = String::new();
        row.write_text(1, &mut text);
        row.write_text(5, &mut text);
        assert_eq!(text, "Won");
    }

    /// A row typed the way the derive types one.
    #[derive(Clone)]
    struct Deal {
        id: i64,
        title: String,
        stage: &'static str,
        amount: i64,
        hot: bool,
    }

    const fn column(name: &'static str, label: &'static str) -> Column {
        Column {
            name,
            label,
            source: name,
            kind: ValueKind::Text,
            nullable: false,
            sortable: false,
            numeric: false,
            hide_on_mobile: false,
            search: false,
            strong: false,
            hidden: false,
            truncate: false,
            wrap: false,
            locked: false,
            sticky: Sticky::No,
            align: Align::Start,
            width: None,
            group: None,
            filter: FilterKind::None,
            options: &[],
        }
    }

    const STAGES: &[(&str, &str)] = &[("lead", "Lead"), ("won", "Won")];

    impl TableRow for Deal {
        const COLUMNS: &'static [Column] = &[
            Column {
                sortable: true,
                search: true,
                filter: FilterKind::Text,
                ..column("title", "Title")
            },
            Column {
                filter: FilterKind::Multi,
                options: STAGES,
                ..column("stage", "Stage")
            },
            Column {
                kind: ValueKind::Int,
                nullable: true,
                sortable: true,
                numeric: true,
                filter: FilterKind::Range,
                ..column("amount", "Amount")
            },
            Column {
                hidden: true,
                filter: FilterKind::Boolean,
                ..column("hot", "Hot")
            },
        ];
        const CONFIG: TableConfig = TableConfig {
            key_kind: ValueKind::Int,
            sort: &[("amount", Direction::Desc), ("title", Direction::Asc)],
            per: 25,
            per_pages: &[10, 25, 50],
            cursor: false,
            version: None,
            count: Count::Exact,
        };

        fn key(&self) -> Option<String> {
            Some(self.id.to_string())
        }

        fn write_cell(&self, column: usize, out: &mut CellWriter<'_>) {
            match column {
                0 => out.text(&self.title),
                1 => out.plain(crate::table::option_label(STAGES, self.stage)),
                2 => out.display(&self.amount),
                3 => out.plain(if self.hot { "Yes" } else { "No" }),
                _ => {}
            }
        }

        fn write_text(&self, column: usize, out: &mut String) {
            match column {
                0 => out.push_str(&self.title),
                1 => out.push_str(crate::table::option_label(STAGES, self.stage)),
                2 => out.push_str(&self.amount.to_string()),
                3 => out.push_str(if self.hot { "Yes" } else { "No" }),
                _ => {}
            }
        }
    }

    /// The same table, described.
    fn described() -> DescribedTable {
        DescribedTable::new()
            .column(
                ColumnSpec::new("title", "Title")
                    .sortable()
                    .search()
                    .filter(FilterKind::Text),
            )
            .column(
                ColumnSpec::new("stage", "Stage")
                    .filter(FilterKind::Multi)
                    .options([("lead", "Lead"), ("won", "Won")]),
            )
            .column(
                ColumnSpec::new("amount", "Amount")
                    .kind(ValueKind::Int)
                    .nullable()
                    .sortable()
                    .numeric()
                    .filter(FilterKind::Range),
            )
            .column(
                ColumnSpec::new("hot", "Hot")
                    .hidden()
                    .filter(FilterKind::Boolean),
            )
            .sort("amount", Direction::Desc)
            .sort("title", Direction::Asc)
            .per(25)
            .per_pages([10, 25, 50])
    }

    fn deals() -> Vec<Deal> {
        (1..=3)
            .map(|id| Deal {
                id,
                title: format!("Deal {id} <Agra>"),
                stage: if id == 2 { "won" } else { "lead" },
                amount: id * 1500,
                hot: id == 3,
            })
            .collect()
    }

    fn as_described(deal: &Deal) -> DescribedRow {
        DescribedRow::new(
            deal.id.to_string(),
            vec![
                DescribedCell::text(deal.title.clone()),
                DescribedCell::plain(crate::table::option_label(STAGES, deal.stage)),
                DescribedCell::plain(deal.amount.to_string()),
                DescribedCell::plain(if deal.hot { "Yes" } else { "No" }),
            ],
        )
    }

    #[test]
    fn a_described_table_is_drawn_exactly_as_the_typed_one() {
        use crate::render::{measure, render};
        use crate::table::{BulkAction, DataTable, NoFilter, RowAction};

        let typed_rows = deals();
        let described_rows: Vec<DescribedRow> = typed_rows.iter().map(as_described).collect();
        for url in [
            "",
            "filter[stage][in]=won&search=deal&sort=title,-amount&filter[amount][gte]=5&page=2",
            "cols=title,hot,stage&w=title:240&density=compact&filter[hot]=true",
            "per_page=50&filter[amount][lte]=3000&filter[amount][null]=true&fb=stage",
        ] {
            let typed_query = TableQuery::<Deal, NoFilter>::parse("/deals", url);
            let described_query = described().query("/deals", url);
            let typed: Node = DataTable::new("deals", &typed_query, &typed_rows)
                .caption("Deals")
                .filter_bar()
                .bulk("/deals/bulk", [BulkAction::new("archive", "Archive")])
                .row_actions(|deal: &Deal| {
                    vec![RowAction::link("Open", format!("/deals/{}", deal.id))]
                })
                .row_href(|deal: &Deal| format!("/deals/{}", deal.id))
                .export("/deals.csv")
                .csrf("token")
                .total(40)
                .into();
            let drawn: Node = DataTable::new("deals", &described_query, &described_rows)
                .caption("Deals")
                .filter_bar()
                .bulk("/deals/bulk", [BulkAction::new("archive", "Archive")])
                .row_actions(|row: &DescribedRow| {
                    vec![RowAction::link(
                        "Open",
                        format!("/deals/{}", row.id().unwrap_or_default()),
                    )]
                })
                .row_href(|row: &DescribedRow| format!("/deals/{}", row.id().unwrap_or_default()))
                .export("/deals.csv")
                .csrf("token")
                .total(40)
                .into();
            assert_eq!(render(&typed), render(&drawn), "{url}");
            assert_eq!(measure(&typed).uses, measure(&drawn).uses, "{url}");
        }
    }

    #[test]
    fn a_described_query_reads_and_writes_the_url_as_the_typed_one() {
        use crate::table::NoFilter;

        for url in [
            "",
            "sort=title",
            "sort=",
            "filter[stage][in]=won,lead&filter[stage][in]=nope&filter[title][contains]=agra",
            "filter[amount][gte]=x&filter[amount][lte]=20&cols=hot&per_page=7&page=3",
            "open=1,2&count=1&density=spacious",
        ] {
            let typed = TableQuery::<Deal, NoFilter>::parse("/deals", url);
            let described = described().query("/deals", url);
            assert_eq!(typed.href(), described.href(), "{url}");
            assert_eq!(typed.filter_key(), described.filter_key(), "{url}");
            assert_eq!(typed.sorts(), described.sorts(), "{url}");
            assert_eq!(typed.filters(), described.filters(), "{url}");
            assert_eq!(
                typed.visible_columns(),
                described.visible_columns(),
                "{url}"
            );
            assert_eq!(typed.per_page(), described.per_page(), "{url}");
        }
    }

    #[test]
    fn a_bulk_post_is_read_against_the_described_key() {
        let post = b"action=archive&ids=a1&ids=b2&_path=%2Fdeals&_q=via%5Bcompany_id%5D%3D7";
        let request = table().bulk(post).unwrap();
        assert_eq!(request.ids(), ["a1", "b2"]);
        assert_eq!(request.query().filter().get("via[company_id]"), Some("7"));
        // Whole-number keys refuse text.
        let numbers = DescribedTable::new().column(ColumnSpec::new("title", "Title"));
        assert_eq!(
            numbers.bulk(b"action=archive&ids=a1").unwrap_err(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
}
