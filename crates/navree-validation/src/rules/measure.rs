//! The rules that measure or compare: `required`, `length`, `range` and
//! `same_as`, the [`Each`] behind `each(...)`, and `pattern`.
//!
//! Each works through a trait the field's type implements, with an
//! implementation for `Option<T>` where `None` passes (`required` is the
//! rule that demands a value).

use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::fmt::Display;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use crate::{FieldPath, Violation};

use super::RuleInput;

/// A value `required` checks: an `Option` is set when it is `Some`.
#[diagnostic::on_unimplemented(
    message = "`required` checks an `Option`, and `{Self}` is not one",
    label = "make the field an `Option<_>`, or drop `required`"
)]
pub trait Required {
    /// Whether there is a value.
    fn is_set(&self) -> bool;
}

impl<T> Required for Option<T> {
    fn is_set(&self) -> bool {
        self.is_some()
    }
}

impl<T: Required + ?Sized> Required for &T {
    fn is_set(&self) -> bool {
        (**self).is_set()
    }
}

/// The value is `Some` (message key `required`).
pub fn required<T: Required + ?Sized>(value: &T) -> Result<(), Violation> {
    if value.is_set() {
        Ok(())
    } else {
        Err(Violation::rule("required"))
    }
}

/// What `length` counts in text: characters (the default) or bytes
/// (`length(bytes, ...)`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    /// Unicode scalar values: `"Zoë"` has 3.
    Chars,
    /// UTF-8 bytes: `"Zoë"` has 4.
    Bytes,
}

/// A length: of text (messages say "characters") or a number of items
/// (messages say "elements").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Measure {
    /// Text, in the unit asked for.
    Text(usize),
    /// A collection's number of items.
    Items(usize),
}

/// A value `length` measures: text, or a collection.
#[diagnostic::on_unimplemented(
    message = "`length` measures text and collections, and `{Self}` is neither",
    label = "`length` needs a string, a list, a set or a map (or an `Option` of one)"
)]
pub trait Length {
    /// The length, or `None` when there is no value (`None` passes).
    fn measure(&self, unit: Unit) -> Option<Measure>;
}

fn text(text: &str, unit: Unit) -> Option<Measure> {
    Some(Measure::Text(match unit {
        Unit::Chars => text.chars().count(),
        Unit::Bytes => text.len(),
    }))
}

impl Length for str {
    fn measure(&self, unit: Unit) -> Option<Measure> {
        text(self, unit)
    }
}

impl Length for String {
    fn measure(&self, unit: Unit) -> Option<Measure> {
        text(self, unit)
    }
}

impl Length for Cow<'_, str> {
    fn measure(&self, unit: Unit) -> Option<Measure> {
        text(self, unit)
    }
}

impl<T: Length + ?Sized> Length for &T {
    fn measure(&self, unit: Unit) -> Option<Measure> {
        (**self).measure(unit)
    }
}

impl<T: Length + ?Sized> Length for Box<T> {
    fn measure(&self, unit: Unit) -> Option<Measure> {
        (**self).measure(unit)
    }
}

impl<T: Length + ?Sized> Length for Arc<T> {
    fn measure(&self, unit: Unit) -> Option<Measure> {
        (**self).measure(unit)
    }
}

impl<T: Length + ?Sized> Length for Rc<T> {
    fn measure(&self, unit: Unit) -> Option<Measure> {
        (**self).measure(unit)
    }
}

impl<T: Length> Length for Option<T> {
    fn measure(&self, unit: Unit) -> Option<Measure> {
        self.as_ref().and_then(|value| value.measure(unit))
    }
}

/// Collections count their items whatever the unit.
macro_rules! items {
    ($($ty:ty => [$($generics:tt)*]),* $(,)?) => {
        $(
            impl<$($generics)*> Length for $ty {
                fn measure(&self, _unit: Unit) -> Option<Measure> {
                    Some(Measure::Items(self.len()))
                }
            }
        )*
    };
}

items! {
    [T] => [T],
    Vec<T> => [T],
    VecDeque<T> => [T],
    BTreeSet<T> => [T],
    HashSet<T, S> => [T, S],
    BTreeMap<K, V> => [K, V],
    HashMap<K, V, S> => [K, V, S],
}

impl<T, const N: usize> Length for [T; N] {
    fn measure(&self, _unit: Unit) -> Option<Measure> {
        Some(Measure::Items(N))
    }
}

/// The length is within `min` and `max` (message keys `min.string` /
/// `max.string` for text, `min.array` / `max.array` for collections).
pub fn length<T: Length + ?Sized>(
    value: &T,
    unit: Unit,
    min: Option<usize>,
    max: Option<usize>,
) -> Result<(), Violation> {
    let Some(measure) = value.measure(unit) else {
        return Ok(());
    };
    let (length, kind) = match measure {
        Measure::Text(length) => (length, "string"),
        Measure::Items(length) => (length, "array"),
    };
    if let Some(min) = min
        && length < min
    {
        return Err(Violation::rule(format!("min.{kind}")).with("min", min));
    }
    if let Some(max) = max
        && length > max
    {
        return Err(Violation::rule(format!("max.{kind}")).with("max", max));
    }
    Ok(())
}

/// A value `range` compares with its bounds: a number.
#[diagnostic::on_unimplemented(
    message = "`range` compares numbers, and `{Self}` is not one",
    label = "`range` needs a number (or an `Option` of one); `length` measures text"
)]
pub trait Range {
    /// The type of the bounds.
    type Bound: PartialOrd + Display;

    /// The value to compare, or `None` when there is none (`None` passes).
    fn value(&self) -> Option<&Self::Bound>;
}

macro_rules! numbers {
    ($($ty:ty),* $(,)?) => {
        $(
            impl Range for $ty {
                type Bound = $ty;

                fn value(&self) -> Option<&$ty> {
                    Some(self)
                }
            }
        )*
    };
}

numbers!(
    i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize, f32, f64
);

impl<T: Range + ?Sized> Range for &T {
    type Bound = T::Bound;

    fn value(&self) -> Option<&T::Bound> {
        (**self).value()
    }
}

impl<T: Range> Range for Option<T> {
    type Bound = T::Bound;

    fn value(&self) -> Option<&T::Bound> {
        self.as_ref().and_then(Range::value)
    }
}

/// The value is within `min` and `max`, both included (message keys
/// `min.numeric` and `max.numeric`). A value that doesn't compare (a
/// floating-point NaN) fails.
pub fn range<T: Range + ?Sized>(
    value: &T,
    min: Option<&T::Bound>,
    max: Option<&T::Bound>,
) -> Result<(), Violation> {
    let Some(value) = value.value() else {
        return Ok(());
    };
    if let Some(min) = min
        && matches!(value.partial_cmp(min), Some(Ordering::Less) | None)
    {
        return Err(Violation::rule("min.numeric").with("min", min));
    }
    if let Some(max) = max
        && matches!(value.partial_cmp(max), Some(Ordering::Greater) | None)
    {
        return Err(Violation::rule("max.numeric").with("max", max));
    }
    Ok(())
}

/// The field equals another field (message key `same`, with `:other` the
/// other field's path, shown by its display name).
pub fn same_as<V, W>(value: &V, other: &W, other_path: &FieldPath<'_>) -> Result<(), Violation>
where
    V: PartialEq<W> + ?Sized,
    W: ?Sized,
{
    if value == other {
        Ok(())
    } else {
        Err(Violation::rule("same").with("other", other_path.to_dotted()))
    }
}

/// A regular expression of `pattern("...")`, compiled the first time it
/// runs (the derive has already checked it compiles).
pub struct Pattern {
    source: &'static str,
    compiled: OnceLock<regex::Regex>,
}

impl Pattern {
    /// The expression `source`.
    pub const fn new(source: &'static str) -> Self {
        Pattern {
            source,
            compiled: OnceLock::new(),
        }
    }

    /// The expression as written.
    pub fn as_str(&self) -> &'static str {
        self.source
    }

    /// Whether `text` matches.
    pub fn is_match(&self, text: &str) -> bool {
        self.compiled
            .get_or_init(|| {
                regex::Regex::new(self.source)
                    .unwrap_or_else(|error| panic!("invalid pattern {:?}: {error}", self.source))
            })
            .is_match(text)
    }
}

/// The text matches `pattern` (message key `regex`, with `:pattern`).
pub fn pattern<T: RuleInput + ?Sized>(value: &T, pattern: &Pattern) -> Result<(), Violation> {
    match value.rule_str() {
        Some(text) if !pattern.is_match(text) => {
            Err(Violation::rule("regex").with("pattern", pattern.as_str()))
        }
        _ => Ok(()),
    }
}

/// A collection whose items `each(...)` checks, each at its own path: an
/// index for lists and sets, the key for maps. An `Option` of a collection
/// has no items when it is `None`.
#[diagnostic::on_unimplemented(
    message = "`each` checks the items of a collection, and `{Self}` is not one",
    label = "`each` needs a list, a set or a map (or an `Option` of one)"
)]
pub trait Each {
    /// The items' type.
    type Item: ?Sized;

    /// Calls `f` with each item and its path.
    fn each(&self, path: &FieldPath<'_>, f: &mut dyn FnMut(&FieldPath<'_>, &Self::Item));
}

impl<T> Each for [T] {
    type Item = T;

    fn each(&self, path: &FieldPath<'_>, f: &mut dyn FnMut(&FieldPath<'_>, &T)) {
        for (index, item) in self.iter().enumerate() {
            f(&path.index(index), item);
        }
    }
}

impl<T, const N: usize> Each for [T; N] {
    type Item = T;

    fn each(&self, path: &FieldPath<'_>, f: &mut dyn FnMut(&FieldPath<'_>, &T)) {
        self.as_slice().each(path, f);
    }
}

impl<T> Each for Vec<T> {
    type Item = T;

    fn each(&self, path: &FieldPath<'_>, f: &mut dyn FnMut(&FieldPath<'_>, &T)) {
        self.as_slice().each(path, f);
    }
}

macro_rules! indexed {
    ($($ty:ty => [$($generics:tt)*]),* $(,)?) => {
        $(
            impl<$($generics)*> Each for $ty {
                type Item = T;

                fn each(&self, path: &FieldPath<'_>, f: &mut dyn FnMut(&FieldPath<'_>, &T)) {
                    for (index, item) in self.iter().enumerate() {
                        f(&path.index(index), item);
                    }
                }
            }
        )*
    };
}

indexed! {
    VecDeque<T> => [T],
    BTreeSet<T> => [T],
    HashSet<T, S> => [T, S],
}

impl<K: Display, V> Each for BTreeMap<K, V> {
    type Item = V;

    fn each(&self, path: &FieldPath<'_>, f: &mut dyn FnMut(&FieldPath<'_>, &V)) {
        for (key, value) in self {
            f(&path.key(key), value);
        }
    }
}

impl<K: Display, V, S> Each for HashMap<K, V, S> {
    type Item = V;

    fn each(&self, path: &FieldPath<'_>, f: &mut dyn FnMut(&FieldPath<'_>, &V)) {
        for (key, value) in self {
            f(&path.key(key), value);
        }
    }
}

impl<C: Each + ?Sized> Each for &C {
    type Item = C::Item;

    fn each(&self, path: &FieldPath<'_>, f: &mut dyn FnMut(&FieldPath<'_>, &C::Item)) {
        (**self).each(path, f);
    }
}

impl<C: Each + ?Sized> Each for Box<C> {
    type Item = C::Item;

    fn each(&self, path: &FieldPath<'_>, f: &mut dyn FnMut(&FieldPath<'_>, &C::Item)) {
        (**self).each(path, f);
    }
}

impl<C: Each> Each for Option<C> {
    type Item = C::Item;

    fn each(&self, path: &FieldPath<'_>, f: &mut dyn FnMut(&FieldPath<'_>, &C::Item)) {
        if let Some(collection) = self {
            collection.each(path, f);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_counts_characters_unless_asked_for_bytes() {
        assert!(length("Zoë", Unit::Chars, None, Some(3)).is_ok());
        assert_eq!(
            length("Zoë", Unit::Bytes, None, Some(3)).unwrap_err(),
            Violation::rule("max.string").with("max", 3)
        );
        assert_eq!(
            length(&vec![1, 2], Unit::Chars, Some(3), None).unwrap_err(),
            Violation::rule("min.array").with("min", 3)
        );
        assert!(length(&None::<String>, Unit::Chars, Some(1), None).is_ok());
        assert!(length(&[0u8; 4], Unit::Bytes, Some(4), Some(4)).is_ok());
    }

    #[test]
    fn range_compares_and_rejects_nan() {
        assert!(range(&5u32, Some(&1), Some(&10)).is_ok());
        assert_eq!(
            range(&0i64, Some(&1), None).unwrap_err().message(),
            "value must not be less than 1"
        );
        assert_eq!(
            range(&Some(11.5f64), None, Some(&10.0))
                .unwrap_err()
                .message(),
            "value must not be greater than 10"
        );
        assert!(range(&f64::NAN, Some(&0.0), None).is_err());
        assert!(range(&None::<u8>, Some(&1), None).is_ok());
    }

    #[test]
    fn each_visits_items_at_their_paths() {
        let mut seen = Vec::new();
        let prices = BTreeMap::from([("EUR", 1), ("GBP", 2)]);
        let root = FieldPath::root();
        prices.each(&root.field("prices"), &mut |path, value| {
            seen.push((path.to_dotted(), *value));
        });
        Some(vec![7]).each(&root.field("list"), &mut |path, value| {
            seen.push((path.to_dotted(), *value));
        });
        None::<Vec<i32>>.each(&root, &mut |_, _| panic!("no items"));
        assert_eq!(
            seen,
            [
                ("prices.EUR".to_owned(), 1),
                ("prices.GBP".to_owned(), 2),
                ("list.0".to_owned(), 7)
            ]
        );
    }

    #[test]
    fn patterns_and_same_as() {
        static CODE: Pattern = Pattern::new("^[A-Z]{3}$");
        assert!(pattern("EUR", &CODE).is_ok());
        assert_eq!(
            pattern("eur", &CODE).unwrap_err().message(),
            "value must match ^[A-Z]{3}$ regular expression"
        );
        assert!(pattern(&None::<String>, &CODE).is_ok());
        let root = FieldPath::root();
        assert!(same_as("a", "a", &root.field("b")).is_ok());
        assert_eq!(
            same_as("a", "b", &root.field("b")).unwrap_err(),
            Violation::rule("same").with("other", "b")
        );
        assert!(required(&Some(1)).is_ok());
        assert_eq!(required(&None::<u8>).unwrap_err().key(), Some("required"));
    }
}
