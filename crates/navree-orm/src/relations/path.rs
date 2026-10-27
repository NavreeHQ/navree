//! Relation paths: `"company"`, `"company.owner"`, a column selection
//! (`"company:id,name"`), an alias for aggregates (`"activities as
//! open_count"`), or a relation method (`Deal::company`).

use std::borrow::Cow;

use crate::error::{OrmError, Result};
use crate::model::Model;
use crate::relations::{RelatedTo, Relation};

/// A relation path, parsed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ParsedPath {
    /// The relation names, outermost first.
    pub(crate) segments: Vec<String>,
    /// The columns selected on the last relation (`"company:id,name"`).
    pub(crate) columns: Option<Vec<String>>,
    /// The name of an aggregate's result column (`"x as open_count"`).
    pub(crate) alias: Option<String>,
}

impl ParsedPath {
    /// Parses `text`.
    pub(crate) fn parse(text: &str) -> Result<ParsedPath> {
        let invalid = |why: &str| OrmError::invalid(format!("`{text}` is not a relation path: {why}"));
        let mut rest = text.trim();
        let mut alias = None;
        let lower = rest.to_ascii_lowercase();
        if let Some(at) = lower.rfind(" as ") {
            let name = rest[at + 4..].trim();
            if name.is_empty() || !is_name(name) {
                return Err(invalid("the alias after `as` must be a column name"));
            }
            alias = Some(name.to_owned());
            rest = rest[..at].trim();
        }
        let mut columns = None;
        if let Some((path, selection)) = rest.split_once(':') {
            let selected: Vec<String> = selection
                .split(',')
                .map(|column| column.trim().to_owned())
                .collect();
            if selected.iter().any(String::is_empty) {
                return Err(invalid("the column list after `:` has an empty column"));
            }
            columns = Some(selected);
            rest = path.trim();
        }
        if rest.is_empty() {
            return Err(invalid("it names no relation"));
        }
        let segments: Vec<String> = rest
            .split('.')
            .map(|segment| segment.trim().to_owned())
            .collect();
        if segments.iter().any(|segment| !is_name(segment)) {
            return Err(invalid(
                "each step is a relation name (letters, digits and `_`)",
            ));
        }
        Ok(ParsedPath {
            segments,
            columns,
            alias,
        })
    }
}

fn is_name(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_')
}

/// A relation path from model `M`: a string (`"company"`,
/// `"company.owner"`, `"company:id,name"`) or a relation method
/// (`Deal::company`, typed: a misspelled method does not compile).
///
/// Implemented for `&str`, `String` and `&String`, and for every function
/// `Fn(&M) -> R` where `R` is a relation of `M` (relation methods; `M`
/// implements `Default`, as models with relations do).
pub trait RelationPath<M: Model> {
    /// The path as text.
    #[doc(hidden)]
    fn relation_path(&self) -> Cow<'_, str>;
}

impl<M: Model> RelationPath<M> for &str {
    fn relation_path(&self) -> Cow<'_, str> {
        Cow::Borrowed(self)
    }
}

impl<M: Model> RelationPath<M> for String {
    fn relation_path(&self) -> Cow<'_, str> {
        Cow::Borrowed(self)
    }
}

impl<M: Model> RelationPath<M> for &String {
    fn relation_path(&self) -> Cow<'_, str> {
        Cow::Borrowed(self.as_str())
    }
}

impl<M, F, R> RelationPath<M> for F
where
    M: Model + Default,
    F: Fn(&M) -> R,
    R: Relation<M>,
{
    fn relation_path(&self) -> Cow<'_, str> {
        Cow::Owned(self(&M::default()).name().to_owned())
    }
}

/// A relation path whose last relation leads to model `T`, for methods
/// whose closure works on a `Builder<T>` (`where_has`, `with_count_constrained`, ...).
///
/// A relation method fixes `T` (so the closure needs no annotation); with a
/// string, annotate the closure (`|q: Builder<Activity>| ...`), and a path
/// leading to another model is an error when the query runs.
pub trait RelationPathTo<M: Model, T: Model>: RelationPath<M> {}

impl<M: Model, T: Model> RelationPathTo<M, T> for &str {}

impl<M: Model, T: Model> RelationPathTo<M, T> for String {}

impl<M: Model, T: Model> RelationPathTo<M, T> for &String {}

impl<M, F, R> RelationPathTo<M, R::Related> for F
where
    M: Model + Default,
    F: Fn(&M) -> R,
    R: RelatedTo<M>,
{
}

/// A relation path whose last relation can be constrained by a closure on
/// `Q`: the relation itself (`HasMany<Deal, Activity>`, so pivot and
/// relation-specific methods work) or a `Builder` of its model.
///
/// A relation method fixes `Q` to the relation's type; with a string,
/// annotate the closure (`|q: HasMany<Deal, Activity>| ...` or
/// `|q: Builder<Activity>| ...`).
pub trait ConstrainPath<M: Model, Q>: RelationPath<M> {}

impl<M: Model, Q> ConstrainPath<M, Q> for &str {}

impl<M: Model, Q> ConstrainPath<M, Q> for String {}

impl<M: Model, Q> ConstrainPath<M, Q> for &String {}

impl<M, F, R> ConstrainPath<M, R> for F
where
    M: Model + Default,
    F: Fn(&M) -> R,
    R: Relation<M>,
{
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_parse_segments_columns_and_aliases() {
        let path = ParsedPath::parse(" company.owner ").unwrap();
        assert_eq!(path.segments, ["company", "owner"]);
        assert_eq!(path.columns, None);
        let path = ParsedPath::parse("company:id, name").unwrap();
        assert_eq!(path.segments, ["company"]);
        assert_eq!(
            path.columns.as_deref(),
            Some(&["id".to_owned(), "name".to_owned()][..])
        );
        let path = ParsedPath::parse("activities AS open_count").unwrap();
        assert_eq!(path.alias.as_deref(), Some("open_count"));
        assert_eq!(path.segments, ["activities"]);
        for bad in ["", "a..b", "a:", "a:x,,y", "a as", "a-b", "a as b c"] {
            assert!(ParsedPath::parse(bad).is_err(), "{bad}");
        }
    }
}
