//! [`Exportable`]: a row type with columns, and [`Column`].

use std::borrow::Cow;

use crate::Cell;

/// One column of an export: the field it comes from, its header, and how
/// XLSX shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Column {
    /// The field the column comes from (`first_name`).
    pub key: Cow<'static, str>,
    /// The header text, or the translation key a
    /// [`HeaderTranslator`](crate::HeaderTranslator) turns into it
    /// (`contacts.first_name`). Without a translation it is written as is.
    pub header: Cow<'static, str>,
    /// An Excel number format for the column's numbers and dates in XLSX
    /// (`"0.00"`, `"#,##0"`, `"dd/mm/yyyy"`); CSV writes the plain value.
    pub format: Option<Cow<'static, str>>,
    /// The column width in XLSX, in characters; by default it fits the
    /// header and the first rows.
    pub width: Option<f64>,
}

impl Column {
    /// A column for field `key` headed `header`.
    pub fn new(key: impl Into<Cow<'static, str>>, header: impl Into<Cow<'static, str>>) -> Self {
        Column {
            key: key.into(),
            header: header.into(),
            format: None,
            width: None,
        }
    }

    /// The Excel number format of the column (XLSX only).
    pub fn format(mut self, format: impl Into<Cow<'static, str>>) -> Self {
        self.format = Some(format.into());
        self
    }

    /// The column width in characters (XLSX only).
    pub fn width(mut self, width: f64) -> Self {
        self.width = Some(width);
        self
    }
}

/// A row type an export writes: its columns, and its cells in the same
/// order.
///
/// Derive it with `#[derive(Exportable)]`:
///
/// ```
/// use chrono::{DateTime, Utc};
/// use navree_sheets::{Cell, Exportable};
///
/// #[derive(Exportable)]
/// #[export(sheet = "Contacts")]
/// struct ContactRow {
///     #[export(header = "Name")]
///     name: String,
///     email: Option<String>,
///     #[export(header = "Deals", format = "#,##0")]
///     deals: u32,
///     #[export(skip)]
///     internal_note: String,
/// }
///
/// let columns = ContactRow::columns();
/// let headers: Vec<_> = columns.iter().map(|c| c.header.as_ref()).collect();
/// assert_eq!(headers, ["Name", "Email", "Deals"]);
/// assert_eq!(ContactRow::sheet_name(), "Contacts");
///
/// let row = ContactRow {
///     name: "Ada".into(),
///     email: None,
///     deals: 3,
///     internal_note: String::new(),
/// };
/// assert_eq!(row.cells(), vec![Cell::Text("Ada".into()), Cell::Empty, Cell::Int(3)]);
/// ```
///
/// | Attribute | Meaning |
/// |---|---|
/// | `#[export(sheet = "Contacts")]` (on the struct) | The XLSX sheet name (default `Sheet1`) |
/// | `#[export(header = "Email")]` | The header, or a translation key; default: the field name in sentence case (`first_name` is `First name`) |
/// | `#[export(format = "0.00")]` | Excel number format of the column (XLSX) |
/// | `#[export(width = 30)]` | Column width in characters (XLSX) |
/// | `#[export(with = "path::to_fn")]` | `fn(&FieldType) -> Cell` instead of [`IntoCell`](crate::IntoCell) |
/// | `#[export(skip)]` | Leaves the field out |
pub trait Exportable {
    /// The columns, in order.
    fn columns() -> Vec<Column>;

    /// The row's cells, one per column, in the same order.
    fn cells(&self) -> Vec<Cell>;

    /// The XLSX sheet name.
    fn sheet_name() -> &'static str {
        "Sheet1"
    }
}

/// Field names in sentence case: `first_name` is `First name`. What
/// `#[derive(Exportable)]` and `#[derive(Importable)]` use when a field has
/// no `header`.
pub fn sentence_case(field: &str) -> String {
    let words = field.trim_matches('_').replace('_', " ");
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentence_case_turns_field_names_into_headers() {
        assert_eq!(sentence_case("first_name"), "First name");
        assert_eq!(sentence_case("email"), "Email");
        assert_eq!(sentence_case("_id"), "Id");
        assert_eq!(sentence_case(""), "");
    }

    #[test]
    fn columns_carry_a_format_and_a_width() {
        let column = Column::new("amount", "Amount").format("0.00").width(12.0);
        assert_eq!(column.format.as_deref(), Some("0.00"));
        assert_eq!(column.width, Some(12.0));
    }
}
