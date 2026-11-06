//! The values a model may choose for a component: a closed set of types, each with its
//! JSON Schema, its check and its example, so the schema a model reads, the check its
//! answer passes and the value the component draws come from one place.
//!
//! - [`UserText`]: text with a most length, drawn only as text.
//! - [`Int`]: a whole number in a range.
//! - [`choices!`](crate::choices): a closed choice, the words the model writes for each.
//! - [`Key`]: the key of a record, which the component loads under the reader's access.
//! - [`Day`]: a calendar day.
//! - [`List`]: a list with a most length.
//! - [`AppLink`]: a page of this app, from the patterns the catalogue names.
//! - `bool`, `Option<P>`, and structs of props ([`props!`](crate::props)).
//!
//! A `String`, a `Vec`, raw HTML, a URL or a colour is not a prop, so a component that
//! asks for one does not compile:
//!
//! ```compile_fail
//! navree_ui::props! {
//!     /// A colour the model picks freely.
//!     pub struct Painted {
//!         /// Any CSS colour.
//!         pub colour: String,
//!     }
//! }
//! ```
//!
//! ```compile_fail
//! navree_ui::props! {
//!     /// Markup the model writes.
//!     pub struct Raw {
//!         /// The markup.
//!         pub html: navree_ui::SafeHtml,
//!     }
//! }
//! ```
//!
//! ```compile_fail
//! navree_ui::props! {
//!     /// As many rows as the model likes.
//!     pub struct Unbounded {
//!         /// The rows.
//!         pub rows: Vec<navree_ui::genui::UserText<40>>,
//!     }
//! }
//! ```
//!
//! ```compile_fail
//! // A list's most is at most 100.
//! navree_ui::props! {
//!     /// Too many rows.
//!     pub struct TooLong {
//!         /// The rows.
//!         pub rows: navree_ui::genui::List<navree_ui::genui::UserText<40>, 100000>,
//!     }
//! }
//! # fn main() { let _ = <TooLong as navree_ui::genui::Prop>::schema(); }
//! ```

use std::fmt;

use chrono::{Datelike, NaiveDate};
use serde_json::{Value, json};

use crate::html::LocalHref;
use crate::node::Node;

/// The most problems one check reports: enough to repair from, never a flood.
pub const MAX_PROBLEMS: usize = 20;

/// The most items a [`List`] may be given.
pub const MAX_LIST: usize = 100;

/// The most characters a [`UserText`] may be given.
pub const MAX_TEXT: usize = 2000;

/// The most characters of a [`Key`].
pub const MAX_KEY: usize = 64;

/// What is wrong with a value a model chose, and where in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    /// Where: `period`, `rows[2].label`; empty for the value as a whole.
    pub path: String,
    /// What, in words a model can repair its answer from.
    pub message: String,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            f.write_str(&self.message)
        } else {
            write!(f, "{}: {}", self.path, self.message)
        }
    }
}

/// A pattern of the app's own pages a model may link to: `/orders/{key}`. Each
/// `{key}` takes a [`Key`]; every other segment is matched as written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkPattern {
    pattern: String,
    segments: Vec<Option<String>>,
}

impl LinkPattern {
    /// Reads `pattern`: a path of plain segments (letters, digits, `-`, `_`, `.`) and
    /// `{key}` segments. Anything else is refused, with why.
    pub fn parse(pattern: &str) -> Result<Self, String> {
        let Some(rest) = pattern.strip_prefix('/') else {
            return Err(format!("a link pattern starts with `/`: `{pattern}`"));
        };
        if rest.is_empty() {
            return Ok(Self::root());
        }
        let mut segments = Vec::new();
        for segment in rest.split('/') {
            if segment == "{key}" {
                segments.push(None);
            } else if !segment.is_empty()
                && segment != "."
                && segment != ".."
                && segment
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
            {
                segments.push(Some(segment.to_owned()));
            } else {
                return Err(format!(
                    "a link pattern is plain segments and `{{key}}`: `{pattern}`"
                ));
            }
        }
        Ok(Self {
            pattern: pattern.to_owned(),
            segments,
        })
    }

    /// The home page, `/`.
    pub fn root() -> Self {
        Self {
            pattern: "/".to_owned(),
            segments: Vec::new(),
        }
    }

    /// The pattern as written.
    pub fn as_str(&self) -> &str {
        &self.pattern
    }

    /// Whether `path` is a page of this pattern.
    pub fn matches(&self, path: &str) -> bool {
        let Some(rest) = path.strip_prefix('/') else {
            return false;
        };
        if self.segments.is_empty() {
            return rest.is_empty();
        }
        let parts: Vec<&str> = rest.split('/').collect();
        parts.len() == self.segments.len()
            && parts
                .iter()
                .zip(&self.segments)
                .all(|(part, segment)| match segment {
                    Some(literal) => part == literal,
                    None => is_key(part),
                })
    }
}

/// Whether `text` is a [`Key`]: 1 to 64 letters, digits, `-` and `_`.
fn is_key(text: &str) -> bool {
    (1..=MAX_KEY).contains(&text.len())
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

/// Reads a model's values: the problems found so far, and the pages it may link to.
#[derive(Debug)]
pub struct Checker<'a> {
    problems: Vec<Problem>,
    links: &'a [LinkPattern],
}

impl<'a> Checker<'a> {
    /// A checker for values whose links must match `links`.
    pub fn new(links: &'a [LinkPattern]) -> Self {
        Self {
            problems: Vec::new(),
            links,
        }
    }

    /// Notes that the value at `path` is wrong. Past [`MAX_PROBLEMS`] the rest are
    /// left out.
    pub fn problem(&mut self, path: &str, message: impl Into<String>) {
        if self.problems.len() < MAX_PROBLEMS {
            self.problems.push(Problem {
                path: path.to_owned(),
                message: message.into(),
            });
        }
    }

    /// The problems found.
    pub fn problems(&self) -> &[Problem] {
        &self.problems
    }

    /// Whether nothing was wrong.
    pub fn is_clean(&self) -> bool {
        self.problems.is_empty()
    }

    /// The problems, taken.
    pub fn into_problems(self) -> Vec<Problem> {
        self.problems
    }

    /// The pages links may go to.
    pub fn links(&self) -> &[LinkPattern] {
        self.links
    }
}

mod sealed {
    /// Only the toolkit's own types and its macros' types are props.
    pub trait Sealed {}
}

/// A value a model may choose for a component. The set is closed: the toolkit's own
/// types, `bool`, `Option`, and the types [`choices!`](crate::choices) and
/// [`props!`](crate::props) write. See the [module's docs](self).
pub trait Prop: Sized + Clone + fmt::Debug + Send + Sync + 'static + sealed::Sealed {
    /// Whether the property may be left out (an `Option`).
    const OPTIONAL: bool = false;

    /// The JSON Schema of the value, as the model reads it.
    fn schema() -> Value;

    /// Reads the value at `path`, or notes in `checker` what is wrong with it.
    fn check(value: &Value, path: &str, checker: &mut Checker<'_>) -> Option<Self>;

    /// The value when the property is left out, for one that may be.
    fn absent() -> Option<Self> {
        None
    }

    /// The value as JSON: what is stored, and what checks again to the same value.
    fn to_json(&self) -> Value;

    /// A value that passes the check, for examples and tests.
    fn example() -> Self;
}

/// The props of a component: a struct written with [`props!`](crate::props), whose
/// doc comment tells the model what the component shows.
pub trait Props: Prop {
    /// What the component shows, from the struct's doc comment.
    fn description() -> String;
}

/// Characters that are refused in text: controls other than a line break and a tab, and
/// the marks that turn text around or hide in it, with which text can read one way and
/// say another.
fn refused_char(c: char) -> bool {
    (c.is_control() && c != '\n' && c != '\t')
        || matches!(
            c,
            '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}'
        )
}

/// Text from outside the app's own code, such as what a model wrote: at most `MAX`
/// characters (and at most [`MAX_TEXT`]), not empty, with no control characters but a
/// line break and a tab, and none of the marks that turn text around. It is drawn as
/// text, never as markup.
///
/// ```
/// use navree_ui::genui::{Checker, Prop, UserText};
/// use serde_json::json;
///
/// let mut checker = Checker::new(&[]);
/// let caption = UserText::<40>::check(&json!("UPI this week"), "caption", &mut checker).unwrap();
/// assert_eq!(caption.as_str(), "UPI this week");
/// assert!(UserText::<5>::check(&json!("far too long"), "caption", &mut checker).is_none());
/// assert!(UserText::<40>::check(&json!("a\u{202E}b"), "caption", &mut checker).is_none());
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct UserText<const MAX: usize>(String);

impl<const MAX: usize> UserText<MAX> {
    const BOUNDED: () = assert!(
        MAX >= 1 && MAX <= MAX_TEXT,
        "a UserText holds 1 to 2000 characters"
    );

    /// The text, checked as a model's would be.
    pub fn new(text: &str) -> Result<Self, String> {
        let () = Self::BOUNDED;
        let text = text.trim();
        if text.is_empty() {
            return Err("must not be empty".to_owned());
        }
        if text.chars().count() > MAX {
            return Err(format!("must be at most {MAX} characters"));
        }
        if text.chars().any(refused_char) {
            return Err("must be plain text, without control characters".to_owned());
        }
        Ok(Self(text.to_owned()))
    }

    /// The text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The text, taken.
    pub fn into_string(self) -> String {
        self.0
    }
}

impl<const MAX: usize> fmt::Display for UserText<MAX> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<const MAX: usize> From<UserText<MAX>> for Node {
    fn from(text: UserText<MAX>) -> Self {
        Node::Text(text.0.into())
    }
}

impl<const MAX: usize> sealed::Sealed for UserText<MAX> {}

impl<const MAX: usize> Prop for UserText<MAX> {
    fn schema() -> Value {
        let () = Self::BOUNDED;
        json!({ "type": "string", "minLength": 1, "maxLength": MAX })
    }

    fn check(value: &Value, path: &str, checker: &mut Checker<'_>) -> Option<Self> {
        let Some(text) = value.as_str() else {
            checker.problem(path, "must be a string");
            return None;
        };
        match Self::new(text) {
            Ok(text) => Some(text),
            Err(message) => {
                checker.problem(path, message);
                None
            }
        }
    }

    fn to_json(&self) -> Value {
        Value::String(self.0.clone())
    }

    fn example() -> Self {
        let text: String = "Example".chars().take(MAX).collect();
        Self(text)
    }
}

/// A whole number from `MIN` to `MAX`. A number written with a zero fraction (`7.0`) is
/// read as the whole number it is.
///
/// ```
/// use navree_ui::genui::{Checker, Int, Prop};
/// use serde_json::json;
///
/// let mut checker = Checker::new(&[]);
/// assert_eq!(Int::<1, 12>::check(&json!(3), "months", &mut checker).unwrap().get(), 3);
/// assert!(Int::<1, 12>::check(&json!(13), "months", &mut checker).is_none());
/// assert!(Int::<1, 12>::check(&json!("3"), "months", &mut checker).is_none());
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Int<const MIN: i64, const MAX: i64>(i64);

impl<const MIN: i64, const MAX: i64> Int<MIN, MAX> {
    const ORDERED: () = assert!(MIN <= MAX, "an Int's least is at most its most");

    /// `value`, when it is in the range.
    pub fn new(value: i64) -> Option<Self> {
        let () = Self::ORDERED;
        (MIN..=MAX).contains(&value).then_some(Self(value))
    }

    /// The number.
    pub fn get(self) -> i64 {
        self.0
    }
}

impl<const MIN: i64, const MAX: i64> sealed::Sealed for Int<MIN, MAX> {}

impl<const MIN: i64, const MAX: i64> Prop for Int<MIN, MAX> {
    fn schema() -> Value {
        let () = Self::ORDERED;
        json!({ "type": "integer", "minimum": MIN, "maximum": MAX })
    }

    fn check(value: &Value, path: &str, checker: &mut Checker<'_>) -> Option<Self> {
        let number = match value {
            Value::Number(number) => number.as_i64().or_else(|| {
                number
                    .as_f64()
                    .filter(|f| f.fract() == 0.0 && f.abs() < 9.0e15)
                    .map(|f| f as i64)
            }),
            _ => None,
        };
        let Some(number) = number else {
            checker.problem(path, "must be a whole number");
            return None;
        };
        match Self::new(number) {
            Some(value) => Some(value),
            None => {
                checker.problem(path, format!("must be from {MIN} to {MAX}"));
                None
            }
        }
    }

    fn to_json(&self) -> Value {
        json!(self.0)
    }

    fn example() -> Self {
        Self(MIN.max(0).min(MAX))
    }
}

/// The key of a record, which the component loads under the reader's own access: 1 to
/// `MAX` (at most 64) letters, digits, `-` and `_`. A whole number the model wrote
/// (`1042`) is read as its digits.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Key<const MAX: usize = 64>(String);

impl<const MAX: usize> Key<MAX> {
    const BOUNDED: () = assert!(
        MAX >= 1 && MAX <= MAX_KEY,
        "a Key holds 1 to 64 characters"
    );

    /// `key`, when it is one.
    pub fn new(key: &str) -> Option<Self> {
        let () = Self::BOUNDED;
        (key.len() <= MAX && is_key(key)).then(|| Self(key.to_owned()))
    }

    /// The key.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<const MAX: usize> fmt::Display for Key<MAX> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<const MAX: usize> sealed::Sealed for Key<MAX> {}

impl<const MAX: usize> Prop for Key<MAX> {
    fn schema() -> Value {
        let () = Self::BOUNDED;
        json!({ "type": "string", "pattern": "^[A-Za-z0-9_-]+$", "minLength": 1, "maxLength": MAX })
    }

    fn check(value: &Value, path: &str, checker: &mut Checker<'_>) -> Option<Self> {
        let text = match value {
            Value::String(text) => text.clone(),
            Value::Number(number) if number.as_u64().is_some() => number.to_string(),
            _ => {
                checker.problem(path, "must be a record's key, as a string");
                return None;
            }
        };
        match Self::new(&text) {
            Some(key) => Some(key),
            None => {
                checker.problem(
                    path,
                    format!("must be 1 to {MAX} letters, digits, `-` and `_`"),
                );
                None
            }
        }
    }

    fn to_json(&self) -> Value {
        Value::String(self.0.clone())
    }

    fn example() -> Self {
        Self("1".to_owned())
    }
}

/// A calendar day, written `2026-11-06`, from the year 1900 to 2200.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Day(NaiveDate);

impl Day {
    /// The day, when it is in range.
    pub fn new(day: NaiveDate) -> Option<Self> {
        (1900..=2200).contains(&day.year()).then_some(Self(day))
    }

    /// The day.
    pub fn date(self) -> NaiveDate {
        self.0
    }
}

impl sealed::Sealed for Day {}

impl Prop for Day {
    fn schema() -> Value {
        json!({ "type": "string", "format": "date" })
    }

    fn check(value: &Value, path: &str, checker: &mut Checker<'_>) -> Option<Self> {
        let day = value
            .as_str()
            .filter(|text| text.len() == 10)
            .and_then(|text| NaiveDate::parse_from_str(text, "%Y-%m-%d").ok())
            .and_then(Self::new);
        if day.is_none() {
            checker.problem(path, "must be a day written YYYY-MM-DD, from 1900 to 2200");
        }
        day
    }

    fn to_json(&self) -> Value {
        Value::String(self.0.format("%Y-%m-%d").to_string())
    }

    fn example() -> Self {
        Self(NaiveDate::from_ymd_opt(2026, 11, 6).expect("a real day"))
    }
}

/// A list of at most `MAX` props (and at most [`MAX_LIST`]).
#[derive(Clone, Debug, PartialEq)]
pub struct List<P, const MAX: usize>(Vec<P>);

impl<P: Prop, const MAX: usize> List<P, MAX> {
    const BOUNDED: () = assert!(
        MAX >= 1 && MAX <= MAX_LIST,
        "a List holds at most 100 items"
    );

    /// `items`, when there are at most `MAX`.
    pub fn new(items: Vec<P>) -> Option<Self> {
        let () = Self::BOUNDED;
        (items.len() <= MAX).then_some(Self(items))
    }

    /// The items.
    pub fn items(&self) -> &[P] {
        &self.0
    }

    /// The items, taken.
    pub fn into_vec(self) -> Vec<P> {
        self.0
    }
}

impl<P: Prop, const MAX: usize> sealed::Sealed for List<P, MAX> {}

impl<P: Prop, const MAX: usize> Prop for List<P, MAX> {
    fn schema() -> Value {
        let () = Self::BOUNDED;
        json!({ "type": "array", "items": P::schema(), "maxItems": MAX })
    }

    fn check(value: &Value, path: &str, checker: &mut Checker<'_>) -> Option<Self> {
        let () = Self::BOUNDED;
        let Some(items) = value.as_array() else {
            checker.problem(path, "must be a list");
            return None;
        };
        if items.len() > MAX {
            checker.problem(path, format!("must hold at most {MAX} items"));
            return None;
        }
        let mut out = Vec::with_capacity(items.len());
        let mut whole = true;
        for (at, item) in items.iter().enumerate() {
            match P::check(item, &format!("{path}[{at}]"), checker) {
                Some(item) => out.push(item),
                None => whole = false,
            }
        }
        whole.then_some(Self(out))
    }

    fn to_json(&self) -> Value {
        Value::Array(self.0.iter().map(Prop::to_json).collect())
    }

    fn example() -> Self {
        Self(vec![P::example()])
    }
}

/// A page of this app a component links to: a path that matches one of the
/// catalogue's [link patterns](crate::genui::Catalog::link), with no query, no
/// fragment and no host.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AppLink(String);

impl AppLink {
    /// The link, as a URL on this site.
    pub fn href(&self) -> LocalHref {
        LocalHref::from(self.0.as_str())
    }

    /// The path.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl sealed::Sealed for AppLink {}

impl Prop for AppLink {
    fn schema() -> Value {
        json!({
            "type": "string",
            "pattern": "^/[A-Za-z0-9_./-]*$",
            "maxLength": 200,
            "description": "A page of this app, from the list of pages you may link to."
        })
    }

    fn check(value: &Value, path: &str, checker: &mut Checker<'_>) -> Option<Self> {
        let Some(text) = value.as_str() else {
            checker.problem(path, "must be a path of this app, as a string");
            return None;
        };
        let plain = text.len() <= 200
            && text.starts_with('/')
            && !text.starts_with("//")
            && LocalHref::parse(text).is_ok()
            && text
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'_' | b'.'));
        if plain && checker.links().iter().any(|pattern| pattern.matches(text)) {
            return Some(Self(text.to_owned()));
        }
        let pages: Vec<&str> = checker.links().iter().map(LinkPattern::as_str).collect();
        if pages.is_empty() {
            checker.problem(path, "no page of this app may be linked to here");
        } else {
            checker.problem(
                path,
                format!("must be a page of this app: one of {}", pages.join(", ")),
            );
        }
        None
    }

    fn to_json(&self) -> Value {
        Value::String(self.0.clone())
    }

    /// The home page, `/`: a catalogue's examples are checked with it allowed.
    fn example() -> Self {
        Self("/".to_owned())
    }
}

impl sealed::Sealed for bool {}

impl Prop for bool {
    fn schema() -> Value {
        json!({ "type": "boolean" })
    }

    fn check(value: &Value, path: &str, checker: &mut Checker<'_>) -> Option<Self> {
        let flag = value.as_bool();
        if flag.is_none() {
            checker.problem(path, "must be true or false");
        }
        flag
    }

    fn to_json(&self) -> Value {
        Value::Bool(*self)
    }

    fn example() -> Self {
        false
    }
}

impl<P: Prop> sealed::Sealed for Option<P> {}

impl<P: Prop> Prop for Option<P> {
    const OPTIONAL: bool = true;

    fn schema() -> Value {
        P::schema()
    }

    fn check(value: &Value, path: &str, checker: &mut Checker<'_>) -> Option<Self> {
        if value.is_null() {
            return Some(None);
        }
        P::check(value, path, checker).map(Some)
    }

    fn absent() -> Option<Self> {
        Some(None)
    }

    fn to_json(&self) -> Value {
        self.as_ref().map_or(Value::Null, Prop::to_json)
    }

    fn example() -> Self {
        Some(P::example())
    }
}

/// What [`props!`](crate::props) and [`choices!`](crate::choices) expand to. Not a
/// public API.
#[doc(hidden)]
pub mod __private {
    pub use super::sealed::Sealed;
    use super::{Checker, Prop};
    pub use serde_json::{Map, Value};

    /// A doc comment's lines as one line of text.
    pub fn doc_text(doc: &str) -> String {
        doc.lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// One field of a struct of props, for its schema.
    pub struct FieldSchema {
        pub name: &'static str,
        pub doc: &'static str,
        pub schema: Value,
        pub optional: bool,
    }

    /// The schema of a struct of props: an object with exactly these properties.
    pub fn object_schema(doc: &str, fields: Vec<FieldSchema>) -> Value {
        let mut properties = Map::new();
        let mut required = Vec::new();
        for field in fields {
            let mut schema = field.schema;
            let doc = doc_text(field.doc);
            if let Value::Object(map) = &mut schema
                && !doc.is_empty()
            {
                let described = match map.get("description").and_then(Value::as_str) {
                    Some(own) => format!("{doc} {own}"),
                    None => doc,
                };
                map.insert("description".to_owned(), Value::String(described));
            }
            if !field.optional {
                required.push(Value::String(field.name.to_owned()));
            }
            properties.insert(field.name.to_owned(), schema);
        }
        let mut schema = Map::new();
        schema.insert("type".to_owned(), Value::String("object".to_owned()));
        let doc = doc_text(doc);
        if !doc.is_empty() {
            schema.insert("description".to_owned(), Value::String(doc));
        }
        schema.insert("properties".to_owned(), Value::Object(properties));
        schema.insert("required".to_owned(), Value::Array(required));
        schema.insert("additionalProperties".to_owned(), Value::Bool(false));
        Value::Object(schema)
    }

    /// The object at `path`, and whether it holds only properties of `names` (each other
    /// one is a problem). Its fields are checked either way, so every problem is told.
    pub fn object_of<'v>(
        value: &'v Value,
        path: &str,
        names: &[&str],
        checker: &mut Checker<'_>,
    ) -> Option<(&'v Map<String, Value>, bool)> {
        let Some(object) = value.as_object() else {
            checker.problem(path, "must be an object");
            return None;
        };
        let mut clean = true;
        for key in object.keys() {
            if !names.contains(&key.as_str()) {
                clean = false;
                let shown: String = key.chars().take(40).collect();
                checker.problem(
                    &join(path, &shown),
                    format!("is not a property here; the properties are: {}", names.join(", ")),
                );
            }
        }
        Some((object, clean))
    }

    /// `path.name`, or `name` at the top.
    pub fn join(path: &str, name: &str) -> String {
        if path.is_empty() {
            name.to_owned()
        } else {
            format!("{path}.{name}")
        }
    }

    /// The field `name` of `object`: checked, or its value when left out.
    pub fn field<P: Prop>(
        object: &Map<String, Value>,
        name: &str,
        path: &str,
        checker: &mut Checker<'_>,
    ) -> Option<P> {
        let at = join(path, name);
        match object.get(name) {
            Some(value) => P::check(value, &at, checker),
            None => match P::absent() {
                Some(value) => Some(value),
                None => {
                    checker.problem(&at, "is required");
                    None
                }
            },
        }
    }

    /// Puts field `name` into `map`, leaving out an absent optional one.
    pub fn put<P: Prop>(map: &mut Map<String, Value>, name: &str, value: &P) {
        let json = value.to_json();
        if !(P::OPTIONAL && json.is_null()) {
            map.insert(name.to_owned(), json);
        }
    }

    /// The schema of a closed choice: its words, each described.
    pub fn choice_schema(doc: &str, values: &[&str], docs: &[&str]) -> Value {
        let described: Vec<String> = values
            .iter()
            .zip(docs)
            .map(|(value, doc)| {
                let doc = doc_text(doc);
                if doc.is_empty() {
                    (*value).to_owned()
                } else {
                    format!("{value}: {doc}")
                }
            })
            .collect();
        let mut description = doc_text(doc);
        if !description.is_empty() {
            description.push(' ');
        }
        description.push_str(&described.join("; "));
        serde_json::json!({ "type": "string", "enum": values, "description": description })
    }

    /// The choice the model wrote, or a problem naming the words it may write.
    pub fn choice_of(
        value: &Value,
        path: &str,
        values: &[&str],
        checker: &mut Checker<'_>,
    ) -> Option<usize> {
        let found = value
            .as_str()
            .and_then(|text| values.iter().position(|word| *word == text));
        if found.is_none() {
            checker.problem(path, format!("must be one of: {}", values.join(", ")));
        }
        found
    }
}

/// Writes a closed choice a model may make: an enum, and the word the model writes
/// for each variant. Each variant's doc comment tells the model what it means.
///
/// ```
/// use navree_ui::genui::{Checker, Prop};
/// use serde_json::json;
///
/// navree_ui::choices! {
///     /// The days to count.
///     pub enum Period {
///         /// Today so far.
///         Today = "today",
///         /// Monday to now.
///         ThisWeek = "this_week",
///     }
/// }
///
/// assert_eq!(Period::ThisWeek.as_str(), "this_week");
/// assert_eq!(Period::schema()["enum"], json!(["today", "this_week"]));
/// let mut checker = Checker::new(&[]);
/// assert_eq!(Period::check(&json!("today"), "period", &mut checker), Some(Period::Today));
/// assert_eq!(Period::check(&json!("This week"), "period", &mut checker), None);
/// ```
#[macro_export]
macro_rules! choices {
    (
        $(#[doc = $doc:literal])*
        $vis:vis enum $name:ident {
            $( $(#[doc = $vdoc:literal])* $variant:ident = $word:literal ),+ $(,)?
        }
    ) => {
        $(#[doc = $doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        $vis enum $name {
            $( $(#[doc = $vdoc])* $variant ),+
        }

        impl $name {
            /// Every choice, in order.
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            /// The word the model writes for it.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $word),+
                }
            }
        }

        impl $crate::genui::__private::Sealed for $name {}

        impl $crate::genui::Prop for $name {
            fn schema() -> $crate::genui::__private::Value {
                $crate::genui::__private::choice_schema(
                    concat!($($doc, "\n",)* ""),
                    &[$($word),+],
                    &[$(concat!($($vdoc, "\n",)* "")),+],
                )
            }

            fn check(
                value: &$crate::genui::__private::Value,
                path: &str,
                checker: &mut $crate::genui::Checker<'_>,
            ) -> ::std::option::Option<Self> {
                $crate::genui::__private::choice_of(value, path, &[$($word),+], checker)
                    .map(|at| Self::ALL[at])
            }

            fn to_json(&self) -> $crate::genui::__private::Value {
                $crate::genui::__private::Value::String(self.as_str().to_owned())
            }

            fn example() -> Self {
                Self::ALL[0]
            }
        }
    };
}

/// Writes the props of a component: a struct whose fields are all [`Prop`]s, its
/// JSON Schema (each field's doc comment describes it to the model; an `Option` may be
/// left out; no other property is allowed), its check, and its example. The struct's
/// doc comment says what the component shows.
///
/// ```
/// use navree_ui::genui::{Checker, Int, Prop, Props, UserText};
/// use serde_json::json;
///
/// navree_ui::props! {
///     /// The orders a customer placed, newest first.
///     pub struct RecentOrders {
///         /// How many orders to show.
///         pub count: Int<1, 20>,
///         /// A heading to show above them. Leave out for the usual one.
///         pub heading: Option<UserText<60>>,
///     }
/// }
///
/// assert!(RecentOrders::description().starts_with("The orders a customer placed"));
/// let schema = RecentOrders::schema();
/// assert_eq!(schema["required"], json!(["count"]));
/// assert_eq!(schema["additionalProperties"], json!(false));
/// let mut checker = Checker::new(&[]);
/// let props = RecentOrders::check(&json!({ "count": 5 }), "", &mut checker).unwrap();
/// assert_eq!(props.count.get(), 5);
/// assert!(RecentOrders::check(&json!({ "count": 5, "style": "x" }), "", &mut checker).is_none());
/// ```
#[macro_export]
macro_rules! props {
    (
        $(#[doc = $doc:literal])*
        $vis:vis struct $name:ident {
            $( $(#[doc = $fdoc:literal])* $fvis:vis $field:ident : $ty:ty ),* $(,)?
        }
    ) => {
        $(#[doc = $doc])*
        #[derive(Clone, Debug, PartialEq)]
        $vis struct $name {
            $( $(#[doc = $fdoc])* $fvis $field: $ty ),*
        }

        impl $crate::genui::__private::Sealed for $name {}

        impl $crate::genui::Prop for $name {
            fn schema() -> $crate::genui::__private::Value {
                #[allow(unused_mut)]
                let mut fields = ::std::vec::Vec::new();
                $(
                    fields.push($crate::genui::__private::FieldSchema {
                        name: stringify!($field),
                        doc: concat!($($fdoc, "\n",)* ""),
                        schema: <$ty as $crate::genui::Prop>::schema(),
                        optional: <$ty as $crate::genui::Prop>::OPTIONAL,
                    });
                )*
                $crate::genui::__private::object_schema(concat!($($doc, "\n",)* ""), fields)
            }

            #[allow(unused_variables)]
            fn check(
                value: &$crate::genui::__private::Value,
                path: &str,
                checker: &mut $crate::genui::Checker<'_>,
            ) -> ::std::option::Option<Self> {
                let (object, clean) = $crate::genui::__private::object_of(
                    value,
                    path,
                    &[$(stringify!($field)),*],
                    checker,
                )?;
                $(
                    let $field = $crate::genui::__private::field::<$ty>(
                        object,
                        stringify!($field),
                        path,
                        checker,
                    );
                )*
                if !clean {
                    return ::std::option::Option::None;
                }
                ::std::option::Option::Some(Self { $( $field: $field? ),* })
            }

            fn to_json(&self) -> $crate::genui::__private::Value {
                #[allow(unused_mut)]
                let mut map = $crate::genui::__private::Map::new();
                $( $crate::genui::__private::put(&mut map, stringify!($field), &self.$field); )*
                $crate::genui::__private::Value::Object(map)
            }

            fn example() -> Self {
                Self { $( $field: <$ty as $crate::genui::Prop>::example() ),* }
            }
        }

        impl $crate::genui::Props for $name {
            fn description() -> ::std::string::String {
                $crate::genui::__private::doc_text(concat!($($doc, "\n",)* ""))
            }
        }
    };
}

/// Walks `value` without recursion and says whether it fits: at most `max_depth`
/// levels of lists and objects, at most `max_values` values in all.
pub(crate) fn fits(value: &Value, max_depth: usize, max_values: usize) -> Result<(), String> {
    let mut stack = vec![(value, 1usize)];
    let mut seen = 0usize;
    while let Some((value, depth)) = stack.pop() {
        seen += 1;
        if seen > max_values {
            return Err(format!("must hold at most {max_values} values in all"));
        }
        match value {
            Value::Array(items) => {
                if depth > max_depth {
                    return Err(format!("must nest at most {max_depth} levels deep"));
                }
                stack.extend(items.iter().map(|item| (item, depth + 1)));
            }
            Value::Object(map) => {
                if depth > max_depth {
                    return Err(format!("must nest at most {max_depth} levels deep"));
                }
                stack.extend(map.values().map(|item| (item, depth + 1)));
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    crate::choices! {
        /// A period.
        pub enum Period {
            /// Today.
            Today = "today",
            /// This week.
            ThisWeek = "this_week",
        }
    }

    crate::props! {
        /// A test surface.
        pub struct Shown {
            /// When.
            pub period: Period,
            /// How many.
            pub count: Int<1, 10>,
            /// A caption.
            pub caption: Option<UserText<20>>,
            /// Rows.
            pub rows: List<Key, 3>,
            /// A page.
            pub page: Option<AppLink>,
        }
    }

    fn links() -> Vec<LinkPattern> {
        vec![LinkPattern::parse("/orders/{key}").unwrap()]
    }

    #[test]
    fn a_value_checks_to_itself_and_back() {
        let links = links();
        let mut checker = Checker::new(&links);
        let value = json!({
            "period": "this_week",
            "count": 3,
            "caption": "UPI",
            "rows": ["1042", 7],
            "page": "/orders/1042"
        });
        let shown = Shown::check(&value, "", &mut checker).expect("valid");
        assert!(checker.is_clean());
        assert_eq!(shown.rows.items()[1].as_str(), "7");
        let again = Shown::check(&shown.to_json(), "", &mut checker).unwrap();
        assert_eq!(again, shown);
    }

    #[test]
    fn every_problem_is_named_with_its_path() {
        let links = links();
        let mut checker = Checker::new(&links);
        let value = json!({
            "period": "THIS WEEK",
            "count": 11,
            "caption": "<script>alert(1)</script> and far too long for twenty",
            "rows": ["ok", "no spaces", "x", "y"],
            "page": "https://evil.example/orders/1",
        });
        assert!(Shown::check(&value, "", &mut checker).is_none());
        let paths: Vec<&str> = checker.problems().iter().map(|p| p.path.as_str()).collect();
        assert!(paths.contains(&"period"), "{paths:?}");
        assert!(paths.contains(&"count"), "{paths:?}");
        assert!(paths.contains(&"caption"), "{paths:?}");
        assert!(paths.contains(&"rows"), "{paths:?}");
        assert!(paths.contains(&"page"), "{paths:?}");
    }

    #[test]
    fn unknown_and_missing_properties_are_refused() {
        let mut checker = Checker::new(&[]);
        let value = json!({ "count": 1, "rows": [], "onclick": "x", "__proto__": {} });
        assert!(Shown::check(&value, "", &mut checker).is_none());
        let text: Vec<String> = checker.problems().iter().map(ToString::to_string).collect();
        assert!(text.iter().any(|p| p.starts_with("onclick:")), "{text:?}");
        assert!(text.iter().any(|p| p.starts_with("__proto__:")), "{text:?}");
    }

    #[test]
    fn links_go_only_to_the_named_pages() {
        let links = links();
        for bad in [
            "/orders/1042?x=1",
            "/orders/1042/../admin",
            "//evil.example",
            "/\\evil.example",
            "javascript:alert(1)",
            "/orders",
            "/orders/a b",
            "/admin/1",
        ] {
            let mut checker = Checker::new(&links);
            assert!(AppLink::check(&json!(bad), "page", &mut checker).is_none(), "{bad}");
        }
        let mut checker = Checker::new(&links);
        assert!(AppLink::check(&json!("/orders/CS-1042"), "page", &mut checker).is_some());
        assert!(LinkPattern::parse("/orders/{id}").is_err());
        assert!(LinkPattern::parse("orders").is_err());
        assert!(LinkPattern::parse("/a/../b").is_err());
    }

    #[test]
    fn text_refuses_controls_and_turned_marks() {
        for bad in ["", "   ", "a\u{0000}b", "a\u{202E}b", "a\u{2066}b", "\u{200B}"] {
            assert!(UserText::<40>::new(bad).is_err(), "{bad:?}");
        }
        assert_eq!(UserText::<40>::new("  two\nlines ").unwrap().as_str(), "two\nlines");
    }

    #[test]
    fn deep_or_wide_values_do_not_fit() {
        let mut deep = json!(1);
        for _ in 0..20 {
            deep = json!([deep]);
        }
        assert!(fits(&deep, 8, 1000).is_err());
        let wide = Value::Array((0..2000).map(|n| json!(n)).collect());
        assert!(fits(&wide, 8, 512).is_err());
        assert!(fits(&json!({ "a": [1, 2, { "b": 3 }] }), 8, 512).is_ok());
    }
}
