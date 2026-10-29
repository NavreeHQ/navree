//! [`Filter`]: the conditions a query puts on filterable fields, and the
//! translation of access rules into them.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::ops::{Bound, RangeBounds};

use navree_access::{Ability, Cond, Conditions};
use navree_orm::{FieldKind, Model};
use serde::Serialize;
use serde_json::Value;

use crate::document::index_value;
use crate::settings::IndexSettings;

/// A comparison operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CmpOp {
    /// Equal.
    Eq,
    /// Not equal; a field without a value counts as not equal.
    Ne,
    /// Less than (never matches a field without a value).
    Lt,
    /// Less than or equal.
    Lte,
    /// Greater than.
    Gt,
    /// Greater than or equal.
    Gte,
}

/// Conditions on filterable fields.
///
/// Queries build filters with [`SearchQuery::filter`](crate::SearchQuery::filter)
/// and friends; engines receive them in [`normalized`](Filter::normalized)
/// form: no `Not`, no `Const` below the top, no null operands, and dates
/// as Unix milliseconds. Comparisons between values of different JSON
/// types never match.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Filter {
    /// Every filter holds.
    All(Vec<Filter>),
    /// At least one filter holds.
    Any(Vec<Filter>),
    /// The filter does not hold.
    Not(Box<Filter>),
    /// `field <op> value`.
    Cmp {
        /// The field.
        field: String,
        /// The operator.
        op: CmpOp,
        /// The operand.
        value: Value,
    },
    /// The field equals one of the values.
    In {
        /// The field.
        field: String,
        /// The values.
        values: Vec<Value>,
    },
    /// The field equals none of the values (a field without a value
    /// matches).
    NotIn {
        /// The field.
        field: String,
        /// The values.
        values: Vec<Value>,
    },
    /// The field has no value (null or missing).
    Null(String),
    /// The field has a value.
    NotNull(String),
    /// Always (`true`) or never (`false`).
    Const(bool),
}

impl Filter {
    /// `field == value`.
    pub fn eq(field: &str, value: impl Serialize) -> Filter {
        Filter::Cmp {
            field: field.to_owned(),
            op: CmpOp::Eq,
            value: json(value),
        }
    }

    /// `field != value` (a field without a value matches).
    pub fn ne(field: &str, value: impl Serialize) -> Filter {
        Filter::Cmp {
            field: field.to_owned(),
            op: CmpOp::Ne,
            value: json(value),
        }
    }

    /// `field` equals one of `values`.
    pub fn is_in<I, V>(field: &str, values: I) -> Filter
    where
        I: IntoIterator<Item = V>,
        V: Serialize,
    {
        Filter::In {
            field: field.to_owned(),
            values: values.into_iter().map(json).collect(),
        }
    }

    /// `field` inside `range` (`10..20`, `10..`, `..=20`).
    pub fn range<T: Serialize>(field: &str, range: impl RangeBounds<T>) -> Filter {
        let bound = |bound: Bound<&T>, inclusive: CmpOp, exclusive: CmpOp| match bound {
            Bound::Included(v) => Some(Filter::Cmp {
                field: field.to_owned(),
                op: inclusive,
                value: json(v),
            }),
            Bound::Excluded(v) => Some(Filter::Cmp {
                field: field.to_owned(),
                op: exclusive,
                value: json(v),
            }),
            Bound::Unbounded => None,
        };
        let parts: Vec<Filter> = [
            bound(range.start_bound(), CmpOp::Gte, CmpOp::Gt),
            bound(range.end_bound(), CmpOp::Lte, CmpOp::Lt),
        ]
        .into_iter()
        .flatten()
        .collect();
        if parts.is_empty() {
            // `..`: any value, but a value.
            Filter::NotNull(field.to_owned())
        } else {
            Filter::All(parts)
        }
    }

    /// `field` has no value.
    pub fn null(field: &str) -> Filter {
        Filter::Null(field.to_owned())
    }

    /// Every field the filter names.
    pub fn fields(&self) -> Vec<&str> {
        let mut out = Vec::new();
        self.collect_fields(&mut out);
        out
    }

    fn collect_fields<'a>(&'a self, out: &mut Vec<&'a str>) {
        match self {
            Filter::All(items) | Filter::Any(items) => {
                for item in items {
                    item.collect_fields(out);
                }
            }
            Filter::Not(inner) => inner.collect_fields(out),
            Filter::Cmp { field, .. }
            | Filter::In { field, .. }
            | Filter::NotIn { field, .. }
            | Filter::Null(field)
            | Filter::NotNull(field) => out.push(field),
            Filter::Const(_) => {}
        }
    }

    /// The same filter in the form engines receive: negations pushed down
    /// to the comparisons, constants folded away (a `Const` only remains
    /// as the whole filter), null operands turned into `Null` / `NotNull`,
    /// and dates turned into Unix milliseconds.
    pub fn normalized(self) -> Filter {
        fold(nnf(self, false))
    }

    /// Whether `fields` (the document's normalized filter fields, a
    /// missing field counting as null) satisfy the filter.
    pub fn matches(&self, fields: &BTreeMap<String, Value>) -> bool {
        let get = |field: &str| fields.get(field).filter(|v| !v.is_null());
        match self {
            Filter::All(items) => items.iter().all(|f| f.matches(fields)),
            Filter::Any(items) => items.iter().any(|f| f.matches(fields)),
            Filter::Not(inner) => !inner.matches(fields),
            Filter::Const(b) => *b,
            Filter::Null(field) => get(field).is_none(),
            Filter::NotNull(field) => get(field).is_some(),
            Filter::In { field, values } => match get(field) {
                Some(v) => values.iter().any(|x| json_eq(v, x)),
                None => values.iter().any(Value::is_null),
            },
            Filter::NotIn { field, values } => match get(field) {
                Some(v) => !values.iter().any(|x| json_eq(v, x)),
                None => !values.iter().any(Value::is_null),
            },
            Filter::Cmp { field, op, value } => match (get(field), value.is_null()) {
                (None, true) => matches!(op, CmpOp::Eq | CmpOp::Lte | CmpOp::Gte),
                (None, false) => *op == CmpOp::Ne,
                (Some(_), true) => *op == CmpOp::Ne,
                (Some(v), false) => match op {
                    CmpOp::Eq => json_eq(v, value),
                    CmpOp::Ne => !json_eq(v, value),
                    CmpOp::Lt => json_cmp(v, value) == Some(Ordering::Less),
                    CmpOp::Lte => {
                        matches!(json_cmp(v, value), Some(Ordering::Less | Ordering::Equal))
                    }
                    CmpOp::Gt => json_cmp(v, value) == Some(Ordering::Greater),
                    CmpOp::Gte => matches!(
                        json_cmp(v, value),
                        Some(Ordering::Greater | Ordering::Equal)
                    ),
                },
            },
        }
    }
}

fn json(value: impl Serialize) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// Negation normal form, with null operands and dates normalized.
fn nnf(filter: Filter, negate: bool) -> Filter {
    match filter {
        Filter::All(items) => {
            let items = items.into_iter().map(|f| nnf(f, negate)).collect();
            if negate {
                Filter::Any(items)
            } else {
                Filter::All(items)
            }
        }
        Filter::Any(items) => {
            let items = items.into_iter().map(|f| nnf(f, negate)).collect();
            if negate {
                Filter::All(items)
            } else {
                Filter::Any(items)
            }
        }
        Filter::Not(inner) => nnf(*inner, !negate),
        Filter::Const(b) => Filter::Const(b != negate),
        Filter::Null(field) if negate => Filter::NotNull(field),
        Filter::Null(field) => Filter::Null(field),
        Filter::NotNull(field) if negate => Filter::Null(field),
        Filter::NotNull(field) => Filter::NotNull(field),
        Filter::In { field, values } => in_filter(field, values, negate),
        Filter::NotIn { field, values } => in_filter(field, values, !negate),
        Filter::Cmp { field, op, value } => cmp_filter(field, op, index_value(&value), negate),
    }
}

/// `field IN values` (or `NOT IN` when `negate`), null members split off.
fn in_filter(field: String, values: Vec<Value>, negate: bool) -> Filter {
    let has_null = values.iter().any(Value::is_null);
    let values: Vec<Value> = values
        .iter()
        .filter(|v| !v.is_null())
        .map(index_value)
        .collect();
    let listed = (!values.is_empty()).then(|| {
        if negate {
            Filter::NotIn {
                field: field.clone(),
                values,
            }
        } else {
            Filter::In {
                field: field.clone(),
                values,
            }
        }
    });
    match (listed, has_null, negate) {
        (Some(listed), false, _) => listed,
        (Some(listed), true, false) => Filter::Any(vec![listed, Filter::Null(field)]),
        (Some(listed), true, true) => Filter::All(vec![listed, Filter::NotNull(field)]),
        (None, true, false) => Filter::Null(field),
        (None, true, true) => Filter::NotNull(field),
        (None, false, negate) => Filter::Const(negate),
    }
}

fn cmp_filter(field: String, op: CmpOp, value: Value, negate: bool) -> Filter {
    use CmpOp::*;
    if value.is_null() {
        // `= null` is "no value", `!= null` "a value"; ranges never match.
        return match (op, negate) {
            (Eq, false) | (Ne, true) => Filter::Null(field),
            (Ne, false) | (Eq, true) => Filter::NotNull(field),
            (_, negate) => Filter::Const(negate),
        };
    }
    let cmp = |op| Filter::Cmp {
        field: field.clone(),
        op,
        value: value.clone(),
    };
    if !negate {
        return cmp(op);
    }
    match op {
        Eq => cmp(Ne),
        Ne => cmp(Eq),
        Lt => Filter::Any(vec![cmp(Gte), Filter::Null(field.clone())]),
        Lte => Filter::Any(vec![cmp(Gt), Filter::Null(field.clone())]),
        Gt => Filter::Any(vec![cmp(Lte), Filter::Null(field.clone())]),
        Gte => Filter::Any(vec![cmp(Lt), Filter::Null(field.clone())]),
    }
}

/// Folds constants: `All` with a `false` is `false`, `Any` with a `true`
/// is `true`, neutral members are dropped and single members unwrapped.
fn fold(filter: Filter) -> Filter {
    match filter {
        Filter::All(items) => {
            let mut kept = Vec::new();
            for item in items.into_iter().map(fold) {
                match item {
                    Filter::Const(false) => return Filter::Const(false),
                    Filter::Const(true) => {}
                    Filter::All(inner) => kept.extend(inner),
                    other => kept.push(other),
                }
            }
            match kept.len() {
                0 => Filter::Const(true),
                1 => kept.pop().expect("one item"),
                _ => Filter::All(kept),
            }
        }
        Filter::Any(items) => {
            let mut kept = Vec::new();
            for item in items.into_iter().map(fold) {
                match item {
                    Filter::Const(true) => return Filter::Const(true),
                    Filter::Const(false) => {}
                    Filter::Any(inner) => kept.extend(inner),
                    other => kept.push(other),
                }
            }
            match kept.len() {
                0 => Filter::Const(false),
                1 => kept.pop().expect("one item"),
                _ => Filter::Any(kept),
            }
        }
        other => other,
    }
}

/// JSON equality, numbers compared by value.
pub(crate) fn json_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => json_cmp(a, b) == Some(Ordering::Equal) || x == y,
        _ => a == b,
    }
}

/// JSON ordering for values of the same type: numbers, strings and
/// booleans; `None` across types.
pub(crate) fn json_cmp(a: &Value, b: &Value) -> Option<Ordering> {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => match (x.as_i64(), y.as_i64()) {
            (Some(x), Some(y)) => Some(x.cmp(&y)),
            _ => x.as_f64()?.partial_cmp(&y.as_f64()?),
        },
        (Value::String(x), Value::String(y)) => Some(x.cmp(y)),
        (Value::Bool(x), Value::Bool(y)) => Some(x.cmp(y)),
        _ => None,
    }
}

/// An access rule set turned into a filter for one index.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum AccessFilter {
    /// The engine can apply it exactly (every condition is on a filterable
    /// field of the index).
    Exact(Filter),
    /// Some condition names a column the index does not filter on, or one
    /// the index cannot compare exactly: apply the SQL filter when loading
    /// the rows.
    Rows,
}

/// What the index holds for a model's column by name; `None` when the
/// model has no such column. The access translation compares operands the
/// way the SQL `accessible` filter does, which depends on the column's
/// type.
pub(crate) type ColumnTypes = fn(&str) -> Option<ColumnKind>;

/// [`ColumnTypes`] for model `M`: by its cast (`string_enum` is text,
/// `int_enum` a number, any other cast inexact), else by the field's kind.
pub(crate) fn column_type<M: Model>(name: &str) -> Option<ColumnKind> {
    let column = M::meta().column(name)?;
    Some(match column.cast_name() {
        Some("string_enum") => ColumnKind::Text,
        Some("int_enum") => ColumnKind::Number,
        Some(_) => ColumnKind::Inexact,
        None => match column.field_kind() {
            FieldKind::Bool => ColumnKind::Boolean,
            FieldKind::Integer { .. } | FieldKind::Float { .. } => ColumnKind::Number,
            FieldKind::Text | FieldKind::Uuid | FieldKind::Ulid => ColumnKind::Text,
            _ => ColumnKind::Inexact,
        },
    })
}

/// What the index holds for a column, as access operands compare with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ColumnKind {
    /// JSON booleans.
    Boolean,
    /// JSON numbers.
    Number,
    /// JSON strings (text, UUIDs, enums stored as text).
    Text,
    /// Anything the index cannot compare exactly: dates and times (the
    /// index keeps Unix milliseconds, the database finer precision),
    /// decimals, JSON, binary and custom types.
    Inexact,
}

/// The records of `subject` on which `ability` allows `action`, as a filter
/// on `settings`' fields: the same rule selection as
/// [`Ability::sql_filter`] (last matching rule wins), so engine results
/// agree with SQL `Accessible` lists.
///
/// A condition is translated when it names a filterable field whose
/// column (typed by `columns`) is a boolean, a number or a string (UUIDs
/// and text enums included). An operand of another JSON type than the
/// column's compares as the SQL filter compares it: `$eq` and the ranges
/// never match, `$ne` always does, and `$in` / `$nin` ignore it. A
/// condition on any other column (dates and times, decimals, JSON, a
/// column the index lacks) makes the rule set [`AccessFilter::Rows`].
pub(crate) fn access_filter(
    ability: &Ability,
    action: &str,
    subject: &str,
    settings: &IndexSettings,
    columns: ColumnTypes,
) -> AccessFilter {
    let relevant = ability.rules().iter().rev().filter(|rule| {
        rule.matches_action(action) && rule.matches_subject(subject) && rule.matches_field(None)
    });
    let mut whens: Vec<(&Conditions, bool)> = Vec::new();
    let mut otherwise = false;
    for rule in relevant {
        match &rule.conditions {
            Some(conditions) if !conditions.is_empty() => whens.push((conditions, !rule.inverted)),
            _ => {
                otherwise = !rule.inverted;
                break;
            }
        }
    }
    while whens.last().is_some_and(|(_, allow)| *allow == otherwise) {
        whens.pop();
    }
    let mut filter = Filter::Const(otherwise);
    for (conditions, allow) in whens.into_iter().rev() {
        let Some(condition) = conditions_filter(conditions, settings, columns) else {
            return AccessFilter::Rows;
        };
        filter = if allow {
            Filter::Any(vec![condition, filter])
        } else {
            Filter::All(vec![Filter::Not(Box::new(condition)), filter])
        };
    }
    AccessFilter::Exact(filter.normalized())
}

/// One rule's conditions on index fields; `None` when a column is not a
/// filterable field, or not one the index compares exactly.
fn conditions_filter(
    conditions: &Conditions,
    settings: &IndexSettings,
    columns: ColumnTypes,
) -> Option<Filter> {
    let mut parts = Vec::new();
    for (column, conds) in conditions.iter() {
        let field = settings.field_for_column(column).filter(|f| f.filter)?;
        let kind = columns(column)?;
        // A non-null operand of another JSON type than the column's; the
        // SQL filter compiles it to a constant (`SqlColumns::accepts`).
        let foreign = |v: &Value| match kind {
            _ if v.is_null() => false,
            ColumnKind::Boolean => !v.is_boolean(),
            ColumnKind::Number => !v.is_number(),
            ColumnKind::Text => !v.is_string(),
            ColumnKind::Inexact => true,
        };
        if kind == ColumnKind::Inexact {
            return None;
        }
        let name = field.name.as_str();
        let kept = |values: &[Value]| -> Vec<Value> {
            values.iter().filter(|v| !foreign(v)).cloned().collect()
        };
        for cond in conds {
            parts.push(match cond {
                Cond::Eq(v) if foreign(v) => Filter::Const(false),
                Cond::Eq(v) => Filter::eq(name, v),
                Cond::Ne(v) if foreign(v) => Filter::Const(true),
                Cond::Ne(v) => Filter::ne(name, v),
                Cond::Lt(v) | Cond::Lte(v) | Cond::Gt(v) | Cond::Gte(v) if foreign(v) => {
                    Filter::Const(false)
                }
                Cond::Lt(v) => cmp(name, CmpOp::Lt, v),
                Cond::Lte(v) => cmp(name, CmpOp::Lte, v),
                Cond::Gt(v) => cmp(name, CmpOp::Gt, v),
                Cond::Gte(v) => cmp(name, CmpOp::Gte, v),
                Cond::In(values) => Filter::is_in(name, kept(values)),
                Cond::Nin(values) => Filter::Not(Box::new(Filter::is_in(name, kept(values)))),
            });
        }
    }
    Some(Filter::All(parts))
}

fn cmp(field: &str, op: CmpOp, value: &Value) -> Filter {
    Filter::Cmp {
        field: field.to_owned(),
        op,
        value: value.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use navree_access::{READ, Rules, field};
    use serde_json::json;

    fn doc(value: Value) -> BTreeMap<String, Value> {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn normalization_pushes_negations_down() {
        let f = Filter::Not(Box::new(Filter::All(vec![
            Filter::eq("stage", "won"),
            Filter::is_in("owner", ["a", "b"]),
        ])))
        .normalized();
        assert_eq!(
            f,
            Filter::Any(vec![
                Filter::ne("stage", "won"),
                Filter::NotIn {
                    field: "owner".into(),
                    values: vec![json!("a"), json!("b")]
                },
            ])
        );
    }

    #[test]
    fn normalization_handles_nulls_and_dates() {
        assert_eq!(
            Filter::eq("x", None::<i32>).normalized(),
            Filter::Null("x".into())
        );
        assert_eq!(
            Filter::ne("x", None::<i32>).normalized(),
            Filter::NotNull("x".into())
        );
        assert_eq!(
            Filter::is_in("x", [json!(1), json!(null)]).normalized(),
            Filter::Any(vec![
                Filter::In {
                    field: "x".into(),
                    values: vec![json!(1)]
                },
                Filter::Null("x".into())
            ])
        );
        assert_eq!(
            Filter::eq("d", "1970-01-02").normalized(),
            Filter::eq("d", 86_400_000)
        );
        assert_eq!(
            Filter::is_in("x", Vec::<i32>::new()).normalized(),
            Filter::Const(false)
        );
    }

    #[test]
    fn constants_fold_away() {
        let f = Filter::All(vec![
            Filter::Const(true),
            Filter::Any(vec![Filter::Const(false), Filter::eq("a", 1)]),
        ])
        .normalized();
        assert_eq!(f, Filter::eq("a", 1));
        let f = Filter::Any(vec![Filter::eq("a", 1), Filter::Const(true)]).normalized();
        assert_eq!(f, Filter::Const(true));
    }

    #[test]
    fn ranges_build_bounds() {
        assert_eq!(
            Filter::range("n", 10..),
            Filter::All(vec![Filter::Cmp {
                field: "n".into(),
                op: CmpOp::Gte,
                value: json!(10)
            }])
        );
        let f = Filter::range("n", 10..20);
        assert!(f.matches(&doc(json!({"n": 10}))));
        assert!(!f.matches(&doc(json!({"n": 20}))));
        assert!(Filter::range("n", ..=20).matches(&doc(json!({"n": 20}))));
        assert!(!Filter::range("n", 10..).matches(&doc(json!({}))));
    }

    #[test]
    fn matching_follows_null_and_type_rules() {
        let d = doc(json!({"stage": "won", "n": 5, "f": 5.0, "flag": true, "none": null}));
        assert!(Filter::eq("stage", "won").matches(&d));
        assert!(Filter::eq("n", 5.0).matches(&d), "numbers compare by value");
        assert!(!Filter::eq("n", "5").matches(&d), "types never mix");
        assert!(Filter::ne("none", "x").matches(&d), "no value is not equal");
        assert!(Filter::ne("missing", 1).matches(&d));
        assert!(Filter::null("none").matches(&d) && Filter::null("missing").matches(&d));
        assert!(!Filter::range("missing", 0..).matches(&d));
        assert!(Filter::eq("flag", true).matches(&d));
        let not_in = Filter::Not(Box::new(Filter::is_in("stage", ["lost"]))).normalized();
        assert!(not_in.matches(&d));
        assert!(
            not_in.matches(&doc(json!({}))),
            "no value is not in the list"
        );
    }

    fn settings() -> IndexSettings {
        IndexSettings::new("Deal", "deals")
            .text("title", 3)
            .filter("owner")
            .column("owner", "owner_id")
            .filter("locked")
            .filter("amount")
            .filter("closes_at")
            .filter("ghost")
    }

    /// The test model's columns (`ghost` is a declared field without a
    /// column).
    fn columns(name: &str) -> Option<ColumnKind> {
        match name {
            "owner_id" | "title" | "team_id" => Some(ColumnKind::Text),
            "locked" => Some(ColumnKind::Boolean),
            "amount" => Some(ColumnKind::Number),
            "closes_at" => Some(ColumnKind::Inexact),
            _ => None,
        }
    }

    fn translate(rules: Rules) -> AccessFilter {
        access_filter(&rules.build(), READ, "Deal", &settings(), columns)
    }

    #[test]
    fn access_rules_become_engine_filters() {
        let mut rules = Rules::new();
        rules.can(READ, "Deal").when(field("owner_id").eq("u1"));
        rules.cannot(READ, "Deal").when(field("locked").eq(true));
        let AccessFilter::Exact(f) = translate(rules) else {
            panic!("translatable");
        };
        assert!(f.matches(&doc(json!({"owner": "u1", "locked": false}))));
        assert!(!f.matches(&doc(json!({"owner": "u1", "locked": true}))));
        assert!(!f.matches(&doc(json!({"owner": "u2", "locked": false}))));
    }

    #[test]
    fn unconditional_rules_are_constants() {
        let mut rules = Rules::new();
        rules.can(READ, "Deal");
        assert_eq!(translate(rules), AccessFilter::Exact(Filter::Const(true)));
        assert_eq!(
            translate(Rules::new()),
            AccessFilter::Exact(Filter::Const(false))
        );
    }

    #[test]
    fn conditions_on_unindexed_columns_fall_back_to_rows() {
        let mut rules = Rules::new();
        rules.can(READ, "Deal").when(field("team_id").eq("t1"));
        assert_eq!(translate(rules), AccessFilter::Rows);
    }

    #[test]
    fn conditions_on_timestamps_and_missing_columns_fall_back_to_rows() {
        // The index keeps milliseconds; the database compares microseconds.
        let mut rules = Rules::new();
        rules
            .can(READ, "Deal")
            .when(field("closes_at").gt("2026-10-25T12:00:00.250001Z"));
        assert_eq!(translate(rules), AccessFilter::Rows);
        // A declared field without a column of the model.
        let mut rules = Rules::new();
        rules.can(READ, "Deal").when(field("ghost").eq(1));
        assert_eq!(translate(rules), AccessFilter::Rows);
    }

    #[test]
    fn operands_of_another_type_compare_like_sql() {
        // `amount = "3"` never holds; `amount != "3"` always does.
        let mut rules = Rules::new();
        rules.can(READ, "Deal").when(field("amount").eq("3"));
        assert_eq!(translate(rules), AccessFilter::Exact(Filter::Const(false)));
        let mut rules = Rules::new();
        rules.can(READ, "Deal").when(field("amount").ne("3"));
        assert_eq!(translate(rules), AccessFilter::Exact(Filter::Const(true)));
        // `$in` ignores members of another type.
        let mut rules = Rules::new();
        rules
            .can(READ, "Deal")
            .when(field("locked").is_in([json!(0), json!(true)]));
        assert_eq!(
            translate(rules),
            AccessFilter::Exact(Filter::In {
                field: "locked".into(),
                values: vec![json!(true)],
            })
        );
    }
}
