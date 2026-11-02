//! [`EditCell`]: a cell the user can change where it stands. The new value shows at
//! once and posts by itself; if the server refuses, or the post fails, the cell goes
//! back to the value it had, and the server's toast says why.
//!
//! ```ignore
//! DataTable::new("deals", &query, &page).cell("title", |row| {
//!     EditCell::text(row.title.clone(), format!("/deals/{}/title", row.id)).label("Deal").into()
//! })
//! ```
//!
//! The handler reads the field `value`, answers a [`crate::Reply`], and sets a status
//! of 400 or more to refuse. With JavaScript off the cell is a form with a Save button.

use std::borrow::Cow;

use crate::html::Href;
use crate::node::{Element, Node, Uses, el};

enum Editor {
    Input(&'static str),
    Select(Vec<(String, String)>),
}

/// A cell that edits in place.
#[must_use]
pub struct EditCell {
    value: String,
    href: Href,
    editor: Editor,
    label: Cow<'static, str>,
    csrf: Option<String>,
    disabled: bool,
}

impl EditCell {
    fn new(value: impl ToString, href: impl Into<Href>, editor: Editor) -> Self {
        Self { value: value.to_string(), href: href.into(), editor, label: "Edit".into(), csrf: None, disabled: false }
    }

    /// Text, posted to `href`.
    pub fn text(value: impl ToString, href: impl Into<Href>) -> Self {
        Self::new(value, href, Editor::Input("text"))
    }

    /// A number, posted to `href`.
    pub fn number(value: impl ToString, href: impl Into<Href>) -> Self {
        Self::new(value, href, Editor::Input("number"))
    }

    /// A date, posted to `href`.
    pub fn date(value: impl ToString, href: impl Into<Href>) -> Self {
        Self::new(value, href, Editor::Input("date"))
    }

    /// One of `options`, as `(value, label)`, posted to `href`.
    pub fn select(value: impl ToString, href: impl Into<Href>, options: impl IntoIterator<Item = (impl ToString, impl ToString)>) -> Self {
        let options = options.into_iter().map(|(value, label)| (value.to_string(), label.to_string())).collect();
        Self::new(value, href, Editor::Select(options))
    }

    /// Names it for assistive technology.
    pub fn label(mut self, label: impl Into<Cow<'static, str>>) -> Self {
        self.label = label.into();
        self
    }

    /// The CSRF token, so it works with JavaScript off.
    pub fn csrf(mut self, token: impl Into<String>) -> Self {
        self.csrf = Some(token.into());
        self
    }

    /// Shows it read-only: this user can't change it. The handler checks again.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    fn control(&self) -> Element {
        match &self.editor {
            Editor::Input(kind) => el("input").slot("input").attr("data-size", "sm").attr("type", *kind).attr("name", "value").attr("value", self.value.clone()).attr("aria-label", self.label.to_string()).flag("disabled", self.disabled),
            Editor::Select(options) => {
                let items: Vec<Element> = options.iter().map(|(value, label)| el("option").attr("value", value.clone()).flag("selected", *value == self.value).child(label.clone())).collect();
                el("select").slot("native-select").attr("data-size", "sm").attr("name", "value").attr("aria-label", self.label.to_string()).flag("disabled", self.disabled).child(items)
            }
        }
    }
}

impl From<EditCell> for Node {
    fn from(cell: EditCell) -> Self {
        let control = cell.control();
        el("form")
            .slot("edit-cell")
            .attr("method", "post")
            .attr("action", cell.href)
            .flag("data-ui-form", true)
            .flag("data-ui-edit", true)
            .uses(Uses::CORE.with(Uses::TABLE))
            .children((
                cell.csrf.map(crate::form::csrf_field),
                control,
                el("noscript").child(el("button").slot("button").attr("data-variant", "outline").attr("data-size", "xs").attr("type", "submit").child("Save")),
            ))
            .into()
    }
}
