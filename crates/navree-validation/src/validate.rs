//! [`Validate`], the trait `#[derive(Validate)]` implements, with the
//! [`FieldPath`] rules report at, the [`Violation`] one failed rule is
//! and the [`Violations`] of a whole value.
//!
//! A violation is the rule key and its parameters (`min.string` with
//! `min = 1`), or a text of your own: the message is picked only when it
//! is rendered, from the app's [`Messages`](crate::Messages) (or in the
//! request's language), so every rule's message can be overridden.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::fmt::{self, Display};
use std::rc::Rc;
use std::sync::Arc;

use crate::Messages;

/// A value whose rules can be checked: what `#[derive(Validate)]`
/// implements from the fields' `#[validate(...)]` rules.
///
/// ```
/// use navree_validation::Validate;
///
/// #[derive(Validate)]
/// struct NewTeam {
///     #[validate(length(min = 1, max = 100))]
///     name: String,
///     #[validate(range(min = 1, max = 500))]
///     seats: u32,
/// }
///
/// let violations = NewTeam { name: String::new(), seats: 0 }.validate().unwrap_err();
/// assert_eq!(violations.len(), 2);
/// assert_eq!(
///     violations.to_string(),
///     "name must be longer than or equal to 1 characters\n\
///      seats must not be less than 1"
/// );
/// ```
///
/// Implement it by hand for a type whose values are valid by construction
/// (an empty body), so fields of that type can be `nested`.
#[diagnostic::on_unimplemented(
    message = "`{Self}` has no rules of its own to check",
    label = "derive `Validate` for it; `nested` takes such types (and lists, maps and `Option`s of them), text and numbers take rules"
)]
pub trait Validate {
    /// Checks every rule, recording each failure under `path` in `out`.
    fn validate_at(&self, path: &FieldPath<'_>, out: &mut Violations);

    /// Checks every rule: `Err` lists every failure, by field path.
    fn validate(&self) -> Result<(), Violations> {
        let mut out = Violations::new();
        self.validate_at(&FieldPath::root(), &mut out);
        out.into_result()
    }
}

/// Where a rule applies: a field inside the value being validated
/// (`customer.address.city`, `items.1.sku`), built as the rules descend
/// without allocating; [`to_dotted`](FieldPath::to_dotted) writes it out
/// when a rule fails.
#[derive(Clone, Copy)]
pub struct FieldPath<'a> {
    parent: Option<&'a FieldPath<'a>>,
    segment: Segment<'a>,
}

#[derive(Clone, Copy)]
enum Segment<'a> {
    Root,
    Field(&'a str),
    Index(usize),
    Key(&'a dyn Display),
}

impl FieldPath<'static> {
    /// The value itself (an empty path).
    pub const fn root() -> Self {
        FieldPath {
            parent: None,
            segment: Segment::Root,
        }
    }
}

impl<'a> FieldPath<'a> {
    /// The field `name` of this value.
    pub fn field<'b>(&'b self, name: &'b str) -> FieldPath<'b> {
        FieldPath {
            parent: Some(self),
            segment: Segment::Field(name),
        }
    }

    /// The item at `index` of this list.
    pub fn index(&self, index: usize) -> FieldPath<'_> {
        FieldPath {
            parent: Some(self),
            segment: Segment::Index(index),
        }
    }

    /// The entry `key` of this map.
    pub fn key<'b>(&'b self, key: &'b dyn Display) -> FieldPath<'b> {
        FieldPath {
            parent: Some(self),
            segment: Segment::Key(key),
        }
    }

    /// The dotted path: `items.1.sku`; empty for the root.
    pub fn to_dotted(&self) -> String {
        let mut segments = Vec::new();
        let mut at = Some(self);
        while let Some(path) = at {
            if !matches!(path.segment, Segment::Root) {
                segments.push(path.segment);
            }
            at = path.parent;
        }
        let mut out = String::new();
        for segment in segments.iter().rev() {
            if !out.is_empty() {
                out.push('.');
            }
            match segment {
                Segment::Root => {}
                Segment::Field(name) => out.push_str(name),
                Segment::Index(index) => out.push_str(&index.to_string()),
                Segment::Key(key) => out.push_str(&key.to_string()),
            }
        }
        out
    }
}

impl fmt::Debug for FieldPath<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("FieldPath").field(&self.to_dotted()).finish()
    }
}

/// One failed rule: a rule key from the message table with its parameters
/// (`min.string`, `min = 1`), or a text of your own.
///
/// Custom rules return it (or anything that converts into it: a `String`
/// or `&str` is a text, which may use `:attribute` for the field's name):
///
/// ```
/// use navree_validation::{Validate, Violation};
///
/// fn even(value: &u32) -> Result<(), Violation> {
///     if value.is_multiple_of(2) {
///         Ok(())
///     } else {
///         // Rendered from the table's `even` message (`invalid` until
///         // the app adds one), with `:value` filled in.
///         Err(Violation::rule("even").with("value", value))
///     }
/// }
///
/// fn not_blank(value: &str) -> Result<(), &'static str> {
///     if value.trim().is_empty() { Err(":attribute must not be blank") } else { Ok(()) }
/// }
///
/// #[derive(Validate)]
/// struct Team {
///     #[validate(custom(even))]
///     size: u32,
///     #[validate(custom(not_blank))]
///     name: String,
/// }
///
/// let violations = Team { size: 3, name: " ".into() }.validate().unwrap_err();
/// assert_eq!(violations.get("size")[0].key(), Some("even"));
/// assert_eq!(violations.get("name")[0].message(), "value must not be blank");
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Violation(Kind);

#[derive(Clone, Debug, PartialEq, Eq)]
enum Kind {
    Rule {
        key: Cow<'static, str>,
        params: Vec<(Cow<'static, str>, String)>,
    },
    Text(Cow<'static, str>),
}

impl Violation {
    /// The rule `key` failed (`email`, `min.numeric`, or a key of your own):
    /// the message is the table's template for it.
    pub fn rule(key: impl Into<Cow<'static, str>>) -> Self {
        Violation(Kind::Rule {
            key: key.into(),
            params: Vec::new(),
        })
    }

    /// A message of your own (`:attribute` is the field's display name).
    pub fn text(text: impl Into<Cow<'static, str>>) -> Self {
        Violation(Kind::Text(text.into()))
    }

    /// Adds the parameter `name` (`:name` in the template); a text has none.
    pub fn with(mut self, name: impl Into<Cow<'static, str>>, value: impl Display) -> Self {
        if let Kind::Rule { params, .. } = &mut self.0 {
            params.push((name.into(), value.to_string()));
        }
        self
    }

    /// The rule key, for a rule violation.
    pub fn key(&self) -> Option<&str> {
        match &self.0 {
            Kind::Rule { key, .. } => Some(key),
            Kind::Text(_) => None,
        }
    }

    /// The parameters, in order.
    pub fn params(&self) -> impl Iterator<Item = (&str, &str)> {
        let params: &[(Cow<'static, str>, String)] = match &self.0 {
            Kind::Rule { params, .. } => params,
            Kind::Text(_) => &[],
        };
        params
            .iter()
            .map(|(name, value)| (name.as_ref(), value.as_str()))
    }

    /// The text, for a violation with a message of its own.
    pub fn as_text(&self) -> Option<&str> {
        match &self.0 {
            Kind::Text(text) => Some(text),
            Kind::Rule { .. } => None,
        }
    }

    /// The message with the built-in templates, for a field called
    /// `value` (the extractors render with the app's table and the field's
    /// name instead).
    pub fn message(&self) -> String {
        self.render("value", Messages::shared_default().as_ref())
    }

    /// The message for `field` with `messages`.
    pub fn render(&self, field: &str, messages: &Messages) -> String {
        match &self.0 {
            Kind::Text(text) => text.replace(":attribute", &messages.attribute(field)),
            Kind::Rule { key, params } => {
                let params: Vec<(&str, String)> = params
                    .iter()
                    .map(|(name, value)| (name.as_ref(), value.clone()))
                    .collect();
                messages.render(field, crate::failure::resolve_key(key, None), &params)
            }
        }
    }
}

impl Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for Violation {}

impl From<String> for Violation {
    fn from(text: String) -> Self {
        Violation::text(text)
    }
}

impl From<&str> for Violation {
    fn from(text: &str) -> Self {
        Violation::text(text.to_owned())
    }
}

impl From<Cow<'static, str>> for Violation {
    fn from(text: Cow<'static, str>) -> Self {
        Violation::text(text)
    }
}

/// Every failed rule of a value, by dotted field path, in the order the
/// rules ran.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Violations(Vec<(String, Violation)>);

impl Violations {
    /// No violations.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records `violation` at `path`.
    pub fn push(&mut self, path: &FieldPath<'_>, violation: Violation) {
        self.0.push((path.to_dotted(), violation));
    }

    /// Records `violation` at the dotted path `path` (`""` for the value
    /// itself).
    pub fn add(&mut self, path: impl Into<String>, violation: Violation) {
        self.0.push((path.into(), violation));
    }

    /// Whether every rule passed.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The number of failed rules.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// The violations at `path`.
    pub fn get(&self, path: &str) -> Vec<&Violation> {
        self.0
            .iter()
            .filter(|(at, _)| at == path)
            .map(|(_, violation)| violation)
            .collect()
    }

    /// Every violation with its path.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Violation)> {
        self.0
            .iter()
            .map(|(path, violation)| (path.as_str(), violation))
    }

    /// `Ok` when there are none.
    pub fn into_result(self) -> Result<(), Violations> {
        if self.is_empty() { Ok(()) } else { Err(self) }
    }

    /// Every message with the built-in templates, by path (fields named by
    /// their path), as [`ValidationErrors`](crate::ValidationErrors): the
    /// extractors' error, without the app's messages.
    pub fn to_errors(&self, messages: &Messages) -> crate::ValidationErrors {
        let mut errors = crate::ValidationErrors::new();
        for (path, violation) in &self.0 {
            errors.add(path.clone(), violation.render(path, messages));
        }
        errors
    }
}

impl IntoIterator for Violations {
    type Item = (String, Violation);
    type IntoIter = std::vec::IntoIter<(String, Violation)>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

/// One message per line, with the built-in templates (the value itself is
/// called `value`).
impl Display for Violations {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let messages = Messages::shared_default();
        for (i, (path, violation)) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str("\n")?;
            }
            let field = if path.is_empty() { "value" } else { path };
            f.write_str(&violation.render(field, &messages))?;
        }
        Ok(())
    }
}

impl std::error::Error for Violations {}

// Containers of validated values: `nested` descends into them.

impl<T: Validate + ?Sized> Validate for &T {
    fn validate_at(&self, path: &FieldPath<'_>, out: &mut Violations) {
        (**self).validate_at(path, out);
    }
}

impl<T: Validate + ?Sized> Validate for Box<T> {
    fn validate_at(&self, path: &FieldPath<'_>, out: &mut Violations) {
        (**self).validate_at(path, out);
    }
}

impl<T: Validate + ?Sized> Validate for Arc<T> {
    fn validate_at(&self, path: &FieldPath<'_>, out: &mut Violations) {
        (**self).validate_at(path, out);
    }
}

impl<T: Validate + ?Sized> Validate for Rc<T> {
    fn validate_at(&self, path: &FieldPath<'_>, out: &mut Violations) {
        (**self).validate_at(path, out);
    }
}

/// `None` has nothing to check (`required` demands a value).
impl<T: Validate> Validate for Option<T> {
    fn validate_at(&self, path: &FieldPath<'_>, out: &mut Violations) {
        if let Some(value) = self {
            value.validate_at(path, out);
        }
    }
}

impl<T: Validate> Validate for [T] {
    fn validate_at(&self, path: &FieldPath<'_>, out: &mut Violations) {
        for (index, item) in self.iter().enumerate() {
            item.validate_at(&path.index(index), out);
        }
    }
}

impl<T: Validate, const N: usize> Validate for [T; N] {
    fn validate_at(&self, path: &FieldPath<'_>, out: &mut Violations) {
        self.as_slice().validate_at(path, out);
    }
}

impl<T: Validate> Validate for Vec<T> {
    fn validate_at(&self, path: &FieldPath<'_>, out: &mut Violations) {
        self.as_slice().validate_at(path, out);
    }
}

impl<T: Validate> Validate for VecDeque<T> {
    fn validate_at(&self, path: &FieldPath<'_>, out: &mut Violations) {
        for (index, item) in self.iter().enumerate() {
            item.validate_at(&path.index(index), out);
        }
    }
}

impl<T: Validate, S> Validate for HashSet<T, S> {
    fn validate_at(&self, path: &FieldPath<'_>, out: &mut Violations) {
        for (index, item) in self.iter().enumerate() {
            item.validate_at(&path.index(index), out);
        }
    }
}

impl<T: Validate> Validate for BTreeSet<T> {
    fn validate_at(&self, path: &FieldPath<'_>, out: &mut Violations) {
        for (index, item) in self.iter().enumerate() {
            item.validate_at(&path.index(index), out);
        }
    }
}

/// Values at their key (`prices.EUR.amount`).
impl<K: Display, V: Validate, S> Validate for HashMap<K, V, S> {
    fn validate_at(&self, path: &FieldPath<'_>, out: &mut Violations) {
        for (key, value) in self {
            value.validate_at(&path.key(key), out);
        }
    }
}

/// Values at their key (`prices.EUR.amount`).
impl<K: Display, V: Validate> Validate for BTreeMap<K, V> {
    fn validate_at(&self, path: &FieldPath<'_>, out: &mut Violations) {
        for (key, value) in self {
            value.validate_at(&path.key(key), out);
        }
    }
}

/// Nothing to check.
impl Validate for () {
    fn validate_at(&self, _path: &FieldPath<'_>, _out: &mut Violations) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_dotted() {
        let root = FieldPath::root();
        assert_eq!(root.to_dotted(), "");
        let items = root.field("items");
        let first = items.index(1);
        let key = "EUR";
        let deep = first.field("prices");
        let deep = deep.key(&key);
        assert_eq!(deep.to_dotted(), "items.1.prices.EUR");
        assert_eq!(format!("{deep:?}"), "FieldPath(\"items.1.prices.EUR\")");
    }

    #[test]
    fn violations_render_and_convert() {
        let rule = Violation::rule("min.numeric").with("min", 18);
        assert_eq!(rule.key(), Some("min.numeric"));
        assert_eq!(rule.params().collect::<Vec<_>>(), [("min", "18")]);
        assert_eq!(rule.message(), "value must not be less than 18");
        let text = Violation::from(":attribute must be odd");
        assert_eq!(text.as_text(), Some(":attribute must be odd"));
        assert_eq!(text.clone().with("x", 1), text);
        assert_eq!(text.message(), "value must be odd");
        assert_eq!(
            text.render(
                "count",
                &Messages::default().with_attribute("count", "the count")
            ),
            "the count must be odd"
        );
        // An unknown key renders the `invalid` template.
        assert_eq!(Violation::rule("even").message(), "value is invalid");
    }

    #[test]
    fn violations_collect_by_path() {
        let mut out = Violations::new();
        assert!(out.clone().into_result().is_ok());
        let root = FieldPath::root();
        out.push(&root.field("name"), Violation::rule("required"));
        out.add("name", Violation::text("other"));
        out.add("", Violation::rule("object"));
        assert_eq!(out.len(), 3);
        assert_eq!(out.get("name").len(), 2);
        assert_eq!(
            out.to_string(),
            "name should not be empty\nother\nvalue must be an object"
        );
        let errors = out.to_errors(&Messages::default());
        assert_eq!(
            errors.get("name").unwrap(),
            ["name should not be empty", "other"]
        );
        assert!(out.into_result().is_err());
    }
}
