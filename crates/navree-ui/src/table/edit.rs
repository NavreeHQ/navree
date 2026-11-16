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
//! of 400 or more to refuse. With JavaScript off the cell is a form with a Save button. A
//! cell that a control of your own draws ([`EditCell::control`]) is read with
//! [`EditedValue`], which runs the control's check before the value is read as its type, and
//! [`EditRefused::reply`] answers the post the check refused.

use std::borrow::Cow;
use std::marker::PhantomData;

use http::StatusCode;
use navree_http::body::{BodyError, BodyLimit, read_body};
use navree_http::extract::{FromRequest, Request, urlencoded};
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::form::{Control, ControlField, Language};
use crate::html::{Href, LocalHref};
use crate::node::{Element, Node, Uses, el};
use crate::op::{Reply, ToastTone};

enum Editor {
    Input(&'static str),
    Select(Vec<(String, String)>),
    Control(Control),
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
        Self {
            value: value.to_string(),
            href: href.into(),
            editor,
            label: "Edit".into(),
            csrf: None,
            disabled: false,
        }
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
    pub fn select(
        value: impl ToString,
        href: impl Into<Href>,
        options: impl IntoIterator<Item = (impl ToString, impl ToString)>,
    ) -> Self {
        let options = options
            .into_iter()
            .map(|(value, label)| (value.to_string(), label.to_string()))
            .collect();
        Self::new(value, href, Editor::Select(options))
    }

    /// Edited by a control of your own, posted to `href`. The control is drawn for a field named
    /// `value`, with the id `edit-` and 12 hex digits of the FNV-1a hash of `href` (so no two
    /// cells of a table share one), no label id, `aria-label` from [`label`](Self::label) (its
    /// text is the control's `label`) and `disabled` from [`disabled`](Self::disabled).
    ///
    /// Only a change of the field named `value` posts the cell, and only Enter and Escape in that
    /// field are the cell's; the other keys and changes inside the control are the control's own.
    /// The handler reads the post with [`EditedValue::read`], which runs the control's check: a
    /// value the control refuses is never stored.
    ///
    /// ```
    /// use navree_ui::form::{Control, ControlField, FieldControl};
    /// use navree_ui::table::EditCell;
    /// use navree_ui::{Node, el};
    ///
    /// struct Swatch;
    ///
    /// impl FieldControl for Swatch {
    ///     fn name(&self) -> &'static str {
    ///         "app.swatch"
    ///     }
    ///
    ///     fn draw(&self, field: &ControlField<'_>) -> Node {
    ///         el("input")
    ///             .attr("type", "color")
    ///             .attr("name", field.name.to_owned())
    ///             .attr("aria-label", field.label.to_owned())
    ///             .flag("disabled", field.disabled)
    ///             .into()
    ///     }
    /// }
    ///
    /// static SWATCH: Swatch = Swatch;
    ///
    /// let cell = EditCell::control("#e8b4b8", "/deliveries/42/colour", Control::new(&SWATCH))
    ///     .label("Colour");
    /// let _ = Node::from(cell);
    /// ```
    pub fn control(value: impl ToString, href: impl Into<Href>, control: Control) -> EditCell {
        Self::new(value, href, Editor::Control(control))
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

    fn editor(&self) -> Node {
        match &self.editor {
            Editor::Input(kind) => el("input")
                .slot("input")
                .attr("data-size", "sm")
                .attr("type", *kind)
                .attr("name", "value")
                .attr("value", self.value.clone())
                .attr("aria-label", self.label.to_string())
                .flag("disabled", self.disabled)
                .into(),
            Editor::Select(options) => {
                let items: Vec<Element> = options
                    .iter()
                    .map(|(value, label)| {
                        el("option")
                            .attr("value", value.clone())
                            .flag("selected", *value == self.value)
                            .child(label.clone())
                    })
                    .collect();
                el("select")
                    .slot("native-select")
                    .attr("data-size", "sm")
                    .attr("name", "value")
                    .attr("aria-label", self.label.to_string())
                    .flag("disabled", self.disabled)
                    .child(items)
                    .into()
            }
            Editor::Control(control) => {
                // An id of the cell's own: the address it posts to is its row's.
                let id = format!(
                    "edit-{}",
                    crate::assets::content_hash(self.href.as_str().as_bytes())
                );
                let mut field = ControlField::new("value");
                field.id = id.as_str();
                field.label = &*self.label;
                field.value = Some(self.value.as_str());
                field.disabled = self.disabled;
                control.get().draw(&field)
            }
        }
    }
}

impl From<EditCell> for Node {
    fn from(cell: EditCell) -> Self {
        let editor = cell.editor();
        el("form")
            .slot("edit-cell")
            .attr("method", "post")
            .attr("action", cell.href)
            .flag("data-ui-form", true)
            .flag("data-ui-edit", true)
            .uses(Uses::CORE.with(Uses::TABLE))
            .children((
                cell.csrf.map(crate::form::csrf_field),
                editor,
                el("noscript").child(
                    el("button")
                        .slot("button")
                        .attr("data-variant", "outline")
                        .attr("data-size", "xs")
                        .attr("type", "submit")
                        .child("Save"),
                ),
            ))
            .into()
    }
}

/// What an inline edit posted: the one field `value`, as text. The handler reads it with
/// [`read`](Self::read), which holds it to the check of the control that drew the cell
/// ([`EditCell::control`]) before it is read as `T`, or with
/// [`read_built_in`](Self::read_built_in) for the four editors the toolkit draws itself.
///
/// The body is read within the route's body limit (413 past it), and a post with no `value`
/// is a 400.
///
/// ```
/// use navree_http::Extension;
/// use navree_kernel::AppContext;
/// use navree_ui::form::{Control, ControlField, FieldControl, Language};
/// use navree_ui::{EditedValue, Node, Reply, el};
///
/// struct Swatch;
///
/// impl FieldControl for Swatch {
///     fn name(&self) -> &'static str {
///         "app.swatch"
///     }
///
///     fn draw(&self, field: &ControlField<'_>) -> Node {
///         el("input").attr("name", field.name.to_owned()).into()
///     }
/// }
///
/// static SWATCH: Swatch = Swatch;
///
/// async fn save_colour(Extension(ctx): Extension<AppContext>, edit: EditedValue<String>) -> Reply {
///     let language = Language::of(Some(&ctx)).await;
///     match edit.read(Control::new(&SWATCH), &language) {
///         Ok(_colour) => Reply::ops(vec![]),
///         Err(refused) => refused.reply("/deliveries"),
///     }
/// }
/// ```
pub struct EditedValue<T> {
    text: String,
    marker: PhantomData<fn() -> T>,
}

impl<T> FromRequest for EditedValue<T> {
    type Rejection = navree_http::AppError;

    async fn from_request(request: Request) -> Result<Self, Self::Rejection> {
        let (parts, body) = request.into_parts();
        let limit = BodyLimit::of(&parts.extensions);
        let bytes = read_body(body, limit).await.map_err(|error| match error {
            BodyError::TooLarge => navree_http::AppError::payload_too_large(limit),
            other => {
                navree_http::AppError::from_status(other.status()).with_detail(other.to_string())
            }
        })?;
        let text = urlencoded::parse(&bytes)
            .find(|(name, _)| name == "value")
            .map(|(_, value)| value.into_owned())
            .ok_or_else(|| navree_http::AppError::bad_request("The edit has no value."))?;
        Ok(Self {
            text,
            marker: PhantomData,
        })
    }
}

/// A value read as a form field is: one pair, `value`.
#[derive(Deserialize)]
struct One<T> {
    value: T,
}

impl<T: DeserializeOwned> EditedValue<T> {
    /// The text that was posted, as it was.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Holds the value to `control`'s check, with the language's app and configuration
    /// (`Language::of` finds them), and then reads it as `T`. A value the control refuses is
    /// refused with the control's message; one that is not a `T` is refused with the form's own
    /// words for a value that is not valid.
    pub fn read(self, control: Control, language: &Language) -> Result<T, EditRefused> {
        if let Err(message) = control.check(&self.text, &language.control_cx()) {
            return Err(EditRefused { message });
        }
        self.parsed(language)
    }

    /// Reads the value as `T`, with no check of a control's. For the four built-in editors only
    /// (text, number, date, select), which have no control to check: never for a cell drawn with
    /// [`EditCell::control`], whose value would reach the app unchecked.
    pub fn read_built_in(self, language: &Language) -> Result<T, EditRefused> {
        self.parsed(language)
    }

    /// The text read as a field of type `T` is.
    fn parsed(self, language: &Language) -> Result<T, EditRefused> {
        let encoded = crate::form::encode(&[("value", self.text.as_str())]);
        match One::<T>::deserialize(urlencoded::Deserializer::new(encoded.as_bytes())) {
            Ok(one) => Ok(one.value),
            Err(_) => Err(EditRefused {
                message: language
                    .messages()
                    .clone()
                    .with_attributes([("value", "Value")])
                    .with_default_messages(crate::form::FORM_MESSAGES.iter().copied())
                    .render("value", "form.invalid", &[]),
            }),
        }
    }
}

/// An inline edit that was refused: by the control's check, or because the value is not of the
/// field's type. Answer the post with [`reply`](Self::reply).
///
/// ```
/// use navree_ui::table::EditRefused;
/// # fn answer(refused: EditRefused) {
/// let message = refused.message().to_owned();
/// let _reply = refused.reply("/deliveries");
/// # let _ = message;
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct EditRefused {
    message: String,
}

impl EditRefused {
    /// Why the edit was refused, in the reader's language.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The answer to the post: status 422 with the message as a danger toast, and the cell goes
    /// back to the value it had. With JavaScript off, back to `back` (303), and the message is
    /// the toast the page there shows.
    pub fn reply(self, back: impl Into<LocalHref>) -> Reply {
        Reply::ops_or_redirect(back.into().into_href())
            .toast(ToastTone::Danger, self.message)
            .status(StatusCode::UNPROCESSABLE_ENTITY)
    }
}
