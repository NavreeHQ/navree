//! [`Row`]: one result row, from an SQL database or a document store.

use std::borrow::Cow;
use std::fmt;

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use sea_orm::{QueryResult, TryGetError, TryGetable};
use uuid::Uuid;

use crate::error::OrmError;
use crate::value::{Decimal, Field, FieldError, Value};

/// One result row: an SQL row (decoded by each column's exact type) or the
/// `(column, value)` pairs a document driver returns.
///
/// Model queries turn rows into records; [`TableBuilder`](crate::TableBuilder)
/// queries hand them out as they are.
///
/// ```
/// use navree_orm::{Row, Value};
///
/// let row = Row::from_values(vec![
///     ("title".to_owned(), Value::from("Renewal")),
///     ("amount".to_owned(), Value::from(1200_i64)),
/// ]);
/// assert_eq!(row.get::<String>("title").unwrap(), "Renewal");
/// assert_eq!(row.get::<i32>("amount").unwrap(), 1200);
/// assert!(row.get::<String>("missing").is_err());
/// assert_eq!(row.columns(), ["title", "amount"]);
/// ```
pub struct Row {
    inner: RowInner,
}

enum RowInner {
    Sql(QueryResult),
    Values(Vec<(String, Value)>),
}

impl Row {
    /// A row from an SQL result.
    pub fn from_sql(result: QueryResult) -> Self {
        Row {
            inner: RowInner::Sql(result),
        }
    }

    /// A row of `(column, value)` pairs (document drivers).
    pub fn from_values(values: Vec<(String, Value)>) -> Self {
        Row {
            inner: RowInner::Values(values),
        }
    }

    /// The column names, in order.
    pub fn columns(&self) -> Vec<String> {
        match &self.inner {
            RowInner::Sql(result) => result.column_names(),
            RowInner::Values(values) => values.iter().map(|(name, _)| name.clone()).collect(),
        }
    }

    /// Whether the row has column `column`.
    pub fn has_column(&self, column: &str) -> bool {
        match &self.inner {
            RowInner::Sql(result) => result.column_names().iter().any(|name| name == column),
            RowInner::Values(values) => values.iter().any(|(name, _)| name == column),
        }
    }

    /// Column `column` read as `T`.
    pub fn get<T: Field>(&self, column: &str) -> Result<T, OrmError> {
        self.field(column).map_err(|error| decode_error("row", column, T::type_name(), &error))
    }

    /// Column `column` decoded by its own type (a number column gives a
    /// number variant, a text column a string, ...).
    pub fn value(&self, column: &str) -> Result<Value, OrmError> {
        self.get::<Value>(column)
    }

    /// Every column as `(name, value)`, decoded by its own type.
    pub fn values(&self) -> Result<Vec<(String, Value)>, OrmError> {
        match &self.inner {
            RowInner::Values(values) => Ok(values.clone()),
            RowInner::Sql(_) => self
                .columns()
                .into_iter()
                .map(|column| {
                    let value = self.value(&column)?;
                    Ok((column, value))
                })
                .collect(),
        }
    }

    /// The row as a JSON object.
    pub fn to_json(&self) -> Result<serde_json::Value, OrmError> {
        Ok(serde_json::Value::Object(
            self.values()?
                .into_iter()
                .map(|(column, value)| (column, crate::value::to_json(&value)))
                .collect(),
        ))
    }

    /// The SQL result, when the row came from an SQL database.
    pub fn sql(&self) -> Option<&QueryResult> {
        match &self.inner {
            RowInner::Sql(result) => Some(result),
            RowInner::Values(_) => None,
        }
    }

    /// Column `column` as `T`, with the bare decoding error.
    pub(crate) fn field<T: Field>(&self, column: &str) -> Result<T, FieldError> {
        match &self.inner {
            RowInner::Sql(result) => T::from_sql(result, column),
            RowInner::Values(values) => match values.iter().find(|(name, _)| name == column) {
                Some((_, value)) => T::from_value(value.clone()),
                None => Err(FieldError::Missing),
            },
        }
    }
}

impl fmt::Debug for Row {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.inner {
            RowInner::Sql(result) => f.debug_tuple("Row").field(result).finish(),
            RowInner::Values(values) => f.debug_tuple("Row").field(values).finish(),
        }
    }
}

/// The [`OrmError::Decode`] for `column` of `model`.
pub(crate) fn decode_error(
    model: impl Into<Cow<'static, str>>,
    column: &str,
    expected: Cow<'static, str>,
    error: &FieldError,
) -> OrmError {
    OrmError::Decode {
        model: model.into(),
        column: column.to_owned(),
        expected,
        message: error.to_string(),
    }
}

/// Decodes `column` by its own type: each candidate type is tried in an
/// order that keeps SQLite's integers integers and PostgreSQL's numerics
/// exact. A NULL decodes as the NULL of the first candidate.
pub(crate) fn sniff(row: &QueryResult, column: &str) -> Result<Value, FieldError> {
    macro_rules! attempt {
        ($ty:ty) => {
            match <Option<$ty> as TryGetable>::try_get(row, "", column) {
                Ok(value) => return Ok(Value::from(value)),
                Err(TryGetError::Null(_)) => return Ok(<Option<$ty> as Field>::null()),
                Err(TryGetError::DbErr(_)) => {}
            }
        };
    }
    if !row.column_names().iter().any(|name| name == column) {
        return Err(FieldError::Missing);
    }
    attempt!(i64);
    attempt!(i32);
    attempt!(i16);
    attempt!(f64);
    attempt!(f32);
    attempt!(Decimal);
    attempt!(bool);
    attempt!(String);
    attempt!(Uuid);
    attempt!(DateTime<Utc>);
    attempt!(DateTime<FixedOffset>);
    attempt!(NaiveDateTime);
    attempt!(NaiveDate);
    attempt!(NaiveTime);
    attempt!(serde_json::Value);
    attempt!(Vec<u8>);
    Err(FieldError::Invalid(
        "the column's type has no ORM value".to_owned(),
    ))
}
