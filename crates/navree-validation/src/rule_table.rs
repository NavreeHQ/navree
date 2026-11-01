//! [`RuleTable`]: a type's rules as data, for whatever needs to read them
//! rather than run them. Forms set `required`, `maxlength` and `min` from it,
//! so a rule is written once.

/// One rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Rule {
    /// `required`.
    Required,
    /// `length(...)`. A bound only known at run time is `None` here: the rule
    /// still runs, it just can't be stated up front.
    Length {
        /// The least.
        min: Option<usize>,
        /// The most.
        max: Option<usize>,
        /// Counted in bytes, not characters.
        bytes: bool,
    },
    /// `range(...)`, with literal bounds as they were written.
    Range {
        /// The least.
        min: Option<&'static str>,
        /// The most.
        max: Option<&'static str>,
    },
    /// A rule on text with no arguments: `email`, `url`, `ip`, and so on.
    Format(&'static str),
    /// `contains("...")`.
    Contains(&'static str),
    /// `starts_with("...")`.
    StartsWith(&'static str),
    /// `ends_with("...")`.
    EndsWith(&'static str),
    /// `pattern("...")`.
    Pattern(&'static str),
    /// `same_as(other)`: the other field's name.
    SameAs(&'static str),
    /// `custom(...)`.
    Custom,
    /// `nested`.
    Nested,
    /// `each(...)`: the rules each item follows.
    Each(&'static [Rule]),
}

/// One field's rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldRules {
    /// The field's name in the input (after renames).
    pub name: &'static str,
    /// Its rules.
    pub rules: &'static [Rule],
}

/// A type's rules as data. `#[derive(Validate)]` implements it; a field with
/// no rules isn't listed.
///
/// ```
/// use navree_validation::{Rule, RuleTable, Validate};
///
/// #[derive(Validate)]
/// struct NewTeam {
///     #[validate(length(min = 1, max = 100))]
///     name: String,
/// }
///
/// assert_eq!(NewTeam::FIELDS[0].name, "name");
/// assert_eq!(NewTeam::FIELDS[0].rules, [Rule::Length { min: Some(1), max: Some(100), bytes: false }]);
/// ```
pub trait RuleTable {
    /// Every field with rules, in order. An enum's is empty: which fields
    /// exist depends on the variant.
    const FIELDS: &'static [FieldRules];
}
