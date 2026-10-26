//! [`Table`]: a data table for the `data_table` macro, with sortable
//! headers, row selection, row actions, pagination in Navree's page shape,
//! and empty and loading states.

use serde::{Deserialize, Serialize};

use crate::MenuItem;

/// Pagination metadata in Navree's page shape (`navree_db::PageMeta`):
/// `{"current_page", "per_page", "total", "last_page"}`.
///
/// ```
/// use navree_ui::PageInfo;
///
/// let page = PageInfo::new(2, 15, 230);
/// assert_eq!(page.last_page, 16);
/// // Any value with the four fields converts (a `navree_db::Page`'s `meta`).
/// let meta = serde_json::json!({ "current_page": 1, "per_page": 15, "total": 0, "last_page": 1 });
/// assert_eq!(PageInfo::from_meta(&meta), Some(PageInfo::new(1, 15, 0)));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageInfo {
    /// The page number (from 1).
    pub current_page: u64,
    /// The page size.
    pub per_page: u64,
    /// The number of matching rows.
    pub total: u64,
    /// The last page number (at least 1).
    pub last_page: u64,
}

impl PageInfo {
    /// Page `current_page` of `total` rows, `per_page` per page.
    pub fn new(current_page: u64, per_page: u64, total: u64) -> Self {
        let per_page = per_page.max(1);
        Self {
            current_page: current_page.max(1),
            per_page,
            total,
            last_page: total.div_ceil(per_page).max(1),
        }
    }

    /// The metadata of any serializable value with the four fields (such as
    /// `navree_db::PageMeta`), or `None` when it has another shape.
    pub fn from_meta(meta: &impl Serialize) -> Option<Self> {
        serde_json::to_value(meta)
            .ok()
            .and_then(|value| serde_json::from_value(value).ok())
    }
}

/// A column: its key (for sorting), its header, and how it aligns.
#[derive(Clone, Debug, Serialize)]
pub struct Column {
    key: String,
    label: String,
    sortable: bool,
    numeric: bool,
    hide_on_mobile: bool,
}

impl Column {
    /// A column `key` titled `label`.
    pub fn new(key: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            sortable: false,
            numeric: false,
            hide_on_mobile: false,
        }
    }

    /// The header sorts by this column: `sort={key}`, then `sort=-{key}`.
    pub fn sortable(mut self) -> Self {
        self.sortable = true;
        self
    }

    /// Right-aligned with tabular figures (amounts, counts, durations).
    pub fn numeric(mut self) -> Self {
        self.numeric = true;
        self
    }

    /// Hidden on phones (below 640px).
    pub fn hide_on_mobile(mut self) -> Self {
        self.hide_on_mobile = true;
        self
    }
}

/// A table cell: its text and how it is drawn.
#[derive(Clone, Debug, Serialize)]
pub struct Cell {
    kind: &'static str,
    text: String,
    sub: Option<String>,
    href: Option<String>,
    tone: Option<&'static str>,
}

/// The tone of a badge or status cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// Neutral (ink and grey).
    Neutral,
    /// Success and positive data (the accent).
    Success,
    /// Warning (amber).
    Warning,
    /// Error or danger.
    Danger,
    /// Outlined, quieter than neutral.
    Outline,
}

impl Tone {
    fn as_str(self) -> &'static str {
        match self {
            Tone::Neutral => "neutral",
            Tone::Success => "success",
            Tone::Warning => "warning",
            Tone::Danger => "danger",
            Tone::Outline => "outline",
        }
    }
}

impl Cell {
    fn of(kind: &'static str, text: impl Into<String>) -> Self {
        Self {
            kind,
            text: text.into(),
            sub: None,
            href: None,
            tone: None,
        }
    }

    /// Plain text.
    pub fn text(text: impl Into<String>) -> Self {
        Self::of("text", text)
    }

    /// The row's main text (a name or title): brighter and medium weight.
    pub fn strong(text: impl Into<String>) -> Self {
        Self::of("strong", text)
    }

    /// Quiet text (dates, secondary values).
    pub fn muted(text: impl Into<String>) -> Self {
        Self::of("muted", text)
    }

    /// Monospaced (ids, keys, versions).
    pub fn mono(text: impl Into<String>) -> Self {
        Self::of("mono", text)
    }

    /// A number, right-aligned with tabular figures (format it yourself:
    /// `"$1,204.00"`, `"12 ms"`).
    pub fn number(text: impl Into<String>) -> Self {
        Self::of("number", text)
    }

    /// A positive amount (won deals, revenue): a number in the accent.
    pub fn positive(text: impl Into<String>) -> Self {
        Self {
            tone: Some("success"),
            ..Self::of("number", text)
        }
    }

    /// A link (the main text of the row, usually).
    pub fn link(text: impl Into<String>, href: impl Into<String>) -> Self {
        Self {
            href: Some(href.into()),
            ..Self::of("link", text)
        }
    }

    /// Two lines: a main text and a quieter second line (name and email).
    pub fn lines(text: impl Into<String>, sub: impl Into<String>) -> Self {
        Self {
            sub: Some(sub.into()),
            ..Self::of("lines", text)
        }
    }

    /// A badge.
    pub fn badge(text: impl Into<String>, tone: Tone) -> Self {
        Self {
            tone: Some(tone.as_str()),
            ..Self::of("badge", text)
        }
    }

    /// A status: a colored dot and the text.
    pub fn status(text: impl Into<String>, tone: Tone) -> Self {
        Self {
            tone: Some(tone.as_str()),
            ..Self::of("status", text)
        }
    }

    /// Links the cell's text to `href` (text, strong and lines cells).
    pub fn href(mut self, href: impl Into<String>) -> Self {
        self.href = Some(href.into());
        self
    }
}

/// A row: an id (used by selection checkboxes), its cells in column order,
/// and its actions menu.
#[derive(Clone, Debug, Serialize)]
pub struct Row {
    id: String,
    cells: Vec<Cell>,
    actions: Vec<MenuItem>,
    selected: bool,
}

impl Row {
    /// A row with id `id` (the value of its selection checkbox).
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            cells: Vec::new(),
            actions: Vec::new(),
            selected: false,
        }
    }

    /// Adds the next cell.
    pub fn cell(mut self, cell: Cell) -> Self {
        self.cells.push(cell);
        self
    }

    /// Adds an entry to the row's actions menu.
    pub fn action(mut self, item: MenuItem) -> Self {
        self.actions.push(item);
        self
    }

    /// Renders the row selected.
    pub fn selected(mut self) -> Self {
        self.selected = true;
        self
    }
}

/// What an empty table shows.
#[derive(Clone, Debug, Serialize)]
pub struct Empty {
    title: String,
    description: Option<String>,
    icon: String,
    action: Option<Action>,
}

impl Empty {
    /// An empty state titled `title` with the `inbox` icon.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            description: None,
            icon: "inbox".to_owned(),
            action: None,
        }
    }

    /// A sentence under the title.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// The icon.
    pub fn icon(mut self, name: impl Into<String>) -> Self {
        self.icon = name.into();
        self
    }

    /// A button under the text.
    pub fn action(mut self, action: Action) -> Self {
        self.action = Some(action);
        self
    }
}

/// A button: a link, or a button opening a dialog.
#[derive(Clone, Debug, Serialize)]
pub struct Action {
    label: String,
    href: Option<String>,
    dialog: Option<String>,
    icon: Option<String>,
    variant: &'static str,
}

impl Action {
    /// A secondary button linking to `href`.
    pub fn link(label: impl Into<String>, href: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            href: Some(href.into()),
            dialog: None,
            icon: None,
            variant: "secondary",
        }
    }

    /// A secondary button opening the dialog with id `dialog`.
    pub fn dialog(label: impl Into<String>, dialog: impl Into<String>) -> Self {
        Self {
            href: None,
            dialog: Some(dialog.into()),
            ..Self::link(label, "")
        }
    }

    /// The lucide icon.
    pub fn icon(mut self, name: impl Into<String>) -> Self {
        self.icon = Some(name.into());
        self
    }

    /// The primary button (green; one per view).
    pub fn primary(mut self) -> Self {
        self.variant = "primary";
        self
    }

    /// A destructive button.
    pub fn danger(mut self) -> Self {
        self.variant = "danger";
        self
    }

    /// A quiet button.
    pub fn ghost(mut self) -> Self {
        self.variant = "ghost";
        self
    }
}

/// A data table, rendered by `{{ ui.data_table(table) }}`.
///
/// URLs are patterns: `{sort}`, `{page}` and `{per_page}` are replaced, so
/// keep the other query parameters in them
/// (`"/helm/users?search=ada&sort={sort}"`).
///
/// ```
/// use navree_ui::{Cell, Column, MenuItem, PageInfo, Row, Table, Tone};
///
/// let table = Table::new("users")
///     .column(Column::new("name", "Name").sortable())
///     .column(Column::new("role", "Role"))
///     .column(Column::new("deals", "Open deals").numeric().sortable())
///     .row(
///         Row::new("42")
///             .cell(Cell::lines("Ada Lovelace", "ada@acme.test").href("/helm/users/42"))
///             .cell(Cell::badge("Admin", Tone::Neutral))
///             .cell(Cell::number("12"))
///             .action(MenuItem::link("Edit", "/helm/users/42/edit").icon("pencil")),
///     )
///     .selectable()
///     .sort("-deals", "/helm/users?sort={sort}")
///     .page(PageInfo::new(1, 15, 1), "/helm/users?sort=-deals&page={page}");
/// assert!(serde_json::to_value(&table).is_ok());
/// ```
#[derive(Clone, Debug, Serialize)]
pub struct Table {
    id: String,
    caption: Option<String>,
    columns: Vec<Column>,
    rows: Vec<Row>,
    selectable: bool,
    bulk: Vec<Action>,
    sort: Option<String>,
    sort_url: Option<String>,
    page: Option<PageInfo>,
    page_url: Option<String>,
    per_page_url: Option<String>,
    per_page_options: Vec<u64>,
    empty: Empty,
    loading: bool,
    loading_rows: u32,
    has_actions: bool,
}

impl Table {
    /// A table with element id `id` (unique in the page).
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            caption: None,
            columns: Vec::new(),
            rows: Vec::new(),
            selectable: false,
            bulk: Vec::new(),
            sort: None,
            sort_url: None,
            page: None,
            page_url: None,
            per_page_url: None,
            per_page_options: vec![10, 15, 25, 50, 100],
            empty: Empty::new("No results").description("Nothing matches yet."),
            loading: false,
            loading_rows: 5,
            has_actions: false,
        }
    }

    /// A caption for assistive technology (visually hidden).
    pub fn caption(mut self, caption: impl Into<String>) -> Self {
        self.caption = Some(caption.into());
        self
    }

    /// Adds a column.
    pub fn column(mut self, column: Column) -> Self {
        self.columns.push(column);
        self
    }

    /// Adds a row (its cells in column order).
    pub fn row(mut self, row: Row) -> Self {
        self.has_actions |= !row.actions.is_empty();
        self.rows.push(row);
        self
    }

    /// Adds rows.
    pub fn rows(mut self, rows: impl IntoIterator<Item = Row>) -> Self {
        for row in rows {
            self = self.row(row);
        }
        self
    }

    /// Selection checkboxes (named `selected[]`, valued with the row id)
    /// with a select-all box in the header.
    pub fn selectable(mut self) -> Self {
        self.selectable = true;
        self
    }

    /// A button in the bar that shows while rows are selected.
    pub fn bulk(mut self, action: Action) -> Self {
        self.selectable = true;
        self.bulk.push(action);
        self
    }

    /// The current sort (`"name"` ascending, `"-name"` descending) and the
    /// URL pattern sortable headers link to (`{sort}` is replaced).
    pub fn sort(mut self, current: impl Into<String>, url: impl Into<String>) -> Self {
        let current = current.into();
        self.sort = (!current.is_empty()).then_some(current);
        self.sort_url = Some(url.into());
        self
    }

    /// The page shown and the URL pattern of other pages (`{page}` is
    /// replaced).
    pub fn page(mut self, page: PageInfo, url: impl Into<String>) -> Self {
        self.page = Some(page);
        self.page_url = Some(url.into());
        self
    }

    /// A rows-per-page select navigating to `url` (`{per_page}` is replaced).
    pub fn per_page(mut self, url: impl Into<String>, options: &[u64]) -> Self {
        self.per_page_url = Some(url.into());
        if !options.is_empty() {
            self.per_page_options = options.to_vec();
        }
        self
    }

    /// What shows when there are no rows.
    pub fn empty(mut self, empty: Empty) -> Self {
        self.empty = empty;
        self
    }

    /// Shows `rows` skeleton rows instead of the data (for a table whose
    /// rows load afterwards).
    pub fn loading(mut self, rows: u32) -> Self {
        self.loading = true;
        self.loading_rows = rows.max(1);
        self
    }
}

/// Field errors in Navree's validation error shape: the `errors` map of a
/// 400 body (`{"field": ["message", ...]}`) and its flattened `message`
/// list. Pass it to the form macros as `errors`.
///
/// ```
/// use std::collections::BTreeMap;
///
/// use navree_http::AppError;
/// use navree_ui::FormErrors;
///
/// let error = AppError::validation(BTreeMap::from([(
///     "email".to_owned(),
///     vec!["email must be a valid email address".to_owned()],
/// )]));
/// let errors = FormErrors::from_app_error(&error);
/// assert_eq!(errors.first("email"), Some("email must be a valid email address"));
/// assert!(!errors.is_empty());
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct FormErrors {
    message: Vec<String>,
    errors: std::collections::BTreeMap<String, Vec<String>>,
}

impl FormErrors {
    /// No errors.
    pub fn new() -> Self {
        Self::default()
    }

    /// The field errors of an [`AppError`](navree_http::AppError) (none
    /// unless it is a validation error).
    pub fn from_app_error(error: &navree_http::AppError) -> Self {
        Self::from_body(&error.body())
    }

    /// The field errors of an error body in Navree's shape (`{"statusCode",
    /// "message", "error", "errors"}`), for example one returned by an API
    /// call the page made.
    pub fn from_body(body: &serde_json::Value) -> Self {
        let errors: std::collections::BTreeMap<String, Vec<String>> = body
            .get("errors")
            .cloned()
            .and_then(|errors| serde_json::from_value(errors).ok())
            .unwrap_or_default();
        let message = match body.get("message") {
            Some(serde_json::Value::Array(items)) => items
                .iter()
                .filter_map(|m| m.as_str().map(str::to_owned))
                .collect(),
            Some(serde_json::Value::String(message)) if !errors.is_empty() => {
                vec![message.clone()]
            }
            _ => errors.values().flatten().cloned().collect(),
        };
        Self { message, errors }
    }

    /// Adds a message for `field`.
    pub fn add(&mut self, field: impl Into<String>, message: impl Into<String>) {
        let message = message.into();
        self.message.push(message.clone());
        self.errors.entry(field.into()).or_default().push(message);
    }

    /// The first message for `field`.
    pub fn first(&self, field: &str) -> Option<&str> {
        self.errors
            .get(field)
            .and_then(|messages| messages.first())
            .map(String::as_str)
    }

    /// Whether there are no errors.
    pub fn is_empty(&self) -> bool {
        self.errors.is_empty() && self.message.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_info_computes_the_last_page() {
        assert_eq!(PageInfo::new(1, 15, 0).last_page, 1);
        assert_eq!(PageInfo::new(1, 15, 15).last_page, 1);
        assert_eq!(PageInfo::new(1, 15, 16).last_page, 2);
        assert_eq!(PageInfo::new(0, 0, 3).current_page, 1);
        assert_eq!(PageInfo::new(0, 0, 3).per_page, 1);
    }

    #[test]
    fn form_errors_read_navree_bodies() {
        let body = serde_json::json!({
            "statusCode": 400,
            "message": ["name is required", "email must be a valid email address"],
            "error": "Bad Request",
            "errors": {
                "name": ["name is required"],
                "email": ["email must be a valid email address"]
            }
        });
        let errors = FormErrors::from_body(&body);
        assert_eq!(errors.first("name"), Some("name is required"));
        assert_eq!(errors.message.len(), 2);

        let mut added = FormErrors::new();
        assert!(added.is_empty());
        added.add("title", "title is required");
        assert_eq!(added.first("title"), Some("title is required"));
        assert_eq!(
            FormErrors::from_body(&serde_json::json!({})),
            FormErrors::new()
        );
    }
}
