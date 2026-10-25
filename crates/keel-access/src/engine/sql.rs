//! SQL list filters: an [`Ability`]'s rules compiled into one `CASE` chain.
//!
//! The chain lists the relevant conditional rules from last to first, so the
//! first `WHEN` that holds is the rule [`Ability::can`] would pick:
//!
//! ```sql
//! CASE WHEN "locked" = TRUE THEN FALSE   -- cannot delete Post where locked
//!      WHEN "author_id" = 7 THEN TRUE    -- can delete Post where author_id = 7
//!      ELSE FALSE END                    -- no rule: deny
//! ```
//!
//! Each condition compiles to SQL that is `TRUE` exactly when the record
//! check holds and `FALSE` or `NULL` otherwise (`CASE` treats `NULL` as not
//! matched), and the chain itself always yields `TRUE` or `FALSE`, so the
//! filter is safe to negate. Null handling follows JavaScript: `$eq: null` is
//! `IS NULL`, `$ne` is `IS DISTINCT FROM`, and `$in` / `$nin` treat a `null`
//! member as matching null columns.
//!
//! The SQL runs on PostgreSQL and on SQLite 3.39+ (`IS DISTINCT FROM`).
//! Parity with [`Ability::can`] assumes each column holds the JSON type its
//! conditions use, and that strings compare in binary (code point) order:
//! SQLite's default collation, `COLLATE "C"` on PostgreSQL.

use sea_query::{BinOper, CaseStatement, Condition, Expr, ExprTrait, SimpleExpr};
use serde_json::Value;

use super::ability::Ability;
use super::condition::{Cond, Conditions};

/// Maps rule fields and condition operands onto SQL for
/// [`Ability::sql_filter_with`].
///
/// The defaults use an unqualified column named like the field and convert
/// operands with [`sql_value`]. Implement it to qualify columns with a table,
/// to map wire names to column names, or to bind typed values (a UUID or
/// timestamp column whose JSON form is a string).
pub trait SqlColumns {
    /// The column for wire field `field`.
    fn column(&self, field: &str) -> Expr {
        Expr::col(field.to_owned())
    }

    /// The SQL value for the non-null JSON operand `value` on `field`.
    fn value(&self, field: &str, value: &Value) -> sea_query::Value {
        let _ = field;
        sql_value(value)
    }
}

/// [`SqlColumns`] with the default mapping.
#[derive(Clone, Copy, Debug, Default)]
pub struct DefaultColumns;

impl SqlColumns for DefaultColumns {}

/// The default SQL value for a JSON scalar: booleans as `Bool`, integers as
/// `BigInt`, other numbers as `Double` and strings as `String`. (`null`,
/// arrays and objects never reach SQL as operands; they map to a null
/// string and the JSON text respectively.)
pub fn sql_value(value: &Value) -> sea_query::Value {
    match value {
        Value::Null => sea_query::Value::String(None),
        Value::Bool(b) => (*b).into(),
        Value::Number(n) => match (n.as_i64(), n.as_f64()) {
            (Some(i), _) => i.into(),
            (None, Some(f)) => f.into(),
            (None, None) => n.to_string().into(),
        },
        Value::String(s) => s.as_str().into(),
        other => other.to_string().into(),
    }
}

impl Ability {
    /// A boolean SQL expression that is true exactly for the rows of
    /// `subject_type` on which `action` is allowed, i.e. for which
    /// [`Ability::can`] with the row as record returns true; `FALSE` when no
    /// rule applies. Uses the default column mapping ([`DefaultColumns`]).
    pub fn sql_filter(&self, action: &str, subject_type: &str) -> SimpleExpr {
        self.sql_filter_with(action, subject_type, &DefaultColumns)
    }

    /// [`Ability::sql_filter`] with a custom column and value mapping.
    pub fn sql_filter_with<C>(&self, action: &str, subject_type: &str, columns: &C) -> SimpleExpr
    where
        C: SqlColumns + ?Sized,
    {
        // Same rule selection as `Ability::can` for a whole record: field-only
        // `cannot` rules do not hide records.
        let relevant = self.rules().iter().rev().filter(|rule| {
            rule.matches_action(action)
                && rule.matches_subject(subject_type)
                && rule.matches_field(None)
        });

        let mut whens: Vec<(&Conditions, bool)> = Vec::new();
        let mut otherwise = false;
        for rule in relevant {
            match &rule.conditions {
                Some(conditions) if !conditions.is_empty() => {
                    whens.push((conditions, !rule.inverted));
                }
                // Matches every record: decides for all rows not caught above.
                _ => {
                    otherwise = !rule.inverted;
                    break;
                }
            }
        }
        // A trailing WHEN with the ELSE result changes nothing.
        while whens.last().is_some_and(|(_, allow)| *allow == otherwise) {
            whens.pop();
        }
        if whens.is_empty() {
            return boolean(otherwise);
        }

        let case = whens
            .into_iter()
            .fold(CaseStatement::new(), |case, (conditions, allow)| {
                case.case(conditions_sql(conditions, columns), boolean(allow))
            });
        case.finally(boolean(otherwise)).into()
    }
}

/// An inlined `TRUE` / `FALSE` (not a bound parameter).
fn boolean(value: bool) -> Expr {
    Expr::Constant(value.into())
}

fn conditions_sql<C>(conditions: &Conditions, columns: &C) -> Condition
where
    C: SqlColumns + ?Sized,
{
    conditions
        .iter()
        .flat_map(|(field, conds)| conds.iter().map(move |cond| (field, cond)))
        .fold(Condition::all(), |all, (field, cond)| {
            all.add(cond_sql(field, cond, columns))
        })
}

fn cond_sql<C>(field: &str, cond: &Cond, columns: &C) -> Expr
where
    C: SqlColumns + ?Sized,
{
    let column = columns.column(field);
    let value = |v: &Value| Expr::val(columns.value(field, v));
    match cond {
        Cond::Eq(Value::Null) => column.is_null(),
        Cond::Eq(v) => column.eq(value(v)),
        Cond::Ne(Value::Null) => column.is_not_null(),
        Cond::Ne(v) => column.binary(BinOper::Custom("IS DISTINCT FROM"), value(v)),
        Cond::Lt(v) => column.lt(value(v)),
        Cond::Lte(v) => column.lte(value(v)),
        Cond::Gt(v) => column.gt(value(v)),
        Cond::Gte(v) => column.gte(value(v)),
        Cond::In(items) => {
            let (has_null, values) = split_null(items);
            let listed =
                (!values.is_empty()).then(|| column.clone().is_in(values.into_iter().map(value)));
            match (listed, has_null) {
                (Some(listed), true) => Condition::any().add(listed).add(column.is_null()).into(),
                (Some(listed), false) => listed,
                (None, true) => column.is_null(),
                (None, false) => boolean(false),
            }
        }
        Cond::Nin(items) => {
            let (has_null, values) = split_null(items);
            let unlisted = (!values.is_empty())
                .then(|| column.clone().is_not_in(values.into_iter().map(value)));
            match (unlisted, has_null) {
                (Some(unlisted), true) => Condition::all()
                    .add(column.is_not_null())
                    .add(unlisted)
                    .into(),
                (Some(unlisted), false) => {
                    Condition::any().add(column.is_null()).add(unlisted).into()
                }
                (None, true) => column.is_not_null(),
                (None, false) => boolean(true),
            }
        }
    }
}

/// Whether `items` contains `null`, and its non-null members.
fn split_null(items: &[Value]) -> (bool, Vec<&Value>) {
    let has_null = items.iter().any(Value::is_null);
    (has_null, items.iter().filter(|v| !v.is_null()).collect())
}
