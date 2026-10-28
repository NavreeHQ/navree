//! Assertions on the database for tests: rows and records that exist or
//! not, row counts, soft deletes, and the number of queries a piece of code
//! runs.
//!
//! The assertions are `async` and panic with a report when they fail: what
//! was expected, and the rows that came closest.
//!
//! ```no_run
//! # use navree_orm::__doc_factories::*;
//! use navree_orm::testing::*;
//!
//! # async fn demo(db: Db) -> Result<()> {
//! let deal = Deal::factory().set(Deal::TITLE, "Renewal").create_one(&db).await?;
//!
//! assert_database_has(&db, "deals", [("title", "Renewal")]).await;
//! // Typed columns encode values as the model stores them (casts included).
//! assert_database_has(&db, Deal::table_name(), [Deal::STAGE.set(Stage::Open), Deal::TITLE.set("Renewal")]).await;
//! assert_database_missing(&db, "deals", [("title", "Gone")]).await;
//! assert_database_count(&db, "deals", 1).await;
//! assert_model_exists(&db, &deal).await;
//!
//! let mut deal = deal;
//! deal.delete(&db).await?;
//! assert_soft_deleted(&db, &deal).await;
//!
//! // Two queries: the deal and its count.
//! expect_query_count(2, async {
//!     Deal::find(&db, deal.id).await.unwrap();
//!     Deal::query().count(&db).await.unwrap();
//! })
//! .await;
//! # Ok(()) }
//! ```

#[cfg(feature = "testing")]
mod app;
#[cfg(feature = "testing")]
pub mod database;
mod queries;

use std::borrow::Cow;
use std::fmt::Write as _;

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use uuid::Uuid;

#[cfg(feature = "testing")]
pub use app::TestAppExt;
pub use queries::{count_queries, expect_query_count};

pub(crate) use queries::note_statement;

use crate::column::{Assignment, Filter};
use crate::exec::Executor;
use crate::model::{Model, PrimaryKey};
use crate::query::table;
use crate::record::Record;
use crate::row::Row;
use crate::value::{Decimal, Field, Value, is_null, to_json};

/// The most rows an assertion reads to compare them; a table with more is
/// reported as too large to inspect.
const MAX_INSPECTED_ROWS: u64 = 10_000;

/// How many close rows a failed assertion lists.
const CLOSEST_ROWS: usize = 3;

/// What an assertion looks for in a table: columns and their values (each
/// compared as the column stores it: numbers by value whatever their width,
/// booleans stored as integers, date-times by instant, JSON structurally),
/// or a condition the database evaluates.
///
/// Built from pairs (`[("title", "Acme")]`), from typed assignments
/// (`[Deal::TITLE.set("Acme")]`, values encoded with the column's casts), or
/// from a [`Filter`] (`Deal::AMOUNT_CENTS.gt(100)`).
#[derive(Clone, Debug)]
pub struct Conditions(Inner);

#[derive(Clone, Debug)]
enum Inner {
    Values(Vec<(Cow<'static, str>, Value)>),
    Filter(Filter),
}

impl<V: Into<Value>, const N: usize> From<[(&'static str, V); N]> for Conditions {
    fn from(pairs: [(&'static str, V); N]) -> Self {
        Conditions(Inner::Values(
            pairs
                .into_iter()
                .map(|(column, value)| (Cow::Borrowed(column), value.into()))
                .collect(),
        ))
    }
}

impl<V: Into<Value>> From<Vec<(&'static str, V)>> for Conditions {
    fn from(pairs: Vec<(&'static str, V)>) -> Self {
        Conditions(Inner::Values(
            pairs
                .into_iter()
                .map(|(column, value)| (Cow::Borrowed(column), value.into()))
                .collect(),
        ))
    }
}

impl<const N: usize> From<[Assignment; N]> for Conditions {
    fn from(assignments: [Assignment; N]) -> Self {
        Conditions::from(assignments.into_iter().collect::<Vec<_>>())
    }
}

impl From<Vec<Assignment>> for Conditions {
    fn from(assignments: Vec<Assignment>) -> Self {
        Conditions(Inner::Values(
            assignments
                .into_iter()
                .map(|assignment| {
                    let column = Cow::Owned(assignment.column().to_owned());
                    match assignment.assigned_value() {
                        Some(value) => (column, value.clone()),
                        None => panic!(
                            "a database assertion compares `{column}` with a value; an expression cannot be compared"
                        ),
                    }
                })
                .collect(),
        ))
    }
}

impl From<Filter> for Conditions {
    fn from(filter: Filter) -> Self {
        Conditions(Inner::Filter(filter))
    }
}

/// A JSON value for a database assertion, compared structurally with the
/// column (key order and spacing do not matter), whether the database
/// stores JSON or text.
///
/// ```
/// use navree_orm::testing::json_value;
///
/// let expected = json_value(serde_json::json!({"source": "web", "score": 3}));
/// assert!(matches!(expected, navree_orm::Value::Json(_)));
/// ```
pub fn json_value(value: serde_json::Value) -> Value {
    Value::Json(value)
}

/// Asserts that a row of `table` matches `conditions`.
///
/// # Panics
///
/// When no row matches (the report lists the closest rows), or the table
/// cannot be read.
pub async fn assert_database_has<E: Executor + ?Sized>(
    exec: &E,
    table_name: &str,
    conditions: impl Into<Conditions>,
) {
    let conditions = conditions.into();
    let found = matching_rows(exec, table_name, &conditions).await;
    if found.matched.is_empty() {
        panic!(
            "assert_database_has: no row of `{table_name}` matches {}.\n{}",
            describe(&conditions),
            found.report(&conditions)
        );
    }
}

/// Asserts that no row of `table` matches `conditions`.
///
/// # Panics
///
/// When a row matches (the report lists them), or the table cannot be read.
pub async fn assert_database_missing<E: Executor + ?Sized>(
    exec: &E,
    table_name: &str,
    conditions: impl Into<Conditions>,
) {
    let conditions = conditions.into();
    let found = matching_rows(exec, table_name, &conditions).await;
    if !found.matched.is_empty() {
        let mut report = String::new();
        for row in found.matched.iter().take(CLOSEST_ROWS) {
            let _ = writeln!(report, "  {}", row_json(row));
        }
        panic!(
            "assert_database_missing: {} row(s) of `{table_name}` match {}:\n{report}",
            found.matched.len(),
            describe(&conditions),
        );
    }
}

/// Asserts that `table` has exactly `expected` rows.
///
/// # Panics
///
/// When it has another number, or cannot be read.
pub async fn assert_database_count<E: Executor + ?Sized>(
    exec: &E,
    table_name: &str,
    expected: u64,
) {
    let count = table(table_name).count(exec).await.unwrap_or_else(|error| {
        panic!("assert_database_count: cannot count `{table_name}`: {error}")
    });
    assert!(
        count == expected,
        "assert_database_count: `{table_name}` has {count} row(s), expected {expected}"
    );
}

/// Asserts that `table` has no rows.
///
/// # Panics
///
/// When it has some, or cannot be read.
pub async fn assert_database_empty<E: Executor + ?Sized>(exec: &E, table_name: &str) {
    let count = table(table_name).count(exec).await.unwrap_or_else(|error| {
        panic!("assert_database_empty: cannot count `{table_name}`: {error}")
    });
    assert!(
        count == 0,
        "assert_database_empty: `{table_name}` has {count} row(s), expected none"
    );
}

/// Asserts that `record`'s row is in the database (by its key, ignoring
/// global scopes: a soft-deleted row counts).
///
/// # Panics
///
/// When the row is not there, or the query fails.
pub async fn assert_model_exists<E: Executor + ?Sized, M: Model>(exec: &E, record: &Record<M>) {
    let key = record.key();
    let exists = M::query()
        .without_global_scopes()
        .where_key(key.clone())
        .exists(exec)
        .await
        .unwrap_or_else(|error| {
            panic!(
                "assert_model_exists: cannot read `{}`: {error}",
                M::meta().table()
            )
        });
    assert!(
        exists,
        "assert_model_exists: {} {} is not in `{}`",
        M::meta().name(),
        key.to_key_string(),
        M::meta().table()
    );
}

/// Asserts that `record`'s row is gone from the database (a soft-deleted
/// row is still there: use [`assert_soft_deleted`] for those).
///
/// # Panics
///
/// When the row is still there, or the query fails.
pub async fn assert_model_missing<E: Executor + ?Sized, M: Model>(exec: &E, record: &Record<M>) {
    let key = record.key();
    let exists = M::query()
        .without_global_scopes()
        .where_key(key.clone())
        .exists(exec)
        .await
        .unwrap_or_else(|error| {
            panic!(
                "assert_model_missing: cannot read `{}`: {error}",
                M::meta().table()
            )
        });
    let trashed = if exists {
        " (soft-deleted rows stay: assert_soft_deleted checks those)"
    } else {
        ""
    };
    assert!(
        !exists,
        "assert_model_missing: {} {} is still in `{}`{}",
        M::meta().name(),
        key.to_key_string(),
        M::meta().table(),
        if M::meta().soft_deletes_enabled() {
            trashed
        } else {
            ""
        }
    );
}

/// Asserts that `record`'s row is soft-deleted (its deleted-at column is
/// set).
///
/// # Panics
///
/// When the model does not soft-delete, the row is gone or not deleted, or
/// the query fails.
pub async fn assert_soft_deleted<E: Executor + ?Sized, M: Model>(exec: &E, record: &Record<M>) {
    let state = soft_delete_state(exec, record, "assert_soft_deleted").await;
    let key = record.key().to_key_string();
    let name = M::meta().name();
    match state {
        Some(true) => {}
        Some(false) => panic!("assert_soft_deleted: {name} {key} is not soft-deleted"),
        None => panic!(
            "assert_soft_deleted: {name} {key} is not in `{}` at all",
            M::meta().table()
        ),
    }
}

/// Asserts that `record`'s row is there and not soft-deleted.
///
/// # Panics
///
/// When the model does not soft-delete, the row is gone or deleted, or the
/// query fails.
pub async fn assert_not_soft_deleted<E: Executor + ?Sized, M: Model>(exec: &E, record: &Record<M>) {
    let state = soft_delete_state(exec, record, "assert_not_soft_deleted").await;
    let key = record.key().to_key_string();
    let name = M::meta().name();
    match state {
        Some(false) => {}
        Some(true) => panic!("assert_not_soft_deleted: {name} {key} is soft-deleted"),
        None => panic!(
            "assert_not_soft_deleted: {name} {key} is not in `{}` at all",
            M::meta().table()
        ),
    }
}

/// Asserts that a row of `table` matching `conditions` has its
/// `deleted_at` column set.
///
/// # Panics
///
/// When no matching row is soft-deleted, or the table cannot be read.
pub async fn assert_soft_deleted_in<E: Executor + ?Sized>(
    exec: &E,
    table_name: &str,
    deleted_at: &str,
    conditions: impl Into<Conditions>,
) {
    let conditions = conditions.into();
    let found = matching_rows(exec, table_name, &conditions).await;
    let deleted = found
        .matched
        .iter()
        .any(|row| row.value(deleted_at).is_ok_and(|value| !is_null(&value)));
    if !deleted {
        panic!(
            "assert_soft_deleted_in: no row of `{table_name}` matching {} has `{deleted_at}` set.\n{}",
            describe(&conditions),
            found.report(&conditions)
        );
    }
}

/// Asserts that a row of `table` matching `conditions` has its
/// `deleted_at` column empty.
///
/// # Panics
///
/// When no matching row is live, or the table cannot be read.
pub async fn assert_not_soft_deleted_in<E: Executor + ?Sized>(
    exec: &E,
    table_name: &str,
    deleted_at: &str,
    conditions: impl Into<Conditions>,
) {
    let conditions = conditions.into();
    let found = matching_rows(exec, table_name, &conditions).await;
    let live = found
        .matched
        .iter()
        .any(|row| row.value(deleted_at).is_ok_and(|value| is_null(&value)));
    if !live {
        panic!(
            "assert_not_soft_deleted_in: no row of `{table_name}` matching {} has `{deleted_at}` empty.\n{}",
            describe(&conditions),
            found.report(&conditions)
        );
    }
}

/// Whether `record`'s row is soft-deleted; `None` when it is gone.
async fn soft_delete_state<E: Executor + ?Sized, M: Model>(
    exec: &E,
    record: &Record<M>,
    assertion: &str,
) -> Option<bool> {
    let meta = M::meta();
    let Some(column) = meta.deleted_at() else {
        panic!(
            "{assertion}: {} does not soft-delete (no #[model(soft_deletes)])",
            meta.name()
        );
    };
    let row = table(meta.table())
        .where_(crate::column::col(meta.key_column()).eq(record.key().to_value()))
        .first(exec)
        .await
        .unwrap_or_else(|error| panic!("{assertion}: cannot read `{}`: {error}", meta.table()))?;
    let value = row
        .value(column)
        .unwrap_or_else(|error| panic!("{assertion}: cannot read `{column}`: {error}"));
    Some(!is_null(&value))
}

/// The rows of a table compared with conditions.
struct Found {
    table: String,
    /// Every row read (values conditions), or a sample (filters).
    rows: Vec<Row>,
    /// The rows that match.
    matched: Vec<Row>,
    /// Whether the table had more rows than were read.
    truncated: bool,
}

impl Found {
    /// The closest rows, for a failure report.
    fn report(&self, conditions: &Conditions) -> String {
        if self.rows.is_empty() {
            return format!("`{}` is empty.", self.table);
        }
        let mut report = format!(
            "`{}` has {}{} row(s); ",
            self.table,
            self.rows.len(),
            if self.truncated { "+" } else { "" }
        );
        match &conditions.0 {
            Inner::Values(expected) => {
                let mut scored: Vec<(usize, &Row)> = self
                    .rows
                    .iter()
                    .map(|row| (matching_columns(row, expected), row))
                    .collect();
                scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
                report.push_str("the closest:\n");
                for (score, row) in scored.into_iter().take(CLOSEST_ROWS) {
                    let differs: Vec<String> = expected
                        .iter()
                        .filter(|(column, value)| !column_matches(row, column, value))
                        .map(|(column, _)| match row.value(column) {
                            Ok(actual) => format!("{column} = {}", to_json(&actual)),
                            Err(_) => format!("{column} (no such column)"),
                        })
                        .collect();
                    let _ = writeln!(
                        report,
                        "  {}  (matches {score} of {}; differs on {})",
                        row_json(row),
                        expected.len(),
                        differs.join(", ")
                    );
                }
            }
            Inner::Filter(_) => {
                report.push_str("the first:\n");
                for row in self.rows.iter().take(CLOSEST_ROWS) {
                    let _ = writeln!(report, "  {}", row_json(row));
                }
            }
        }
        report
    }
}

/// The rows of `table_name` matching `conditions`.
async fn matching_rows<E: Executor + ?Sized>(
    exec: &E,
    table_name: &str,
    conditions: &Conditions,
) -> Found {
    let fail =
        |error: crate::OrmError| -> ! { panic!("cannot read `{table_name}` to check it: {error}") };
    match &conditions.0 {
        Inner::Values(expected) => {
            let rows = table(table_name)
                .limit(MAX_INSPECTED_ROWS + 1)
                .get(exec)
                .await
                .unwrap_or_else(|error| fail(error));
            let truncated = rows.len() as u64 > MAX_INSPECTED_ROWS;
            if let Some(first) = rows.first() {
                let columns = first.columns();
                for (column, _) in expected {
                    if !columns.iter().any(|name| name == column) {
                        panic!(
                            "`{table_name}` has no column `{column}` (it has: {})",
                            columns.join(", ")
                        );
                    }
                }
            }
            let matched = rows
                .iter()
                .filter(|row| matching_columns(row, expected) == expected.len())
                .map(clone_row)
                .collect();
            Found {
                table: table_name.to_owned(),
                rows,
                matched,
                truncated,
            }
        }
        Inner::Filter(filter) => {
            let matched = table(table_name)
                .where_(filter.clone())
                .limit(MAX_INSPECTED_ROWS)
                .get(exec)
                .await
                .unwrap_or_else(|error| fail(error));
            let rows = table(table_name)
                .limit(CLOSEST_ROWS as u64)
                .get(exec)
                .await
                .unwrap_or_else(|error| fail(error));
            Found {
                table: table_name.to_owned(),
                rows,
                matched,
                truncated: false,
            }
        }
    }
}

/// A copy of `row` (as values).
fn clone_row(row: &Row) -> Row {
    Row::from_values(row.values().unwrap_or_default())
}

/// How many of `expected` match in `row`.
fn matching_columns(row: &Row, expected: &[(Cow<'static, str>, Value)]) -> usize {
    expected
        .iter()
        .filter(|(column, value)| column_matches(row, column, value))
        .count()
}

/// Whether `row`'s `column` holds `expected`.
fn column_matches(row: &Row, column: &str, expected: &Value) -> bool {
    match row.value(column) {
        Ok(actual) => value_matches(&actual, expected, |ty| decode_as(row, column, ty)),
        Err(_) => false,
    }
}

/// The kinds of value an expected value compares as.
#[derive(Clone, Copy, Debug)]
enum Kind {
    Bool,
    Integer,
    Float,
    Decimal,
    Text,
    Uuid,
    DateTime,
    Date,
    Time,
    Json,
}

/// `row`'s `column` read as `kind`.
fn decode_as(row: &Row, column: &str, kind: Kind) -> Option<Value> {
    fn get<T: Field>(row: &Row, column: &str) -> Option<Value> {
        row.get::<Option<T>>(column)
            .ok()
            .map(|value| value.to_value())
    }
    match kind {
        Kind::Bool => get::<bool>(row, column),
        Kind::Integer => get::<i64>(row, column),
        Kind::Float => get::<f64>(row, column),
        Kind::Decimal => get::<Decimal>(row, column),
        Kind::Text => get::<String>(row, column),
        Kind::Uuid => get::<Uuid>(row, column),
        Kind::DateTime => get::<DateTime<Utc>>(row, column),
        Kind::Date => get::<NaiveDate>(row, column),
        Kind::Time => get::<NaiveTime>(row, column),
        Kind::Json => get::<serde_json::Value>(row, column),
    }
}

/// Whether `actual` holds `expected`: the same NULL, the same value
/// decoded as the expected one's kind, JSON structurally.
fn value_matches(actual: &Value, expected: &Value, decode: impl Fn(Kind) -> Option<Value>) -> bool {
    if is_null(expected) || is_null(actual) {
        return is_null(expected) && is_null(actual);
    }
    if crate::value::same_value(actual, expected) {
        return true;
    }
    let kind = match expected {
        Value::Bool(_) => Kind::Bool,
        Value::I8(_)
        | Value::I16(_)
        | Value::I32(_)
        | Value::I64(_)
        | Value::U8(_)
        | Value::U16(_)
        | Value::U32(_)
        | Value::U64(_) => Kind::Integer,
        Value::F32(_) | Value::F64(_) => Kind::Float,
        Value::Decimal(_) => Kind::Decimal,
        Value::String(_) | Value::Enum { .. } => Kind::Text,
        Value::Uuid(_) => Kind::Uuid,
        Value::Timestamp(_) | Value::TimestampTz(_) | Value::DateTime(_) => Kind::DateTime,
        Value::Date(_) => Kind::Date,
        Value::Time(_) => Kind::Time,
        Value::Json(_) => Kind::Json,
        _ => return false,
    };
    let expected = normalize(expected.clone(), kind);
    match decode(kind) {
        Some(decoded) => crate::value::same_value(&decoded, &expected),
        None => false,
    }
}

/// `value` in the form a column decoded as `kind` takes.
fn normalize(value: Value, kind: Kind) -> Value {
    fn convert<T: Field>(value: Value) -> Value {
        match T::from_value(value.clone()) {
            Ok(converted) => converted.to_value(),
            Err(_) => value,
        }
    }
    match kind {
        Kind::Integer => convert::<i64>(value),
        Kind::Float => convert::<f64>(value),
        Kind::DateTime => match value {
            Value::DateTime(naive) => Value::from(naive.and_utc()),
            other => match DateTime::<FixedOffset>::from_value(other.clone()) {
                Ok(at) => Value::from(at.with_timezone(&Utc)),
                Err(_) => convert::<NaiveDateTime>(other),
            },
        },
        _ => value,
    }
}

/// `row` as compact JSON, for reports.
fn row_json(row: &Row) -> String {
    row.to_json()
        .map(|json| json.to_string())
        .unwrap_or_else(|error| format!("<unreadable row: {error}>"))
}

/// `conditions` for a report: `title = "Acme", stage = "won"`.
fn describe(conditions: &Conditions) -> String {
    match &conditions.0 {
        Inner::Values(expected) => expected
            .iter()
            .map(|(column, value)| format!("{column} = {}", to_json(value)))
            .collect::<Vec<_>>()
            .join(", "),
        Inner::Filter(filter) => match filter.to_sql(crate::sql::Dialect::Postgres) {
            Ok(statement) => statement.sql,
            Err(_) => "the condition".to_owned(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(values: Vec<(&str, Value)>) -> Row {
        Row::from_values(
            values
                .into_iter()
                .map(|(column, value)| (column.to_owned(), value))
                .collect(),
        )
    }

    #[test]
    fn values_compare_as_the_column_stores_them() {
        let stored = row(vec![
            ("active", Value::I64(1)),
            ("count", Value::I32(3)),
            ("price", Value::String("12.50".into())),
            ("meta", Value::String(r#"{"b": 2, "a": 1}"#.into())),
            ("at", Value::String("2026-10-27T09:30:00+00:00".into())),
            ("note", Value::Null(crate::Kind::String)),
        ]);
        assert!(column_matches(&stored, "active", &Value::from(true)));
        assert!(!column_matches(&stored, "active", &Value::from(false)));
        assert!(column_matches(&stored, "count", &Value::from(3_i64)));
        assert!(column_matches(
            &stored,
            "price",
            &Value::from(Decimal::new(125, 1))
        ));
        assert!(column_matches(
            &stored,
            "meta",
            &json_value(serde_json::json!({"a": 1, "b": 2}))
        ));
        assert!(!column_matches(
            &stored,
            "meta",
            &json_value(serde_json::json!({"a": 1}))
        ));
        let at = DateTime::parse_from_rfc3339("2026-10-27T11:30:00+02:00").unwrap();
        assert!(column_matches(&stored, "at", &Value::from(at)));
        assert!(column_matches(
            &stored,
            "note",
            &Value::Null(crate::Kind::String)
        ));
        assert!(!column_matches(&stored, "note", &Value::from("x")));
        assert!(!column_matches(&stored, "missing", &Value::from("x")));
    }

    #[test]
    fn conditions_come_from_pairs_assignments_and_filters() {
        let pairs = Conditions::from([("title", "Acme")]);
        assert_eq!(describe(&pairs), r#"title = "Acme""#);
        let assignments = Conditions::from([Assignment::value("stage", "won")]);
        assert_eq!(describe(&assignments), r#"stage = "won""#);
        let filter = Conditions::from(crate::col("amount").gt(5));
        assert_eq!(describe(&filter), r#""amount" > $1"#);
    }
}
