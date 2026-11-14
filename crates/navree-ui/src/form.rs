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
#[cfg(feature = "storage")]
use std::sync::Arc;

use chrono::{NaiveDate, NaiveTime};
use http::header::{ALLOW, CONTENT_TYPE, HOST, ORIGIN};
use http::request::Parts;
use http::{HeaderValue, Method, StatusCode};
use navree_http::body::{BodyLimit, read_body};
use navree_http::extract::{FromRequest, Request, urlencoded};
use navree_http::response::{IntoResponse, Response};
use navree_http::{AppError, HttpOptions};
use navree_kernel::AppContext;
use navree_validation::{AppMessages, Rule, Validate, Violations, check_async_rules};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};

pub use navree_validation::{FieldRules, RuleTable};

use crate::charts::Grouping;
use crate::components::{
    Alert, AlertTone, Button, Checkbox, Field, Input, InputGroup, InputKind, NativeSelect,
    RadioGroup, Switch, Textarea,
};
use crate::dates::{DateLocale, DatePicker, DateRules, TimePicker, TimeRules};
use crate::html::Href;
use crate::localize::ReaderWords;
use crate::node::{Element, Node, Uses, el};
use crate::op::{Envelope, Op};
use crate::shell::AppShell;
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
    /// A date from a calendar in a popover, typed by the locale's rules; a native
    /// date field with JavaScript off.
    DatePicker,
    /// A time of day typed or chosen from a list; a native time field with
    /// JavaScript off.
    TimePicker,
    /// A phone number with a country picker (a [`PhoneInput`](crate::PhoneInput)
    /// with the `phone` feature, else a `tel` input). It posts the number as
    /// typed and the country as `{name}_country`, and the server reads the
    /// number in that country.
    Phone,
    /// The phone input on a text field (`#[form(widget = phone)]` on a
    /// `String`): the field gets the number in E.164, and a number that isn't
    /// valid, or has an extension (which E.164 has no room for), is refused.
    PhoneText,
    /// A [`Slider`](crate::Slider) on a number (`#[form(widget = slider)]`). Its grid
    /// is the field's `slider` function, or the field's `range` rule (0 to 100 without
    /// one) in steps of 1, and [`decode`] refuses a value off the grid.
    Slider,
    /// A file, drawn as an [`Upload`](crate::Upload) (`#[form(widget = file)]` on a
    /// `String` or an `Option<String>`): the field gets the stored file's path on its
    /// disk. With [`FormView::upload`] and [`Upload::send_to`](crate::Upload::send_to)
    /// the module sends a chosen file at once and the form posts its key
    /// ([`file_key`](crate::upload::file_key)); with no script the form is a multipart
    /// form and [`Submit`] stores the file through the app's `Uploads`. A key that was
    /// not signed by the app, or has expired, is refused.
    File,
}

impl Widget {
    /// Whether it is the phone input, on a phone type or on text.
    pub fn is_phone(self) -> bool {
        matches!(self, Widget::Phone | Widget::PhoneText)
    }
}

/// One field of a [`Form`]. The derive writes these, with [`new`](Self::new) and its setters:
/// the struct is non-exhaustive, so a field can be added without breaking a form that was
/// derived before it.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
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
    /// A `chrono::DateTime<Utc>`: the date and time a browser posts have no zone, and are
    /// read as UTC. A value posted with its own offset is read with it, and the year is
    /// held to the calendar in UTC too.
    pub instant: bool,
    /// A `Decimal`: a number with more than 28 decimal places is refused, not rounded.
    pub decimal: bool,
    /// A date picker's rules, from `#[form(date_rules = path)]`: the function that gives the
    /// days that can't be chosen. The picker draws them for the browser, and [`decode`]
    /// holds the posted value to them, so the rules are written once and a value drawn as
    /// off is refused by the server. A `custom` rule is for what the rules can't say.
    pub date_rules: Option<fn() -> crate::dates::DateRules>,
    /// A time picker's rules, from `#[form(time_rules = path)]`, used the same way.
    pub time_rules: Option<fn() -> crate::dates::TimeRules>,
    /// A slider's grid, from `#[form(slider = path)]`: the function that gives its
    /// minimum, maximum and step. The slider draws it for the browser, and [`decode`]
    /// holds the posted value to it.
    pub slider: Option<fn() -> crate::flow::SliderSpec>,
}

impl FormField {
    /// A field named `name` (as it posts), labelled `label` and drawn as `widget`, with no
    /// hint, rule or choice. The setters below are for the rest.
    pub const fn new(name: &'static str, label: &'static str, widget: Widget) -> Self {
        Self {
            name,
            label,
            widget,
            optional: false,
            placeholder: None,
            help: None,
            prompt: None,
            error: None,
            addon: None,
            autofocus: false,
            wide: false,
            options: &[],
            max: None,
            instant: false,
            decimal: false,
            date_rules: None,
            time_rules: None,
            slider: None,
        }
    }

    /// Sets whether it may be left empty.
    pub const fn optional(mut self, optional: bool) -> Self {
        self.optional = optional;
        self
    }

    /// Sets the hint while empty.
    pub const fn placeholder(mut self, placeholder: Option<&'static str>) -> Self {
        self.placeholder = placeholder;
        self
    }

    /// Sets the help below it.
    pub const fn help(mut self, help: Option<&'static str>) -> Self {
        self.help = help;
        self
    }

    /// Sets a select's empty first choice.
    pub const fn prompt(mut self, prompt: Option<&'static str>) -> Self {
        self.prompt = prompt;
        self
    }

    /// Sets the message for any failed rule.
    pub const fn error(mut self, error: Option<&'static str>) -> Self {
        self.error = error;
        self
    }

    /// Sets the field drawn inside this one's input group.
    pub const fn addon(mut self, addon: Option<&'static str>) -> Self {
        self.addon = addon;
        self
    }

    /// Sets whether it takes focus when the page loads.
    pub const fn autofocus(mut self, autofocus: bool) -> Self {
        self.autofocus = autofocus;
        self
    }

    /// Sets whether it takes the whole width of the form.
    pub const fn wide(mut self, wide: bool) -> Self {
        self.wide = wide;
        self
    }

    /// Sets a select's or radio group's choices.
    pub const fn options(mut self, options: &'static [(&'static str, &'static str)]) -> Self {
        self.options = options;
        self
    }

    /// Sets the most values a [`Many`] field takes.
    pub const fn max(mut self, max: Option<u32>) -> Self {
        self.max = max;
        self
    }

    /// Sets whether it is a `DateTime<Utc>`.
    pub const fn instant(mut self, instant: bool) -> Self {
        self.instant = instant;
        self
    }

    /// Sets whether it is a `Decimal`.
    pub const fn decimal(mut self, decimal: bool) -> Self {
        self.decimal = decimal;
        self
    }

    /// Sets a date picker's rules.
    pub const fn date_rules(mut self, rules: Option<fn() -> crate::dates::DateRules>) -> Self {
        self.date_rules = rules;
        self
    }

    /// Sets a time picker's rules.
    pub const fn time_rules(mut self, rules: Option<fn() -> crate::dates::TimeRules>) -> Self {
        self.time_rules = rules;
        self
    }

    /// Sets a slider's grid.
    pub const fn slider(mut self, grid: Option<fn() -> crate::flow::SliderSpec>) -> Self {
        self.slider = grid;
        self
    }
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

    /// The database rules (`unique`, `exists`) [`Submit`] runs once the form's own rules
    /// pass, in the request's unit of work, as `ValidAsync` runs them for an API: a
    /// failed one is a field error under its field, in the app's words. `#[derive(Form)]`
    /// gives the type's own `navree_validation::AsyncRules` here, so a struct that
    /// implements them needs nothing more (a generic struct doesn't get them). None by
    /// default.
    fn database_rules(&self) -> Vec<navree_validation::AsyncRule> {
        Vec::new()
    }
}

/// What `#[derive(Form)]` uses to find a type's `AsyncRules`: the rules of a type
/// that implements them, and none for one that doesn't. Not public API.
#[doc(hidden)]
pub mod __rules {
    pub use navree_validation::AsyncRule;
    use navree_validation::AsyncRules;

    /// A form's value, asked for its database rules.
    pub struct Probe<'a, T>(pub &'a T);

    /// Chosen when the type implements `AsyncRules`.
    pub trait ViaRules {
        /// The type's rules.
        fn navree_database_rules(&self) -> Vec<AsyncRule>;
    }

    impl<T: AsyncRules> ViaRules for Probe<'_, T> {
        fn navree_database_rules(&self) -> Vec<AsyncRule> {
            self.0.async_rules()
        }
    }

    /// Chosen otherwise.
    pub trait NoRules {
        /// No rules.
        fn navree_database_rules(&self) -> Vec<AsyncRule>;
    }

    impl<T> NoRules for &Probe<'_, T> {
        fn navree_database_rules(&self) -> Vec<AsyncRule> {
            Vec::new()
        }
    }
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
    rust_decimal::Decimal,
);

#[cfg(feature = "phone")]
impl FieldValue for navree_validation::Phone {
    fn field_value(&self) -> Option<String> {
        Some(self.as_str().to_owned())
    }
}

#[cfg(feature = "phone")]
impl FieldValue for navree_phone::PhoneNumber {
    fn field_value(&self) -> Option<String> {
        Some(self.as_str().to_owned())
    }
}

/// A date and time as the browser's field holds it: to the minute, or to the second when
/// the seconds are not zero, so editing a value does not drop them.
fn moment_text(at: &chrono::NaiveDateTime) -> String {
    use chrono::Timelike;
    let format = if at.second() == 0 && at.nanosecond() == 0 {
        "%Y-%m-%dT%H:%M"
    } else {
        "%Y-%m-%dT%H:%M:%S"
    };
    at.format(format).to_string()
}

impl FieldValue for chrono::NaiveDateTime {
    fn field_value(&self) -> Option<String> {
        Some(moment_text(self))
    }
}

/// An instant is shown in UTC, and read back as UTC.
impl FieldValue for chrono::DateTime<chrono::Utc> {
    fn field_value(&self) -> Option<String> {
        Some(moment_text(&self.naive_utc()))
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
        self.display_grouped(Grouping::Thousands)
    }

    /// The amount with its digits grouped as a reader of a language groups them:
    /// `123456750` is `1,234,567.50` with [`Grouping::Thousands`] and `12,34,567.50`
    /// with [`Grouping::Indian`] (lakhs and crores). [`Grouping::None`] writes no commas.
    /// [`Money::parse`] reads all three back.
    ///
    /// ```
    /// use navree_ui::Money;
    /// use navree_ui::charts::Grouping;
    ///
    /// assert_eq!(Money(123_456_750).display_grouped(Grouping::Indian), "12,34,567.50");
    /// assert_eq!(Money(1_000_000_000).display_grouped(Grouping::Indian), "1,00,00,000.00");
    /// assert_eq!(Money(123_456_750).display_grouped(Grouping::Thousands), "1,234,567.50");
    /// ```
    pub fn display_grouped(self, grouping: Grouping) -> String {
        let cents = self.0.unsigned_abs();
        let whole = (cents / 100).to_string();
        let mut out = String::with_capacity(whole.len() + whole.len() / 2 + 4);
        if self.0 < 0 {
            out.push('-');
        }
        for (at, digit) in whole.chars().enumerate() {
            if at > 0 && starts_group(whole.len() - at, grouping) {
                out.push(',');
            }
            out.push(digit);
        }
        let _ = write!(out, ".{:02}", cents % 100);
        out
    }
}

/// Whether a comma goes before a digit with `left` digits from it to the end of the
/// whole part (itself included).
fn starts_group(left: usize, grouping: Grouping) -> bool {
    match grouping {
        Grouping::Thousands => left.is_multiple_of(3),
        // Three digits, then pairs: 12,34,567.
        Grouping::Indian => left == 3 || (left > 3 && (left - 3).is_multiple_of(2)),
        _ => false,
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

fn native_in(rules: &[FieldRules], field: &FormField) -> Native {
    let mut out = Native::default();
    if !field.optional
        && !matches!(
            field.widget,
            Widget::Checkbox
                | Widget::Switch
                | Widget::Multi
                | Widget::Phone
                | Widget::PhoneText
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
    let Some(rules) = rules.iter().find(|rules| rules.name == field.name) else {
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
            Rule::Format("phone") => out.kind = Some(InputKind::Tel),
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

/// What a form is drawn from: a [`Form`]'s id, fields and rules, or those of a form
/// described at run time.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FormShape {
    pub(crate) id: &'static str,
    pub(crate) fields: &'static [FormField],
    pub(crate) rules: &'static [FieldRules],
}

impl FormShape {
    /// The shape of `T`.
    pub(crate) const fn of<T: Form>() -> Self {
        Self {
            id: T::ID,
            fields: T::FIELDS,
            rules: T::RULES,
        }
    }
}

/// Draws a [`Form`]: a `<form>` that posts with or without the runtime.
///
/// ```ignore
/// FormView::<NewDeal>::new().post("/deals").submit("Create deal")
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct FormView<T> {
    shape: FormShape,
    hidden: Vec<(String, String)>,
    action: Option<Href>,
    method: &'static str,
    submit: Node,
    cancel: Option<(Node, Href)>,
    values: Vec<(String, String)>,
    errors: BTreeMap<String, Vec<String>>,
    options: BTreeMap<&'static str, Vec<(String, String)>>,
    rich: BTreeMap<&'static str, Vec<crate::select::Choice>>,
    searches: BTreeMap<&'static str, Href>,
    uploads: BTreeMap<&'static str, crate::Upload>,
    columns: bool,
    csrf: Option<String>,
    date_locale: DateLocale,
    slider_labels: crate::flow::SliderLabels,
    today: Option<chrono::NaiveDate>,
    #[cfg(feature = "phone")]
    phone_region: Option<navree_phone::Region>,
    #[cfg(feature = "phone")]
    phone_flags: Option<fn(navree_phone::Region) -> Node>,
    marker: PhantomData<fn() -> T>,
}

impl<T: Form> FormView<T> {
    /// An empty form.
    pub fn new() -> Self {
        Self::shaped(FormShape::of::<T>())
    }

    /// An empty form drawn from `shape`: the fields of a form described at run time.
    pub(crate) fn shaped(shape: FormShape) -> Self {
        Self {
            shape,
            hidden: Vec::new(),
            action: None,
            method: "post",
            submit: Node::from("Save"),
            cancel: None,
            values: Vec::new(),
            errors: BTreeMap::new(),
            options: BTreeMap::new(),
            rich: BTreeMap::new(),
            searches: BTreeMap::new(),
            uploads: BTreeMap::new(),
            columns: true,
            csrf: None,
            date_locale: DateLocale::english(),
            slider_labels: crate::flow::SliderLabels::default(),
            today: None,
            #[cfg(feature = "phone")]
            phone_region: None,
            #[cfg(feature = "phone")]
            phone_flags: None,
            marker: PhantomData,
        }
    }

    /// The country a phone field starts on when its number has none: the
    /// workspace's or the user's (feature `phone`).
    #[cfg(feature = "phone")]
    pub fn phone_region(mut self, region: Option<navree_phone::Region>) -> Self {
        self.phone_region = region;
        self
    }

    /// Draws the flags in the country lists of the form's phone fields with
    /// `flag`, in place of the flag emoji (feature `phone`). See
    /// [`PhoneInput::flags`](crate::PhoneInput::flags).
    #[cfg(feature = "phone")]
    pub fn phone_flags(mut self, flag: fn(navree_phone::Region) -> Node) -> Self {
        self.phone_flags = Some(flag);
        self
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

    /// Names the days and times of the form's date and time pickers after a
    /// language and region, and says what today is. Without a day, today is the day
    /// in UTC.
    pub fn dates(mut self, locale: DateLocale, today: chrono::NaiveDate) -> Self {
        self.date_locale = locale;
        self.today = Some(today);
        self
    }

    /// [`dates`](Self::dates) with today counted in the reader's zone (navree-time's
    /// `CurrentZone`), on the app's clock.
    #[cfg(feature = "time")]
    pub fn dates_in(self, locale: DateLocale, zone: &navree_time::CurrentZone) -> Self {
        self.dates(locale, zone.today().date())
    }

    /// Draws the form in the reader's language: the date pickers' names and words, the
    /// sliders' words and the submit button's text (set [`submit`](Self::submit) after it for
    /// a button of your own). [`ReaderWords::of`] reads them for the request.
    pub fn words(mut self, words: &ReaderWords) -> Self {
        self.date_locale = words.dates.clone();
        self.slider_labels = words.slider.clone();
        self.submit = Node::from(words.save.to_string());
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

    /// Sets the choices of a picker field as [`Choice`](crate::Choice)s, for a list
    /// that shows more than a label: an icon or a picture before it, a line under
    /// it, words it is found by. A browser's own `select` shows only the labels.
    pub fn choices(
        mut self,
        field: &'static str,
        choices: impl IntoIterator<Item = crate::select::Choice>,
    ) -> Self {
        self.rich.insert(field, choices.into_iter().collect());
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

    /// How the file field `field` is drawn: an [`Upload`](crate::Upload) with its limits,
    /// its words and, with [`send_to`](crate::Upload::send_to), the route the module sends
    /// a chosen file to at once. The form gives it the field's name, label, error and key.
    ///
    /// ```
    /// # // In navree-ui's own examples the derive names the crate `crate`: these are its paths.
    /// # use navree_ui::{InputKind, form};
    /// use navree_ui::form::FormView;
    /// use navree_ui::{Form, Upload, UploadLimits, Validate, render};
    /// use serde::Deserialize;
    ///
    /// #[derive(Deserialize, Validate, Form)]
    /// struct Claim {
    ///     #[validate(length(min = 1))]
    ///     title: String,
    ///     #[validate(skip)]
    ///     #[form(widget = file)]
    ///     receipt: String,
    /// }
    ///
    /// # fn main() {
    /// let limits = UploadLimits::new(2 * 1024 * 1024).accept(["application/pdf"]);
    /// let form = FormView::<Claim>::new()
    ///     .post("/claims")
    ///     .upload("receipt", Upload::new("receipt", "Receipt").limits(limits).send_to("/claims/receipt"));
    /// let html = render(&form.into());
    /// assert!(html.contains(r#"enctype="multipart/form-data""#));
    /// assert!(html.contains(r#"data-send="/claims/receipt""#));
    /// # }
    /// ```
    pub fn upload(mut self, field: &'static str, upload: crate::Upload) -> Self {
        self.uploads.insert(field, upload);
        self
    }

    /// Puts every field on its own line.
    pub fn one_column(mut self) -> Self {
        self.columns = false;
        self
    }

    /// Adds a hidden field that posts `value` as `name`, as it is: an id, or the step
    /// the form is at. The field is the page's, not one of the form's own, so a form
    /// read back with [`Submit`] drops it.
    pub fn hidden(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.hidden.push((name.into(), value.into()));
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
    fn choices_of(&self, field: &FormField) -> Vec<crate::select::Choice> {
        if let Some(choices) = self.rich.get(field.name) {
            return choices.clone();
        }
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

    fn picker_id(&self, field: &FormField) -> String {
        format!("{}-{}-picker", self.shape.id, field.name)
    }

    fn value_of(&self, name: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value.as_str())
    }

    fn control(&self, field: &FormField) -> Node {
        let rules = native_in(self.shape.rules, field);
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
                // A browser's date and time field steps by the minute unless told
                // otherwise, and refuses a value that has seconds.
                if kind == InputKind::DateTime && value.is_some_and(|text| text.len() > 16) {
                    input = input.attr("step", "1");
                }
                self.finish_input(input, field, &rules, value).into()
            }
            Widget::DatePicker => {
                let mut picker = DatePicker::new(field.name)
                    .locale(self.date_locale.clone())
                    .invalid(invalid);
                if let Some(today) = self.today {
                    picker = picker.today(today);
                }
                picker = match value {
                    Some(text) => picker.value_text(text),
                    None => picker,
                };
                // The rules the server holds the value to, drawn into the picker for the browser.
                if let Some(date_rules) = field.date_rules {
                    picker = picker.rules(date_rules());
                }
                if let Some(text) = field.placeholder {
                    picker = picker.placeholder(text);
                }
                if rules.required {
                    picker = picker.required();
                }
                picker.into()
            }
            Widget::TimePicker => {
                let mut picker = TimePicker::new(field.name)
                    .locale(self.date_locale.clone())
                    .invalid(invalid);
                if let Some(text) = value {
                    picker = picker.value_text(text);
                }
                if let Some(time_rules) = field.time_rules {
                    picker = picker.rules(time_rules());
                }
                if let Some(text) = field.placeholder {
                    picker = picker.placeholder(text);
                }
                if rules.required {
                    picker = picker.required();
                }
                picker.into()
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
                    .id(self.picker_id(field))
                    .options(self.choices_of(field))
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
                    .id(self.picker_id(field))
                    .options(self.choices_of(field))
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
                    .id(self.picker_id(field))
                    .options(self.choices_of(field))
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
            Widget::Phone | Widget::PhoneText => self.phone(field, &rules, value, invalid),
            Widget::Slider => {
                let spec = slider_spec_in(self.shape.rules, field);
                let shown = value
                    .and_then(|text| crate::flow::Decimal::from_browser(text.trim()).ok())
                    .unwrap_or_else(|| spec.minimum());
                crate::flow::Slider::new(field.name)
                    .labels(self.slider_labels.clone())
                    .spec(spec)
                    .value(shown)
                    .invalid(invalid)
                    .into()
            }
            Widget::File => {
                let mut upload = self.uploads.get(field.name).cloned().map_or_else(
                    || crate::Upload::new(field.name, field.label),
                    |upload| upload.named(field.name),
                );
                // The value a refused form posts back is the key of the file it already sent.
                if let Some(key) = value {
                    upload = upload.key(key);
                }
                if let Some(error) = self.errors.get(field.name) {
                    upload = upload.error(error.join(" "));
                }
                if let Some(help) = field.help {
                    upload = upload.hint(help);
                }
                if rules.required {
                    upload = upload.required();
                }
                upload.into()
            }
        }
    }

    #[cfg(feature = "phone")]
    fn phone(&self, field: &FormField, rules: &Native, value: Option<&str>, invalid: bool) -> Node {
        let country = self
            .value_of(&format!("{}{}", field.name, crate::phone::COUNTRY_SUFFIX))
            .and_then(navree_phone::Region::parse)
            .or(self.phone_region);
        let mut input = crate::phone::PhoneInput::new(field.name)
            .region(country)
            .invalid(invalid);
        if let Some(flag) = self.phone_flags {
            input = input.flags(flag);
        }
        if let Some(value) = value {
            input = input.text(value);
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
        input.into()
    }

    #[cfg(not(feature = "phone"))]
    fn phone(&self, field: &FormField, rules: &Native, value: Option<&str>, invalid: bool) -> Node {
        let input = Input::new(InputKind::Tel, field.name)
            .autocomplete("tel")
            .invalid(invalid);
        self.finish_input(input, field, rules, value).into()
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
        if self
            .shape
            .fields
            .iter()
            .any(|other| other.addon == Some(field.name))
        {
            return None;
        }
        let error = self
            .errors
            .get(field.name)
            .map(|messages| messages.join(" "));
        let id = format!("{}-{}", self.shape.id, field.name);
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
            // The upload draws its own label, hint and error.
            Widget::File => el("div")
                .slot("field")
                .flag("data-invalid", error.is_some())
                .child(self.control(field)),
            _ => {
                let mut control = self.control(field);
                if let Some(addon) = field
                    .addon
                    .and_then(|name| self.shape.fields.iter().find(|other| other.name == name))
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
                // A date and time with no zone in it is read as UTC: the field says so.
                let in_utc = field.instant && field.widget == Widget::Input(InputKind::DateTime);
                match (field.help, in_utc) {
                    (Some(help), false) => box_field = box_field.help(help),
                    (Some(help), true) => {
                        let stop = if help.ends_with(['.', '!', '?']) {
                            ""
                        } else {
                            "."
                        };
                        box_field = box_field.help(format!("{help}{stop} Time in UTC."));
                    }
                    (None, true) => box_field = box_field.help("Time in UTC"),
                    (None, false) => {}
                }
                let box_node: Node = box_field.into();
                box_node.into_element()?
            }
        };
        Some(holder.flag("data-wide", field.wide))
    }

    /// The fields of the form and the errors no field can show, without the `<form>`
    /// around them: what a [`Wizard`](crate::Wizard) draws a step with.
    pub(crate) fn fields_node(&self) -> Node {
        let boxes: Vec<Element> = self
            .shape
            .fields
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
            .filter(|(name, _)| !self.shape.fields.iter().any(|field| field.name == *name))
            .map(|(_, messages)| Alert::new(AlertTone::Danger).title(messages.join(" ")))
            .collect();
        Node::list((whole, el("div").slot(grid).child(boxes)))
    }

    /// Each field as its label and the text a person would read for its value: a
    /// choice by its label, a switch as `yes` or `no`. Passwords and hidden fields
    /// are left out. A field with no value has an empty text.
    pub(crate) fn summary_rows(&self, yes: &str, no: &str) -> Vec<(String, String)> {
        self.shape
            .fields
            .iter()
            .filter(|field| {
                !matches!(
                    field.widget,
                    Widget::Input(InputKind::Hidden | InputKind::Password | InputKind::File)
                        | Widget::File
                )
            })
            .map(|field| {
                let values = self.values_of(field.name);
                let text = match field.widget {
                    Widget::Checkbox | Widget::Switch => {
                        let on = is_on(values.first().map(String::as_str));
                        (if on { yes } else { no }).to_owned()
                    }
                    Widget::Select
                    | Widget::Listbox
                    | Widget::Combobox
                    | Widget::Radio
                    | Widget::Multi => {
                        let choices = self.choices_of(field);
                        values
                            .iter()
                            .filter(|value| !value.is_empty())
                            .map(|value| {
                                choices
                                    .iter()
                                    .find(|choice| choice.value() == value)
                                    .map_or_else(
                                        || value.clone(),
                                        |choice| choice.label().to_owned(),
                                    )
                            })
                            .collect::<Vec<_>>()
                            .join(", ")
                    }
                    _ => values.join(", "),
                };
                (field.label.to_owned(), text)
            })
            .collect()
    }

    /// The form without its buttons: its id, where it posts, what it sends unseen
    /// and its fields. What a native client's form is read from.
    #[cfg(feature = "native-preview")]
    pub(crate) fn without_actions(&self) -> Element {
        let hidden: Vec<Element> = self
            .hidden
            .iter()
            .map(|(name, value)| {
                el("input")
                    .attr("type", "hidden")
                    .attr("name", name.clone())
                    .attr("value", value.clone())
            })
            .collect();
        el("form")
            .slot("form")
            .attr("id", self.shape.id)
            .attr("method", self.method)
            .attr_opt("action", self.action.clone().map(AttrHref))
            .flag("data-ui-form", true)
            .children((
                self.csrf.clone().map(csrf_field),
                hidden,
                self.fields_node(),
            ))
    }

    fn into_node(self) -> Node {
        let fields = self.fields_node();
        // A file goes with the form when no script sent it first.
        let files = self
            .shape
            .fields
            .iter()
            .any(|field| field.widget == Widget::File);
        let hidden: Vec<Element> = self
            .hidden
            .into_iter()
            .map(|(name, value)| {
                el("input")
                    .attr("type", "hidden")
                    .attr("name", name)
                    .attr("value", value)
            })
            .collect();
        el("form")
            .slot("form")
            .attr("id", self.shape.id)
            .attr("method", self.method)
            .attr_opt("action", self.action.map(AttrHref))
            .attr_opt("enctype", files.then_some("multipart/form-data"))
            .flag("data-ui-form", true)
            .uses(Uses::CORE)
            .children((
                self.csrf.map(csrf_field),
                hidden,
                fields,
                el("div").slot("form-actions").children((
                    self.cancel.map(|(label, href)| {
                        Button::ghost(label).href(href).role(crate::Role::Cancel)
                    }),
                    Button::primary(self.submit).submit(),
                )),
            ))
            .into()
    }
}

/// The name of the field that carries the CSRF token in a plain form: the one the session
/// layer reads ([`navree_http::csrf::CSRF_FIELD`]).
pub const CSRF_FIELD: &str = navree_http::csrf::CSRF_FIELD;

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

/// The words of a form's own checks, by rule key: what a value of the wrong type, a
/// list with more values than it takes, a number with too many decimal places and a
/// phone number that doesn't read say. `:attribute` is the field's label.
///
/// They are defaults. A form reads its messages from the app's validation messages
/// (navree-validation's `AppMessages`): the app's `validation.messages` replace them
/// (`form.number = ":attribute takes digits only"`, or for one field
/// `amount.form.amount`), a translator renders them in the reader's language when it
/// has them (with navree-i18n, a `validation-form-number` message), and a field's
/// `#[form(error = "...")]` wins over all of them.
pub const FORM_MESSAGES: &[(&str, &str)] = &[
    (
        "form.amount",
        ":attribute: enter an amount, like 12,500.50.",
    ),
    ("form.choice", ":attribute: choose one of the options."),
    ("form.date", ":attribute: enter a date."),
    (
        "form.decimal_places",
        ":attribute: use at most 28 decimal places.",
    ),
    ("form.file", ":attribute: choose the file again."),
    ("form.invalid", ":attribute is not valid."),
    ("form.max_choices", "Choose at most :max."),
    ("form.moment", ":attribute: enter a date and time."),
    ("form.number", ":attribute: enter a number."),
    (
        "form.phone",
        ":attribute: enter a phone number, with its country.",
    ),
    (
        "form.phone_extension",
        ":attribute: enter the number without an extension.",
    ),
    ("form.time", ":attribute: enter a time."),
    ("form.unreadable", "This form could not be read."),
];

/// The reader's language for a form: the app's validation messages (with its translator,
/// which renders them in the request's language) and the [`ReaderWords`] of the request
/// (the names of months, the date and time conventions, the slider's words). One value
/// carries both to [`decode_for`], to a [`Wizard`](crate::Wizard), to
/// [`Draft::read_for`](crate::Draft::read_for) and to
/// [`TestView::submit_as_for`](crate::TestView::submit_as_for).
///
/// ```
/// use navree_ui::form::Language;
///
/// // Without an app or a translator, a form speaks English.
/// let language = Language::english();
/// assert_eq!(language.words().check_form, "Check this form");
/// ```
#[derive(Clone)]
pub struct Language {
    messages: AppMessages,
    words: ReaderWords,
}

impl Language {
    /// English, as a form says it without an app: the built-in messages, and English
    /// dates and sliders.
    pub fn english() -> Self {
        Self::new(AppMessages::default(), ReaderWords::english())
    }

    /// A language from the messages and the words of the reader.
    pub fn new(messages: AppMessages, words: ReaderWords) -> Self {
        Self { messages, words }
    }

    /// The language of the request behind `ctx`: the app's validation messages and its
    /// [`Localizer`](crate::localize::Localizer). Without an app, English.
    pub async fn of(ctx: Option<&AppContext>) -> Self {
        Self::new(AppMessages::of(ctx).await, ReaderWords::of(ctx).await)
    }

    /// The app's validation messages.
    pub fn messages(&self) -> &AppMessages {
        &self.messages
    }

    /// The reader's words.
    pub fn words(&self) -> &ReaderWords {
        &self.words
    }
}

impl Default for Language {
    fn default() -> Self {
        Self::english()
    }
}

impl std::fmt::Debug for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Language")
            .field("words", &self.words)
            .finish_non_exhaustive()
    }
}

/// The app's messages as a form of `T` says them: its labels name its fields (over the
/// app's `validation.attributes`; a translator's own names still come first), and the
/// form's own words fill the keys the app has no message for.
fn words<T: Form>(messages: &AppMessages) -> AppMessages {
    messages
        .clone()
        .with_attributes(T::FIELDS.iter().map(|field| (field.name, field.label)))
        .with_default_messages(FORM_MESSAGES.iter().copied())
}

/// The messages of failed rules, by field: a field's own `error`, else the rule's
/// message in the app's words.
fn rule_errors<T: Form>(violations: &Violations, words: &AppMessages) -> FieldErrors {
    let mut errors = FieldErrors::new();
    for (path, violation) in violations.iter() {
        let text = match T::FIELDS
            .iter()
            .find(|field| field.name == path)
            .and_then(|field| field.error)
        {
            Some(text) => text.to_owned(),
            None => words.render_violation(path, violation),
        };
        let list = errors.entry(path.to_owned()).or_default();
        if !list.contains(&text) {
            list.push(text);
        }
    }
    errors
}

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
        if T::FIELDS.iter().any(|field| field.name == name) || is_phone_country::<T>(&name) {
            kept.push((name.into_owned(), value.into_owned()));
        }
    }
    Some(kept)
}

/// Whether `T` has a file field.
fn has_files<T: Form>() -> bool {
    T::FIELDS.iter().any(|field| field.widget == Widget::File)
}

/// Whether the request's body is a multipart form.
fn is_multipart(parts: &Parts) -> bool {
    parts
        .headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .trim_start()
                .get(..19)
                .is_some_and(|kind| kind.eq_ignore_ascii_case("multipart/form-data"))
        })
}

/// A post's names and values, in order.
type Pairs = Vec<(String, String)>;

/// The files a post brought for a form's file fields: each field's name and the stored file's
/// path.
#[derive(Default)]
struct Posted {
    files: Vec<(String, String)>,
}

/// The pairs of a multipart post, and the files of its file fields, each stored through the
/// app's `Uploads` (the one registered under the field's name, else the app's one). A file
/// the upload refuses is a field error, under the field, in the upload's words. When the post
/// cannot be read, the status to answer; when no upload can store a file, the error.
async fn read_multipart<T: Form>(
    request: Request,
    ctx: Option<&AppContext>,
) -> Result<(Pairs, Posted), Result<StatusCode, AppError>> {
    let Ok(mut form) =
        <navree_http::extract::Multipart as FromRequest>::from_request(request).await
    else {
        return Err(Ok(StatusCode::BAD_REQUEST));
    };
    let mut pairs = Vec::with_capacity(T::FIELDS.len());
    // Only an upload adds to it, and an upload needs the `storage` feature.
    #[cfg_attr(not(feature = "storage"), allow(unused_mut))]
    let mut posted = Posted::default();
    #[cfg(feature = "storage")]
    let mut kept: Vec<(Arc<crate::Uploads>, navree_storage::StoredFile)> = Vec::new();
    let mut parts = 0_usize;
    // The file fields that already had a file in this post, stored or refused.
    let mut sent: Vec<String> = Vec::new();
    let outcome: Result<(), Result<StatusCode, AppError>> = loop {
        let field = match form.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => break Ok(()),
            Err(error) => break Err(Ok(error.to_app_error().status)),
        };
        parts += 1;
        if parts > MAX_PAIRS {
            break Err(Ok(StatusCode::PAYLOAD_TOO_LARGE));
        }
        let Some(name) = field.name().map(str::to_owned) else {
            continue;
        };
        let is_file = T::FIELDS
            .iter()
            .any(|f| f.name == name && f.widget == Widget::File);
        match field.file_name().map(str::to_owned) {
            // A file name of nothing is a browser saying no file was chosen.
            Some(raw) if is_file && !raw.trim().is_empty() => {
                // A file field holds one file: the first is the field's, and a later part under
                // the same name is not stored, so nothing is left on the disk that no value names.
                if sent.contains(&name) {
                    continue;
                }
                sent.push(name.clone());
                #[cfg(feature = "storage")]
                {
                    let Some(uploads) = uploads_for(ctx, &name).await else {
                        break Err(Err(AppError::internal(format!(
                            "the form's file field `{name}` needs an `Uploads` in the app (registered under `{name}`, or one for the app)"
                        ))));
                    };
                    match uploads.store_field(field, &raw).await {
                        Ok(Ok(stored)) => {
                            posted.files.push((name.clone(), stored.path.clone()));
                            kept.push((uploads, stored));
                        }
                        Ok(Err(refused)) => {
                            pairs.push((format!("{FILE_REFUSED}{name}"), refused.message))
                        }
                        Err(error) => break Err(Err(error)),
                    }
                }
                #[cfg(not(feature = "storage"))]
                {
                    let _ = (field, raw, ctx);
                    break Err(Err(AppError::internal(format!(
                        "the form's file field `{name}` needs navree-ui's `storage` feature to store a posted file"
                    ))));
                }
            }
            Some(_) => {}
            None => {
                if !(T::FIELDS.iter().any(|f| f.name == name) || is_phone_country::<T>(&name)) {
                    continue;
                }
                match field.text().await {
                    Ok(text) => pairs.push((name, text)),
                    Err(error) => break Err(Ok(error.to_app_error().status)),
                }
            }
        }
    };
    match outcome {
        Ok(()) => Ok((pairs, posted)),
        Err(failure) => {
            // A post that failed half way keeps none of its files.
            #[cfg(feature = "storage")]
            for (uploads, stored) in kept {
                uploads.forget(std::slice::from_ref(&stored)).await;
            }
            Err(failure)
        }
    }
}

/// The upload a file field stores its files with: the one registered under its name, else the
/// app's one.
#[cfg(feature = "storage")]
async fn uploads_for(ctx: Option<&AppContext>, name: &str) -> Option<Arc<crate::Uploads>> {
    let ctx = ctx?;
    match ctx.get_named::<crate::Uploads>(name).await {
        Ok(uploads) => Some(uploads),
        Err(_) => ctx.try_get::<crate::Uploads>().await,
    }
}

/// The prefix of the pair that carries why the upload refused a file field's file.
const FILE_REFUSED: &str = "\0refused:";

/// Each file field's value as the form reads it, and as a refused form shows it again, and the
/// errors of the file fields. The value is the path of the file the post brought; else of the
/// file its key names ([`file_key`](crate::upload::file_key), signed by the app): a key that
/// does not read is no file, and an error. A refused form shows the key of each file, so it is
/// not chosen again.
fn file_values<T: Form>(
    pairs: Pairs,
    posted: Posted,
    signer: Option<&navree_http::UrlSigner>,
    words: &AppMessages,
) -> (Pairs, Pairs, FieldErrors) {
    let mut errors = FieldErrors::new();
    let mut read = Vec::with_capacity(pairs.len());
    let mut shown = Vec::with_capacity(pairs.len());
    let mut keys: BTreeMap<String, String> = BTreeMap::new();
    for (name, value) in pairs {
        if let Some(field) = name.strip_prefix(FILE_REFUSED) {
            errors.entry(field.to_owned()).or_default().push(value);
            continue;
        }
        let is_file = T::FIELDS
            .iter()
            .any(|f| f.name == name && f.widget == Widget::File);
        if is_file {
            // The first key wins: a form posts one per field.
            if !value.trim().is_empty() {
                keys.entry(name).or_insert(value);
            }
            continue;
        }
        read.push((name.clone(), value.clone()));
        shown.push((name, value));
    }
    for field in T::FIELDS.iter().filter(|f| f.widget == Widget::File) {
        let name = field.name;
        if errors.contains_key(name) {
            continue;
        }
        let brought = posted
            .files
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, path)| path.clone());
        let path = match brought {
            Some(path) => Some(path),
            None => match keys.get(name) {
                Some(key) => {
                    match signer.and_then(|signer| crate::upload::file_path(signer, key)) {
                        Some(path) => Some(path),
                        None => {
                            errors.insert(
                                name.to_owned(),
                                vec![words.render(name, "form.file", &[])],
                            );
                            None
                        }
                    }
                }
                None => None,
            },
        };
        if let Some(path) = path {
            if let Some(signer) = signer {
                shown.push((name.to_owned(), crate::upload::file_key(signer, &path)));
            }
            read.push((name.to_owned(), path));
        }
    }
    (read, shown, errors)
}

/// Whether `name` is the country posted with one of `T`'s phone fields.
fn is_phone_country<T: Form>(name: &str) -> bool {
    name.strip_suffix("_country").is_some_and(|field| {
        T::FIELDS
            .iter()
            .any(|f| f.name == field && f.widget.is_phone())
    })
}

/// The pairs with each phone field's number read in the country posted with
/// it, and the errors of those that don't read. A valid number is written
/// in E.164 (a `Phone` keeps its extension after it); one that isn't valid
/// in its country, or an extension in a text field (E.164 has no room for
/// it), is a field error, and the text stays as typed for the form to show.
/// A `Phone` posted without a country is left to read in the request's
/// region; text without one must carry its calling code.
#[cfg(feature = "phone")]
fn read_phones<T: Form>(
    pairs: &[(String, String)],
    words: &AppMessages,
) -> (Vec<(String, String)>, FieldErrors) {
    use navree_phone::{PhoneNumber, Region};
    let mut errors = FieldErrors::new();
    let read = pairs
        .iter()
        .map(|(name, value)| {
            let as_typed = (name.clone(), value.clone());
            let Some(field) = T::FIELDS
                .iter()
                .find(|field| field.name == name && field.widget.is_phone())
            else {
                return as_typed;
            };
            if value.trim().is_empty() {
                return as_typed;
            }
            let text = field.widget == Widget::PhoneText;
            let country = format!("{name}{}", crate::phone::COUNTRY_SUFFIX);
            let region = pairs
                .iter()
                .find(|(other, _)| *other == country)
                .and_then(|(_, code)| Region::parse(code));
            if region.is_none() && !text {
                return as_typed;
            }
            let refuse = |errors: &mut FieldErrors, why: String| {
                let message = field.error.map_or(why, str::to_owned);
                errors.insert(name.clone(), vec![message]);
            };
            match PhoneNumber::parse(value, region) {
                Ok(number) if text && number.extension().is_some() => {
                    refuse(&mut errors, words.render(name, "form.phone_extension", &[]));
                    as_typed
                }
                Ok(number) => {
                    let written = match number.extension() {
                        Some(extension) => format!("{number} ext. {extension}"),
                        None => number.as_str().to_owned(),
                    };
                    (name.clone(), written)
                }
                Err(_) => {
                    refuse(&mut errors, words.render(name, "form.phone", &[]));
                    as_typed
                }
            }
        })
        .collect();
    (read, errors)
}

/// What a rejected form may show again: never what a password input holds, and a bounded
/// amount. A `String` field named `password` (or ending in it) is a password input; so is any
/// field with `#[form(widget = password)]`.
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

/// Whether `bytes` is `HH:MM`.
fn is_hour_minute(bytes: &[u8]) -> bool {
    bytes.len() == 5
        && bytes[2] == b':'
        && bytes[..2].iter().chain(&bytes[3..]).all(u8::is_ascii_digit)
}

/// Whether the field holds a date and time: a `NaiveDateTime` on the browser's field, or
/// an instant.
fn is_moment(field: &FormField) -> bool {
    field.instant || field.widget == Widget::Input(InputKind::DateTime)
}

/// What a browser posts, put the way the type reads it. A time is posted as hours and
/// minutes: `14:30` is `14:30:00`. A date and time is posted to the minute and with no
/// zone: `2026-11-04T14:30` is `2026-11-04T14:30:00`, and for an instant it is in UTC, so
/// it ends in `Z`. Text that already has a zone, or isn't a date and time, is left as it is.
fn browser_text<T: Form>(name: &str, value: &str) -> String {
    let Some(field) = T::FIELDS.iter().find(|field| field.name == name) else {
        return value.to_owned();
    };
    let bytes = value.as_bytes();
    if matches!(
        field.widget,
        Widget::TimePicker | Widget::Input(InputKind::Time)
    ) && is_hour_minute(bytes)
    {
        return format!("{value}:00");
    }
    if !is_moment(field) {
        return value.to_owned();
    }
    let mut text = value.to_owned();
    if bytes.len() == 16 && bytes[10] == b'T' && is_hour_minute(&bytes[11..]) {
        text.push_str(":00");
    }
    if field.instant && text.parse::<chrono::NaiveDateTime>().is_ok() {
        text.push('Z');
    }
    text
}

/// The date and times `text` says that the calendar and the clock have to hold: the wall
/// time as typed, and for an instant that names a zone the same moment in UTC, which is what
/// is stored (`9999-12-31T23:30-05:00` is on 10,000-01-01 in UTC, past the calendar). A field
/// that is not an instant takes no zone, so a text with one is the type's own error.
fn moments(field: &FormField, text: &str) -> Vec<chrono::NaiveDateTime> {
    let text = text.trim();
    if let Ok(wall) = text.parse::<chrono::NaiveDateTime>() {
        return vec![wall];
    }
    if !field.instant {
        return Vec::new();
    }
    match chrono::DateTime::parse_from_rfc3339(text) {
        Ok(at) => vec![at.naive_local(), at.naive_utc()],
        Err(_) => Vec::new(),
    }
}

/// The grid of a slider field: its own function, or its `range` rule in steps of 1.
fn slider_spec<T: Form>(field: &FormField) -> crate::flow::SliderSpec {
    slider_spec_in(T::RULES, field)
}

/// [`slider_spec`] against `rules`.
fn slider_spec_in(rules: &[FieldRules], field: &FormField) -> crate::flow::SliderSpec {
    if let Some(grid) = field.slider {
        return grid();
    }
    let rules = native_in(rules, field);
    let number = |text: Option<&str>, otherwise: i64| {
        text.and_then(|text| crate::flow::Decimal::from_browser(text).ok())
            .unwrap_or_else(|| crate::flow::Decimal::from(otherwise))
    };
    crate::flow::SliderSpec::new(number(rules.min, 0), number(rules.max, 100))
}

/// What the date and time fields say of the posted text, which the typed value can't: a
/// year the calendar doesn't draw (outside 1 to 9999) and a time that is not a time of day
/// (a leap second, a fraction of a second) are always refused, and the rules a field was
/// given with `date_rules` or `time_rules` hold the server as they held the browser, so a
/// value the picker drew as off is off here. Text that isn't a day or a time is left to the
/// type's own error. The days' and times' words are the reader's (`reader`), or the field's
/// own `error`; the decimal places' are the app's (`form.decimal_places`).
fn refused_by_pickers<T: Form>(
    pairs: &[(String, String)],
    words: &AppMessages,
    reader: &ReaderWords,
) -> FieldErrors {
    let dates = &reader.dates;
    let mut out = FieldErrors::new();
    for (name, value) in pairs {
        let Some(field) = T::FIELDS.iter().find(|field| field.name == name) else {
            continue;
        };
        let why = match field.widget {
            // A date and time has no picker rules of its own, but the calendar and the
            // time of day are the same: a year outside 1 to 9999, a leap second and a
            // fraction of a second are refused.
            _ if is_moment(field) => moments(field, value).iter().find_map(|at| {
                DateRules::default()
                    .check(at.date())
                    .err()
                    .map(|error| error.message(dates))
                    .or_else(|| {
                        TimeRules::default()
                            .check(at.time())
                            .err()
                            .map(|error| error.message(dates))
                    })
            }),
            // A number is read exactly or not at all: past 28 decimal places a `Decimal` would
            // round, and what is stored would not be what was typed.
            Widget::Input(InputKind::Number) if field.decimal => {
                let text = value.trim();
                (rust_decimal::Decimal::from_str_exact(text).is_err()
                    && text.parse::<rust_decimal::Decimal>().is_ok())
                .then(|| words.render(name, "form.decimal_places", &[]))
            }
            Widget::DatePicker | Widget::Input(InputKind::Date) => {
                let Ok(day) = value.trim().parse::<NaiveDate>() else {
                    continue;
                };
                let rules = field.date_rules.map(|rules| rules()).unwrap_or_default();
                rules.check(day).err().map(|error| error.message(dates))
            }
            Widget::TimePicker | Widget::Input(InputKind::Time) => {
                let Ok(time) = value.trim().parse::<NaiveTime>() else {
                    continue;
                };
                let rules = field.time_rules.map(|rules| rules()).unwrap_or_default();
                rules.check(time).err().map(|error| error.message(dates))
            }
            Widget::Slider => {
                // An optional field left empty is no value, and the rules say if it may be.
                if value.is_empty() && field.optional {
                    continue;
                }
                let spec = slider_spec::<T>(field);
                spec.check(value.trim())
                    .err()
                    .map(|error| spec.message(error, &reader.slider))
            }
            _ => None,
        };
        if let Some(text) = why {
            let text = field.error.map_or(text, str::to_owned);
            let list = out.entry(name.clone()).or_default();
            if !list.contains(&text) {
                list.push(text);
            }
        }
    }
    out
}

/// Decodes submitted pairs into `T` and runs its rules: what [`Submit`] and
/// tests share. Empty values of optional fields are absent, an unchecked box
/// is `false`, and a missing text field is empty. A pair that isn't one of `T`'s
/// fields is dropped, so a field marked `#[form(skip)]` can't be set by the client.
///
/// The date and time fields are held to their pickers' rules too, whether or not the
/// type has a `custom` rule of its own: see `date_rules` and `time_rules` on
/// [`FormField`], and what is always refused (a year outside 1 to 9999, a leap second,
/// a fraction of a second).
pub fn decode<T>(pairs: &[(String, String)]) -> Result<T, FieldErrors>
where
    T: Form + DeserializeOwned + Validate,
{
    decode_with(pairs, &AppMessages::default())
}

/// [`decode`] in the app's words: the rules' messages, and the form's own
/// ([`FORM_MESSAGES`]), come from `messages` (the app's `validation.messages` and its
/// translator, navree-validation's `AppMessages::of`), with the form's labels as the
/// fields' names. The date, time and slider messages are English; [`decode_for`] says
/// them in the reader's language.
pub fn decode_with<T>(pairs: &[(String, String)], messages: &AppMessages) -> Result<T, FieldErrors>
where
    T: Form + DeserializeOwned + Validate,
{
    decode_in::<T>(pairs, messages, &ReaderWords::english())
}

/// [`decode`] in the reader's language: the messages of the app, and the names of months
/// and the words of the date, time and slider checks of the reader. What [`Submit`] uses
/// ([`Language::of`] reads both from the request's app).
pub fn decode_for<T>(pairs: &[(String, String)], language: &Language) -> Result<T, FieldErrors>
where
    T: Form + DeserializeOwned + Validate,
{
    decode_in::<T>(pairs, &language.messages, &language.words)
}

fn decode_in<T>(
    pairs: &[(String, String)],
    messages: &AppMessages,
    reader: &ReaderWords,
) -> Result<T, FieldErrors>
where
    T: Form + DeserializeOwned + Validate,
{
    let words = words::<T>(messages);
    let over = over_limit::<T>(pairs, &words);
    match decode_values::<T>(pairs, &words, reader) {
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
fn over_limit<T: Form>(pairs: &[(String, String)], words: &AppMessages) -> FieldErrors {
    let mut errors = FieldErrors::new();
    for field in T::FIELDS {
        let (Widget::Multi, Some(max)) = (field.widget, field.max) else {
            continue;
        };
        if list_values(pairs, field.name).count() > max as usize {
            errors.insert(
                field.name.to_owned(),
                vec![words.render(field.name, "form.max_choices", &[("max", max.to_string())])],
            );
        }
    }
    errors
}

fn decode_values<T>(
    pairs: &[(String, String)],
    words: &AppMessages,
    reader: &ReaderWords,
) -> Result<T, FieldErrors>
where
    T: Form + DeserializeOwned + Validate,
{
    // A browser posts `14:30` for a time and `2026-11-04T14:30` for a date and time; the
    // type reads `14:30:00` and `2026-11-04T14:30:00`.
    let pairs: Vec<(String, String)> = pairs
        .iter()
        .map(|(name, value)| (name.clone(), browser_text::<T>(name, value)))
        .collect();
    let refused = refused_by_pickers::<T>(&pairs, words, reader);
    let outcome = read::<T>(&pairs, words);
    if refused.is_empty() {
        return outcome;
    }
    // The value read fine, or failed for other reasons: the rules' words join whatever else
    // there is to say. A date and time or a decimal says one thing about itself, though: when
    // the rules or the type already said what is wrong with it, that is the message.
    let mut errors = outcome.err().unwrap_or_default();
    for (name, texts) in refused {
        let says_once = T::FIELDS
            .iter()
            .any(|field| field.name == name && (is_moment(field) || field.decimal));
        if says_once && errors.contains_key(&name) {
            continue;
        }
        let list = errors.entry(name).or_default();
        for text in texts {
            if !list.contains(&text) {
                list.push(text);
            }
        }
    }
    Err(errors)
}

/// The value the pairs say, and the rules of its type.
fn read<T>(pairs: &[(String, String)], words: &AppMessages) -> Result<T, FieldErrors>
where
    T: Form + DeserializeOwned + Validate,
{
    #[cfg(feature = "phone")]
    let (phones, phone_errors) = read_phones::<T>(pairs, words);
    #[cfg(feature = "phone")]
    let pairs = phones.as_slice();
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
            | Widget::DatePicker
            | Widget::TimePicker
            | Widget::Money
            | Widget::Phone
            | Widget::PhoneText
            | Widget::Slider
            | Widget::File => kept.push((field.name, "")),
            // Read above, with the values it has.
            Widget::Multi => {}
        }
    }
    // Serde stops at the first value of the wrong type. To name every such field,
    // take the one it names out and read again: a field that is gone reports
    // only at the end, as missing, which ends the loop.
    let mut wrong = FieldErrors::new();
    // Phone numbers that don't read in their country are wrong like a value
    // of the wrong type: reported first, with the rest of those.
    #[cfg(feature = "phone")]
    wrong.extend(phone_errors);
    let value: T = loop {
        let encoded = encode(&kept);
        match serde_path_to_error::deserialize(urlencoded::Deserializer::new(encoded.as_bytes())) {
            Ok(value) if wrong.is_empty() => break value,
            Ok(_) => return Err(wrong),
            Err(error) => {
                let more = type_error::<T>(&error, words);
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
        Err(violations) => Err(rule_errors::<T>(&violations, words)),
    }
}

/// What a value of the wrong type says: the field's own `error`, else the app's words for
/// its kind of field ([`FORM_MESSAGES`]).
fn type_error<T: Form>(
    error: &serde_path_to_error::Error<urlencoded::Error>,
    words: &AppMessages,
) -> FieldErrors {
    let path = error.path().to_string();
    let field = T::FIELDS.iter().find(|field| field.name == path);
    let text = match field {
        Some(FormField {
            error: Some(text), ..
        }) => (*text).to_owned(),
        Some(field) => words.render(field.name, type_error_key(field), &[]),
        None => words.render("_form", "form.unreadable", &[]),
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

/// The rule key of a value of the wrong type in `field` (see [`FORM_MESSAGES`]).
fn type_error_key(field: &FormField) -> &'static str {
    match field.widget {
        Widget::Money => "form.amount",
        Widget::Input(InputKind::Number) => "form.number",
        Widget::Input(InputKind::Date) | Widget::DatePicker => "form.date",
        Widget::Input(InputKind::Time) | Widget::TimePicker => "form.time",
        Widget::Input(InputKind::DateTime) => "form.moment",
        _ if field.instant => "form.moment",
        Widget::Select | Widget::Listbox | Widget::Combobox | Widget::Multi | Widget::Radio => {
            "form.choice"
        }
        Widget::Phone | Widget::PhoneText => "form.phone",
        _ => "form.invalid",
    }
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
    refusal_of(parts, false).await
}

/// [`refusal`], for a form that may also come as a multipart body: one with a file field.
async fn refusal_of(parts: &Parts, multipart: bool) -> Option<StatusCode> {
    if matches!(parts.method, Method::GET | Method::HEAD) {
        return Some(StatusCode::METHOD_NOT_ALLOWED);
    }
    if !from_this_site(parts) && !from_a_trusted_front_end(parts).await {
        return Some(StatusCode::FORBIDDEN);
    }
    if !is_form_body(parts) && !(multipart && is_multipart(parts)) {
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
/// `http.cors.allowed_origins`, a body that isn't a urlencoded form (or, for a form with a
/// file field, a multipart one) with 415, and a `GET` or `HEAD` with 405, so no link or
/// prefetch can fill one. Behind navree-session the token check also runs.
///
/// It checks a post as the API checks a body: the messages are the app's (its
/// `validation.messages`, and its translator, so a reader gets them in their language),
/// with the form's labels as the fields' names; and once the form's own rules pass, its
/// database rules ([`Form::database_rules`]: the type's `AsyncRules`, `unique` and
/// `exists`) run in the request's unit of work, a failed one answered as a field error
/// like any other. A rule that can't run (no unit of work, a database error) is the
/// app's error answer (a 500), not a field error.
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
    /// A database rule could not run: the error, answered as the app answers it.
    failed: Option<AppError>,
    options: BTreeMap<&'static str, Vec<(String, String)>>,
    rich: BTreeMap<&'static str, Vec<crate::select::Choice>>,
    /// The app's shell for the request, when the form is drawn on the page of its own.
    shell: Option<AppShell>,
    /// The reader's words, for the heading of the page of its own.
    reader: ReaderWords,
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
            .field("failed", &self.failed.is_some())
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
    /// async fn store(parts: Parts, uow: Uow, Submit(deal): Submit<NewDeal>) -> Result<Response> {
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
            from_runtime: crate::asked::who_asked(&parts.headers).is_runtime(),
            action: parts.uri.path_and_query().map_or_else(
                || parts.uri.path().to_owned(),
                |path| path.as_str().to_owned(),
            ),
            refused: None,
            failed: None,
            options: BTreeMap::new(),
            rich: BTreeMap::new(),
            shell: None,
            reader: ReaderWords::english(),
            marker: PhantomData,
        }
    }

    /// Draws the page this refusal falls back on the way the request's reader expects:
    /// in the app's shell (the `Shell` it registered, or its [`ErrorShell`](crate::ErrorShell),
    /// with the signed-in user's menu) and with the heading in the reader's language (the
    /// app's [`Localizer`](crate::localize::Localizer)). [`Submit`] does this for the form it
    /// refuses; call it on a refusal built with [`after_decode`](Self::after_decode). A
    /// refusal drawn with [`respond`](Self::respond) draws its own page and ignores both.
    ///
    /// ```ignore
    /// return Ok(Rejected::after_decode(&deal, errors, &parts).for_request(&parts).await.into_response());
    /// ```
    pub async fn for_request(mut self, parts: &Parts) -> Self {
        let ctx = parts.extensions.get::<AppContext>().cloned();
        self.reader = ReaderWords::of(ctx.as_ref()).await;
        self.shell = shell_of_request(parts).await;
        self
    }

    /// The errors by field.
    pub fn errors(&self) -> &FieldErrors {
        &self.errors
    }

    /// What was posted, as `(field, text)` in the order it was sent: the fields of the
    /// form and nothing else, so a `#[form(skip)]` field a client made up is not here,
    /// and never what a password input holds (a `String` named `password`, or any field with
    /// `widget = password`). A form refused before it was read (a body from another
    /// site, one too large) has none.
    ///
    /// A handler reads it to look up what the person picked: a searched combobox posts
    /// only an id, and the form can show the id's label only when it is given one with
    /// [`options`](Self::options).
    pub fn values(&self) -> impl Iterator<Item = (&str, &str)> + '_ {
        self.values
            .iter()
            .filter(|(name, _)| T::FIELDS.iter().any(|field| field.name == name))
            .map(|(name, text)| (name.as_str(), text.as_str()))
    }

    /// The first text posted under `name`: `None` when the form has no such field or
    /// nothing was posted under it. An empty field was posted, so it is `Some("")`.
    pub fn value(&self, name: &str) -> Option<&str> {
        self.values()
            .find(|(field, _)| *field == name)
            .map(|(_, text)| text)
    }

    /// Every text posted under `name`, in order: a field that is a list ([`Many`]) has
    /// several.
    pub fn values_of(&self, name: &str) -> Vec<&str> {
        self.values()
            .filter(|(field, _)| *field == name)
            .map(|(_, text)| text)
            .collect()
    }

    /// Gives the labels of the choices posted for a picker field, as
    /// [`FormView::options`] does for a form that is drawn the first time. Without it a
    /// searched combobox, which posts an id and has no list to look it up in, shows the
    /// id when the form comes back.
    ///
    /// Only give the label of something the caller may pick. A posted id is whatever a client
    /// chose to send, so look it up through the caller's own scoped query and never with an
    /// unscoped `find`: a refusal that names a person the caller may not see is a leak.
    /// `OptionSource::pickable` does that and gives [`choices`](Self::choices) what it
    /// needs.
    pub fn options(
        mut self,
        field: &'static str,
        options: impl IntoIterator<Item = (String, String)>,
    ) -> Self {
        self.options.insert(field, options.into_iter().collect());
        self
    }

    /// Gives the choices of a picker field with more than a label: see
    /// [`FormView::choices`]. This and [`options`](Self::options) are for the form that
    /// comes back with script off; the runtime keeps the page and shows only the errors.
    ///
    /// ```ignore
    /// Err(rejected) => {
    ///     // Only the people this caller may pick, from the query the search uses.
    ///     let owners = PEOPLE
    ///         .pickable(Person::query(), rejected.values_of("owner_id"), &uow, OptionSource::as_int, |record| {
    ///             let person = record.into_inner();
    ///             Choice::new(person.id.to_string(), person.name)
    ///         })
    ///         .await?;
    ///     Ok(rejected.choices("owner_id", owners).into_response())
    /// }
    /// ```
    pub fn choices(
        mut self,
        field: &'static str,
        choices: impl IntoIterator<Item = crate::select::Choice>,
    ) -> Self {
        self.rich.insert(field, choices.into_iter().collect());
        self
    }

    /// Ops for the runtime; with JavaScript off, what `page` makes of a form
    /// holding what was typed and the errors.
    pub fn respond<R: IntoResponse>(self, page: impl FnOnce(FormView<T>) -> R) -> Response {
        if let Some(error) = self.failed {
            return error.into_response();
        }
        if let Some(status) = self.refused {
            let mut response = status.into_response();
            if status == StatusCode::METHOD_NOT_ALLOWED {
                response
                    .headers_mut()
                    .insert(ALLOW, HeaderValue::from_static("POST"));
            }
            return response;
        }
        // A native client shows the messages at its own fields, as the runtime does.
        #[cfg(feature = "native-preview")]
        if let Some(response) = crate::native::swap::errors(T::ID, &self.errors) {
            return response;
        }
        if self.from_runtime {
            let ops = vec![Op::Errors {
                form: T::ID.to_owned(),
                fields: self.errors,
            }];
            return (StatusCode::UNPROCESSABLE_ENTITY, Envelope::new(ops)).into_response();
        }
        let mut form = FormView::<T>::new()
            .words(&self.reader)
            .post(self.action)
            .with_values(self.values)
            .with_errors(self.errors);
        for (field, options) in self.options {
            form = form.options(field, options);
        }
        for (field, choices) in self.rich {
            form = form.choices(field, choices);
        }
        let mut response = page(form).into_response();
        *response.status_mut() = StatusCode::UNPROCESSABLE_ENTITY;
        response
    }
}

impl<T: Form> IntoResponse for Rejected<T> {
    /// The form on a page of its own: inside the app's shell when the request had one (see
    /// [`for_request`](Rejected::for_request)), else alone; the heading is in the reader's
    /// language.
    fn into_response(self) -> Response {
        let title = self.reader.check_form.clone();
        let shell = self.shell.clone();
        self.respond(move |form| match shell {
            Some(shell) => shell.page(title).body(Node::from(form)).into_response(),
            None => View::new(title.clone())
                .body(el("main").slot("page").children((
                    el("h1").slot("page-title").child(title.to_string()),
                    Node::from(form),
                )))
                .into_response(),
        })
    }
}

/// The app's shell for the request of `parts`, drawn as an error page's is: the `Shell` the
/// app registered or the one its `ErrorShell` builds for the signed-in user. `None` for a
/// guest (whose page is not the app's), when the app has neither, or when the request did not
/// go through the HTTP pipeline.
async fn shell_of_request(parts: &Parts) -> Option<AppShell> {
    let mut context = parts
        .extensions
        .get::<std::sync::Arc<navree_http::ErrorContext>>()?
        .as_ref()
        .clone();
    context.user = Some(
        navree_http::CurrentPrincipal::from_parts(parts)
            .ok()?
            .0
            .user_id,
    );
    let shell = crate::pages::shell_of(&context).await?;
    Some(shell.for_request(parts.uri.path(), &parts.headers))
}

impl<T> FromRequest for Submit<T>
where
    T: Form + DeserializeOwned + Validate + Send + 'static,
{
    type Rejection = Rejected<T>;

    async fn from_request(request: Request) -> Result<Self, Self::Rejection> {
        let (parts, body): (Parts, _) = request.into_parts();
        let from_runtime = crate::asked::who_asked(&parts.headers).is_runtime();
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
            failed: None,
            options: BTreeMap::new(),
            rich: BTreeMap::new(),
            shell: None,
            reader: ReaderWords::english(),
            marker: PhantomData,
        };
        let files = has_files::<T>();
        if let Some(status) = refusal_of(&parts, files).await {
            return Err(refuse(status, action));
        }
        let ctx = parts.extensions.get::<AppContext>().cloned();
        // A form with a file field is a multipart form when no script sent its files first.
        let (values, stored) = if files && is_multipart(&parts) {
            let request = Request::from_parts(parts.clone(), body);
            match read_multipart::<T>(request, ctx.as_ref()).await {
                Ok(read) => read,
                Err(Ok(status)) => return Err(refuse(status, action)),
                Err(Err(error)) => {
                    let mut failed = refuse(StatusCode::INTERNAL_SERVER_ERROR, action);
                    failed.refused = None;
                    failed.failed = Some(error);
                    return Err(failed);
                }
            }
        } else {
            let limit = BodyLimit::of(&parts.extensions);
            let Ok(bytes) = read_body(body, limit).await else {
                return Err(refuse(StatusCode::PAYLOAD_TOO_LARGE, action));
            };
            let Some(values) = read_pairs::<T>(&bytes) else {
                return Err(refuse(StatusCode::PAYLOAD_TOO_LARGE, action));
            };
            (values, Posted::default())
        };
        // The app's messages and its translator, and the reader's words: the reader's language.
        let language = Language::of(ctx.as_ref()).await;
        // A file field's value is the path of its file, and a refused form shows its key.
        let (values, echoed, file_errors) = if files {
            let signer = match &ctx {
                Some(ctx) => ctx.try_get::<navree_http::UrlSigner>().await,
                None => None,
            };
            let (values, echoed, errors) = file_values::<T>(
                values,
                stored,
                signer.as_deref(),
                &words::<T>(&language.messages),
            );
            (values, Some(echoed), errors)
        } else {
            (values, None, FieldErrors::new())
        };
        let rejected =
            |values: Vec<(String, String)>, errors: FieldErrors, action: String| Rejected {
                values: echo::<T>(values),
                errors,
                from_runtime,
                action,
                refused: None,
                failed: None,
                options: BTreeMap::new(),
                rich: BTreeMap::new(),
                shell: None,
                reader: language.words.clone(),
                marker: PhantomData,
            };
        let decoded = match decode_for::<T>(&values, &language) {
            Ok(value) if file_errors.is_empty() => Ok(value),
            Ok(_) => Err(file_errors),
            Err(mut errors) => {
                // What is wrong with a file is what its field says.
                errors.extend(file_errors);
                Err(errors)
            }
        };
        let value = match decoded {
            Ok(value) => value,
            Err(errors) => {
                let shown = echoed.unwrap_or(values);
                let mut refused = rejected(shown, errors, action);
                // Only a post that gets its page back needs the shell.
                if !from_runtime {
                    refused.shell = shell_of_request(&parts).await;
                }
                return Err(refused);
            }
        };
        // Then the database rules, as `ValidAsync` runs them for an API.
        let rules = Form::database_rules(&value);
        if rules.is_empty() {
            return Ok(Self(value));
        }
        match check_async_rules(&parts, rules).await {
            Ok(violations) if violations.is_empty() => Ok(Self(value)),
            Ok(violations) => {
                let errors = rule_errors::<T>(&violations, &words::<T>(&language.messages));
                let mut refused = rejected(echoed.unwrap_or(values), errors, action);
                if !from_runtime {
                    refused.shell = shell_of_request(&parts).await;
                }
                Err(refused)
            }
            Err(error) => {
                let mut failed = rejected(Vec::new(), FieldErrors::new(), action);
                failed.failed = Some(error);
                Err(failed)
            }
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
        // Lakhs and crores.
        assert_eq!(
            Money(123_456_750).display_grouped(Grouping::Indian),
            "12,34,567.50"
        );
        assert_eq!(
            Money(10_000_000).display_grouped(Grouping::Indian),
            "1,00,000.00"
        );
        assert_eq!(
            Money(1_000_000_000).display_grouped(Grouping::Indian),
            "1,00,00,000.00"
        );
        assert_eq!(Money(99_900).display_grouped(Grouping::Indian), "999.00");
        assert_eq!(Money(100_000).display_grouped(Grouping::Indian), "1,000.00");
        assert_eq!(
            Money(-123_456_750).display_grouped(Grouping::Indian),
            "-12,34,567.50"
        );
        assert_eq!(Money(5).display_grouped(Grouping::Indian), "0.05");
        assert_eq!(
            Money(123_456_750).display_grouped(Grouping::None),
            "1234567.50"
        );
        for money in [
            Money(0),
            Money(99_900),
            Money(123_456_750),
            Money(-1_000_000_000),
        ] {
            assert_eq!(
                Money::parse(&money.display_grouped(Grouping::Indian)),
                Some(money)
            );
            assert_eq!(
                Money::parse(&money.display_grouped(Grouping::Thousands)),
                Some(money)
            );
        }
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
