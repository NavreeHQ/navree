//! [`Row`]: one result row, from an SQL database or a document store.

use std::borrow::Cow;
use std::fmt;

use crate::error::OrmError;
use crate::value::{Field, FieldError, Value};

/// One result row: navree-db's row of an SQL query (each column read by its
/// own type), or the `(column, value)` pairs a document driver returns.
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
    inner: navree_db::Row,
}

impl Row {
    /// A row navree-db read.
    pub fn from_sql(row: navree_db::Row) -> Self {
        Row { inner: row }
    }

    /// A row of `(column, value)` pairs (document drivers).
    pub fn from_values(values: Vec<(String, Value)>) -> Self {
        Row {
            inner: navree_db::Row::from_values(values),
        }
    }

    /// The column names, in order.
    pub fn columns(&self) -> Vec<String> {
        self.inner.columns()
    }

    /// Whether the row has column `column`.
    pub fn has_column(&self, column: &str) -> bool {
        self.inner.has_column(column)
    }

    /// Column `column` read as `T`.
    pub fn get<T: Field>(&self, column: &str) -> Result<T, OrmError> {
        self.field(column)
            .map_err(|error| decode_error("row", column, T::type_name(), &error))
    }

    /// Column `column` read by its own type (a number column gives a number
    /// variant, a text column a string, ...).
    pub fn value(&self, column: &str) -> Result<Value, OrmError> {
        self.get::<Value>(column)
    }

    /// Every column as `(name, value)`, read by its own type.
    pub fn values(&self) -> Result<Vec<(String, Value)>, OrmError> {
        self.columns()
            .into_iter()
            .map(|column| {
                let value = self.value(&column)?;
                Ok((column, value))
            })
            .collect()
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

    /// navree-db's row underneath.
    pub fn db_row(&self) -> &navree_db::Row {
        &self.inner
    }

    /// navree-db's row underneath, by value.
    pub fn into_db_row(self) -> navree_db::Row {
        self.inner
    }

    /// Column `column` as `T`, with the bare decoding error.
    pub(crate) fn field<T: Field>(&self, column: &str) -> Result<T, FieldError> {
        T::from_sql(&self.inner, column)
    }
}

impl From<navree_db::Row> for Row {
    fn from(row: navree_db::Row) -> Self {
        Row::from_sql(row)
    }
}

/// The row as a JSON object of its columns (see [`Row::to_json`]).
impl serde::Serialize for Row {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_json()
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
}

impl fmt::Debug for Row {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Row").field(&self.inner).finish()
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
