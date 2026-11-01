//! Forms from a validated struct: [`Form`] (derived), [`FormView`] (draws it)
//! and [`Submit`] (reads it back).
//!
//! The struct is the one source: its doc comments are the labels, its types
//! pick the widgets, and its `#[validate]` rules set `required`, `maxlength`
//! and `min` in the browser and run again on the server.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::marker::PhantomData;

use http::request::Parts;
use http::{Method, StatusCode};
use navree_http::body::{BodyLimit, read_body};
use navree_http::extract::{FromRequest, Request, urlencoded};
use navree_http::response::{IntoResponse, Response};
use navree_validation::{Messages, Rule, Validate};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};

pub use navree_validation::{FieldRules, RuleTable};

use crate::components::{Button, Checkbox, Field, Input, InputGroup, InputKind, NativeSelect, RadioGroup, Switch, Textarea};
use crate::html::Href;
use crate::node::{Element, Node, Uses, el};
use crate::op::{Envelope, Op, UI_HEADER};
use crate::view::View;

/// How a field is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Widget {
    /// A single-line input.
    Input(InputKind),
    /// A multi-line input.
    Textarea {
        /// Its height in rows.
        rows: u32,
    },
    /// The browser's select.
    Select,
    /// Radio buttons.
    Radio,
    /// A checkbox.
    Checkbox,
    /// A switch.
    Switch,
    /// An amount of money, typed like `12,500.50`.
    Money,
}

/// One field of a [`Form`]. The derive writes these.
#[derive(Clone, Copy, Debug)]
pub struct FormField {
    /// The name it posts as.
    pub name: &'static str,
    /// Its label.
    pub label: &'static str,
    /// How it's drawn.
    pub widget: Widget,
    /// An `Option`: it may be left empty.
    pub optional: bool,
    /// The hint while empty.
    pub placeholder: Option<&'static str>,
    /// The help below it.
    pub help: Option<&'static str>,
    /// A select's empty first choice.
    pub prompt: Option<&'static str>,
    /// The message for any failed rule, in place of the rule's own.
    pub error: Option<&'static str>,
    /// Another field drawn inside this one's input group.
    pub addon: Option<&'static str>,
    /// Takes focus when the page loads.
    pub autofocus: bool,
    /// Takes the whole width of the form.
    pub wide: bool,
    /// A select's or radio group's choices: `(value, label)`.
    pub options: &'static [(&'static str, &'static str)],
}

/// A struct that can be drawn as a form. `#[derive(Form)]` implements it.
pub trait Form {
    /// The form's id.
    const ID: &'static str;
    /// Its fields, in order.
    const FIELDS: &'static [FormField];
    /// Its rules.
    const RULES: &'static [FieldRules];

    /// The current values, as text, for an edit form.
    fn values(&self) -> Vec<(&'static str, String)>;
}

/// An enum whose variants are a select's choices. `#[derive(Options)]`
/// implements it.
pub trait Options {
    /// `(value, label)` for each variant.
    const OPTIONS: &'static [(&'static str, &'static str)];

    /// This variant's value.
    fn option_value(&self) -> &'static str;
}

/// A value as a form shows it. `None` leaves the field empty.
pub trait FieldValue {
    /// The text of the value.
    fn field_value(&self) -> Option<String>;
}

impl<T: FieldValue> FieldValue for Option<T> {
    fn field_value(&self) -> Option<String> {
        self.as_ref().and_then(FieldValue::field_value)
    }
}

macro_rules! display_values {
    ($($type:ty),+ $(,)?) => {
        $(impl FieldValue for $type {
            fn field_value(&self) -> Option<String> {
                Some(self.to_string())
            }
        })+
    };
}

display_values!(
    String, bool, i8, i16, i32, i64, isize, u8, u16, u32, u64, usize, f32, f64, uuid::Uuid, chrono::NaiveDate, chrono::NaiveTime,
);

impl FieldValue for chrono::NaiveDateTime {
    fn field_value(&self) -> Option<String> {
        Some(self.format("%Y-%m-%dT%H:%M").to_string())
    }
}

/// An amount of money in its smallest unit, typed as `12,500.50`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Money(pub i64);

impl Money {
    /// Reads `12,500.50`, `12500` or `-3.5`. More than two decimals, or too
    /// large a number, is `None`.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim().replace(',', "");
        let (negative, text) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text.as_str()),
        };
        let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
        if (whole.is_empty() && fraction.is_empty()) || fraction.len() > 2 {
            return None;
        }
        if !whole.bytes().chain(fraction.bytes()).all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        let whole: i64 = if whole.is_empty() { 0 } else { whole.parse().ok()? };
        let fraction: i64 = match fraction.len() {
            0 => 0,
            1 => fraction.parse::<i64>().ok()? * 10,
            _ => fraction.parse().ok()?,
        };
        let cents = whole.checked_mul(100)?.checked_add(fraction)?;
        Some(Self(if negative { -cents } else { cents }))
    }

    /// `1250050` as `12,500.50`.
    pub fn display(self) -> String {
        let cents = self.0.unsigned_abs();
        let whole = (cents / 100).to_string();
        let mut out = String::with_capacity(whole.len() + whole.len() / 3 + 4);
        if self.0 < 0 {
            out.push('-');
        }
        for (at, digit) in whole.chars().enumerate() {
            if at > 0 && (whole.len() - at).is_multiple_of(3) {
                out.push(',');
            }
            out.push(digit);
        }
        let _ = write!(out, ".{:02}", cents % 100);
        out
    }
}

impl FieldValue for Money {
    fn field_value(&self) -> Option<String> {
        Some(self.display())
    }
}

impl<'de> Deserialize<'de> for Money {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).ok_or_else(|| serde::de::Error::custom("enter an amount, like 12,500.50"))
    }
}

/// The rules with an effect in the browser, read once per field.
#[derive(Default)]
struct Native {
    required: bool,
    min_length: Option<usize>,
    max_length: Option<usize>,
    min: Option<&'static str>,
    max: Option<&'static str>,
    kind: Option<InputKind>,
}

fn native<T: Form>(field: &FormField) -> Native {
    let mut out = Native::default();
    if !field.optional && !matches!(field.widget, Widget::Checkbox | Widget::Switch | Widget::Textarea { .. } | Widget::Input(InputKind::Text | InputKind::Email | InputKind::Url | InputKind::Tel | InputKind::Password | InputKind::Search | InputKind::Hidden)) {
        out.required = true;
    }
    let Some(rules) = T::RULES.iter().find(|rules| rules.name == field.name) else {
        return out;
    };
    for rule in rules.rules {
        match rule {
            Rule::Required => out.required = true,
            // Native length counts UTF-16 units, so only a character count maps.
            Rule::Length { min, max, bytes: false } => {
                out.min_length = *min;
                out.max_length = *max;
                if min.is_some_and(|min| min > 0) {
                    out.required = true;
                }
            }
            Rule::Range { min, max } => {
                out.min = *min;
                out.max = *max;
            }
            Rule::Format("email") => out.kind = Some(InputKind::Email),
            Rule::Format("url") => out.kind = Some(InputKind::Url),
            // A Rust regex isn't always a browser's: `pattern` stays on the server.
            _ => {}
        }
    }
    out
}

/// Draws a [`Form`]: a `<form>` that posts with or without the runtime.
///
/// ```ignore
/// FormView::<NewDeal>::new().post("/deals").submit("Create deal")
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct FormView<T> {
    action: Option<Href>,
    method: &'static str,
    submit: Node,
    cancel: Option<(Node, Href)>,
    values: Vec<(String, String)>,
    errors: BTreeMap<String, Vec<String>>,
    options: BTreeMap<&'static str, Vec<(String, String)>>,
    columns: bool,
    csrf: Option<String>,
    marker: PhantomData<fn() -> T>,
}

impl<T: Form> FormView<T> {
    /// An empty form.
    pub fn new() -> Self {
        Self {
            action: None,
            method: "post",
            submit: Node::from("Save"),
            cancel: None,
            values: Vec::new(),
            errors: BTreeMap::new(),
            options: BTreeMap::new(),
            columns: true,
            csrf: None,
            marker: PhantomData,
        }
    }

    /// Posts to `action`.
    pub fn post(mut self, action: impl Into<Href>) -> Self {
        self.action = Some(action.into());
        self.method = "post";
        self
    }

    /// Puts the session's CSRF token in the form as the field `_csrf`, which is
    /// how a plain form with JavaScript off proves it came from this site. The
    /// runtime sends the token as a header itself, so this costs nothing with
    /// JavaScript on.
    pub fn csrf(mut self, token: impl Into<String>) -> Self {
        self.csrf = Some(token.into());
        self
    }

    /// Sets the submit button's text.
    pub fn submit(mut self, label: impl Into<Node>) -> Self {
        self.submit = label.into();
        self
    }

    /// Adds a link back.
    pub fn cancel(mut self, label: impl Into<Node>, href: impl Into<Href>) -> Self {
        self.cancel = Some((label.into(), href.into()));
        self
    }

    /// Fills the form from a value.
    pub fn of(mut self, value: &T) -> Self {
        self.values = value.values().into_iter().map(|(name, text)| (name.to_owned(), text)).collect();
        self
    }

    /// Fills the form from what was submitted.
    pub fn with_values(mut self, values: Vec<(String, String)>) -> Self {
        self.values = values;
        self
    }

    /// Shows errors under the fields.
    pub fn with_errors(mut self, errors: BTreeMap<String, Vec<String>>) -> Self {
        self.errors = errors;
        self
    }

    /// Sets the choices of a select whose options come from the database.
    pub fn options(mut self, field: &'static str, options: impl IntoIterator<Item = (String, String)>) -> Self {
        self.options.insert(field, options.into_iter().collect());
        self
    }

    /// Puts every field on its own line.
    pub fn one_column(mut self) -> Self {
        self.columns = false;
        self
    }

    fn value_of(&self, name: &str) -> Option<&str> {
        self.values.iter().find(|(field, _)| field == name).map(|(_, value)| value.as_str())
    }

    fn control(&self, field: &FormField) -> Node {
        let rules = native::<T>(field);
        let value = self.value_of(field.name);
        let invalid = self.errors.contains_key(field.name);
        match field.widget {
            Widget::Input(kind) => {
                let kind = rules.kind.filter(|_| kind == InputKind::Text).unwrap_or(kind);
                let mut input = Input::new(kind, field.name).invalid(invalid);
                if kind == InputKind::Number && rules.min.is_none_or(|min| min.contains('.')) {
                    input = input.attr("step", "any");
                }
                self.finish_input(input, field, &rules, value).into()
            }
            Widget::Money => self.finish_input(Input::text(field.name).inputmode("decimal").invalid(invalid), field, &rules, value).into(),
            Widget::Textarea { rows } => {
                let mut area = Textarea::new(field.name).rows(rows);
                if let Some(value) = value {
                    area = area.value(value.to_owned());
                }
                if let Some(placeholder) = field.placeholder {
                    area = area.placeholder(placeholder);
                }
                if rules.required {
                    area = area.required();
                }
                area = lengths_textarea(area, &rules);
                area.into()
            }
            Widget::Select => self.select(field, rules.required, value).into(),
            Widget::Radio => {
                let mut group = RadioGroup::new(field.name, field.label).options(field.options.iter().map(|(value, label)| crate::components::OptionItem::new(*value, *label)));
                if let Some(value) = value {
                    group = group.selected(value.to_owned());
                }
                if rules.required {
                    group = group.required();
                }
                group.into()
            }
            Widget::Checkbox => Checkbox::new(field.name).checked(is_on(value)).label(field.label).into(),
            Widget::Switch => Switch::new(field.name).checked(is_on(value)).label(field.label).into(),
        }
    }

    fn finish_input(&self, mut input: Input, field: &FormField, rules: &Native, value: Option<&str>) -> Input {
        if let Some(value) = value {
            input = input.value(value.to_owned());
        }
        if let Some(placeholder) = field.placeholder {
            input = input.placeholder(placeholder);
        }
        if rules.required {
            input = input.required();
        }
        if field.autofocus {
            input = input.autofocus();
        }
        if let Some(min) = rules.min_length {
            input = input.attr("minlength", min_attr(min));
        }
        if let Some(max) = rules.max_length {
            input = input.attr("maxlength", min_attr(max));
        }
        if let Some(min) = rules.min {
            input = input.attr("min", min);
        }
        if let Some(max) = rules.max {
            input = input.attr("max", max);
        }
        input
    }

    fn select(&self, field: &FormField, required: bool, value: Option<&str>) -> NativeSelect {
        let mut select = NativeSelect::new(field.name);
        if let Some(prompt) = field.prompt {
            select = select.prompt(prompt);
        }
        if let Some(options) = self.options.get(field.name) {
            select = select.options(options.iter().map(|(value, label)| crate::components::OptionItem::new(value.clone(), label.clone())));
        } else {
            select = select.options(field.options.iter().map(|(value, label)| crate::components::OptionItem::new(*value, *label)));
        }
        if let Some(value) = value {
            // A choice from the database, on a form drawn without them: it stays.
            if field.options.is_empty() && !self.options.contains_key(field.name) {
                select = select.option(value.to_owned(), value.to_owned());
            }
            select = select.selected(value.to_owned());
        }
        if required {
            select = select.required();
        }
        select
    }

    fn field(&self, field: &FormField) -> Option<Element> {
        // A field drawn inside another's input group has no box of its own.
        if T::FIELDS.iter().any(|other| other.addon == Some(field.name)) {
            return None;
        }
        let error = self.errors.get(field.name).map(|messages| messages.join(" "));
        let id = format!("{}-{}", T::ID, field.name);
        let holder = match field.widget {
            Widget::Input(InputKind::Hidden) => return Some(input_only(self.control(field))),
            Widget::Checkbox | Widget::Switch => {
                let error_id = error.as_ref().map(|_| format!("{id}-error"));
                let mut box_el = el("div").slot("field").flag("data-invalid", error.is_some()).child(with_id(self.control(field), &id));
                if let Some(help) = field.help {
                    box_el = box_el.child(el("p").slot("field-description").child(help));
                }
                box_el.child(error.map(|text| el("p").slot("field-error").attr_opt("id", error_id).child(text)))
            }
            Widget::Radio => {
                let box_el = el("div").slot("field").flag("data-invalid", error.is_some()).child(self.control(field));
                box_el.child(error.map(|text| el("p").slot("field-error").child(text)))
            }
            _ => {
                let mut control = self.control(field);
                if let Some(addon) = field.addon.and_then(|name| T::FIELDS.iter().find(|other| other.name == name)) {
                    let mut select = self.control(addon);
                    if let Node::Element(element) = &mut select {
                        *element = std::mem::replace(element, el("x")).attr("aria-label", addon.label);
                    }
                    control = InputGroup::new(control).end(select).into();
                }
                let mut box_field = Field::new(id, field.label).error_opt(error).control(control);
                if let Some(help) = field.help {
                    box_field = box_field.help(help);
                }
                let box_node: Node = box_field.into();
                let Node::Element(element) = box_node else { return None };
                element
            }
        };
        Some(holder.flag("data-wide", field.wide))
    }

    fn into_node(self) -> Node {
        let boxes: Vec<Element> = T::FIELDS.iter().filter_map(|field| self.field(field)).collect();
        let grid = if self.columns { "form-grid" } else { "form-stack" };
        el("form")
            .slot("form")
            .attr("id", T::ID)
            .attr("method", self.method)
            .attr_opt("action", self.action.map(AttrHref))
            .flag("data-ui-form", true)
            .uses(Uses::CORE)
            .children((
                self.csrf.map(csrf_field),
                el("div").slot(grid).child(boxes),
                el("div").slot("form-actions").children((
                    self.cancel.map(|(label, href)| Button::ghost(label).href(href)),
                    Button::primary(self.submit).submit(),
                )),
            ))
            .into()
    }
}

/// The name of the field that carries the CSRF token in a plain form.
pub const CSRF_FIELD: &str = "_csrf";

/// The hidden field that carries `token`, for a form that is not a [`FormView`].
pub fn csrf_field(token: impl Into<crate::node::AttrValue>) -> Element {
    el("input").attr("type", "hidden").attr("name", CSRF_FIELD).attr("value", token)
}

/// An already checked URL, as an attribute value.
struct AttrHref(Href);

impl From<AttrHref> for crate::node::AttrValue {
    fn from(href: AttrHref) -> Self {
        href.0.into()
    }
}

fn min_attr(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn lengths_textarea(mut area: Textarea, rules: &Native) -> Textarea {
    if let Some(min) = rules.min_length {
        area = area.attr("minlength", min_attr(min));
    }
    if let Some(max) = rules.max_length {
        area = area.attr("maxlength", min_attr(max));
    }
    area
}

fn is_on(value: Option<&str>) -> bool {
    matches!(value, Some("true" | "on" | "1"))
}

fn input_only(control: Node) -> Element {
    match control {
        Node::Element(element) => element,
        _ => el("div"),
    }
}

/// Sets the id of the input inside `node`, for a label that holds it.
fn with_id(mut node: Node, id: &str) -> Node {
    fn walk(node: &mut Node, id: &str) -> bool {
        if let Node::Element(element) = node {
            if element.tag == "input" {
                element.attrs.push(crate::node::Attr { name: "id", value: crate::node::AttrValue::Text(id.into()) });
                return true;
            }
            return element.children.iter_mut().any(|child| walk(child, id));
        }
        false
    }
    walk(&mut node, id);
    node
}

impl<T: Form> Default for FormView<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Form> From<FormView<T>> for Node {
    fn from(view: FormView<T>) -> Self {
        view.into_node()
    }
}

/// Field errors by name.
pub type FieldErrors = BTreeMap<String, Vec<String>>;

/// The most name-and-value pairs read from one body. A form has far fewer: a body
/// with more is refused before anything is decoded.
const MAX_PAIRS: usize = 1000;

/// The most text a rejected form sends back, over all its fields. Past this a
/// field comes back empty, so a body can't be reflected at five times its size.
const MAX_ECHO: usize = 256 * 1024;

/// Reads a form body, keeping only the pairs whose name is one of `T`'s fields:
/// nothing is allocated for any other name. `None` when the body holds more than
/// [`MAX_PAIRS`] pairs.
fn read_pairs<T: Form>(bytes: &[u8]) -> Option<Vec<(String, String)>> {
    let mut kept = Vec::with_capacity(T::FIELDS.len());
    for (count, (name, value)) in urlencoded::parse(bytes).enumerate() {
        if count >= MAX_PAIRS {
            return None;
        }
        if T::FIELDS.iter().any(|field| field.name == name) {
            kept.push((name.into_owned(), value.into_owned()));
        }
    }
    Some(kept)
}

/// What a rejected form may show again: never a password, and a bounded amount.
fn echo<T: Form>(values: Vec<(String, String)>) -> Vec<(String, String)> {
    let mut left = MAX_ECHO;
    values
        .into_iter()
        .filter(|(name, value)| {
            let secret = T::FIELDS.iter().any(|field| field.name == name && field.widget == Widget::Input(InputKind::Password));
            if secret || value.len() > left {
                return false;
            }
            left -= value.len();
            true
        })
        .collect()
}

/// Decodes submitted pairs into `T` and runs its rules: what [`Submit`] and
/// tests share. Empty values of optional fields are absent, an unchecked box
/// is `false`, and a missing text field is empty. A pair that isn't one of `T`'s
/// fields is dropped, so a field marked `#[form(skip)]` can't be set by the client.
pub fn decode<T>(pairs: &[(String, String)]) -> Result<T, FieldErrors>
where
    T: Form + DeserializeOwned + Validate,
{
    let mut kept: Vec<(&str, &str)> = Vec::with_capacity(pairs.len() + T::FIELDS.len());
    for (name, value) in pairs {
        let Some(field) = T::FIELDS.iter().find(|field| field.name == name) else {
            continue;
        };
        if value.is_empty() && field.optional {
            continue;
        }
        kept.push((name, value));
    }
    for field in T::FIELDS {
        if kept.iter().any(|(name, _)| *name == field.name) || field.optional {
            continue;
        }
        match field.widget {
            Widget::Checkbox | Widget::Switch => kept.push((field.name, "false")),
            Widget::Input(_) | Widget::Textarea { .. } | Widget::Select | Widget::Radio | Widget::Money => kept.push((field.name, "")),
        }
    }
    // Serde stops at the first value of the wrong type. To name every such field,
    // take the one it names out and read again: a field that is gone reports
    // only at the end, as missing, which ends the loop.
    let mut wrong = FieldErrors::new();
    let value: T = loop {
        let encoded = encode(&kept);
        match serde_path_to_error::deserialize(urlencoded::Deserializer::new(encoded.as_bytes())) {
            Ok(value) if wrong.is_empty() => break value,
            Ok(_) => return Err(wrong),
            Err(error) => {
                let more = type_error::<T>(&error);
                let before = kept.len();
                let mut known = false;
                for name in more.keys() {
                    known |= T::FIELDS.iter().any(|field| field.name == name);
                    kept.retain(|(kept_name, _)| kept_name != name);
                }
                let progressed = kept.len() < before;
                if !known && !wrong.is_empty() {
                    // Only a field taken out above can be missing now.
                    return Err(wrong);
                }
                for (name, texts) in more {
                    wrong.entry(name).or_insert(texts);
                }
                if !known || !progressed {
                    return Err(wrong);
                }
            }
        }
    };
    match value.validate() {
        Ok(()) => Ok(value),
        Err(violations) => {
            let mut messages = Messages::default();
            for field in T::FIELDS {
                messages = messages.with_attribute(field.name, field.label);
            }
            let mut errors = FieldErrors::new();
            for (path, violation) in violations.iter() {
                let text = match T::FIELDS.iter().find(|field| field.name == path).and_then(|field| field.error) {
                    Some(text) => text.to_owned(),
                    None => violation.render(path, &messages),
                };
                let list = errors.entry(path.to_owned()).or_default();
                if !list.contains(&text) {
                    list.push(text);
                }
            }
            Err(errors)
        }
    }
}

/// What a value of the wrong type says.
fn type_error<T: Form>(error: &serde_path_to_error::Error<urlencoded::Error>) -> FieldErrors {
    let path = error.path().to_string();
    let field = T::FIELDS.iter().find(|field| field.name == path);
    let text = match field {
        Some(FormField { error: Some(text), .. }) => (*text).to_owned(),
        Some(FormField { widget: Widget::Money, label, .. }) => format!("{label}: enter an amount, like 12,500.50."),
        Some(FormField { widget: Widget::Input(InputKind::Number), label, .. }) => format!("{label}: enter a number."),
        Some(FormField { widget: Widget::Input(InputKind::Date), label, .. }) => format!("{label}: enter a date."),
        Some(FormField { widget: Widget::Select | Widget::Radio, label, .. }) => format!("{label}: choose one of the options."),
        Some(FormField { label, .. }) => format!("{label} is not valid."),
        None => "This form could not be read.".to_owned(),
    };
    let mut errors = FieldErrors::new();
    errors.insert(if field.is_some() { path } else { String::from("_form") }, vec![text]);
    errors
}

/// Encodes pairs for [`urlencoded`].
pub(crate) fn encode(pairs: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(pairs.iter().map(|(name, value)| name.len() + value.len() + 2).sum());
    for (at, (name, value)) in pairs.iter().enumerate() {
        if at > 0 {
            out.push('&');
        }
        encode_into(&mut out, name);
        out.push('=');
        encode_into(&mut out, value);
    }
    out
}

/// Percent-encodes `text` for a query string.
pub(crate) fn encode_into(out: &mut String, text: &str) {
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(char::from(byte)),
            _ => {
                out.push('%');
                out.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0').to_ascii_uppercase());
                out.push(char::from_digit(u32::from(byte & 15), 16).unwrap_or('0').to_ascii_uppercase());
            }
        }
    }
}

/// A form submission, decoded and validated.
///
/// Unlike a validated JSON body, a failure isn't a JSON 400. From the runtime
/// it answers ops that show the errors under the inputs; with JavaScript off
/// it answers the form again, with what was typed and the errors.
///
/// ```ignore
/// async fn store(Submit(input): Submit<NewDeal>) -> Reply {
///     Reply::redirect("/deals").toast(ToastTone::Success, "Deal created")
/// }
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Submit<T>(pub T);

/// What a failed [`Submit`] answers. By itself it draws the form on a plain
/// page. Take `Result<Submit<T>, Rejected<T>>` and call
/// [`respond`](Self::respond) to draw it in your own page, with its choices
/// from the database.
#[derive(Debug)]
pub struct Rejected<T> {
    values: Vec<(String, String)>,
    errors: FieldErrors,
    from_runtime: bool,
    action: String,
    too_large: bool,
    marker: PhantomData<fn() -> T>,
}

impl<T: Form> Rejected<T> {
    /// The errors by field.
    pub fn errors(&self) -> &FieldErrors {
        &self.errors
    }

    /// Ops for the runtime; with JavaScript off, what `page` makes of a form
    /// holding what was typed and the errors.
    pub fn respond<R: IntoResponse>(self, page: impl FnOnce(FormView<T>) -> R) -> Response {
        if self.too_large {
            return StatusCode::PAYLOAD_TOO_LARGE.into_response();
        }
        if self.from_runtime {
            let ops = vec![Op::Errors { form: T::ID.to_owned(), fields: self.errors }];
            return (StatusCode::UNPROCESSABLE_ENTITY, Envelope::new(ops)).into_response();
        }
        let form = FormView::<T>::new().post(self.action).with_values(self.values).with_errors(self.errors);
        let mut response = page(form).into_response();
        *response.status_mut() = StatusCode::UNPROCESSABLE_ENTITY;
        response
    }
}

impl<T: Form> IntoResponse for Rejected<T> {
    fn into_response(self) -> Response {
        self.respond(|form| {
            View::new("Check this form").body(el("main").slot("page").children((el("h1").slot("page-title").child("Check this form"), Node::from(form))))
        })
    }
}

impl<T> FromRequest for Submit<T>
where
    T: Form + DeserializeOwned + Validate + Send + 'static,
{
    type Rejection = Rejected<T>;

    async fn from_request(request: Request) -> Result<Self, Self::Rejection> {
        let (parts, body): (Parts, _) = request.into_parts();
        let from_runtime = parts.headers.contains_key(UI_HEADER);
        let action = parts.uri.path_and_query().map_or_else(|| parts.uri.path().to_owned(), |path| path.as_str().to_owned());
        let limit = BodyLimit::of(&parts.extensions);
        let bytes = if parts.method == Method::GET || parts.method == Method::HEAD {
            bytes::Bytes::copy_from_slice(parts.uri.query().unwrap_or_default().as_bytes())
        } else {
            match read_body(body, limit).await {
                Ok(bytes) => bytes,
                Err(_) => {
                    return Err(Rejected { values: Vec::new(), errors: FieldErrors::new(), from_runtime, action, too_large: true, marker: PhantomData });
                }
            }
        };
        let Some(values) = read_pairs::<T>(&bytes) else {
            return Err(Rejected { values: Vec::new(), errors: FieldErrors::new(), from_runtime, action, too_large: true, marker: PhantomData });
        };
        match decode::<T>(&values) {
            Ok(value) => Ok(Self(value)),
            Err(errors) => Err(Rejected { values: echo::<T>(values), errors, from_runtime, action, too_large: false, marker: PhantomData }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn money_reads_and_writes() {
        assert_eq!(Money::parse("12,500.50"), Some(Money(1_250_050)));
        assert_eq!(Money::parse("12500"), Some(Money(1_250_000)));
        assert_eq!(Money::parse("-3.5"), Some(Money(-350)));
        assert_eq!(Money::parse(".5"), Some(Money(50)));
        for bad in ["", ".", "1.234", "abc", "1e5", "--1", "99999999999999999999"] {
            assert_eq!(Money::parse(bad), None, "{bad}");
        }
        assert_eq!(Money(1_250_050).display(), "12,500.50");
        assert_eq!(Money(-350).display(), "-3.50");
        assert_eq!(Money(5).display(), "0.05");
        assert_eq!(Money(100_000_000).display(), "1,000,000.00");
        assert_eq!(Money::parse(&Money(i64::MAX).display()), Some(Money(i64::MAX)));
    }

    #[test]
    fn encoding_keeps_everything_a_decoder_reads_back() {
        let encoded = encode(&[("a b", "x&y=z/é+")]);
        let pairs: Vec<_> = urlencoded::parse(encoded.as_bytes()).collect();
        assert_eq!(pairs[0].0, "a b");
        assert_eq!(pairs[0].1, "x&y=z/é+");
    }
}
