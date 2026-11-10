//! [`Value`]: what a statement binds and a row holds, and [`FromValue`]:
//! how Rust types are read from one.

use std::fmt;

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use rust_decimal::Decimal;
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use uuid::Uuid;

/// A value a statement binds or a row holds.
///
/// Every value is bound as a parameter, never written into the SQL. A NULL
/// keeps its [`Kind`], because PostgreSQL types every parameter
/// (`Value::Null(Kind::I64)` binds a `BIGINT` NULL). Rust values convert
/// with `From` (`Value::from(42_i64)`, `Value::from("text")`,
/// `Value::from(None::<Uuid>)`), and read back with [`FromValue`].
///
/// ```
/// use navree_db::{FromValue, Kind, Value};
///
/// assert_eq!(Value::from(None::<i64>), Value::Null(Kind::I64));
/// assert_eq!(i64::from_value(Value::I32(7)).unwrap(), 7);
/// assert_eq!(bool::from_value(Value::I64(1)).unwrap(), true);
/// let id = uuid::Uuid::nil();
/// assert_eq!(uuid::Uuid::from_value(Value::String(id.to_string())).unwrap(), id);
/// ```
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Value {
    /// SQL `NULL`, of a kind.
    Null(Kind),
    /// A boolean.
    Bool(bool),
    /// An 8-bit integer.
    I8(i8),
    /// A 16-bit integer.
    I16(i16),
    /// A 32-bit integer.
    I32(i32),
    /// A 64-bit integer.
    I64(i64),
    /// An unsigned 8-bit integer.
    U8(u8),
    /// An unsigned 16-bit integer.
    U16(u16),
    /// An unsigned 32-bit integer.
    U32(u32),
    /// An unsigned 64-bit integer.
    U64(u64),
    /// A 32-bit float.
    F32(f32),
    /// A 64-bit float.
    F64(f64),
    /// An exact decimal (`NUMERIC`, `DECIMAL`).
    Decimal(Decimal),
    /// Text.
    String(String),
    /// Bytes.
    Bytes(Vec<u8>),
    /// JSON (`JSONB` on PostgreSQL, `JSON` on MySQL, text on SQLite).
    Json(serde_json::Value),
    /// A UUID (`UUID` on PostgreSQL, 16 bytes on MySQL and SQLite).
    Uuid(Uuid),
    /// A date.
    Date(NaiveDate),
    /// A time of day.
    Time(NaiveTime),
    /// A date and time without a time zone (`TIMESTAMP`, `DATETIME`).
    DateTime(NaiveDateTime),
    /// An instant in UTC (`TIMESTAMPTZ`, MySQL `TIMESTAMP`).
    Timestamp(DateTime<Utc>),
    /// An instant with its UTC offset.
    TimestampTz(DateTime<FixedOffset>),
    /// A value of a named enumerated type: bound as its text on every SQL
    /// backend (a renderer casts it to `type_name` on PostgreSQL, where
    /// enums are types of their own), and what a document store reads its
    /// own typed values from (navree-mongo's `ObjectId`).
    Enum {
        /// The type's name, e.g. `mood` or `ObjectId`.
        type_name: String,
        /// The value.
        value: String,
    },
    /// A vector of 32-bit floats (embeddings): pgvector's `vector` on
    /// PostgreSQL, little-endian `f32` bytes on MySQL and SQLite (MySQL 9's
    /// and MariaDB's `VECTOR`, sqlite-vec's blobs).
    Vector(Vec<f32>),
}

/// The kind of a [`Value`]: what a typed `NULL` is, and what a column
/// holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Kind {
    /// [`Value::Bool`].
    Bool,
    /// [`Value::I8`].
    I8,
    /// [`Value::I16`].
    I16,
    /// [`Value::I32`].
    I32,
    /// [`Value::I64`].
    I64,
    /// [`Value::U8`].
    U8,
    /// [`Value::U16`].
    U16,
    /// [`Value::U32`].
    U32,
    /// [`Value::U64`].
    U64,
    /// [`Value::F32`].
    F32,
    /// [`Value::F64`].
    F64,
    /// [`Value::Decimal`].
    Decimal,
    /// [`Value::String`].
    String,
    /// [`Value::Bytes`].
    Bytes,
    /// [`Value::Json`].
    Json,
    /// [`Value::Uuid`].
    Uuid,
    /// [`Value::Date`].
    Date,
    /// [`Value::Time`].
    Time,
    /// [`Value::DateTime`].
    DateTime,
    /// [`Value::Timestamp`].
    Timestamp,
    /// [`Value::TimestampTz`].
    TimestampTz,
    /// [`Value::Enum`] (its NULL binds as text).
    Enum,
    /// [`Value::Vector`].
    Vector,
}

impl Kind {
    /// The kind's name in messages (`i64`, `text`, `timestamp`, ...).
    pub fn name(self) -> &'static str {
        match self {
            Kind::Bool => "bool",
            Kind::I8 => "i8",
            Kind::I16 => "i16",
            Kind::I32 => "i32",
            Kind::I64 => "i64",
            Kind::U8 => "u8",
            Kind::U16 => "u16",
            Kind::U32 => "u32",
            Kind::U64 => "u64",
            Kind::F32 => "f32",
            Kind::F64 => "f64",
            Kind::Decimal => "decimal",
            Kind::String => "text",
            Kind::Bytes => "bytes",
            Kind::Json => "json",
            Kind::Uuid => "uuid",
            Kind::Date => "date",
            Kind::Time => "time",
            Kind::DateTime => "datetime",
            Kind::Timestamp => "timestamp",
            Kind::TimestampTz => "timestamptz",
            Kind::Enum => "enum",
            Kind::Vector => "vector",
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl Value {
    /// A `NULL` of `kind`.
    pub fn null(kind: Kind) -> Value {
        Value::Null(kind)
    }

    /// Whether this is `NULL`.
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null(_))
    }

    /// The value's kind (a `NULL`'s own).
    pub fn kind(&self) -> Kind {
        match self {
            Value::Null(kind) => *kind,
            Value::Bool(_) => Kind::Bool,
            Value::I8(_) => Kind::I8,
            Value::I16(_) => Kind::I16,
            Value::I32(_) => Kind::I32,
            Value::I64(_) => Kind::I64,
            Value::U8(_) => Kind::U8,
            Value::U16(_) => Kind::U16,
            Value::U32(_) => Kind::U32,
            Value::U64(_) => Kind::U64,
            Value::F32(_) => Kind::F32,
            Value::F64(_) => Kind::F64,
            Value::Decimal(_) => Kind::Decimal,
            Value::String(_) => Kind::String,
            Value::Bytes(_) => Kind::Bytes,
            Value::Json(_) => Kind::Json,
            Value::Uuid(_) => Kind::Uuid,
            Value::Date(_) => Kind::Date,
            Value::Time(_) => Kind::Time,
            Value::DateTime(_) => Kind::DateTime,
            Value::Timestamp(_) => Kind::Timestamp,
            Value::TimestampTz(_) => Kind::TimestampTz,
            Value::Enum { .. } => Kind::Enum,
            Value::Vector(_) => Kind::Vector,
        }
    }

    /// The value as JSON: numbers as numbers (decimals as text, so nothing
    /// is rounded; a float that is not finite as `null`), bytes as an array
    /// of numbers, UUIDs and dates as text (date-times in RFC 3339, UTC
    /// with a `Z` for [`Value::Timestamp`]), JSON as itself, an enum as its
    /// text, a vector as an array of numbers.
    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::Value as Json;
        match self {
            Value::Null(_) => Json::Null,
            Value::Bool(b) => Json::Bool(*b),
            Value::I8(n) => Json::from(*n),
            Value::I16(n) => Json::from(*n),
            Value::I32(n) => Json::from(*n),
            Value::I64(n) => Json::from(*n),
            Value::U8(n) => Json::from(*n),
            Value::U16(n) => Json::from(*n),
            Value::U32(n) => Json::from(*n),
            Value::U64(n) => Json::from(*n),
            Value::F32(n) => {
                serde_json::Number::from_f64(f64::from(*n)).map_or(Json::Null, Json::Number)
            }
            Value::F64(n) => serde_json::Number::from_f64(*n).map_or(Json::Null, Json::Number),
            Value::Decimal(d) => Json::String(d.normalize().to_string()),
            Value::String(s) => Json::String(s.clone()),
            Value::Bytes(b) => Json::Array(b.iter().map(|byte| Json::from(*byte)).collect()),
            Value::Json(json) => json.clone(),
            Value::Uuid(id) => Json::String(id.to_string()),
            Value::Date(d) => Json::String(d.to_string()),
            Value::Time(t) => Json::String(t.to_string()),
            Value::DateTime(dt) => Json::String(dt.format("%Y-%m-%dT%H:%M:%S%.f").to_string()),
            Value::Timestamp(at) => {
                Json::String(at.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true))
            }
            Value::TimestampTz(at) => {
                Json::String(at.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, false))
            }
            Value::Enum { value, .. } => Json::String(value.clone()),
            Value::Vector(items) => Json::Array(
                items
                    .iter()
                    .map(|item| {
                        serde_json::Number::from_f64(f64::from(*item))
                            .map_or(Json::Null, Json::Number)
                    })
                    .collect(),
            ),
        }
    }

    /// A value of the enumerated type `type_name`.
    pub fn enumeration(type_name: impl Into<String>, value: impl Into<String>) -> Value {
        Value::Enum {
            type_name: type_name.into(),
            value: value.into(),
        }
    }

    /// The value written as an SQL literal, for reading (logs, test
    /// output): `NULL`, `TRUE`, `42`, `'it''s'`, `X'00ff'`. Statements bind
    /// their values instead.
    pub fn to_sql_literal(&self) -> String {
        fn quoted(text: &str) -> String {
            format!("'{}'", text.replace('\'', "''"))
        }
        match self {
            Value::Null(_) => "NULL".to_owned(),
            Value::Bool(true) => "TRUE".to_owned(),
            Value::Bool(false) => "FALSE".to_owned(),
            Value::I8(n) => n.to_string(),
            Value::I16(n) => n.to_string(),
            Value::I32(n) => n.to_string(),
            Value::I64(n) => n.to_string(),
            Value::U8(n) => n.to_string(),
            Value::U16(n) => n.to_string(),
            Value::U32(n) => n.to_string(),
            Value::U64(n) => n.to_string(),
            Value::F32(n) => n.to_string(),
            Value::F64(n) => n.to_string(),
            Value::Decimal(d) => d.to_string(),
            Value::String(s) => quoted(s),
            Value::Bytes(b) => {
                let hex: String = b.iter().map(|byte| format!("{byte:02x}")).collect();
                format!("X'{hex}'")
            }
            Value::Json(json) => quoted(&json.to_string()),
            Value::Uuid(id) => quoted(&id.to_string()),
            Value::Enum { value, .. } => quoted(value),
            Value::Vector(items) => {
                let items: Vec<String> = items.iter().map(f32::to_string).collect();
                quoted(&format!("[{}]", items.join(",")))
            }
            other => match other.to_json() {
                serde_json::Value::String(text) => quoted(&text),
                json => json.to_string(),
            },
        }
    }
}

impl fmt::Display for Value {
    /// [`to_sql_literal`](Value::to_sql_literal).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_sql_literal())
    }
}

/// A Rust type with a [`Kind`]: what `None` of it binds as.
pub trait HasKind {
    /// The kind of the values of this type.
    const KIND: Kind;
}

macro_rules! value_from {
    ($($ty:ty => $variant:ident, $kind:ident;)*) => {
        $(
            impl From<$ty> for Value {
                fn from(value: $ty) -> Value {
                    Value::$variant(value)
                }
            }

            impl HasKind for $ty {
                const KIND: Kind = Kind::$kind;
            }
        )*
    };
}

value_from! {
    bool => Bool, Bool;
    i8 => I8, I8;
    i16 => I16, I16;
    i32 => I32, I32;
    i64 => I64, I64;
    u8 => U8, U8;
    u16 => U16, U16;
    u32 => U32, U32;
    u64 => U64, U64;
    f32 => F32, F32;
    f64 => F64, F64;
    Decimal => Decimal, Decimal;
    String => String, String;
    Vec<u8> => Bytes, Bytes;
    serde_json::Value => Json, Json;
    Uuid => Uuid, Uuid;
    NaiveDate => Date, Date;
    NaiveTime => Time, Time;
    NaiveDateTime => DateTime, DateTime;
    DateTime<Utc> => Timestamp, Timestamp;
    DateTime<FixedOffset> => TimestampTz, TimestampTz;
    Vec<f32> => Vector, Vector;
}

impl From<&str> for Value {
    fn from(value: &str) -> Value {
        Value::String(value.to_owned())
    }
}

impl HasKind for &str {
    const KIND: Kind = Kind::String;
}

impl From<&String> for Value {
    fn from(value: &String) -> Value {
        Value::String(value.clone())
    }
}

impl From<char> for Value {
    fn from(value: char) -> Value {
        Value::String(value.to_string())
    }
}

impl From<&[u8]> for Value {
    fn from(value: &[u8]) -> Value {
        Value::Bytes(value.to_vec())
    }
}

impl<T: Into<Value> + HasKind> From<Option<T>> for Value {
    fn from(value: Option<T>) -> Value {
        match value {
            Some(value) => value.into(),
            None => Value::Null(T::KIND),
        }
    }
}

/// Why a [`Value`] is not the Rust type asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValueError {
    expected: &'static str,
    found: String,
}

impl ValueError {
    /// A value that cannot be read as `expected`.
    pub fn new(expected: &'static str, found: &Value) -> ValueError {
        let found = match found {
            Value::Null(_) => "NULL".to_owned(),
            other => format!("{} {}", other.kind(), abbreviated(&other.to_sql_literal())),
        };
        ValueError { expected, found }
    }

    /// The Rust type that was asked for.
    pub fn expected(&self) -> &'static str {
        self.expected
    }
}

impl fmt::Display for ValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "expected {}, found {}", self.expected, self.found)
    }
}

impl std::error::Error for ValueError {}

/// `text` cut to 64 characters (values in messages).
fn abbreviated(text: &str) -> String {
    const MAX: usize = 64;
    if text.chars().count() <= MAX {
        return text.to_owned();
    }
    let cut: String = text.chars().take(MAX).collect();
    format!("{cut}...")
}

/// A Rust type read from a [`Value`] (a row's column, through
/// [`Row::get`](crate::Row::get)).
///
/// Reading is exact first, then tolerant where databases store the same
/// thing differently: any integer width that fits, integers from exact
/// decimals, booleans from `0`/`1` (MySQL and SQLite), UUIDs from text or
/// 16 bytes, date-times, decimals and JSON from text (SQLite, MySQL
/// expressions). `NULL` reads only as an `Option`.
pub trait FromValue: Sized {
    /// Reads `value` as this type.
    fn from_value(value: Value) -> Result<Self, ValueError>;
}

impl FromValue for Value {
    fn from_value(value: Value) -> Result<Self, ValueError> {
        Ok(value)
    }
}

impl<T: FromValue> FromValue for Option<T> {
    fn from_value(value: Value) -> Result<Self, ValueError> {
        match value {
            Value::Null(_) => Ok(None),
            value => T::from_value(value).map(Some),
        }
    }
}

/// The value as an `i128`, when it is an integer (or a whole decimal or
/// float, or integer text).
fn integer(value: &Value) -> Option<i128> {
    match value {
        Value::Bool(b) => Some(i128::from(*b)),
        Value::I8(n) => Some(i128::from(*n)),
        Value::I16(n) => Some(i128::from(*n)),
        Value::I32(n) => Some(i128::from(*n)),
        Value::I64(n) => Some(i128::from(*n)),
        Value::U8(n) => Some(i128::from(*n)),
        Value::U16(n) => Some(i128::from(*n)),
        Value::U32(n) => Some(i128::from(*n)),
        Value::U64(n) => Some(i128::from(*n)),
        Value::Decimal(d) if d.fract().is_zero() => d.to_i128(),
        Value::F64(f) if f.fract() == 0.0 && f.is_finite() => f.to_i128(),
        Value::F32(f) if f.fract() == 0.0 && f.is_finite() => f.to_i128(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

macro_rules! integer_from_value {
    ($($ty:ty),*) => {
        $(
            impl FromValue for $ty {
                fn from_value(value: Value) -> Result<Self, ValueError> {
                    integer(&value)
                        .and_then(|n| <$ty>::try_from(n).ok())
                        .ok_or_else(|| ValueError::new(stringify!($ty), &value))
                }
            }
        )*
    };
}

integer_from_value!(i8, i16, i32, i64, u8, u16, u32, u64);

impl FromValue for bool {
    fn from_value(value: Value) -> Result<Self, ValueError> {
        match &value {
            Value::Bool(b) => Ok(*b),
            Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
                "t" | "true" | "1" => Ok(true),
                "f" | "false" | "0" => Ok(false),
                _ => Err(ValueError::new("bool", &value)),
            },
            other => match integer(other) {
                Some(0) => Ok(false),
                Some(1) => Ok(true),
                _ => Err(ValueError::new("bool", &value)),
            },
        }
    }
}

impl FromValue for f64 {
    fn from_value(value: Value) -> Result<Self, ValueError> {
        match &value {
            Value::F64(f) => Ok(*f),
            Value::F32(f) => Ok(f64::from(*f)),
            Value::Decimal(d) => d.to_f64().ok_or_else(|| ValueError::new("f64", &value)),
            Value::String(s) => s.trim().parse().map_err(|_| ValueError::new("f64", &value)),
            other => integer(other)
                .and_then(|n| n.to_f64())
                .ok_or_else(|| ValueError::new("f64", &value)),
        }
    }
}

impl FromValue for f32 {
    fn from_value(value: Value) -> Result<Self, ValueError> {
        match &value {
            Value::F32(f) => Ok(*f),
            // Narrowing is what reading a `REAL` column as `f32` asks for.
            #[allow(clippy::cast_possible_truncation)]
            Value::F64(f) => Ok(*f as f32),
            _ => f64::from_value(value.clone())
                .ok()
                .and_then(|f| f.to_f32())
                .ok_or_else(|| ValueError::new("f32", &value)),
        }
    }
}

impl FromValue for Decimal {
    fn from_value(value: Value) -> Result<Self, ValueError> {
        match &value {
            Value::Decimal(d) => Ok(*d),
            Value::F64(f) => {
                Decimal::from_f64(*f).ok_or_else(|| ValueError::new("Decimal", &value))
            }
            Value::F32(f) => {
                Decimal::from_f32(*f).ok_or_else(|| ValueError::new("Decimal", &value))
            }
            Value::String(s) => {
                let text = s.trim();
                text.parse::<Decimal>()
                    .or_else(|_| Decimal::from_scientific(text))
                    .map_err(|_| ValueError::new("Decimal", &value))
            }
            other => integer(other)
                .and_then(Decimal::from_i128)
                .ok_or_else(|| ValueError::new("Decimal", &value)),
        }
    }
}

impl FromValue for String {
    fn from_value(value: Value) -> Result<Self, ValueError> {
        match value {
            Value::String(s) => Ok(s),
            Value::Json(serde_json::Value::String(s)) => Ok(s),
            Value::Bytes(bytes) => String::from_utf8(bytes)
                .map_err(|error| ValueError::new("String", &Value::Bytes(error.into_bytes()))),
            Value::Uuid(id) => Ok(id.to_string()),
            Value::Enum { value, .. } => Ok(value),
            Value::Null(kind) => Err(ValueError::new("String", &Value::Null(kind))),
            Value::Json(json) => Ok(json.to_string()),
            other => match other.to_json() {
                serde_json::Value::String(text) => Ok(text),
                json => Ok(json.to_string()),
            },
        }
    }
}

impl FromValue for Vec<u8> {
    fn from_value(value: Value) -> Result<Self, ValueError> {
        match value {
            Value::Bytes(bytes) => Ok(bytes),
            Value::String(s) => Ok(s.into_bytes()),
            Value::Uuid(id) => Ok(id.as_bytes().to_vec()),
            other => Err(ValueError::new("Vec<u8>", &other)),
        }
    }
}

impl FromValue for serde_json::Value {
    fn from_value(value: Value) -> Result<Self, ValueError> {
        match value {
            Value::Json(json) => Ok(json),
            Value::String(s) => {
                serde_json::from_str(&s).map_err(|_| ValueError::new("JSON", &Value::String(s)))
            }
            Value::Bytes(bytes) => serde_json::from_slice(&bytes)
                .map_err(|_| ValueError::new("JSON", &Value::Bytes(bytes))),
            Value::Null(kind) => Err(ValueError::new("JSON", &Value::Null(kind))),
            other => Ok(other.to_json()),
        }
    }
}

impl FromValue for Uuid {
    fn from_value(value: Value) -> Result<Self, ValueError> {
        match &value {
            Value::Uuid(id) => Ok(*id),
            Value::String(s) => {
                Uuid::parse_str(s.trim()).map_err(|_| ValueError::new("Uuid", &value))
            }
            Value::Bytes(bytes) => {
                Uuid::from_slice(bytes).map_err(|_| ValueError::new("Uuid", &value))
            }
            _ => Err(ValueError::new("Uuid", &value)),
        }
    }
}

/// A vector from a [`Value::Vector`], JSON or text holding an array of
/// numbers (`[1,2,3]`), or little-endian `f32` bytes (MySQL's and SQLite's
/// form).
impl FromValue for Vec<f32> {
    fn from_value(value: Value) -> Result<Self, ValueError> {
        fn from_json(json: &serde_json::Value) -> Option<Vec<f32>> {
            json.as_array()?
                .iter()
                .map(|item| {
                    // A vector's components are 32-bit floats.
                    #[allow(clippy::cast_possible_truncation)]
                    item.as_f64().map(|f| f as f32)
                })
                .collect()
        }
        match &value {
            Value::Vector(items) => Ok(items.clone()),
            Value::Json(json) => from_json(json).ok_or_else(|| ValueError::new("Vec<f32>", &value)),
            Value::String(text) => serde_json::from_str::<serde_json::Value>(text)
                .ok()
                .as_ref()
                .and_then(from_json)
                .ok_or_else(|| ValueError::new("Vec<f32>", &value)),
            Value::Bytes(bytes) if bytes.len() % 4 == 0 => Ok(bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|chunk| f32::from_le_bytes(*chunk))
                .collect()),
            _ => Err(ValueError::new("Vec<f32>", &value)),
        }
    }
}

/// Date-time text as SQLite and MySQL expressions write it: RFC 3339
/// (`2026-10-28T09:30:00Z`, `+02:00`), or `YYYY-MM-DD HH:MM:SS[.f]` with an
/// optional offset.
fn parse_date_time(text: &str) -> Option<DateTime<FixedOffset>> {
    let text = text.trim();
    if let Ok(at) = DateTime::parse_from_rfc3339(text) {
        return Some(at);
    }
    for format in ["%Y-%m-%d %H:%M:%S%.f%:z", "%Y-%m-%d %H:%M:%S%.f%#z"] {
        if let Ok(at) = DateTime::parse_from_str(text, format) {
            return Some(at);
        }
    }
    parse_naive_date_time(text).map(|naive| naive.and_utc().fixed_offset())
}

/// Text holding a date and time without an offset.
fn parse_naive_date_time(text: &str) -> Option<NaiveDateTime> {
    let text = text.trim();
    for format in [
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M",
    ] {
        if let Ok(at) = NaiveDateTime::parse_from_str(text, format) {
            return Some(at);
        }
    }
    NaiveDate::parse_from_str(text, "%Y-%m-%d")
        .ok()
        .and_then(|date| date.and_hms_opt(0, 0, 0))
}

impl FromValue for DateTime<Utc> {
    fn from_value(value: Value) -> Result<Self, ValueError> {
        match &value {
            Value::Timestamp(at) => Ok(*at),
            Value::TimestampTz(at) => Ok(at.with_timezone(&Utc)),
            // A date-time without a zone is stored in UTC.
            Value::DateTime(at) => Ok(at.and_utc()),
            Value::String(s) => parse_date_time(s)
                .map(|at| at.with_timezone(&Utc))
                .ok_or_else(|| ValueError::new("DateTime<Utc>", &value)),
            _ => Err(ValueError::new("DateTime<Utc>", &value)),
        }
    }
}

impl FromValue for DateTime<FixedOffset> {
    fn from_value(value: Value) -> Result<Self, ValueError> {
        match &value {
            Value::TimestampTz(at) => Ok(*at),
            Value::Timestamp(at) => Ok(at.fixed_offset()),
            Value::DateTime(at) => Ok(at.and_utc().fixed_offset()),
            Value::String(s) => {
                parse_date_time(s).ok_or_else(|| ValueError::new("DateTime<FixedOffset>", &value))
            }
            _ => Err(ValueError::new("DateTime<FixedOffset>", &value)),
        }
    }
}

impl FromValue for NaiveDateTime {
    fn from_value(value: Value) -> Result<Self, ValueError> {
        match &value {
            Value::DateTime(at) => Ok(*at),
            Value::Timestamp(at) => Ok(at.naive_utc()),
            Value::TimestampTz(at) => Ok(at.naive_utc()),
            Value::String(s) => parse_naive_date_time(s)
                .or_else(|| parse_date_time(s).map(|at| at.naive_utc()))
                .ok_or_else(|| ValueError::new("NaiveDateTime", &value)),
            _ => Err(ValueError::new("NaiveDateTime", &value)),
        }
    }
}

impl FromValue for NaiveDate {
    fn from_value(value: Value) -> Result<Self, ValueError> {
        match &value {
            Value::Date(date) => Ok(*date),
            Value::DateTime(at) => Ok(at.date()),
            Value::Timestamp(at) => Ok(at.date_naive()),
            Value::String(s) => NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d")
                .ok()
                .or_else(|| parse_naive_date_time(s).map(|at| at.date()))
                .ok_or_else(|| ValueError::new("NaiveDate", &value)),
            _ => Err(ValueError::new("NaiveDate", &value)),
        }
    }
}

impl FromValue for NaiveTime {
    fn from_value(value: Value) -> Result<Self, ValueError> {
        match &value {
            Value::Time(time) => Ok(*time),
            Value::DateTime(at) => Ok(at.time()),
            Value::String(s) => {
                let text = s.trim();
                ["%H:%M:%S%.f", "%H:%M"]
                    .iter()
                    .find_map(|format| NaiveTime::parse_from_str(text, format).ok())
                    .ok_or_else(|| ValueError::new("NaiveTime", &value))
            }
            _ => Err(ValueError::new("NaiveTime", &value)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nones_keep_their_kind() {
        assert_eq!(Value::from(None::<String>), Value::Null(Kind::String));
        assert_eq!(Value::from(None::<Uuid>), Value::Null(Kind::Uuid));
        assert_eq!(Value::from(Some(3_i16)), Value::I16(3));
        assert_eq!(Value::from("a"), Value::String("a".into()));
        assert!(Value::Null(Kind::Json).is_null());
        assert_eq!(Value::Null(Kind::Date).kind(), Kind::Date);
    }

    #[test]
    fn integers_read_across_widths_when_they_fit() {
        assert_eq!(i64::from_value(Value::I16(-4)).unwrap(), -4);
        assert_eq!(u8::from_value(Value::I64(255)).unwrap(), 255);
        assert!(u8::from_value(Value::I64(256)).is_err());
        assert!(u32::from_value(Value::I32(-1)).is_err());
        assert_eq!(
            i64::from_value(Value::Decimal(Decimal::new(1200, 2))).unwrap(),
            12
        );
        assert!(i64::from_value(Value::Decimal(Decimal::new(1250, 2))).is_err());
        assert_eq!(i32::from_value(Value::String(" 17 ".into())).unwrap(), 17);
        let error = i64::from_value(Value::Null(Kind::I64)).unwrap_err();
        assert_eq!(error.to_string(), "expected i64, found NULL");
        assert_eq!(
            Option::<i64>::from_value(Value::Null(Kind::I64)).unwrap(),
            None
        );
    }

    #[test]
    fn booleans_read_from_integers_and_text() {
        assert!(bool::from_value(Value::I64(1)).unwrap());
        assert!(!bool::from_value(Value::I8(0)).unwrap());
        assert!(bool::from_value(Value::I64(2)).is_err());
        assert!(bool::from_value(Value::String("t".into())).unwrap());
    }

    #[test]
    fn text_forms_read_as_typed_values() {
        let id = Uuid::now_v7();
        assert_eq!(
            Uuid::from_value(Value::Bytes(id.as_bytes().to_vec())).unwrap(),
            id
        );
        assert_eq!(
            DateTime::<Utc>::from_value(Value::String("2026-10-28 09:30:00.5+00:00".into()))
                .unwrap()
                .to_rfc3339(),
            "2026-10-28T09:30:00.500+00:00"
        );
        assert_eq!(
            DateTime::<Utc>::from_value(Value::String("2026-10-28T11:30:00+02:00".into()))
                .unwrap()
                .to_rfc3339(),
            "2026-10-28T09:30:00+00:00"
        );
        assert_eq!(
            NaiveDateTime::from_value(Value::String("2026-10-28 09:30:00".into()))
                .unwrap()
                .to_string(),
            "2026-10-28 09:30:00"
        );
        assert_eq!(
            Decimal::from_value(Value::String("12.50".into())).unwrap(),
            Decimal::new(1250, 2)
        );
        assert_eq!(
            serde_json::Value::from_value(Value::String("{\"a\":1}".into())).unwrap(),
            serde_json::json!({"a": 1})
        );
        assert_eq!(
            NaiveTime::from_value(Value::String("09:30:00".into()))
                .unwrap()
                .to_string(),
            "09:30:00"
        );
    }

    #[test]
    fn json_and_literals() {
        assert_eq!(
            Value::Decimal(Decimal::new(1250, 2)).to_json(),
            serde_json::json!("12.5")
        );
        assert_eq!(Value::U64(u64::MAX).to_json(), serde_json::json!(u64::MAX));
        assert_eq!(Value::String("it's".into()).to_sql_literal(), "'it''s'");
        assert_eq!(Value::Bytes(vec![0, 255]).to_sql_literal(), "X'00ff'");
        assert_eq!(Value::Null(Kind::I64).to_string(), "NULL");
        let mood = Value::enumeration("mood", "happy");
        assert_eq!(mood.kind(), Kind::Enum);
        assert_eq!(mood.to_json(), serde_json::json!("happy"));
        assert_eq!(String::from_value(mood).unwrap(), "happy");
        let vector = Value::from(vec![0.5_f32, -2.0]);
        assert_eq!(vector.to_json(), serde_json::json!([0.5, -2.0]));
        assert_eq!(vector.to_sql_literal(), "'[0.5,-2]'");
        assert_eq!(Vec::<f32>::from_value(vector).unwrap(), [0.5, -2.0]);
        assert_eq!(
            Vec::<f32>::from_value(Value::String("[1, 2.5]".into())).unwrap(),
            [1.0, 2.5]
        );
        assert_eq!(
            Vec::<f32>::from_value(Value::Bytes(1.5_f32.to_le_bytes().to_vec())).unwrap(),
            [1.5]
        );
        assert_eq!(Value::from(None::<Vec<f32>>), Value::Null(Kind::Vector));
        let at = DateTime::parse_from_rfc3339("2026-10-28T09:30:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(
            Value::Timestamp(at).to_json(),
            serde_json::json!("2026-10-28T09:30:00Z")
        );
    }
}
