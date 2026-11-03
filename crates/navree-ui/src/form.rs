//! Forms from a validated struct: [`Form`] (derived), [`FormView`] (draws it)
//! and [`Submit`] (reads it back).
//!
//! The struct is the one source: its doc comments are the labels, its types
//! pick the widgets, and its `#[validate]` rules set `required`, `maxlength`,
//! `min` and, when a rule says `browser`, `pattern` in the browser, and run again on
//! the server.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::marker::PhantomData;

use http::header::{ALLOW, CONTENT_TYPE, HOST, ORIGIN};
use http::request::Parts;
use http::{HeaderValue, Method, StatusCode};
use navree_http::HttpOptions;
use navree_http::body::{BodyLimit, read_body};
use navree_http::extract::{FromRequest, Request, urlencoded};
use navree_http::response::{IntoResponse, Response};
use navree_kernel::AppContext;
use navree_validation::{Messages, Rule, Validate};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};

pub use navree_validation::{FieldRules, RuleTable};

use crate::components::{
    Alert, AlertTone, Button, Checkbox, Field, Input, InputGroup, InputKind, NativeSelect,
    RadioGroup, Switch, Textarea,
};
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
    /// A styled listbox over a native select ([`Select`](crate::Select)).
    Listbox,
    /// A searchable list ([`Combobox`](crate::Combobox)): the options are the form's
    /// options, or are searched on the server with [`FormView::search`].
    Combobox,
    /// Many choices as chips ([`MultiSelect`](crate::MultiSelect)), for a field of the
    /// type [`Many`].
    Multi,
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
    /// The most values a [`Many`] field takes: the browser stops at it, and
    /// [`decode`] refuses a post that goes past it.
    pub max: Option<u32>,
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

    /// The texts of a value that is several: one for each. A single value gives its own.
    fn field_values(&self) -> Vec<String> {
        self.field_value().into_iter().collect()
    }
}

impl<T: FieldValue> FieldValue for Option<T> {
    fn field_value(&self) -> Option<String> {
        self.as_ref().and_then(FieldValue::field_value)
    }

    fn field_values(&self) -> Vec<String> {
        self.as_ref()
            .map(FieldValue::field_values)
            .unwrap_or_default()
    }
}

/// What separates the values of a [`Many`] while a form is read: a control character
/// no person types, so no value can hold it. A value that does is left out.
const SEPARATOR: char = '\u{1f}';

/// The most values one list field takes from a post.
pub const MAX_MANY: usize = 1000;

/// A field with several values, for a [`MultiSelect`](crate::MultiSelect): the form
/// reads every value posted under the field's name, in order, up to [`MAX_MANY`].
/// Nothing posted is an empty list, so no `#[serde(default)]` is needed.
///
/// The values are read as text: strings, enums with `#[derive(Options)]`, UUIDs and
/// dates. A list of numbers is a list of strings you parse.
///
/// ```
/// use navree_ui::form::Many;
///
/// let tags: Many<String> = vec!["a".to_owned(), "b".to_owned()].into();
/// assert_eq!(tags.len(), 2);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Many<T>(pub Vec<T>);

impl<T> Default for Many<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<T> From<Vec<T>> for Many<T> {
    fn from(values: Vec<T>) -> Self {
        Self(values)
    }
}

impl<T> std::ops::Deref for Many<T> {
    type Target = Vec<T>;

    fn deref(&self) -> &Vec<T> {
        &self.0
    }
}

impl<T> std::ops::DerefMut for Many<T> {
    fn deref_mut(&mut self) -> &mut Vec<T> {
        &mut self.0
    }
}

impl<T> IntoIterator for Many<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<T: FieldValue> FieldValue for Many<T> {
    fn field_value(&self) -> Option<String> {
        None
    }

    fn field_values(&self) -> Vec<String> {
        self.0.iter().flat_map(FieldValue::field_values).collect()
    }
}

impl<T: Serialize> Serialize for Many<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

/// Reads a [`Many`] from what a form holds (the values joined by the separator) and
/// from what it writes (a list), so it reads back what it serialized.
impl<'de, T: serde::de::DeserializeOwned> Deserialize<'de> for Many<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::{Error as _, IntoDeserializer, SeqAccess, Visitor};

        struct Values<T>(PhantomData<fn() -> T>);

        impl<'de, T: serde::de::DeserializeOwned> Visitor<'de> for Values<T> {
            type Value = Many<T>;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a list of values, or the values of a form field joined together")
            }

            fn visit_str<E: serde::de::Error>(self, text: &str) -> Result<Many<T>, E> {
                if text.is_empty() {
                    return Ok(Many(Vec::new()));
                }
                text.split(SEPARATOR)
                    .map(|piece| {
                        T::deserialize(
                            IntoDeserializer::<serde::de::value::Error>::into_deserializer(piece),
                        )
                        .map_err(E::custom)
                    })
                    .collect::<Result<Vec<T>, E>>()
                    .map(Many)
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Many<T>, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = seq.next_element::<T>()? {
                    if values.len() == MAX_MANY {
                        return Err(A::Error::custom("too many values"));
                    }
                    values.push(value);
                }
                Ok(Many(values))
            }
        }

        deserializer.deserialize_any(Values(PhantomData))
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
    String,
    bool,
    i8,
    i16,
    i32,
    i64,
    isize,
    u8,
    u16,
    u32,
    u64,
    usize,
    f32,
    f64,
    uuid::Uuid,
    chrono::NaiveDate,
    chrono::NaiveTime,
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
    /// Reads `12,500.50`, `12,34,567`, `12500` or `-3.5`. Commas separate groups:
    /// the last of three digits, the others of two or three. More than two
    /// decimals, or too large a number, is `None`.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let (negative, text) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
        if (whole.is_empty() && fraction.is_empty()) || fraction.len() > 2 || !grouped(whole) {
            return None;
        }
        let digits = whole.replace(',', "");
        if !digits
            .bytes()
            .chain(fraction.bytes())
            .all(|byte| byte.is_ascii_digit())
        {
            return None;
        }
        // Wider than the result, so the most negative amount reads back.
        let whole: i128 = if digits.is_empty() {
            0
        } else {
            digits.parse().ok()?
        };
        let fraction: i128 = match fraction.len() {
            0 => 0,
            1 => fraction.parse::<i128>().ok()? * 10,
            _ => fraction.parse().ok()?,
        };
        let cents = whole.checked_mul(100)?.checked_add(fraction)?;
        i64::try_from(if negative { -cents } else { cents })
            .ok()
            .map(Self)
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

/// Whether the commas in `whole`, if any, separate groups of digits.
fn grouped(whole: &str) -> bool {
    if !whole.contains(',') {
        return true;
    }
    let groups: Vec<&str> = whole.split(',').collect();
    let last = groups.len() - 1;
    groups.iter().enumerate().all(|(at, group)| match at {
        0 => (1..=3).contains(&group.len()),
        _ if at == last => group.len() == 3,
        _ => (2..=3).contains(&group.len()),
    })
}

impl FieldValue for Money {
    fn field_value(&self) -> Option<String> {
        Some(self.display())
    }
}

impl<'de> Deserialize<'de> for Money {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text)
            .ok_or_else(|| serde::de::Error::custom("enter an amount, like 12,500.50"))
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
    pattern: Option<&'static str>,
}

fn native<T: Form>(field: &FormField) -> Native {
    let mut out = Native::default();
    if !field.optional
        && !matches!(
            field.widget,
            Widget::Checkbox
                | Widget::Switch
                | Widget::Multi
                | Widget::Textarea { .. }
                | Widget::Input(
                    InputKind::Text
                        | InputKind::Email
                        | InputKind::Url
                        | InputKind::Tel
                        | InputKind::Password
                        | InputKind::Search
                        | InputKind::Hidden
                )
        )
    {
        out.required = true;
    }
    let Some(rules) = T::RULES.iter().find(|rules| rules.name == field.name) else {
        return out;
    };
    for rule in rules.rules {
        match rule {
            Rule::Required => out.required = true,
            // Native length counts UTF-16 units, so only a character count maps.
            Rule::Length {
                min,
                max,
                bytes: false,
            } => {
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
            // Only a pattern whose rule says `browser` reaches the page: a Rust regex
            // isn't always a browser's, and a pattern can say more than a page should.
            // The server checks every pattern again.
            Rule::BrowserPattern(pattern) => out.pattern = Some(pattern),
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
    searches: BTreeMap<&'static str, Href>,
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
            searches: BTreeMap::new(),
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
        self.values = value
            .values()
            .into_iter()
            .map(|(name, text)| (name.to_owned(), text))
            .collect();
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
    pub fn options(
        mut self,
        field: &'static str,
        options: impl IntoIterator<Item = (String, String)>,
    ) -> Self {
        self.options.insert(field, options.into_iter().collect());
        self
    }

    /// Searches a combobox or multi-select field on the server: the runtime asks `url`
    /// with `?q=text` and shows the answer, a [`ChoiceList`](crate::ChoiceList). Give
    /// the field's current choices with [`options`](Self::options), so an edit form can
    /// show their labels.
    pub fn search(mut self, field: &'static str, url: impl Into<Href>) -> Self {
        self.searches.insert(field, url.into());
        self
    }

    /// Puts every field on its own line.
    pub fn one_column(mut self) -> Self {
        self.columns = false;
        self
    }

    /// Every value the form holds for `name`: a field that is a list has several.
    fn values_of(&self, name: &str) -> Vec<String> {
        self.values
            .iter()
            .filter(|(field, _)| field == name)
            .map(|(_, value)| value.clone())
            .collect()
    }

    /// The choices of a field: the form's own, or the ones the field's type lists.
    fn choices(&self, field: &FormField) -> Vec<crate::select::Choice> {
        match self.options.get(field.name) {
            Some(options) => options
                .iter()
                .map(|(value, label)| crate::select::Choice::new(value.clone(), label.clone()))
                .collect(),
            None => field
                .options
                .iter()
                .map(|(value, label)| crate::select::Choice::new(*value, *label))
                .collect(),
        }
    }

    fn picker_id(field: &FormField) -> String {
        format!("{}-{}-picker", T::ID, field.name)
    }

    fn value_of(&self, name: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value.as_str())
    }

    fn control(&self, field: &FormField) -> Node {
        let rules = native::<T>(field);
        let value = self.value_of(field.name);
        let invalid = self.errors.contains_key(field.name);
        match field.widget {
            Widget::Input(kind) => {
                let kind = rules
                    .kind
                    .filter(|_| kind == InputKind::Text)
                    .unwrap_or(kind);
                let mut input = Input::new(kind, field.name).invalid(invalid);
                if kind == InputKind::Number && rules.min.is_none_or(|min| min.contains('.')) {
                    input = input.attr("step", "any");
                }
                self.finish_input(input, field, &rules, value).into()
            }
            Widget::Money => self
                .finish_input(
                    Input::text(field.name)
                        .inputmode("decimal")
                        .invalid(invalid),
                    field,
                    &rules,
                    value,
                )
                .into(),
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
            Widget::Listbox => {
                let mut select = crate::select::Select::new(field.name)
                    .id(Self::picker_id(field))
                    .options(self.choices(field))
                    .required_if(rules.required)
                    .invalid(invalid);
                if let Some(prompt) = field.prompt.or(field.placeholder) {
                    select = select.placeholder(prompt);
                }
                if let Some(value) = value {
                    select = select.selected(value.to_owned());
                }
                select.into()
            }
            Widget::Combobox => {
                let mut combo = crate::select::Combobox::new(field.name)
                    .id(Self::picker_id(field))
                    .options(self.choices(field))
                    .required_if(rules.required)
                    .invalid(invalid);
                if let Some(prompt) = field.placeholder.or(field.prompt) {
                    combo = combo.placeholder(prompt);
                }
                if let Some(url) = self.searches.get(field.name) {
                    combo = combo.search(url.clone());
                }
                if let Some(value) = value {
                    combo = combo.selected(value.to_owned());
                }
                combo.into()
            }
            Widget::Multi => {
                let mut multi = crate::select::MultiSelect::new(field.name)
                    .id(Self::picker_id(field))
                    .options(self.choices(field))
                    .required_if(rules.required)
                    .invalid(invalid)
                    .selected_values(self.values_of(field.name));
                if let Some(prompt) = field.placeholder.or(field.prompt) {
                    multi = multi.placeholder(prompt);
                }
                if let Some(url) = self.searches.get(field.name) {
                    multi = multi.search(url.clone());
                }
                if let Some(max) = field.max {
                    multi = multi.max(max);
                }
                multi.into()
            }
            Widget::Radio => {
                let mut group = RadioGroup::new(field.name, field.label).options(
                    field
                        .options
                        .iter()
                        .map(|(value, label)| crate::components::OptionItem::new(*value, *label)),
                );
                if let Some(value) = value {
                    group = group.selected(value.to_owned());
                }
                if rules.required {
                    group = group.required();
                }
                group.into()
            }
            Widget::Checkbox => Checkbox::new(field.name)
                .checked(is_on(value))
                .label(field.label)
                .into(),
            Widget::Switch => Switch::new(field.name)
                .checked(is_on(value))
                .label(field.label)
                .into(),
        }
    }

    fn finish_input(
        &self,
        mut input: Input,
        field: &FormField,
        rules: &Native,
        value: Option<&str>,
    ) -> Input {
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
        if let Some(pattern) = rules.pattern {
            input = input.attr("pattern", pattern);
        }
        input
    }

    fn select(&self, field: &FormField, required: bool, value: Option<&str>) -> NativeSelect {
        let mut select = NativeSelect::new(field.name);
        if let Some(prompt) = field.prompt {
            select = select.prompt(prompt);
        }
        if let Some(options) = self.options.get(field.name) {
            select = select.options(options.iter().map(|(value, label)| {
                crate::components::OptionItem::new(value.clone(), label.clone())
            }));
        } else {
            select = select.options(
                field
                    .options
                    .iter()
                    .map(|(value, label)| crate::components::OptionItem::new(*value, *label)),
            );
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
        if T::FIELDS
            .iter()
            .any(|other| other.addon == Some(field.name))
        {
            return None;
        }
        let error = self
            .errors
            .get(field.name)
            .map(|messages| messages.join(" "));
        let id = format!("{}-{}", T::ID, field.name);
        let holder = match field.widget {
            Widget::Input(InputKind::Hidden) => return Some(input_only(self.control(field))),
            Widget::Checkbox | Widget::Switch => {
                let error_id = error.as_ref().map(|_| format!("{id}-error"));
                let mut box_el = el("div")
                    .slot("field")
                    .flag("data-invalid", error.is_some())
                    .child(with_id(self.control(field), &id));
                if let Some(help) = field.help {
                    box_el = box_el.child(el("p").slot("field-description").child(help));
                }
                box_el.child(error.map(|text| {
                    el("p")
                        .slot("field-error")
                        .attr_opt("id", error_id)
                        .child(text)
                }))
            }
            Widget::Radio => {
                let box_el = el("div")
                    .slot("field")
                    .flag("data-invalid", error.is_some())
                    .child(self.control(field));
                box_el.child(error.map(|text| el("p").slot("field-error").child(text)))
            }
            _ => {
                let mut control = self.control(field);
                if let Some(addon) = field
                    .addon
                    .and_then(|name| T::FIELDS.iter().find(|other| other.name == name))
                {
                    let mut select = self.control(addon);
                    if let Node::Element(element) = &mut select {
                        *element =
                            std::mem::replace(element, el("x")).attr("aria-label", addon.label);
                    }
                    control = InputGroup::new(control).end(select).into();
                }
                let mut box_field = Field::new(id, field.label)
                    .error_opt(error)
                    .control(control);
                if let Some(help) = field.help {
                    box_field = box_field.help(help);
                }
                let box_node: Node = box_field.into();
                box_node.into_element()?
            }
        };
        Some(holder.flag("data-wide", field.wide))
    }

    fn into_node(self) -> Node {
        let boxes: Vec<Element> = T::FIELDS
            .iter()
            .filter_map(|field| self.field(field))
            .collect();
        let grid = if self.columns {
            "form-grid"
        } else {
            "form-stack"
        };
        // An error no field can show: the runtime makes these toasts.
        let whole: Vec<Alert> = self
            .errors
            .iter()
            .filter(|(name, _)| !T::FIELDS.iter().any(|field| field.name == *name))
            .map(|(_, messages)| Alert::new(AlertTone::Danger).title(messages.join(" ")))
            .collect();
        el("form")
            .slot("form")
            .attr("id", T::ID)
            .attr("method", self.method)
            .attr_opt("action", self.action.map(AttrHref))
            .flag("data-ui-form", true)
            .uses(Uses::CORE)
            .children((
                self.csrf.map(csrf_field),
                whole,
                el("div").slot(grid).child(boxes),
                el("div").slot("form-actions").children((
                    self.cancel
                        .map(|(label, href)| Button::ghost(label).href(href)),
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
    el("input")
        .attr("type", "hidden")
        .attr("name", CSRF_FIELD)
        .attr("value", token)
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
    control.into_element().unwrap_or_else(|| el("div"))
}

/// Sets the id of the input inside `node`, for a label that holds it.
fn with_id(mut node: Node, id: &str) -> Node {
    fn walk(node: &mut Node, id: &str) -> bool {
        if let Node::Element(element) = node {
            if element.tag == "input" {
                element.attrs.push(crate::node::Attr {
                    name: "id",
                    value: crate::node::AttrValue::Text(id.into()),
                });
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
            let secret = T::FIELDS.iter().any(|field| {
                field.name == name && field.widget == Widget::Input(InputKind::Password)
            });
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
    let over = over_limit::<T>(pairs);
    match decode_values::<T>(pairs) {
        Ok(value) if over.is_empty() => Ok(value),
        Ok(_) => Err(over),
        Err(mut errors) => {
            for (name, texts) in over {
                errors.entry(name).or_insert(texts);
            }
            Err(errors)
        }
    }
}

/// The values a list field holds in a post: not empty, and none that holds the
/// separator, as [`decode`] reads them.
fn list_values<'a>(pairs: &'a [(String, String)], name: &'a str) -> impl Iterator<Item = &'a str> {
    pairs
        .iter()
        .filter(move |(field, _)| field == name)
        .map(|(_, value)| value.as_str())
        .filter(|value| !value.is_empty() && !value.contains(SEPARATOR))
}

/// The list fields that took more values than their limit. A limit the browser keeps
/// is only a convenience, so the post is held to it too: with JavaScript off, this is
/// the only check.
fn over_limit<T: Form>(pairs: &[(String, String)]) -> FieldErrors {
    let mut errors = FieldErrors::new();
    for field in T::FIELDS {
        let (Widget::Multi, Some(max)) = (field.widget, field.max) else {
            continue;
        };
        if list_values(pairs, field.name).count() > max as usize {
            errors.insert(
                field.name.to_owned(),
                vec![format!("Choose at most {max}.")],
            );
        }
    }
    errors
}

fn decode_values<T>(pairs: &[(String, String)]) -> Result<T, FieldErrors>
where
    T: Form + DeserializeOwned + Validate,
{
    // A list field's values are read together: the deserializer takes a name once.
    let lists: Vec<(&'static str, String)> = T::FIELDS
        .iter()
        .filter(|field| field.widget == Widget::Multi)
        .map(|field| {
            let mut joined = String::new();
            let mut count = 0;
            for (_, value) in pairs.iter().filter(|(name, _)| name == field.name) {
                if value.is_empty() || value.contains(SEPARATOR) {
                    continue;
                }
                if count == MAX_MANY {
                    break;
                }
                if count > 0 {
                    joined.push(SEPARATOR);
                }
                joined.push_str(value);
                count += 1;
            }
            (field.name, joined)
        })
        .collect();
    let mut kept: Vec<(&str, &str)> = Vec::with_capacity(pairs.len() + T::FIELDS.len());
    for (name, value) in pairs {
        let Some(field) = T::FIELDS.iter().find(|field| field.name == name) else {
            continue;
        };
        if field.widget == Widget::Multi {
            continue;
        }
        if value.is_empty() && field.optional {
            continue;
        }
        kept.push((name, value));
    }
    for (name, joined) in &lists {
        if !(joined.is_empty() && T::FIELDS.iter().any(|f| f.name == *name && f.optional)) {
            kept.push((name, joined));
        }
    }
    for field in T::FIELDS {
        if kept.iter().any(|(name, _)| *name == field.name) || field.optional {
            continue;
        }
        match field.widget {
            Widget::Checkbox | Widget::Switch => kept.push((field.name, "false")),
            Widget::Input(_)
            | Widget::Textarea { .. }
            | Widget::Select
            | Widget::Listbox
            | Widget::Combobox
            | Widget::Radio
            | Widget::Money => kept.push((field.name, "")),
            // Read above, with the values it has.
            Widget::Multi => {}
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
                let text = match T::FIELDS
                    .iter()
                    .find(|field| field.name == path)
                    .and_then(|field| field.error)
                {
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
        Some(FormField {
            error: Some(text), ..
        }) => (*text).to_owned(),
        Some(FormField {
            widget: Widget::Money,
            label,
            ..
        }) => format!("{label}: enter an amount, like 12,500.50."),
        Some(FormField {
            widget: Widget::Input(InputKind::Number),
            label,
            ..
        }) => format!("{label}: enter a number."),
        Some(FormField {
            widget: Widget::Input(InputKind::Date),
            label,
            ..
        }) => format!("{label}: enter a date."),
        Some(FormField {
            widget:
                Widget::Select | Widget::Listbox | Widget::Combobox | Widget::Multi | Widget::Radio,
            label,
            ..
        }) => format!("{label}: choose one of the options."),
        Some(FormField { label, .. }) => format!("{label} is not valid."),
        None => "This form could not be read.".to_owned(),
    };
    let mut errors = FieldErrors::new();
    errors.insert(
        if field.is_some() {
            path
        } else {
            String::from("_form")
        },
        vec![text],
    );
    errors
}

/// Encodes pairs for [`urlencoded`].
pub(crate) fn encode(pairs: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(
        pairs
            .iter()
            .map(|(name, value)| name.len() + value.len() + 2)
            .sum(),
    );
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
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(byte))
            }
            _ => {
                out.push('%');
                out.push(
                    char::from_digit(u32::from(byte >> 4), 16)
                        .unwrap_or('0')
                        .to_ascii_uppercase(),
                );
                out.push(
                    char::from_digit(u32::from(byte & 15), 16)
                        .unwrap_or('0')
                        .to_ascii_uppercase(),
                );
            }
        }
    }
}

/// Whether the browser says this request came from this site. Browsers state it in
/// `Sec-Fetch-Site`, which no page can set, or in `Origin` when they predate that or
/// the page isn't on HTTPS. A client that sends neither (curl, a test) isn't a browser
/// acting for a page. A sibling subdomain is another site here: it can set cookies,
/// but it shouldn't be able to post.
fn from_this_site(parts: &Parts) -> bool {
    if let Some(site) = parts.headers.get("sec-fetch-site") {
        return matches!(site.as_bytes(), b"same-origin" | b"none");
    }
    let Some(origin) = parts.headers.get(ORIGIN) else {
        return true;
    };
    let host = parts
        .headers
        .get(HOST)
        .and_then(|host| host.to_str().ok())
        .or_else(|| parts.uri.authority().map(http::uri::Authority::as_str));
    let authority = origin
        .to_str()
        .ok()
        .and_then(|origin| origin.split_once("://"))
        .map(|(_, authority)| authority);
    matches!((host, authority), (Some(host), Some(authority)) if host.eq_ignore_ascii_case(authority))
}

/// Whether the request's `Origin` is a front end the app trusts to write from
/// another site: `http.cors.allowed_origins`, the list the session layer also reads.
async fn from_a_trusted_front_end(parts: &Parts) -> bool {
    let Some(origin) = parts
        .headers
        .get(ORIGIN)
        .and_then(|origin| origin.to_str().ok())
    else {
        return false;
    };
    let origin = origin.trim().trim_end_matches('/');
    let Some(context) = parts.extensions.get::<AppContext>() else {
        return false;
    };
    let Some(options) = context.try_get::<HttpOptions>().await else {
        return false;
    };
    options
        .cors
        .allowed_origins
        .iter()
        .any(|allowed| allowed.trim().trim_end_matches('/') == origin)
}

/// Whether the request holds a urlencoded form.
fn is_form_body(parts: &Parts) -> bool {
    parts
        .headers
        .get(CONTENT_TYPE)
        .and_then(|kind| kind.to_str().ok())
        .and_then(|kind| kind.split(';').next())
        .is_some_and(|kind| {
            kind.trim()
                .eq_ignore_ascii_case("application/x-www-form-urlencoded")
        })
}

/// Why a request isn't a form submission at all, before its body is read.
pub(crate) async fn refusal(parts: &Parts) -> Option<StatusCode> {
    if matches!(parts.method, Method::GET | Method::HEAD) {
        return Some(StatusCode::METHOD_NOT_ALLOWED);
    }
    if !from_this_site(parts) && !from_a_trusted_front_end(parts).await {
        return Some(StatusCode::FORBIDDEN);
    }
    if !is_form_body(parts) {
        return Some(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }
    None
}

/// A form submission, decoded and validated.
///
/// Unlike a validated JSON body, a failure isn't a JSON 400. From the runtime
/// it answers ops that show the errors under the inputs; with JavaScript off
/// it answers the form again, with what was typed and the errors.
///
/// It has its own answer to cross-site request forgery, with or without a session:
/// a body from another site is refused with 403 unless its `Origin` is in
/// `http.cors.allowed_origins`, a body that isn't a urlencoded form with 415, and a
/// `GET` or `HEAD` with 405, so no link or prefetch can fill one. Behind navree-session
/// the token check also runs.
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
pub struct Rejected<T> {
    values: Vec<(String, String)>,
    errors: FieldErrors,
    from_runtime: bool,
    action: String,
    refused: Option<StatusCode>,
    marker: PhantomData<fn() -> T>,
}

/// Names only: what a form holds (a password, a token) stays out of logs.
impl<T> std::fmt::Debug for Rejected<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self.values.iter().map(|(name, _)| name.as_str()).collect();
        f.debug_struct("Rejected")
            .field("fields", &names)
            .field("errors", &self.errors)
            .field("from_runtime", &self.from_runtime)
            .field("refused", &self.refused)
            .finish()
    }
}

impl<T: Form> Rejected<T> {
    /// A refusal of a form that decoded, for a reason found afterwards: a value that
    /// isn't among the options the caller may pick (see `Choice::all_among` and
    /// `OptionSource::all_pickable`), a name that is taken. It answers as a failed
    /// rule does: the runtime gets ops that show `errors` under the fields, and
    /// without JavaScript the form comes back with what was sent. Take the request's
    /// `Parts` beside `Submit` to give it.
    ///
    /// ```ignore
    /// async fn store(parts: Parts, Submit(deal): Submit<NewDeal>, uow: Uow) -> Result<Response> {
    ///     if !PEOPLE.all_pickable(Person::query(), &deal.owner_id, &uow, OptionSource::as_int).await? {
    ///         let errors = FieldErrors::from([("owner_id".to_owned(), vec!["Choose from the list.".to_owned()])]);
    ///         return Ok(Rejected::after_decode(&deal, errors, &parts).into_response());
    ///     }
    ///     // ...
    /// }
    /// ```
    pub fn after_decode(value: &T, errors: FieldErrors, parts: &Parts) -> Self {
        let values = value
            .values()
            .into_iter()
            .map(|(name, text)| (name.to_owned(), text))
            .collect();
        Self {
            values: echo::<T>(values),
            errors,
            from_runtime: parts.headers.contains_key(UI_HEADER),
            action: parts.uri.path_and_query().map_or_else(
                || parts.uri.path().to_owned(),
                |path| path.as_str().to_owned(),
            ),
            refused: None,
            marker: PhantomData,
        }
    }

    /// The errors by field.
    pub fn errors(&self) -> &FieldErrors {
        &self.errors
    }

    /// Ops for the runtime; with JavaScript off, what `page` makes of a form
    /// holding what was typed and the errors.
    pub fn respond<R: IntoResponse>(self, page: impl FnOnce(FormView<T>) -> R) -> Response {
        if let Some(status) = self.refused {
            let mut response = status.into_response();
            if status == StatusCode::METHOD_NOT_ALLOWED {
                response
                    .headers_mut()
                    .insert(ALLOW, HeaderValue::from_static("POST"));
            }
            return response;
        }
        if self.from_runtime {
            let ops = vec![Op::Errors {
                form: T::ID.to_owned(),
                fields: self.errors,
            }];
            return (StatusCode::UNPROCESSABLE_ENTITY, Envelope::new(ops)).into_response();
        }
        let form = FormView::<T>::new()
            .post(self.action)
            .with_values(self.values)
            .with_errors(self.errors);
        let mut response = page(form).into_response();
        *response.status_mut() = StatusCode::UNPROCESSABLE_ENTITY;
        response
    }
}

impl<T: Form> IntoResponse for Rejected<T> {
    fn into_response(self) -> Response {
        self.respond(|form| {
            View::new("Check this form").body(el("main").slot("page").children((
                el("h1").slot("page-title").child("Check this form"),
                Node::from(form),
            )))
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
        let action = parts.uri.path_and_query().map_or_else(
            || parts.uri.path().to_owned(),
            |path| path.as_str().to_owned(),
        );
        let refuse = |status: StatusCode, action: String| Rejected {
            values: Vec::new(),
            errors: FieldErrors::new(),
            from_runtime,
            action,
            refused: Some(status),
            marker: PhantomData,
        };
        if let Some(status) = refusal(&parts).await {
            return Err(refuse(status, action));
        }
        let limit = BodyLimit::of(&parts.extensions);
        let Ok(bytes) = read_body(body, limit).await else {
            return Err(refuse(StatusCode::PAYLOAD_TOO_LARGE, action));
        };
        let Some(values) = read_pairs::<T>(&bytes) else {
            return Err(refuse(StatusCode::PAYLOAD_TOO_LARGE, action));
        };
        match decode::<T>(&values) {
            Ok(value) => Ok(Self(value)),
            Err(errors) => Err(Rejected {
                values: echo::<T>(values),
                errors,
                from_runtime,
                action,
                refused: None,
                marker: PhantomData,
            }),
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
        for bad in [
            "",
            ".",
            "1.234",
            "abc",
            "1e5",
            "--1",
            "99999999999999999999",
        ] {
            assert_eq!(Money::parse(bad), None, "{bad}");
        }
        assert_eq!(Money(1_250_050).display(), "12,500.50");
        assert_eq!(Money(-350).display(), "-3.50");
        assert_eq!(Money(5).display(), "0.05");
        assert_eq!(Money(100_000_000).display(), "1,000,000.00");
        assert_eq!(
            Money::parse(&Money(i64::MAX).display()),
            Some(Money(i64::MAX))
        );
    }

    #[test]
    fn a_list_field_reads_back_what_it_serialized_and_what_a_form_holds() {
        let tags: Many<String> = vec!["a".to_owned(), "b c".to_owned()].into();
        let json = serde_json::to_string(&tags).unwrap();
        assert_eq!(json, r#"["a","b c"]"#);
        let back: Many<String> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, tags, "a list reads back");
        let empty: Many<String> = serde_json::from_str("[]").unwrap();
        assert!(empty.is_empty());
        // A form's own text: the values joined by the separator.
        let joined: Many<String> = serde_json::from_str(r#""a\u001fb""#).unwrap();
        assert_eq!(joined.0, ["a", "b"]);
        assert!(serde_json::from_str::<Many<String>>("12").is_err());
        let too_many = format!("[{}]", vec!["\"x\""; MAX_MANY + 1].join(","));
        assert!(serde_json::from_str::<Many<String>>(&too_many).is_err());
    }

    #[test]
    fn encoding_keeps_everything_a_decoder_reads_back() {
        let encoded = encode(&[("a b", "x&y=z/é+")]);
        let pairs: Vec<_> = urlencoded::parse(encoded.as_bytes()).collect();
        assert_eq!(pairs[0].0, "a b");
        assert_eq!(pairs[0].1, "x&y=z/é+");
    }
}
