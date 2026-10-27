//! [`Expression`] (raw SQL) and [`DefaultValue`] (a column default).

use std::fmt;

/// A raw SQL expression, written into statements as it is: a default such
/// as `CURRENT_TIMESTAMP`, a generated column's formula, an index
/// expression.
///
/// ```
/// use navree_schema::{Expression, raw};
///
/// let expr: Expression = raw("lower(email)");
/// assert_eq!(expr.as_str(), "lower(email)");
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Expression(String);

impl Expression {
    /// An expression from SQL text.
    pub fn new(sql: impl Into<String>) -> Expression {
        Expression(sql.into())
    }

    /// The SQL text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Expression {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for Expression {
    fn from(sql: &str) -> Expression {
        Expression::new(sql)
    }
}

impl From<String> for Expression {
    fn from(sql: String) -> Expression {
        Expression(sql)
    }
}

/// A raw SQL [`Expression`]: `t.timestamp("seen_at").default(raw("CURRENT_TIMESTAMP"))`.
pub fn raw(sql: impl Into<String>) -> Expression {
    Expression::new(sql)
}

/// A column default: a typed literal or an [`Expression`].
///
/// Literals are rendered for each database: integers and floats bare,
/// strings quoted, booleans `TRUE`/`FALSE` on PostgreSQL and `1`/`0`
/// elsewhere.
///
/// ```
/// use navree_schema::{DefaultValue, raw};
///
/// assert_eq!(DefaultValue::from(0), DefaultValue::Int(0));
/// assert_eq!(DefaultValue::from("lead"), DefaultValue::Text("lead".into()));
/// assert_eq!(DefaultValue::from(true), DefaultValue::Bool(true));
/// assert!(matches!(DefaultValue::from(raw("now()")), DefaultValue::Expression(_)));
/// ```
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum DefaultValue {
    /// `NULL`.
    Null,
    /// A boolean.
    Bool(bool),
    /// A signed integer.
    Int(i64),
    /// An unsigned integer.
    UInt(u64),
    /// A floating-point number.
    Float(f64),
    /// A string.
    Text(String),
    /// Raw SQL.
    Expression(Expression),
}

impl From<bool> for DefaultValue {
    fn from(value: bool) -> DefaultValue {
        DefaultValue::Bool(value)
    }
}

macro_rules! signed_defaults {
    ($($ty:ty),*) => {$(
        impl From<$ty> for DefaultValue {
            fn from(value: $ty) -> DefaultValue {
                DefaultValue::Int(i64::from(value))
            }
        }
    )*};
}
signed_defaults!(i8, i16, i32, i64);

macro_rules! unsigned_defaults {
    ($($ty:ty),*) => {$(
        impl From<$ty> for DefaultValue {
            fn from(value: $ty) -> DefaultValue {
                DefaultValue::UInt(u64::from(value))
            }
        }
    )*};
}
unsigned_defaults!(u8, u16, u32, u64);

impl From<f32> for DefaultValue {
    fn from(value: f32) -> DefaultValue {
        DefaultValue::Float(f64::from(value))
    }
}

impl From<f64> for DefaultValue {
    fn from(value: f64) -> DefaultValue {
        DefaultValue::Float(value)
    }
}

impl From<&str> for DefaultValue {
    fn from(value: &str) -> DefaultValue {
        DefaultValue::Text(value.to_owned())
    }
}

impl From<String> for DefaultValue {
    fn from(value: String) -> DefaultValue {
        DefaultValue::Text(value)
    }
}

impl From<Expression> for DefaultValue {
    fn from(value: Expression) -> DefaultValue {
        DefaultValue::Expression(value)
    }
}

impl From<serde_json::Value> for DefaultValue {
    /// A JSON document, stored as its text (`'{"a":1}'`).
    fn from(value: serde_json::Value) -> DefaultValue {
        DefaultValue::Text(value.to_string())
    }
}

impl<T: Into<DefaultValue>> From<Option<T>> for DefaultValue {
    /// `None` is `NULL`.
    fn from(value: Option<T>) -> DefaultValue {
        value.map_or(DefaultValue::Null, Into::into)
    }
}

impl DefaultValue {
    /// Whether the default is computed by the database rather than a
    /// constant (SQLite cannot add such a column in place).
    pub(crate) fn is_expression(&self) -> bool {
        matches!(self, DefaultValue::Expression(_))
    }
}
