//! [`DataTable`]: draws a [`TableQuery`] and its rows as one region.
//!
//! Everything a user can change is a link or a control of one GET form, so all
//! of it works with JavaScript off and survives reload, back and forward. The
//! runtime makes it morph in place, and adds what only a browser can do: drag to
//! resize, drag to reorder, the selection across pages.

use std::borrow::Cow;
use std::fmt::Write as _;

use serde::de::DeserializeOwned;

use super::query::{FilterValue, State};
use super::{Align, CellWriter, Column, Direction, FilterBar, FilterKind, NoFilter, Sticky, TableQuery, TableRow, ValueKind};
use crate::components::{AlertDialog, Button, ButtonVariant, Empty, Input, NativeSelect, Pagination, Size, Skeleton, SkeletonShape, icon};
use crate::html::{Href, SafeHtml, escape_into};
use crate::icons::{self, Icon};
use crate::node::{Attr, AttrValue, Element, IntoNodes, Node, Uses, el};

/// How tall rows are.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TableDensity {
    /// Tight, for scanning a lot.
    Compact,
    /// The default.
    #[default]
    Default,
    /// With room.
    Spacious,
}

impl TableDensity {
    pub(crate) fn parse(text: &str) -> Self {
        match text {
            "compact" => Self::Compact,
            "spacious" => Self::Spacious,
            _ => Self::Default,
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Compact => "compact",
            Self::Default => "default",
            Self::Spacious => "spacious",
        }
    }
}

/// How tall the scrolling part is, as a closed choice: inline styles aren't
/// allowed, and a sticky header needs a height to stick in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TableHeight {
    /// As tall as its rows.
    #[default]
    Auto,
    /// About 20rem.
    Sm,
    /// About 32rem.
    Md,
    /// About 48rem.
    Lg,
    /// What's left of the window.
    Fill,
}

impl TableHeight {
    const fn as_str(self) -> Option<&'static str> {
        match self {
            Self::Auto => None,
            Self::Sm => Some("sm"),
            Self::Md => Some("md"),
            Self::Lg => Some("lg"),
            Self::Fill => Some("fill"),
        }
    }
}

/// Where a table's filters live.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FilterMode {
    /// In a popover from each column's header.
    #[default]
    Popover,
    /// In a row under the headers. Ranges and multi selects open a popover from it.
    Row,
}

/// How many rows match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Total {
    /// Exactly this many.
    Exact(u64),
    /// At least this many: a capped count hit its cap.
    AtLeast(u64),
    /// Not counted.
    Unknown,
}

/// One page of rows, as [`TableQuery::load`] answers.
#[derive(Clone, Debug)]
pub struct TablePage<R> {
    /// The rows.
    pub rows: Vec<R>,
    /// How many match.
    pub total: Total,
    /// The page shown: less than asked when rows were deleted meanwhile.
    pub page: u32,
    /// Whether a next page has rows.
    pub has_more: bool,
    /// The cursor of the next page.
    pub next: Option<String>,
    /// The cursor of the previous page.
    pub prev: Option<String>,
    /// What the version column's highest value is, when the table has one.
    pub version: Option<String>,
}

impl<R> TablePage<R> {
    /// A page of `rows` of `total`.
    pub fn new(rows: Vec<R>, total: u64) -> Self {
        Self { rows, total: Total::Exact(total), page: 1, has_more: false, next: None, prev: None, version: None }
    }
}

/// What a [`DataTable`] draws.
#[derive(Clone, Copy, Debug)]
pub struct TableData<'a, R> {
    rows: &'a [R],
    total: Total,
    page: Option<u32>,
    has_more: bool,
    next: Option<&'a str>,
    prev: Option<&'a str>,
    version: Option<&'a str>,
}

impl<'a, R> From<&'a [R]> for TableData<'a, R> {
    fn from(rows: &'a [R]) -> Self {
        Self { rows, total: Total::Exact(rows.len() as u64), page: None, has_more: false, next: None, prev: None, version: None }
    }
}

impl<'a, R, const N: usize> From<&'a [R; N]> for TableData<'a, R> {
    fn from(rows: &'a [R; N]) -> Self {
        rows.as_slice().into()
    }
}

impl<'a, R> From<&'a Vec<R>> for TableData<'a, R> {
    fn from(rows: &'a Vec<R>) -> Self {
        rows.as_slice().into()
    }
}

impl<'a, R> From<&'a TablePage<R>> for TableData<'a, R> {
    fn from(page: &'a TablePage<R>) -> Self {
        Self {
            rows: &page.rows,
            total: page.total,
            page: Some(page.page),
            has_more: page.has_more,
            next: page.next.as_deref(),
            prev: page.prev.as_deref(),
            version: page.version.as_deref(),
        }
    }
}

/// One choice of a filter whose choices come from the page: a relation's rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilterOption {
    value: String,
    label: String,
    count: Option<u64>,
}

impl FilterOption {
    /// A choice posting `value`, showing `label`.
    pub fn new(value: impl ToString, label: impl Into<String>) -> Self {
        Self { value: value.to_string(), label: label.into(), count: None }
    }

    /// How many rows hold it, shown beside it.
    #[must_use]
    pub fn count(mut self, count: u64) -> Self {
        self.count = Some(count);
        self
    }
}

/// An action on the selected rows.
#[derive(Clone, Debug)]
#[must_use]
pub struct BulkAction {
    id: &'static str,
    label: Cow<'static, str>,
    icon: Option<Icon>,
    destructive: bool,
    confirm: Option<Cow<'static, str>>,
    denied: Option<Cow<'static, str>>,
}

impl BulkAction {
    /// The action `id`, what the server reads, showing `label`.
    pub fn new(id: &'static str, label: impl Into<Cow<'static, str>>) -> Self {
        Self { id, label: label.into(), icon: None, destructive: false, confirm: None, denied: None }
    }

    /// Adds an icon.
    pub fn icon(mut self, glyph: Icon) -> Self {
        self.icon = Some(glyph);
        self
    }

    /// Marks it as destroying something.
    pub fn destructive(mut self) -> Self {
        self.destructive = true;
        self
    }

    /// Asks first. `{n}` in the text is how many rows.
    pub fn confirm(mut self, question: impl Into<Cow<'static, str>>) -> Self {
        self.confirm = Some(question.into());
        self
    }

    /// Shows it disabled, with why. The handler checks again: this is courtesy.
    pub fn denied(mut self, reason: impl Into<Cow<'static, str>>) -> Self {
        self.denied = Some(reason.into());
        self
    }
}

/// One thing a row's menu does.
#[derive(Clone, Debug)]
#[must_use]
pub struct RowAction {
    label: Cow<'static, str>,
    href: Href,
    post: bool,
    destructive: bool,
    icon: Option<Icon>,
}

impl RowAction {
    /// A link.
    pub fn link(label: impl Into<Cow<'static, str>>, href: impl Into<Href>) -> Self {
        Self { label: label.into(), href: href.into(), post: false, destructive: false, icon: None }
    }

    /// A button that posts to `href`.
    pub fn post(label: impl Into<Cow<'static, str>>, href: impl Into<Href>) -> Self {
        Self { label: label.into(), href: href.into(), post: true, destructive: false, icon: None }
    }

    /// Marks it as destroying something.
    pub fn destructive(mut self) -> Self {
        self.destructive = true;
        self
    }

    /// Adds an icon.
    pub fn icon(mut self, glyph: Icon) -> Self {
        self.icon = Some(glyph);
        self
    }
}

/// The words of a table. English by default; pass your own for another language.
/// `{n}`, `{from}`, `{to}`, `{total}` and `{label}` are filled in.
#[derive(Clone, Debug)]
#[allow(missing_docs)]
pub struct TableLabels {
    pub search: Cow<'static, str>,
    pub columns: Cow<'static, str>,
    pub view: Cow<'static, str>,
    pub views: Cow<'static, str>,
    pub export: Cow<'static, str>,
    pub apply: Cow<'static, str>,
    pub clear_filters: Cow<'static, str>,
    pub remove_filter: Cow<'static, str>,
    pub remove_sort: Cow<'static, str>,
    pub sorted_by: Cow<'static, str>,
    pub rows_per_page: Cow<'static, str>,
    pub showing: Cow<'static, str>,
    pub showing_open: Cow<'static, str>,
    pub one_result: Cow<'static, str>,
    pub count_all: Cow<'static, str>,
    pub first: Cow<'static, str>,
    pub previous: Cow<'static, str>,
    pub next: Cow<'static, str>,
    pub empty: Cow<'static, str>,
    pub empty_filtered: Cow<'static, str>,
    pub empty_filtered_hint: Cow<'static, str>,
    pub failed: Cow<'static, str>,
    pub retry: Cow<'static, str>,
    pub lost_access: Cow<'static, str>,
    pub loading: Cow<'static, str>,
    pub select_row: Cow<'static, str>,
    pub select_page: Cow<'static, str>,
    pub selected: Cow<'static, str>,
    pub page_selected: Cow<'static, str>,
    pub select_all: Cow<'static, str>,
    pub all_selected: Cow<'static, str>,
    pub some_selected: Cow<'static, str>,
    pub off_page: Cow<'static, str>,
    pub clear_selection: Cow<'static, str>,
    pub selection_ended: Cow<'static, str>,
    pub confirm_title: Cow<'static, str>,
    pub confirm: Cow<'static, str>,
    pub cancel: Cow<'static, str>,
    pub sort_asc: Cow<'static, str>,
    pub sort_desc: Cow<'static, str>,
    pub sort_add: Cow<'static, str>,
    pub sort_clear: Cow<'static, str>,
    pub hide_column: Cow<'static, str>,
    pub column_options: Cow<'static, str>,
    pub move_up: Cow<'static, str>,
    pub move_down: Cow<'static, str>,
    pub resize: Cow<'static, str>,
    pub reset_columns: Cow<'static, str>,
    pub empty_value: Cow<'static, str>,
    pub any: Cow<'static, str>,
    pub yes: Cow<'static, str>,
    pub no: Cow<'static, str>,
    pub min: Cow<'static, str>,
    pub max: Cow<'static, str>,
    pub from: Cow<'static, str>,
    pub to: Cow<'static, str>,
    pub find_option: Cow<'static, str>,
    pub show_details: Cow<'static, str>,
    pub hide_details: Cow<'static, str>,
    pub actions: Cow<'static, str>,
    pub density: Cow<'static, str>,
    pub compact: Cow<'static, str>,
    pub default_density: Cow<'static, str>,
    pub spacious: Cow<'static, str>,
    pub save_view: Cow<'static, str>,
    pub view_name: Cow<'static, str>,
    pub delete_view: Cow<'static, str>,
    pub make_default: Cow<'static, str>,
    pub default_view: Cow<'static, str>,
}

impl Default for TableLabels {
    fn default() -> Self {
        Self {
            search: "Search".into(),
            columns: "Columns".into(),
            view: "View".into(),
            views: "Views".into(),
            export: "Export".into(),
            apply: "Apply".into(),
            clear_filters: "Clear filters".into(),
            remove_filter: "Remove filter {label}".into(),
            remove_sort: "Remove {label}".into(),
            sorted_by: "Sorted by".into(),
            rows_per_page: "Rows per page".into(),
            showing: "Showing {from} to {to} of {total} results".into(),
            showing_open: "Showing {from} to {to}".into(),
            one_result: "1 result".into(),
            count_all: "Count all".into(),
            first: "First".into(),
            previous: "Previous".into(),
            next: "Next".into(),
            empty: "Nothing here yet.".into(),
            empty_filtered: "No results match these filters.".into(),
            empty_filtered_hint: "Change or clear the search and filters.".into(),
            failed: "That didn't load. The rows below may be out of date.".into(),
            retry: "Try again".into(),
            lost_access: "You no longer have access to this.".into(),
            loading: "Loading".into(),
            select_row: "Select {label}".into(),
            select_page: "Select all on this page".into(),
            selected: "{n} selected".into(),
            page_selected: "All {n} on this page are selected.".into(),
            select_all: "Select all {n} results".into(),
            all_selected: "All {n} results are selected.".into(),
            some_selected: "{n} of {total} results are selected.".into(),
            off_page: "{n} not on this page".into(),
            clear_selection: "Clear selection".into(),
            selection_ended: "The filter changed, so the selection of all results ended.".into(),
            confirm_title: "Are you sure?".into(),
            confirm: "Yes, go ahead".into(),
            cancel: "Cancel".into(),
            sort_asc: "Sort ascending".into(),
            sort_desc: "Sort descending".into(),
            sort_add: "Add to sort".into(),
            sort_clear: "Stop sorting".into(),
            hide_column: "Hide column".into(),
            column_options: "{label} options".into(),
            move_up: "Move {label} up".into(),
            move_down: "Move {label} down".into(),
            resize: "Resize {label}".into(),
            reset_columns: "Reset columns".into(),
            empty_value: "Empty".into(),
            any: "Any".into(),
            yes: "Yes".into(),
            no: "No".into(),
            min: "Min".into(),
            max: "Max".into(),
            from: "From".into(),
            to: "To".into(),
            find_option: "Find".into(),
            show_details: "Show details".into(),
            hide_details: "Hide details".into(),
            actions: "Actions".into(),
            density: "Density".into(),
            compact: "Compact".into(),
            default_density: "Default".into(),
            spacious: "Spacious".into(),
            save_view: "Save this view".into(),
            view_name: "Name".into(),
            delete_view: "Delete view {label}".into(),
            make_default: "Open with this view".into(),
            default_view: "Default".into(),
        }
    }
}

/// `1234567` as `1,234,567`.
pub(crate) fn grouped(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (at, digit) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

type RowFn<'a, R, T> = Box<dyn Fn(&R) -> T + 'a>;

/// A table with search, filters, sort links, selection and pagination, as one region.
///
/// ```ignore
/// DataTable::new("deals", &query, &page).caption("Deals").sticky_header()
/// ```
#[must_use]
pub struct DataTable<'a, R, F = NoFilter> {
    id: &'static str,
    query: &'a TableQuery<R, F>,
    data: TableData<'a, R>,
    empty: Option<Empty>,
    caption: Option<Cow<'static, str>>,
    labels: Cow<'a, TableLabels>,
    mode: FilterMode,
    height: TableHeight,
    sticky_header: bool,
    bleed: bool,
    scroll_to: Option<&'static str>,
    keyboard_grid: bool,
    failed: Option<Cow<'static, str>>,
    loading: bool,
    bulk_url: Option<Href>,
    bulk_actions: Vec<BulkAction>,
    can_select: bool,
    select_all_matching: bool,
    csrf: Option<String>,
    selectable_if: Option<RowFn<'a, R, bool>>,
    row_actions: Option<RowFn<'a, R, Vec<RowAction>>>,
    row_href: Option<RowFn<'a, R, String>>,
    sticky_rows: Option<RowFn<'a, R, bool>>,
    detail: Option<RowFn<'a, R, Node>>,
    export: Option<Href>,
    options: Vec<(&'static str, Vec<FilterOption>)>,
    foot: Vec<(&'static str, Node)>,
    views: Option<(&'a [super::SavedView], Href)>,
    toolbar: Vec<Node>,
    can_reorder: bool,
    can_resize: bool,
}

impl<'a, R: TableRow, F: FilterBar + Default + DeserializeOwned> DataTable<'a, R, F> {
    /// The rows of this page; `id` names the region.
    pub fn new(id: &'static str, query: &'a TableQuery<R, F>, data: impl Into<TableData<'a, R>>) -> Self {
        Self {
            id,
            query,
            data: data.into(),
            empty: None,
            caption: None,
            labels: Cow::Owned(TableLabels::default()),
            mode: FilterMode::Popover,
            height: TableHeight::Auto,
            sticky_header: false,
            bleed: false,
            scroll_to: None,
            keyboard_grid: false,
            failed: None,
            loading: false,
            bulk_url: None,
            bulk_actions: Vec::new(),
            can_select: true,
            select_all_matching: true,
            csrf: None,
            selectable_if: None,
            row_actions: None,
            row_href: None,
            sticky_rows: None,
            detail: None,
            export: None,
            options: Vec::new(),
            foot: Vec::new(),
            views: None,
            toolbar: Vec::new(),
            can_reorder: true,
            can_resize: true,
        }
    }

    /// Sets how many rows there are across all pages.
    pub fn total(mut self, total: u64) -> Self {
        self.data.total = Total::Exact(total);
        self
    }

    /// Sets what shows when there are no rows and no filters.
    pub fn empty(mut self, empty: Empty) -> Self {
        self.empty = Some(empty);
        self
    }

    /// Names the table for assistive technology.
    pub fn caption(mut self, caption: impl Into<Cow<'static, str>>) -> Self {
        self.caption = Some(caption.into());
        self
    }

    /// Sets the words.
    pub fn labels(mut self, labels: &'a TableLabels) -> Self {
        self.labels = Cow::Borrowed(labels);
        self
    }

    /// Puts the filters in a row under the headers.
    pub fn filter_row(mut self) -> Self {
        self.mode = FilterMode::Row;
        self
    }

    /// Sets how tall the scrolling part is.
    pub fn height(mut self, height: TableHeight) -> Self {
        self.height = height;
        self
    }

    /// Keeps the header in view; with no height of its own, the table takes [`TableHeight::Lg`].
    pub fn sticky_header(mut self) -> Self {
        self.sticky_header = true;
        if self.height == TableHeight::Auto {
            self.height = TableHeight::Lg;
        }
        self
    }

    /// Runs the dividers out through a card's padding.
    pub fn bleed(mut self) -> Self {
        self.bleed = true;
        self
    }

    /// Scrolls to the element with this id after a page change.
    pub fn scroll_to(mut self, id: &'static str) -> Self {
        self.scroll_to = Some(id);
        self
    }

    /// Makes the body one tab stop with arrow keys, Home, End, Page Up and Page Down.
    pub fn keyboard_grid(mut self) -> Self {
        self.keyboard_grid = true;
        self
    }

    /// Shows that the rows couldn't load, keeping the toolbar and filters so the
    /// user can change the query.
    pub fn failed(mut self, message: impl Into<Cow<'static, str>>) -> Self {
        self.failed = Some(message.into());
        self
    }

    /// Shows skeleton rows: a first load still on its way.
    pub fn loading(mut self) -> Self {
        self.loading = true;
        self
    }

    /// Adds selection, with actions posting to `url`. See [`super::BulkRequest`].
    pub fn bulk(mut self, url: impl Into<Href>, actions: impl IntoIterator<Item = BulkAction>) -> Self {
        self.bulk_url = Some(url.into());
        self.bulk_actions = actions.into_iter().collect();
        self
    }

    /// Whether this user can select at all. With `false`, no boxes and no actions.
    pub fn can_select(mut self, can: bool) -> Self {
        self.can_select = can;
        self
    }

    /// Doesn't offer "select all results", where acting on a filter is unsafe.
    pub fn without_select_all_matching(mut self) -> Self {
        self.select_all_matching = false;
        self
    }

    /// Which rows can be ticked. Others show a disabled box. "Select all results"
    /// isn't offered, since no count of the selectable rows exists.
    pub fn selectable_if(mut self, can: impl Fn(&R) -> bool + 'a) -> Self {
        self.selectable_if = Some(Box::new(can));
        self.select_all_matching = false;
        self
    }

    /// The CSRF token, so the bulk form and a row's posts work with JavaScript off.
    pub fn csrf(mut self, token: impl Into<String>) -> Self {
        self.csrf = Some(token.into());
        self
    }

    /// Adds a menu to each row.
    pub fn row_actions(mut self, actions: impl Fn(&R) -> Vec<RowAction> + 'a) -> Self {
        self.row_actions = Some(Box::new(actions));
        self
    }

    /// Makes a click anywhere on a row, outside its controls, go here.
    pub fn row_href(mut self, href: impl Fn(&R) -> String + 'a) -> Self {
        self.row_href = Some(Box::new(href));
        self
    }

    /// Which rows stay in view while the table scrolls.
    pub fn sticky_rows(mut self, sticky: impl Fn(&R) -> bool + 'a) -> Self {
        self.sticky_rows = Some(Box::new(sticky));
        self
    }

    /// What a row opens to. Only open rows are drawn: `open=` is in the URL.
    pub fn detail(mut self, detail: impl Fn(&R) -> Node + 'a) -> Self {
        self.detail = Some(Box::new(detail));
        self
    }

    /// Adds a link to `url`, carrying the state. See `TableQuery::export_csv`.
    pub fn export(mut self, url: impl Into<Href>) -> Self {
        self.export = Some(url.into());
        self
    }

    /// Sets the choices of a filter whose choices come from the page.
    pub fn options(mut self, column: &'static str, options: Vec<FilterOption>) -> Self {
        self.options.push((column, options));
        self
    }

    /// Adds a footer cell under a column.
    pub fn foot(mut self, column: &'static str, cell: impl Into<Node>) -> Self {
        self.foot.push((column, cell.into()));
        self
    }

    /// Offers saved views, saved by posting to `url`. See [`super::SavedView`].
    pub fn views(mut self, views: &'a [super::SavedView], url: impl Into<Href>) -> Self {
        self.views = Some((views, url.into()));
        self
    }

    /// Adds controls to the toolbar.
    pub fn toolbar(mut self, controls: impl IntoNodes) -> Self {
        controls.push_into(&mut self.toolbar);
        self
    }

    /// Turns off dragging columns to a new order.
    pub fn fixed_order(mut self) -> Self {
        self.can_reorder = false;
        self
    }

    /// Turns off dragging columns to a new width.
    pub fn fixed_widths(mut self) -> Self {
        self.can_resize = false;
        self
    }

    fn form_id(&self) -> String {
        format!("{}-f", self.id)
    }

    fn bulk_id(&self) -> String {
        format!("{}-b", self.id)
    }

    fn selectable(&self) -> bool {
        self.can_select && self.bulk_url.is_some()
    }

    fn has_actions(&self) -> bool {
        self.row_actions.is_some()
    }

    fn has_detail(&self) -> bool {
        self.detail.is_some()
    }

    /// How many body columns there are, with the extras.
    fn span(&self) -> usize {
        self.query.state.cols.len() + usize::from(self.selectable()) + usize::from(self.has_actions()) + usize::from(self.has_detail())
    }

    /// The page shown.
    fn page(&self) -> u32 {
        self.data.page.unwrap_or_else(|| self.query.page())
    }

    /// This table's URL, with the page shown rather than the page asked for.
    fn canonical(&self) -> String {
        let page = self.page();
        self.query.href_with(|state| state.page = page)
    }

    fn region_link(&self, href: String) -> Element {
        el("a").href(href).flag("data-ui-in-region", true)
    }

    /// The filter's choices: from the definition, or from the page.
    fn choices(&self, column: &'static Column) -> Vec<FilterOption> {
        if !column.options.is_empty() {
            return column.options.iter().map(|(value, label)| FilterOption::new(*value, *label)).collect();
        }
        self.options.iter().find(|(name, _)| *name == column.name).map_or_else(Vec::new, |(_, options)| options.clone())
    }

    /// A popover from `trigger`, placed under it.
    fn popover(&self, id: String, trigger: Element, label: String, body: impl IntoNodes) -> Node {
        let trigger = trigger.attr("popovertarget", id.clone()).attr("aria-haspopup", "dialog").attr("type", "button");
        let popup = el("div")
            .slot("popover-content")
            .attr("id", id)
            .attr("popover", "auto")
            .attr("role", "dialog")
            .attr("tabindex", -1_i64)
            .attr("aria-label", label)
            .flag("data-ui-place", true)
            .attr("data-side", "bottom")
            .attr("data-align", "end")
            .uses(Uses::OVERLAY)
            .child(body);
        Node::list((trigger, popup))
    }

    /// A small input for a filter, tied to the toolbar's form.
    fn filter_input(&self, kind: &'static str, name: String, value: Option<&str>, placeholder: Option<String>, label: String) -> Element {
        el("input")
            .slot("input")
            .attr("data-size", "sm")
            .attr("type", kind)
            .attr("name", name)
            .attr("form", self.form_id())
            .attr_opt("value", value.map(str::to_owned))
            .attr_opt("placeholder", placeholder)
            .attr("aria-label", label)
    }

    /// The controls of column `at`'s filter.
    fn filter_controls(&self, at: usize, column: &'static Column, in_row: bool) -> Node {
        let labels = &self.labels;
        let active = self.query.state.filters.iter().find(|filter| filter.column == at).map(|filter| &filter.value);
        let name = format!("f.{}", column.name);
        let body: Node = match column.filter {
            FilterKind::None => Node::Empty,
            FilterKind::Text => {
                let value = match active {
                    Some(FilterValue::Text(text)) => Some(text.as_str()),
                    _ => None,
                };
                self.filter_input("search", name, value, Some(column.label.to_owned()), format!("{}: {}", labels.search, column.label)).into()
            }
            FilterKind::Range => {
                let (min, max) = match active {
                    Some(FilterValue::Range { min, max }) => (min.as_deref(), max.as_deref()),
                    _ => (None, None),
                };
                let kind = column.kind.input_type();
                let date = matches!(column.kind, ValueKind::Date | ValueKind::DateTime);
                let (low, high) = if date { (&labels.from, &labels.to) } else { (&labels.min, &labels.max) };
                let step = (kind == "number" && column.kind != ValueKind::Int).then_some("any");
                let inputmode = (kind == "text").then_some("decimal");
                let input = |suffix: &str, value, word: &Cow<'static, str>| {
                    el("label").slot("filter-field").children((
                        el("span").child(word.to_string()),
                        self.filter_input(kind, format!("{name}.{suffix}"), value, None, format!("{} {word}", column.label)).attr_opt("step", step).attr_opt("inputmode", inputmode),
                    ))
                };
                el("div").slot("filter-range").children((input("min", min, low), input("max", max, high))).into()
            }
            FilterKind::Select | FilterKind::Boolean => {
                let choices: Vec<FilterOption> = if column.filter == FilterKind::Boolean {
                    vec![FilterOption::new("true", labels.yes.to_string()), FilterOption::new("false", labels.no.to_string())]
                } else {
                    self.choices(column)
                };
                let selected = match active {
                    Some(FilterValue::Values(values)) => values.first().cloned(),
                    Some(FilterValue::Bool(flag)) => Some(flag.to_string()),
                    _ => None,
                };
                let mut select = NativeSelect::new(name).prompt(labels.any.to_string()).attr("form", self.form_id()).attr("aria-label", column.label);
                for choice in &choices {
                    select = select.option(choice.value.clone(), choice.label.clone());
                }
                if let Some(selected) = selected {
                    select = select.selected(selected);
                }
                select.into()
            }
            FilterKind::Multi | FilterKind::Relation => {
                let choices = self.choices(column);
                let selected: &[String] = match active {
                    Some(FilterValue::Values(values)) => values,
                    _ => &[],
                };
                if choices.is_empty() {
                    // No list of choices: the key itself.
                    let value = selected.first().map(String::as_str);
                    self.filter_input("text", name, value, None, column.label.to_owned()).into()
                } else {
                    let find = (choices.len() > 8).then(|| el("input").slot("input").attr("data-size", "sm").attr("type", "search").attr("placeholder", labels.find_option.to_string()).attr("aria-label", labels.find_option.to_string()).flag("data-ui-find-option", true));
                    let items: Vec<Element> = choices
                        .iter()
                        .map(|choice| {
                            el("label").slot("filter-option").children((
                                el("input")
                                    .slot("checkbox")
                                    .attr("type", "checkbox")
                                    .attr("name", name.clone())
                                    .attr("value", choice.value.clone())
                                    .attr("form", self.form_id())
                                    .flag("checked", selected.iter().any(|value| *value == choice.value)),
                                el("bdi").child(choice.label.clone()),
                                choice.count.map(|count| el("span").slot("filter-count").child(grouped(count))),
                            ))
                        })
                        .collect();
                    Node::list((find, el("div").slot("filter-options").child(items)))
                }
            }
        };
        let empty = (column.nullable && !in_row && column.filter != FilterKind::None).then(|| {
            el("label").slot("filter-option").children((
                el("input").slot("checkbox").attr("type", "checkbox").attr("name", format!("f.{}.empty", column.name)).attr("value", "1").attr("form", self.form_id()).flag("checked", matches!(active, Some(FilterValue::Empty))),
                el("span").child(labels.empty_value.to_string()),
            ))
        });
        Node::list((body, empty))
    }

    /// The label of a filter in force.
    fn chip_text(&self, column: &'static Column, value: &FilterValue) -> String {
        let labels = &self.labels;
        let name_of = |value: &str| -> String {
            if let Some((_, label)) = column.options.iter().find(|(option, _)| *option == value) {
                return (*label).to_owned();
            }
            self.options.iter().find(|(name, _)| *name == column.name).and_then(|(_, options)| options.iter().find(|option| option.value == value)).map_or_else(|| value.to_owned(), |option| option.label.clone())
        };
        match value {
            FilterValue::Text(text) => format!("{}: {text}", column.label),
            FilterValue::Values(values) => {
                let mut names = values.iter().take(3).map(|value| name_of(value)).collect::<Vec<_>>().join(", ");
                if values.len() > 3 {
                    let _ = write!(names, " +{}", values.len() - 3);
                }
                format!("{}: {names}", column.label)
            }
            FilterValue::Bool(flag) => format!("{}: {}", column.label, if *flag { &labels.yes } else { &labels.no }),
            FilterValue::Empty => format!("{}: {}", column.label, labels.empty_value),
            FilterValue::Range { min, max } => match (min, max) {
                (Some(min), Some(max)) => format!("{}: {min} – {max}", column.label),
                (Some(min), None) => format!("{}: ≥ {min}", column.label),
                (None, Some(max)) => format!("{}: ≤ {max}", column.label),
                (None, None) => column.label.to_owned(),
            },
        }
    }

    /// The URL with `at`'s place in the sort set to `direction`, alone or added.
    fn sort_href(&self, at: usize, direction: Direction, add: bool) -> String {
        self.query.href_with(|state| {
            state.page = 1;
            state.after = None;
            state.before = None;
            if add {
                if let Some(term) = state.sort.iter_mut().find(|term| term.column == at) {
                    term.direction = direction;
                } else {
                    state.sort.push(super::SortTerm { column: at, direction });
                }
            } else {
                state.sort = vec![super::SortTerm { column: at, direction }];
            }
        })
    }

    /// The header menu of a column: its sort, its filter, hiding it.
    fn column_menu(&self, at: usize, column: &'static Column) -> Option<Node> {
        let labels = &self.labels;
        let state = &self.query.state;
        let current = state.sort.iter().find(|term| term.column == at).map(|term| term.direction);
        let can_hide = !column.locked && state.cols.len() > 1;
        let filter_here = column.filter != FilterKind::None && self.mode == FilterMode::Popover;
        if !column.sortable && !filter_here && !can_hide {
            return None;
        }
        let item = |text: &Cow<'static, str>, glyph: Icon, href: String, on: bool| -> Element {
            self.region_link(href).slot("menu-link").flag("data-active", on).children((icon(glyph), text.to_string()))
        };
        let mut links: Vec<Element> = Vec::new();
        if column.sortable {
            links.push(item(&labels.sort_asc, icons::ARROW_UP, self.sort_href(at, Direction::Asc, false), current == Some(Direction::Asc)));
            links.push(item(&labels.sort_desc, icons::ARROW_DOWN, self.sort_href(at, Direction::Desc, false), current == Some(Direction::Desc)));
            if !state.sort.is_empty() && !(state.sort.len() == 1 && current.is_some()) && current.is_none() {
                links.push(item(&labels.sort_add, icons::PLUS, self.sort_href(at, Direction::Asc, true), false));
            }
            if current.is_some() {
                let clear = self.query.href_with(|state| {
                    state.sort.retain(|term| term.column != at);
                    state.page = 1;
                    state.after = None;
                    state.before = None;
                });
                links.push(item(&labels.sort_clear, icons::X, clear, false));
            }
        }
        if can_hide {
            let hide = self.query.href_with(|state| state.cols.retain(|column| *column != at));
            links.push(item(&labels.hide_column, icons::EYE_OFF, hide, false));
        }
        let filter = filter_here.then(|| el("div").slot("menu-filter").child(self.filter_controls(at, column, false)));
        let active = state.filters.iter().any(|filter| filter.column == at);
        let glyph = if filter_here { icons::LIST_FILTER } else { icons::ELLIPSIS };
        let trigger = el("button")
            .slot("button")
            .attr("data-variant", "ghost")
            .attr("data-size", "xs")
            .flag("data-icon-only", true)
            .flag("data-th-menu", true)
            .flag("data-active", active)
            .attr("aria-label", labels.column_options.replace("{label}", column.label))
            .child(icon(glyph));
        Some(self.popover(format!("{}-m-{}", self.id, column.name), trigger, labels.column_options.replace("{label}", column.label), (el("div").slot("menu-links").child(links), filter)))
    }

    /// A `<th>` for column `at`.
    fn head_cell(&self, at: usize, column: &'static Column, sticky_at: Option<&str>) -> Element {
        let labels = &self.labels;
        let state = &self.query.state;
        let rank = state.sort.iter().position(|term| term.column == at);
        let sorted = rank.map(|rank| state.sort[rank].direction);
        let mut th = el("th")
            .attr("scope", "col")
            .attr("data-col", column.name)
            .flag("data-numeric", column.numeric)
            .flag("data-hide-mobile", column.hide_on_mobile)
            .attr_opt("data-align", align_str(column.align))
            .attr_opt("data-sticky", sticky_at.map(str::to_owned));
        if let Some(direction) = sorted {
            th = th.attr("aria-sort", if direction == Direction::Asc { "ascending" } else { "descending" });
        }
        let title: Node = if column.sortable {
            // The first click sorts up, the next down. Shift adds to the sort.
            let next = if sorted == Some(Direction::Asc) { Direction::Desc } else { Direction::Asc };
            let glyph = match sorted {
                Some(Direction::Asc) => icons::ARROW_UP,
                Some(Direction::Desc) => icons::ARROW_DOWN,
                None => icons::ARROW_UP_DOWN,
            };
            let order = (state.sort.len() > 1).then(|| rank.map(|rank| el("span").slot("sort-rank").child((rank + 1) as i64))).flatten();
            self.region_link(self.sort_href(at, next, false))
                .slot("sort-link")
                .attr("data-ui-add-sort", self.sort_href(at, next, true))
                .children((column.label, icon(glyph), order))
                .into()
        } else {
            el("span").slot("th-label").child(column.label).into()
        };
        let drag = self.can_reorder && state.cols.len() > 1;
        let box_ = el("div").slot("th").flag("data-ui-col-drag", drag).children((title, self.column_menu(at, column)));
        let handle = self.can_resize.then(|| {
            el("span")
                .slot("col-resize")
                .attr("role", "separator")
                .attr("aria-orientation", "vertical")
                .attr("tabindex", 0_i64)
                .attr("aria-label", labels.resize.replace("{label}", column.label))
                .attr("aria-valuemin", 40_i64)
                .attr("aria-valuemax", 1600_i64)
                .attr("aria-valuenow", i64::from(state.widths.iter().find(|(column, _)| *column == at).map(|(_, width)| *width).or(column.width).unwrap_or(160)))
        });
        th.children((box_, handle))
    }

    /// The header rows: group headings, column headings, the filter row.
    fn head(&self, sticky: &[Option<String>]) -> Element {
        let state = &self.query.state;
        let columns: Vec<(usize, &'static Column)> = state.cols.iter().map(|at| (*at, TableQuery::<R, F>::column(*at))).collect();
        let has_groups = columns.iter().any(|(_, column)| column.group.is_some());
        let select_head = self.selectable().then(|| {
            el("th").attr("scope", "col").slot("cell-select").attr("data-sticky", "s").child(
                el("input").slot("checkbox").attr("type", "checkbox").flag("data-ui-select-page", true).attr("aria-label", self.labels.select_page.to_string()),
            )
        });
        let detail_head = self.has_detail().then(|| el("td").slot("cell-toggle"));
        let actions_head = self.has_actions().then(|| el("th").attr("scope", "col").slot("cell-actions").attr("data-sticky", "e").child(el("span").attr("data-ui-sr", "").child(self.labels.actions.to_string())));
        let cells: Vec<Element> = columns.iter().zip(sticky).map(|((at, column), sticky)| self.head_cell(*at, column, sticky.as_deref())).collect();
        let mut rows: Vec<Element> = Vec::with_capacity(3);
        if has_groups {
            let mut groups: Vec<Element> = Vec::new();
            if self.selectable() {
                groups.push(el("td").slot("cell-select").attr("data-sticky", "s"));
            }
            if self.has_detail() {
                groups.push(el("td").slot("cell-toggle"));
            }
            let mut at = 0;
            while at < columns.len() {
                let group = columns[at].1.group;
                let mut span = 1;
                while at + span < columns.len() && columns[at + span].1.group == group {
                    span += 1;
                }
                let cell = match group {
                    Some(group) => el("th").attr("scope", "colgroup").slot("th-group").attr("colspan", span as i64).child(group),
                    None => el("td").attr("colspan", span as i64),
                };
                groups.push(cell);
                at += span;
            }
            if self.has_actions() {
                groups.push(el("td").slot("cell-actions").attr("data-sticky", "e"));
            }
            rows.push(el("tr").slot("group-row").child(groups));
        }
        rows.push(el("tr").children((select_head, detail_head, cells, actions_head)));
        if self.mode == FilterMode::Row && columns.iter().any(|(_, column)| column.filter != FilterKind::None) {
            let filters: Vec<Element> = columns
                .iter()
                .zip(sticky)
                .map(|((at, column), sticky)| {
                    let cell = el("td").attr_opt("data-sticky", sticky.clone()).flag("data-hide-mobile", column.hide_on_mobile);
                    match column.filter {
                        FilterKind::None => cell,
                        FilterKind::Text | FilterKind::Select | FilterKind::Boolean => cell.child(self.filter_controls(*at, column, true)),
                        _ => {
                            let active = state.filters.iter().find(|filter| filter.column == *at);
                            let trigger = el("button").slot("button").attr("data-variant", "outline").attr("data-size", "sm").flag("data-filter-trigger", true).flag("data-active", active.is_some()).children((
                                active.map_or_else(|| self.labels.any.to_string(), |filter| self.chip_text(column, &filter.value).split_once(": ").map_or(String::new(), |(_, value)| value.to_owned())),
                                icon(icons::CHEVRON_DOWN),
                            ));
                            cell.child(self.popover(format!("{}-p-{}", self.id, column.name), trigger, column.label.to_owned(), self.filter_controls(*at, column, false)))
                        }
                    }
                })
                .collect();
            let select = self.selectable().then(|| el("td").slot("cell-select").attr("data-sticky", "s"));
            let toggle = self.has_detail().then(|| el("td").slot("cell-toggle"));
            let actions = self.has_actions().then(|| el("td").slot("cell-actions").attr("data-sticky", "e"));
            rows.push(el("tr").slot("filter-row").children((select, toggle, filters, actions)));
        }
        el("thead").child(rows)
    }

    /// Hidden inputs for all the state the form's controls don't hold.
    fn state_inputs(&self) -> Vec<Element> {
        let state = &self.query.state;
        let mut inputs = Vec::new();
        let hidden = |name: String, value: String| el("input").attr("type", "hidden").attr("name", name).attr("value", value);
        let mut pairs = Vec::new();
        // Filters on columns not shown have no control, but stay in force.
        for filter in state.filters.iter().filter(|filter| !state.cols.contains(&filter.column) || (self.mode == FilterMode::Row && TableQuery::<R, F>::column(filter.column).filter == FilterKind::None)) {
            let one = State { filters: vec![filter.clone()], q: None, ..state.clone() };
            one.filter_pairs::<R>(&mut pairs);
        }
        for (name, value) in pairs {
            inputs.push(hidden(name, value));
        }
        for (name, value) in state.pairs::<R>() {
            if matches!(name.as_str(), "sort" | "per" | "cols" | "w" | "density") && !(name == "per") {
                inputs.push(hidden(name, value));
            }
        }
        inputs
    }

    fn toolbar_node(&self) -> Element {
        let labels = &self.labels;
        let state = &self.query.state;
        let form = self.form_id();
        let searchable = R::COLUMNS.iter().any(|column| column.search);
        let search = searchable.then(|| {
            let input = Input::search("q").placeholder(labels.search.to_string()).attr("aria-label", labels.search.to_string());
            let input = match &state.q {
                Some(q) => input.value(q.clone()),
                None => input,
            };
            el("div").slot("table-search").children((icon(icons::SEARCH), input))
        });
        let controls = self.query.filter().controls();
        let tool = |glyph: Icon, text: String| {
            el("button").slot("button").attr("data-variant", "outline").attr("data-size", "sm").children((icon(glyph), text))
        };
        // Columns: ticks, and links that move each one.
        let shown = &state.cols;
        let order: Vec<usize> = shown.iter().copied().chain((0..R::COLUMNS.len()).filter(|at| !shown.contains(at))).collect();
        let column_rows: Vec<Element> = order
            .iter()
            .map(|at| {
                let column = TableQuery::<R, F>::column(*at);
                let place = shown.iter().position(|shown| shown == at);
                let move_link = |to: Option<usize>, glyph: Icon, label: String| -> Element {
                    let link = el("a").slot("column-move").attr("aria-label", label).child(icon(glyph));
                    match (place, to) {
                        (Some(from), Some(to)) => link.href(self.query.href_with(|state| state.cols.swap(from, to))).flag("data-ui-in-region", true),
                        _ => link.attr("role", "link").attr("aria-disabled", "true"),
                    }
                };
                el("div").slot("column-row").attr("data-col", column.name).children((
                    el("label").slot("filter-option").children((
                        el("input").slot("checkbox").attr("type", "checkbox").attr("name", "cols").attr("value", column.name).attr("form", form.clone()).flag("checked", place.is_some()).flag("disabled", column.locked),
                        column.label,
                    )),
                    column.locked.then(|| el("input").attr("type", "hidden").attr("name", "cols").attr("value", column.name).attr("form", form.clone())),
                    place.is_some().then(|| {
                        Node::list((
                            move_link(place.and_then(|from| from.checked_sub(1)), icons::CHEVRON_UP, labels.move_up.replace("{label}", column.label)),
                            move_link(place.and_then(|from| (from + 1 < shown.len()).then_some(from + 1)), icons::CHEVRON_DOWN, labels.move_down.replace("{label}", column.label)),
                        ))
                    }),
                ))
            })
            .collect();
        let reset = (state.pairs::<R>().iter().any(|(name, _)| name == "cols" || name == "w")).then(|| {
            self.region_link(self.query.href_with(|state| {
                state.cols = R::COLUMNS.iter().enumerate().filter(|(_, column)| !column.hidden).map(|(at, _)| at).collect();
                state.widths.clear();
            }))
            .slot("menu-link")
            .children((icon(icons::ROTATE_CCW), labels.reset_columns.to_string()))
        });
        let columns = self.popover(format!("{}-columns", self.id), tool(icons::COLUMNS_3, labels.columns.to_string()), labels.columns.to_string(), (el("div").slot("column-list").child(column_rows), reset));
        // Density.
        let density_link = |density: super::TableDensity, text: &Cow<'static, str>| -> Element {
            self.region_link(self.query.href_with(|state| state.density = density)).slot("menu-link").flag("data-active", state.density == density).attr_opt("aria-current", (state.density == density).then_some("true")).child(text.to_string())
        };
        let view = self.popover(
            format!("{}-view", self.id),
            el("button").slot("button").attr("data-variant", "outline").attr("data-size", "sm").flag("data-icon-only", true).attr("aria-label", labels.density.to_string()).child(icon(icons::SLIDERS_HORIZONTAL)),
            labels.density.to_string(),
            el("div").slot("menu-links").children((
                density_link(TableDensity::Compact, &labels.compact),
                density_link(TableDensity::Default, &labels.default_density),
                density_link(TableDensity::Spacious, &labels.spacious),
            )),
        );
        let views = self.views.as_ref().map(|(views, url)| super::saved::menu(self.id, &self.query.query_string(), views, url, self.query.path(), labels, self.csrf.as_deref(), |id, trigger, label, body| self.popover(id, trigger, label, body)));
        let export = self.export.as_ref().map(|url| {
            let query = self.query.href_with(|state| {
                state.page = 1;
                state.open.clear();
            });
            let query = query.split_once('?').map_or("", |(_, query)| query);
            let href = if query.is_empty() { url.as_str().to_owned() } else { format!("{}?{query}", url.as_str()) };
            el("a").slot("button").attr("data-variant", "outline").attr("data-size", "sm").href(href).flag("data-ui-noboost", true).flag("download", true).children((icon(icons::DOWNLOAD), labels.export.to_string()))
        });
        // The saved-view forms sit outside the toolbar's form, since forms can't nest.
        // Every control reaches the form by its `form` attribute.
        el("div").slot("table-toolbar").children((
            el("form")
                .slot("toolbar-form")
                .attr("id", form)
                .attr("role", "search")
                .attr("method", "get")
                .attr("action", self.query.path.clone())
                .attr("data-ui-target", self.id)
                .children((
                    search,
                    controls,
                    self.toolbar.clone(),
                    self.state_inputs(),
                    // With JavaScript off, a select or a tick needs a button.
                    el("noscript").child(Button::outline(labels.apply.to_string()).size(Size::Sm).submit()),
                )),
            el("div").slot("toolbar-end").children((views, columns, view, export)),
        ))
    }

    /// Active filters and sort terms, each with a way off, and Clear filters.
    fn chips(&self) -> Option<Element> {
        let labels = &self.labels;
        let state = &self.query.state;
        // A sort the user asked for, of more than one column, needs a way off; the
        // table's own default doesn't.
        let many_sorts = state.sort.len() > 1 && !state.is_default_sort::<R>();
        if !self.query.is_filtered() && !many_sorts {
            return None;
        }
        let mut chips: Vec<Element> = Vec::new();
        if let Some(q) = &state.q {
            let text = format!("{}: {q}", labels.search);
            let href = self.query.href_with(|state| {
                state.q = None;
                state.page = 1;
                state.after = None;
                state.before = None;
            });
            chips.push(self.chip(text.clone(), href, labels.remove_filter.replace("{label}", &text)));
        }
        for filter in &state.filters {
            let column = TableQuery::<R, F>::column(filter.column);
            let text = self.chip_text(column, &filter.value);
            let href = self.query.href_with(|state| {
                state.filters.retain(|other| other.column != filter.column);
                state.page = 1;
                state.after = None;
                state.before = None;
            });
            chips.push(self.chip(text.clone(), href, labels.remove_filter.replace("{label}", &text)));
        }
        if many_sorts {
            for term in &state.sort {
                let column = TableQuery::<R, F>::column(term.column);
                let arrow = if term.direction == Direction::Asc { "↑" } else { "↓" };
                let href = self.query.href_with(|state| {
                    state.sort.retain(|other| other.column != term.column);
                    state.page = 1;
                    state.after = None;
                    state.before = None;
                });
                let text = format!("{}: {} {arrow}", labels.sorted_by, column.label);
                chips.push(self.chip(text.clone(), href, labels.remove_sort.replace("{label}", &text)));
            }
        }
        let clear = (!state.filters.is_empty() || state.q.is_some()).then(|| self.region_link(self.clear_href()).slot("chips-clear").child(labels.clear_filters.to_string()));
        Some(el("div").slot("table-chips").children((chips, clear)))
    }

    fn chip(&self, text: String, href: String, label: String) -> Element {
        self.region_link(href).slot("filter-chip").attr("aria-label", label).children((el("bdi").child(text), icon(icons::X)))
    }

    /// The URL with no search and no filter.
    fn clear_href(&self) -> String {
        self.query.href_with(|state| {
            state.q = None;
            state.filters.clear();
            state.page = 1;
            state.after = None;
            state.before = None;
        })
    }

    /// The bar over the table: count, "select all", actions.
    fn bulk_bar(&self) -> Option<Node> {
        if !self.selectable() {
            return None;
        }
        let labels = &self.labels;
        let url = self.bulk_url.clone()?;
        let actions: Vec<Node> = self
            .bulk_actions
            .iter()
            .map(|action| {
                let mut button = Button::new(action.label.to_string())
                    .variant(if action.destructive { ButtonVariant::Destructive } else { ButtonVariant::Outline })
                    .size(Size::Sm)
                    .named("action", action.id)
                    .disabled(action.denied.is_some())
                    .submit();
                if let Some(glyph) = action.icon {
                    button = button.icon(glyph);
                }
                if let Some(question) = &action.confirm {
                    button = button.attr("data-ui-confirm", question.to_string());
                }
                if let Some(reason) = &action.denied {
                    button = button.attr("title", reason.to_string());
                }
                button.into()
            })
            .collect();
        let hidden = |name: &'static str, value: String| el("input").attr("type", "hidden").attr("name", name).attr("value", value);
        let form = el("form")
            .slot("table-bulk")
            .attr("id", self.bulk_id())
            .attr("method", "post")
            .attr("action", url)
            .flag("data-ui-form", true)
            .flag("data-ui-bulk", true)
            .attr("aria-label", labels.actions.to_string())
            .children((
                hidden("_table", self.id.to_owned()),
                hidden("_path", self.query.path.clone()),
                hidden("_q", self.query.query_string()),
                self.csrf.as_ref().map(|token| crate::form::csrf_field(token.clone())),
                el("div").slot("bulk-status").children((
                    el("span").slot("bulk-count").attr("aria-live", "polite"),
                    el("button").slot("bulk-link").attr("type", "button").flag("data-ui-select-all", true).flag("hidden", true),
                    el("button").slot("bulk-link").attr("type", "button").flag("data-ui-select-clear", true).child(labels.clear_selection.to_string()),
                )),
                el("div").slot("bulk-actions").child(actions),
            ));
        let dialog = self.bulk_actions.iter().any(|action| action.confirm.is_some()).then(|| {
            AlertDialog::new(format!("{}-confirm", self.id), labels.confirm_title.to_string(), Node::Empty)
                .cancel(labels.cancel.to_string())
                .action(Button::destructive(labels.confirm.to_string()).attr("data-ui-confirm-yes", ""))
        });
        Some(Node::list((form, dialog)))
    }

    fn cell_open(column: &'static Column, sticky: Option<&str>) -> String {
        let mut open = String::from("<td");
        if column.numeric {
            open.push_str(" data-numeric");
        }
        if column.hide_on_mobile {
            open.push_str(" data-hide-mobile");
        }
        if let Some(align) = align_str(column.align) {
            let _ = write!(open, " data-align=\"{align}\"");
        }
        if column.strong {
            open.push_str(" data-strong");
        }
        if column.wrap {
            open.push_str(" data-wrap");
        }
        if column.truncate {
            open.push_str(" data-truncate");
        }
        if let Some(sticky) = sticky {
            let _ = write!(open, " data-sticky=\"{sticky}\"");
        }
        open
    }

    fn toggle_cell(&self, key: &str, open: bool, out: &mut String) {
        let labels = &self.labels;
        let href = self.query.href_with(|state| {
            if open {
                state.open.retain(|other| other != key);
            } else if state.open.len() < super::MAX_OPEN_ROWS {
                state.open.push(key.to_owned());
            }
        });
        out.push_str("<td data-slot=\"cell-toggle\"><a data-slot=\"row-toggle\" data-ui-in-region href=\"");
        escape_into(out, Href::from(href).as_str());
        let _ = write!(out, "\" aria-expanded=\"{open}\" aria-label=\"");
        escape_into(out, if open { &labels.hide_details } else { &labels.show_details });
        out.push_str("\">");
        out.push_str(&crate::render::render(&icon(if open { icons::CHEVRON_DOWN } else { icons::CHEVRON_RIGHT }).into()));
        out.push_str("</a></td>");
    }

    fn actions_cell(&self, key: &str, row: &R, out: &mut String) {
        let Some(actions) = &self.row_actions else { return };
        let items: Vec<Node> = actions(row)
            .into_iter()
            .map(|action| {
                let content = (action.icon.map(icon), action.label.to_string());
                if action.post {
                    el("form")
                        .attr("method", "post")
                        .attr("action", action.href)
                        .flag("data-ui-form", true)
                        .children((
                            self.csrf.as_ref().map(|token| crate::form::csrf_field(token.clone())),
                            el("button").slot("menu-link").attr("type", "submit").flag("data-destructive", action.destructive).children(content),
                        ))
                        .into()
                } else {
                    el("a").slot("menu-link").href(action.href).flag("data-destructive", action.destructive).children(content).into()
                }
            })
            .collect();
        let trigger = el("button").slot("button").attr("data-variant", "ghost").attr("data-size", "sm").flag("data-icon-only", true).attr("aria-label", self.labels.actions.to_string()).child(icon(icons::ELLIPSIS));
        let menu = self.popover(format!("{}-a-{key}", self.id), trigger, self.labels.actions.to_string(), el("div").slot("menu-links").child(items));
        out.push_str("<td data-slot=\"cell-actions\" data-sticky=\"e\">");
        crate::render::render_into(out, &menu);
        out.push_str("</td>");
    }

    /// Rows are the bulk of a page, so they skip the tree: each writes straight to
    /// one buffer, sized from the first, through the same escaping as everything else.
    fn body_rows(&self, sticky: &[Option<String>]) -> String {
        let state = &self.query.state;
        let opens: Vec<(usize, String)> = state.cols.iter().zip(sticky).map(|(at, sticky)| (*at, Self::cell_open(TableQuery::<R, F>::column(*at), sticky.as_deref()))).collect();
        let selectable = self.selectable();
        let form = self.bulk_id();
        let span = self.span();
        let mut out = String::new();
        let mut label = String::new();
        for (index, row) in self.data.rows.iter().enumerate() {
            let key = row.key();
            out.push_str("<tr");
            if let Some(key) = &key {
                out.push_str(" data-key=\"");
                escape_into(&mut out, key);
                out.push('"');
            }
            if let Some(href) = &self.row_href {
                out.push_str(" data-ui-row-href=\"");
                escape_into(&mut out, Href::from(href(row)).as_str());
                out.push('"');
            }
            if self.sticky_rows.as_ref().is_some_and(|sticky| sticky(row)) {
                out.push_str(" data-sticky-row");
            }
            out.push('>');
            if selectable {
                out.push_str("<td data-slot=\"cell-select\" data-sticky=\"s\"><input data-slot=\"checkbox\" type=\"checkbox\" data-ui-select-row name=\"ids\" value=\"");
                escape_into(&mut out, key.as_deref().unwrap_or_default());
                out.push_str("\" form=\"");
                out.push_str(&form);
                out.push_str("\" aria-label=\"");
                label.clear();
                if let Some((first, _)) = opens.first() {
                    row.write_text(*first, &mut label);
                }
                let mut name = String::with_capacity(label.len() + 8);
                name.push_str(&self.labels.select_row.replace("{label}", &label));
                escape_into(&mut out, &name);
                out.push('"');
                if self.selectable_if.as_ref().is_some_and(|can| !can(row)) {
                    out.push_str(" disabled");
                }
                out.push_str("></td>");
            }
            let is_open = key.as_ref().is_some_and(|key| state.open.iter().any(|open| open == key));
            if self.has_detail() {
                self.toggle_cell(key.as_deref().unwrap_or_default(), is_open, &mut out);
            }
            for (at, open) in &opens {
                out.push_str(open);
                if TableQuery::<R, F>::column(*at).truncate {
                    out.push_str(" title=\"");
                    label.clear();
                    row.write_text(*at, &mut label);
                    escape_into(&mut out, &label);
                    out.push('"');
                }
                out.push('>');
                row.write_cell(*at, &mut CellWriter::new(&mut out));
                out.push_str("</td>");
            }
            if self.has_actions() {
                self.actions_cell(key.as_deref().unwrap_or_default(), row, &mut out);
            }
            out.push_str("</tr>");
            if is_open && let Some(detail) = &self.detail {
                out.push_str("<tr data-slot=\"row-detail\"");
                if let Some(key) = &key {
                    out.push_str(" data-key=\"");
                    escape_into(&mut out, key);
                    out.push_str("-detail\"");
                }
                let _ = write!(out, "><td colspan=\"{span}\">");
                crate::render::render_into(&mut out, &detail(row));
                out.push_str("</td></tr>");
            }
            if index == 0 {
                out.reserve((out.len() + 16) * self.data.rows.len());
            }
        }
        out
    }

    /// A body with one row spanning every column: empty, failed or loading.
    fn message_body(&self) -> Element {
        let labels = &self.labels;
        let span = self.span() as i64;
        if self.loading {
            let cells = self.span();
            let rows: Vec<Element> = (0..6).map(|_| el("tr").child((0..cells).map(|_| el("td").child(Skeleton::new(SkeletonShape::Line))).collect::<Vec<_>>())).collect();
            return el("tbody").child(rows);
        }
        let content: Node = if let Some(message) = &self.failed {
            Empty::new(message.to_string())
                .icon(icons::CIRCLE_ALERT)
                .actions(el("a").slot("button").attr("data-variant", "outline").attr("data-size", "sm").href(self.query.href()).flag("data-ui-in-region", true).child(labels.retry.to_string()))
                .into()
        } else if self.query.is_filtered() {
            Empty::new(labels.empty_filtered.to_string())
                .icon(icons::SEARCH)
                .description(labels.empty_filtered_hint.to_string())
                .actions(el("a").slot("button").attr("data-variant", "outline").attr("data-size", "sm").href(self.clear_href()).flag("data-ui-in-region", true).child(labels.clear_filters.to_string()))
                .into()
        } else {
            self.empty.clone().unwrap_or_else(|| Empty::new(labels.empty.to_string()).icon(icons::INBOX)).into()
        };
        el("tbody").attr("data-message", "").child(el("tr").child(el("td").attr("colspan", span).child(content)))
    }

    fn colgroup(&self) -> Option<Element> {
        let state = &self.query.state;
        let any = self.can_resize || !state.widths.is_empty() || state.cols.iter().any(|at| TableQuery::<R, F>::column(*at).width.is_some());
        if !any {
            return None;
        }
        // `<col width>` sets a width with no style attribute, which the page's
        // policy forbids; `data-fixed` makes the table honour it.
        let mut cols: Vec<Element> = Vec::with_capacity(self.span());
        if self.selectable() {
            cols.push(el("col").slot("col-select"));
        }
        if self.has_detail() {
            cols.push(el("col").slot("col-toggle"));
        }
        for at in &state.cols {
            let width = state.widths.iter().find(|(column, _)| column == at).map(|(_, width)| *width).or(TableQuery::<R, F>::column(*at).width);
            cols.push(el("col").attr("data-col", TableQuery::<R, F>::column(*at).name).attr_opt("width", width.map(i64::from)));
        }
        if self.has_actions() {
            cols.push(el("col").slot("col-actions"));
        }
        Some(el("colgroup").child(cols))
    }

    fn summary(&self, shown: usize) -> Element {
        let labels = &self.labels;
        let state = &self.query.state;
        let per = u64::from(state.per);
        let from = if R::CONFIG.cursor { 0 } else { u64::from(self.page() - 1) * per + 1 };
        let to = from + (shown as u64).saturating_sub(1);
        let base = |template: &str, total: String| template.replace("{from}", &grouped(from)).replace("{to}", &grouped(to)).replace("{total}", &total);
        let count_link = |text: &str| -> Element { self.region_link(self.query.href_with(|state| state.count = true)).slot("count-link").child(text.to_owned()) };
        let text: Node = match (self.data.total, R::CONFIG.cursor) {
            (Total::Exact(1), _) => labels.one_result.to_string().into(),
            (Total::Exact(total), false) => base(&labels.showing, grouped(total)).into(),
            (Total::Exact(total), true) => format!("{} {}", grouped(total), if total == 1 { "result" } else { "results" }).into(),
            (Total::AtLeast(total), true) => Node::list((format!("{}+ results", grouped(total)), " ", count_link(&labels.count_all))),
            (Total::AtLeast(total), false) => Node::list((base(&labels.showing, format!("{}+", grouped(total))), " ", count_link(&labels.count_all))),
            (Total::Unknown, true) => count_link(&labels.count_all).into(),
            (Total::Unknown, false) => Node::list((base(&labels.showing_open, String::new()), " ", count_link(&labels.count_all))),
        };
        el("span").slot("table-summary").attr("aria-live", "polite").child(text)
    }

    fn page_size(&self) -> Option<Element> {
        let config = R::CONFIG;
        let labels = &self.labels;
        let state = &self.query.state;
        if config.per_pages.len() < 2 {
            return None;
        }
        let mut select = NativeSelect::new("per").attr("form", self.form_id()).attr("data-size", "sm").attr("aria-label", labels.rows_per_page.to_string());
        for per in config.per_pages {
            select = select.option(per.to_string(), per.to_string());
        }
        select = select.selected(state.per.to_string());
        Some(el("label").slot("page-size").children((el("span").child(labels.rows_per_page.to_string()), select)))
    }

    fn nav_link(&self, text: &str, glyph: Option<Icon>, href: Option<String>, end: bool) -> Element {
        let link = el("a").slot("pagination-text");
        let link = match href {
            Some(href) => link.href(href).flag("data-ui-in-region", true),
            None => link.attr("role", "link").attr("aria-disabled", "true"),
        };
        if end { link.children((text.to_owned(), glyph.map(icon))) } else { link.children((glyph.map(icon), text.to_owned())) }
    }

    fn pagination(&self) -> Option<Node> {
        let labels = &self.labels;
        let page = self.page();
        if R::CONFIG.cursor {
            if self.data.next.is_none() && self.data.prev.is_none() {
                return None;
            }
            let goto = |after: Option<&str>, before: Option<&str>| {
                self.query.href_with(|state| {
                    state.after = after.map(str::to_owned);
                    state.before = before.map(str::to_owned);
                })
            };
            let first = self.data.prev.map(|_| goto(None, None));
            let prev = self.data.prev.map(|cursor| goto(None, Some(cursor)));
            let next = self.data.next.map(|cursor| goto(Some(cursor), None));
            return Some(
                el("nav")
                    .slot("pagination")
                    .attr("aria-label", "Pagination")
                    .children((
                        self.nav_link(&labels.first, Some(icons::CHEVRONS_LEFT), first, false),
                        self.nav_link(&labels.previous, Some(icons::CHEVRON_LEFT), prev, false),
                        self.nav_link(&labels.next, Some(icons::CHEVRON_RIGHT), next, true),
                    ))
                    .into(),
            );
        }
        let per = u64::from(self.query.state.per);
        match self.data.total {
            Total::Exact(total) => {
                let pages = u32::try_from(total.div_ceil(per).max(1)).unwrap_or(u32::MAX);
                (pages > 1).then(|| in_region(Pagination::new(page, pages, |page| self.query.href_with(|state| state.page = page)).into()))
            }
            _ => {
                if page == 1 && !self.data.has_more {
                    return None;
                }
                let prev = (page > 1).then(|| self.query.href_with(|state| state.page = page - 1));
                let next = self.data.has_more.then(|| self.query.href_with(|state| state.page = page + 1));
                Some(
                    el("nav")
                        .slot("pagination")
                        .attr("aria-label", "Pagination")
                        .children((self.nav_link(&labels.previous, Some(icons::CHEVRON_LEFT), prev, false), self.nav_link(&labels.next, Some(icons::CHEVRON_RIGHT), next, true)))
                        .into(),
                )
            }
        }
    }

    /// What the runtime reads: the words it needs, as one attribute.
    fn runtime_labels(&self) -> String {
        let labels = &self.labels;
        serde_json::json!({
            "selected": labels.selected,
            "page": labels.page_selected,
            "all": labels.select_all,
            "allOn": labels.all_selected,
            "some": labels.some_selected,
            "off": labels.off_page,
            "ended": labels.selection_ended,
            "failed": labels.failed,
            "lost": labels.lost_access,
        })
        .to_string()
    }

    fn into_node(self) -> Node {
        let state = &self.query.state;
        // Sticky start columns are numbered, so the stylesheet can place each by
        // its own offset variable.
        let mut starts = 0;
        let sticky: Vec<Option<String>> = state
            .cols
            .iter()
            .map(|at| match TableQuery::<R, F>::column(*at).sticky {
                Sticky::No => None,
                Sticky::End => Some("e".to_owned()),
                Sticky::Start => {
                    starts += 1;
                    Some(starts.to_string())
                }
            })
            .collect();
        let mut root = el("div")
            .slot("data-table")
            .attr("id", self.id)
            .flag("data-ui-region", true)
            .flag("data-ui-table", true)
            .attr("data-ui-src", self.canonical())
            .attr("data-filter-key", self.query.filter_key())
            .attr_opt("data-density", (state.density != TableDensity::Default).then_some(state.density.as_str()))
            .attr_opt("data-height", self.height.as_str())
            .flag("data-sticky-header", self.sticky_header)
            .flag("data-bleed", self.bleed)
            .flag("data-selectable", self.selectable())
            .flag("data-ui-grid", self.keyboard_grid)
            .attr_opt("data-scroll-to", self.scroll_to)
            .attr_opt("data-version", self.data.version.map(str::to_owned))
            .attr("data-l", self.runtime_labels())
            .uses(Uses::CORE.with(Uses::TABLE));
        if let Total::Exact(total) = self.data.total {
            root = root.attr("data-total", i64::try_from(total).unwrap_or(i64::MAX));
            if self.select_all_matching && self.selectable() {
                root = root.flag("data-can-select-all", true);
            }
        }
        let has_rows = !self.data.rows.is_empty() && self.failed.is_none() && !self.loading;
        let body: Element = if has_rows { el("tbody").child(SafeHtml::trusted(self.body_rows(&sticky))) } else { self.message_body() };
        let foot = (!self.foot.is_empty() && has_rows).then(|| {
            let mut cells: Vec<Element> = Vec::new();
            if self.selectable() {
                cells.push(el("td"));
            }
            if self.has_detail() {
                cells.push(el("td"));
            }
            for at in &state.cols {
                let column = TableQuery::<R, F>::column(*at);
                let cell = self.foot.iter().find(|(name, _)| *name == column.name).map(|(_, cell)| cell.clone());
                cells.push(el("td").flag("data-numeric", column.numeric).child(cell));
            }
            if self.has_actions() {
                cells.push(el("td"));
            }
            el("tfoot").child(el("tr").child(cells))
        });
        let table = el("table").slot("table").flag("data-fixed", !state.widths.is_empty()).children((
            self.caption.as_ref().map(|caption| el("caption").child(caption.to_string())),
            self.colgroup(),
            self.head(&sticky),
            body,
            foot,
        ));
        let alert = el("div").slot("table-alert").attr("role", "alert").flag("hidden", true).children((
            icon(icons::CIRCLE_ALERT),
            el("span").slot("alert-text"),
            el("a").slot("button").attr("data-variant", "outline").attr("data-size", "sm").attr("href", Href::from(self.canonical())).flag("data-ui-in-region", true).flag("data-ui-retry", true).child(self.labels.retry.to_string()),
        ));
        let footer = (has_rows).then(|| {
            el("div").slot("table-footer").children((
                self.summary(self.data.rows.len()),
                el("div").slot("footer-end").children((self.page_size(), self.pagination())),
            ))
        });
        root.children((self.toolbar_node(), self.chips(), alert, self.bulk_bar(), el("div").slot("table-container").attr("tabindex", 0_i64).attr("role", "region").attr("aria-label", self.caption.as_ref().map_or_else(|| "Table".to_owned(), ToString::to_string)).child(table), footer)).into()
    }
}

fn align_str(align: Align) -> Option<&'static str> {
    match align {
        Align::Start => None,
        Align::Center => Some("center"),
        Align::End => Some("end"),
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
                element.attrs.push(Attr { name: "data-ui-in-region", value: AttrValue::Flag });
            }
            element.children.iter_mut().for_each(walk);
        }
    }
    walk(&mut node);
    node
}
