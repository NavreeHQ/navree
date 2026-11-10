//! [`SchemaConnection`]: what the schema builder needs from a database
//! connection, with the [`SchemaRow`]s and [`SchemaValue`]s it reads.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::error::SchemaError;
use crate::target::Dialect;

/// A boxed, sendable future.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// What the schema builder needs from a connection: its dialect, a way to
/// run a statement and a way to read rows.
///
/// `navree-db` implements it for its pools, transactions, units of work and
/// `Db`, so a [`Schema`](crate::Schema) works on any of them:
/// `Schema::new(&db)`. References, `Arc`s and `Box`es of a connection are
/// connections too.
///
/// An implementation for another driver needs the three methods; errors
/// carry the statement that failed ([`SchemaError::database`]):
///
/// ```
/// use navree_schema::{BoxFuture, Dialect, SchemaConnection, SchemaError, SchemaRow, SchemaValue};
///
/// /// A connection that refuses everything.
/// struct Closed;
///
/// impl SchemaConnection for Closed {
///     fn backend(&self) -> Dialect {
///         Dialect::Sqlite
///     }
///
///     fn execute<'a>(&'a self, sql: &'a str) -> BoxFuture<'a, Result<(), SchemaError>> {
///         Box::pin(async move { Err(SchemaError::database(sql, "the connection is closed")) })
///     }
///
///     fn query<'a>(
///         &'a self,
///         sql: &'a str,
///         _params: &'a [SchemaValue],
///     ) -> BoxFuture<'a, Result<Vec<SchemaRow>, SchemaError>> {
///         Box::pin(async move { Err(SchemaError::database(sql, "the connection is closed")) })
///     }
/// }
/// ```
pub trait SchemaConnection: Send + Sync {
    /// The database family.
    fn backend(&self) -> Dialect;

    /// Runs `sql` (unprepared: DDL takes no parameters; it may hold several
    /// statements separated by `;`).
    fn execute<'a>(&'a self, sql: &'a str) -> BoxFuture<'a, Result<(), SchemaError>>;

    /// Runs a query with `params` bound to its placeholders, written in the
    /// dialect's own form (`$1`, `$2` on PostgreSQL; `?` on MySQL and
    /// SQLite), and returns its rows.
    fn query<'a>(
        &'a self,
        sql: &'a str,
        params: &'a [SchemaValue],
    ) -> BoxFuture<'a, Result<Vec<SchemaRow>, SchemaError>>;
}

macro_rules! forward {
    ($($ty:ty),*) => {
        $(
            impl<C: SchemaConnection + ?Sized> SchemaConnection for $ty {
                fn backend(&self) -> Dialect {
                    (**self).backend()
                }

                fn execute<'a>(&'a self, sql: &'a str) -> BoxFuture<'a, Result<(), SchemaError>> {
                    (**self).execute(sql)
                }

                fn query<'a>(
                    &'a self,
                    sql: &'a str,
                    params: &'a [SchemaValue],
                ) -> BoxFuture<'a, Result<Vec<SchemaRow>, SchemaError>> {
                    (**self).query(sql, params)
                }
            }
        )*
    };
}

forward!(&C, Arc<C>, Box<C>);

/// A value a catalog query binds or returns: what the schema builder reads
/// from a database's catalogs (names, types, sizes, flags, definitions).
#[derive(Clone, Debug, PartialEq)]
pub enum SchemaValue {
    /// SQL `NULL`.
    Null,
    /// An integer of any width.
    Int(i64),
    /// A floating-point number.
    Float(f64),
    /// Text (and anything a connection reads as text: exact decimals,
    /// dates, UUIDs, JSON).
    Text(String),
    /// Bytes (MySQL returns some catalog text in binary columns).
    Bytes(Vec<u8>),
    /// A boolean.
    Bool(bool),
}

impl SchemaValue {
    /// Whether the value is `NULL`.
    pub fn is_null(&self) -> bool {
        matches!(self, SchemaValue::Null)
    }

    /// The value as text, whatever its kind (`None` for `NULL`): bytes as
    /// UTF-8 (lossy), numbers and booleans written out (`true`/`false`).
    pub fn as_text(&self) -> Option<String> {
        match self {
            SchemaValue::Null => None,
            SchemaValue::Int(v) => Some(v.to_string()),
            SchemaValue::Float(v) => Some(v.to_string()),
            SchemaValue::Text(v) => Some(v.clone()),
            SchemaValue::Bytes(v) => Some(String::from_utf8_lossy(v).into_owned()),
            SchemaValue::Bool(v) => Some(v.to_string()),
        }
    }

    /// The value as an integer: integers, booleans (`1`/`0`), whole floats,
    /// and text that parses.
    pub fn as_int(&self) -> Option<i64> {
        match self {
            SchemaValue::Int(v) => Some(*v),
            SchemaValue::Bool(v) => Some(i64::from(*v)),
            #[allow(clippy::cast_possible_truncation)]
            SchemaValue::Float(v) if v.fract() == 0.0 && v.abs() < 9.0e15 => Some(*v as i64),
            SchemaValue::Text(_) | SchemaValue::Bytes(_) => {
                self.as_text().and_then(|t| t.trim().parse().ok())
            }
            _ => None,
        }
    }

    /// The value as a boolean: booleans, non-zero numbers, and text such as
    /// `t`, `true`, `yes` or `1` (`false` otherwise, `NULL` included).
    pub fn as_bool(&self) -> bool {
        match self {
            SchemaValue::Bool(v) => *v,
            SchemaValue::Null => false,
            other => other.as_int().map_or_else(
                || {
                    other.as_text().is_some_and(|t| {
                        matches!(
                            t.trim().to_ascii_lowercase().as_str(),
                            "yes" | "y" | "true" | "t" | "1"
                        )
                    })
                },
                |n| n != 0,
            ),
        }
    }
}

impl From<&str> for SchemaValue {
    fn from(value: &str) -> SchemaValue {
        SchemaValue::Text(value.to_owned())
    }
}

impl From<String> for SchemaValue {
    fn from(value: String) -> SchemaValue {
        SchemaValue::Text(value)
    }
}

impl From<i64> for SchemaValue {
    fn from(value: i64) -> SchemaValue {
        SchemaValue::Int(value)
    }
}

impl From<bool> for SchemaValue {
    fn from(value: bool) -> SchemaValue {
        SchemaValue::Bool(value)
    }
}

impl fmt::Display for SchemaValue {
    /// The value as text, `NULL` for `NULL`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_text().as_deref().unwrap_or("NULL"))
    }
}

/// One row a [`SchemaConnection::query`] returns: its column names and
/// values, read by name or position.
///
/// ```
/// use navree_schema::{SchemaRow, SchemaValue};
///
/// let row = SchemaRow::new(
///     vec!["name".into(), "rows".into(), "partitioned".into()],
///     vec![SchemaValue::Text("events".into()), SchemaValue::Text("42".into()), SchemaValue::Int(1)],
/// );
/// assert_eq!(row.text("name").as_deref(), Some("events"));
/// assert_eq!(row.int("rows"), Some(42));
/// assert!(row.boolean("partitioned"));
/// assert_eq!(row.text("missing"), None);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct SchemaRow {
    columns: Arc<[String]>,
    values: Vec<SchemaValue>,
}

impl SchemaRow {
    /// A row of `values` under `columns` (in the same order).
    pub fn new(columns: impl Into<Arc<[String]>>, values: Vec<SchemaValue>) -> SchemaRow {
        SchemaRow {
            columns: columns.into(),
            values,
        }
    }

    /// The column names, in order.
    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    /// The values, in column order.
    pub fn values(&self) -> &[SchemaValue] {
        &self.values
    }

    /// The value of `column` (the first of that name), if the row has it.
    pub fn get(&self, column: &str) -> Option<&SchemaValue> {
        self.columns
            .iter()
            .position(|c| c == column)
            .and_then(|at| self.values.get(at))
    }

    /// The value at `index`, if any.
    pub fn get_index(&self, index: usize) -> Option<&SchemaValue> {
        self.values.get(index)
    }

    /// Column `column` as text, whatever the driver read it as (`None` when
    /// it is `NULL` or absent).
    pub fn text(&self, column: &str) -> Option<String> {
        self.get(column).and_then(SchemaValue::as_text)
    }

    /// Column `column` as an integer.
    pub fn int(&self, column: &str) -> Option<i64> {
        self.get(column).and_then(SchemaValue::as_int)
    }

    /// Column `column` as a boolean (`false` when absent or `NULL`).
    pub fn boolean(&self, column: &str) -> bool {
        self.get(column).is_some_and(SchemaValue::as_bool)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_read_tolerantly() {
        assert_eq!(
            SchemaValue::Bytes(b"abc".to_vec()).as_text().unwrap(),
            "abc"
        );
        assert_eq!(SchemaValue::Text(" 12 ".into()).as_int(), Some(12));
        assert_eq!(SchemaValue::Float(3.0).as_int(), Some(3));
        assert_eq!(SchemaValue::Float(3.5).as_int(), None);
        assert!(SchemaValue::Text("YES".into()).as_bool());
        assert!(SchemaValue::Int(2).as_bool());
        assert!(!SchemaValue::Text("f".into()).as_bool());
        assert!(!SchemaValue::Null.as_bool());
        assert_eq!(SchemaValue::Null.to_string(), "NULL");
        assert_eq!(SchemaValue::Bool(true).to_string(), "true");
    }
}
