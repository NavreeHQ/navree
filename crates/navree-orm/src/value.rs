//! [`Value`], [`Field`] and the conversions between Rust types and column
//! values.
//!
//! [`Value`] is navree-db's value: what statements bind and rows hold, a
//! NULL keeping its [`Kind`] (PostgreSQL needs the type of a NULL
//! parameter). The enum is non-exhaustive, so code matching on it keeps a
//! `_` arm.
//!
//! [`Field`] is implemented by every Rust type a model field can have
//! without a cast. Reading a column takes the value navree-db's row reads
//! by the column's own type ([`navree_db::Row::value`]), then converts it
//! leniently between representations of the same thing: `NUMERIC` 42 reads
//! as an `i64`, the text `2026-10-27 08:00:00+00:00` (SQLite's date-times)
//! as a `DateTime<Utc>`, MySQL's `TINYINT(1)` as a `bool`. Document rows,
//! aggregates and dynamic columns convert the same way.

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use uuid::Uuid;

pub use navree_db::{Kind, Value};
pub use rust_decimal::Decimal;

/// Why a column value could not become a field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldError {
    /// The value is NULL and the field is not an `Option`.
    Null,
    /// The row has no such column.
    Missing,
    /// The value has another type, or does not fit the field.
    Invalid(String),
}

impl fmt::Display for FieldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FieldError::Null => f.write_str("the value is NULL (make the field an Option)"),
            FieldError::Missing => f.write_str("the row has no such column"),
            FieldError::Invalid(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for FieldError {}

/// What a field's values are, as JSON and OpenAPI see them: the shape
/// serialized records write and schemas describe. A nullable field has the
/// kind of its inner type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FieldKind {
    /// `true` / `false`.
    Bool,
    /// An integer of `bits` bits.
    Integer {
        /// 8, 16, 32 or 64.
        bits: u8,
        /// Whether it can be negative.
        signed: bool,
    },
    /// A floating-point number of `bits` bits (32 or 64).
    Float {
        /// 32 or 64.
        bits: u8,
    },
    /// An exact decimal, written as a string (`"12.50"`).
    Decimal,
    /// Text.
    Text,
    /// Bytes, written as an array of numbers.
    Bytes,
    /// A UUID, written as its hyphenated text.
    Uuid,
    /// A ULID, written as its 26-character text.
    Ulid,
    /// A date, `YYYY-MM-DD`.
    Date,
    /// A time of day, `HH:MM:SS`.
    Time,
    /// A date-time, written in RFC 3339.
    DateTime,
    /// Any JSON value.
    Json,
    /// A URL, written as text.
    Url,
    /// A vector of numbers.
    Vector,
    /// Anything else (no known shape).
    Any,
}

/// A Rust type a model field (or a column read with
/// [`Row::get`](crate::Row::get)) can have.
///
/// Implemented for `bool`, `i8`..`i64`, `u8`..`u64`, `f32`, `f64`,
/// [`Decimal`], `String`, `Vec<u8>`, `Uuid`, [`Ulid`](crate::Ulid),
/// `chrono`'s `NaiveDate`, `NaiveTime`, `NaiveDateTime`, `DateTime<Utc>`
/// and `DateTime<FixedOffset>`, `serde_json::Value`, [`Value`] (any
/// column, decoded dynamically) and `Option<T>` of any of them. Other
/// types (enums, serde structs) use a [cast](crate::cast).
///
/// ```
/// use navree_orm::{Field, Kind, Value};
///
/// assert_eq!(42_i64.to_value(), Value::I64(42));
/// assert_eq!(<Option<i64>>::null(), Value::Null(Kind::I64));
/// // Lenient between representations of the same number:
/// assert_eq!(i64::from_value(Value::F64(42.0)).unwrap(), 42);
/// assert!(i64::from_value(Value::F64(4.2)).is_err());
/// assert_eq!(<Option<i64>>::from_value(Value::Null(Kind::I64)).unwrap(), None);
/// ```
pub trait Field: Sized + Send + Sync + 'static {
    /// What the values are as JSON (for serialization and OpenAPI
    /// schemas); [`FieldKind::Any`] unless the type says.
    const KIND: FieldKind = FieldKind::Any;

    /// The column value of `self`.
    fn to_value(&self) -> Value;

    /// A NULL of this field's type.
    fn null() -> Value;

    /// Reads the field from column `column` of a row: by default the value
    /// navree-db reads by the column's type ([`navree_db::Row::value`]),
    /// converted with [`from_value`](Field::from_value). A missing column
    /// is [`FieldError::Missing`].
    fn from_sql(row: &navree_db::Row, column: &str) -> Result<Self, FieldError> {
        Self::from_value(column_value(row, column)?)
    }

    /// Converts a value: a column of a row, a document field, an aggregate,
    /// an assignment.
    fn from_value(value: Value) -> Result<Self, FieldError>;

    /// Converts untyped JSON input (mass assignment with `fill`, a request
    /// body) as leniently as [`from_value`](Field::from_value): numbers and
    /// numeric strings for numbers, RFC 3339 and database text for
    /// date-times, the text form of UUIDs. `null` is `None` for an
    /// `Option`, an error otherwise. JSON fields take the JSON as it is,
    /// decimals read a number's exact text, bytes an array of numbers.
    ///
    /// ```
    /// use navree_orm::{Decimal, Field};
    /// use serde_json::json;
    ///
    /// assert_eq!(i32::from_json(json!(7)).unwrap(), 7);
    /// assert_eq!(Decimal::from_json(json!(12.10)).unwrap().to_string(), "12.1");
    /// assert_eq!(Decimal::from_json(json!("12.10")).unwrap().to_string(), "12.10");
    /// assert_eq!(<Option<String>>::from_json(json!(null)).unwrap(), None);
    /// assert_eq!(serde_json::Value::from_json(json!("text")).unwrap(), json!("text"));
    /// assert!(bool::from_json(json!("maybe")).is_err());
    /// ```
    fn from_json(json: serde_json::Value) -> Result<Self, FieldError> {
        Self::from_value(json_to_value(json))
    }

    /// The type's name in error messages, e.g. `i64` or `Option<String>`.
    fn type_name() -> Cow<'static, str>;
}

/// Column `column` of `row` as navree-db reads it by the column's type:
/// [`FieldError::Missing`] when the row has no such column, the driver's
/// message when the column cannot be read.
pub(crate) fn column_value(row: &navree_db::Row, column: &str) -> Result<Value, FieldError> {
    let Some(index) = row.column_index(column) else {
        return Err(FieldError::Missing);
    };
    row.value(index)
        .map_err(|error| FieldError::Invalid(error.to_string()))
}

/// JSON input as the [`Value`] it most naturally is: `null` a NULL,
/// booleans and strings themselves, integers `BIGINT` (or `BIGINT
/// UNSIGNED` above `i64`), other numbers `DOUBLE`, arrays and objects JSON.
pub(crate) fn json_to_value(json: serde_json::Value) -> Value {
    use serde_json::Value as Json;

    match json {
        Json::Null => Value::Null(Kind::Json),
        Json::Bool(b) => Value::Bool(b),
        Json::Number(n) => match (n.as_i64(), n.as_u64(), n.as_f64()) {
            (Some(i), _, _) => Value::I64(i),
            (None, Some(u), _) => Value::U64(u),
            (None, None, Some(f)) => Value::F64(f),
            (None, None, None) => Value::String(n.to_string()),
        },
        Json::String(s) => Value::String(s),
        other => Value::Json(other),
    }
}

/// Whether `value` is a NULL (of any kind).
///
/// ```
/// use navree_orm::{Kind, Value, is_null};
///
/// assert!(is_null(&Value::Null(Kind::Uuid)));
/// assert!(!is_null(&Value::Bool(false)));
/// ```
pub fn is_null(value: &Value) -> bool {
    value.is_null()
}

/// `value` as JSON, the way the field would serialize: numbers as numbers,
/// decimals as exact strings (`"12.50"`), UUIDs and date-times as their
/// serde text (RFC 3339), JSON as itself, bytes and vectors as arrays of
/// numbers, an enum value as its text, NULL as `null`. Non-finite floats
/// become `null`.
///
/// ```
/// use navree_orm::{Decimal, Kind, Value, to_json};
///
/// assert_eq!(to_json(&Value::from(42_i64)), serde_json::json!(42));
/// assert_eq!(to_json(&Value::Decimal(Decimal::new(1250, 2))), serde_json::json!("12.50"));
/// assert_eq!(to_json(&Value::Null(Kind::String)), serde_json::Value::Null);
/// ```
pub fn to_json(value: &Value) -> serde_json::Value {
    use serde_json::Value as Json;

    fn serde<T: serde::Serialize>(value: &T) -> Json {
        serde_json::to_value(value).unwrap_or(Json::Null)
    }
    fn float(value: f64) -> Json {
        serde_json::Number::from_f64(value).map_or(Json::Null, Json::Number)
    }

    match value {
        Value::Null(_) => Json::Null,
        Value::Bool(v) => Json::Bool(*v),
        Value::I8(v) => Json::from(*v),
        Value::I16(v) => Json::from(*v),
        Value::I32(v) => Json::from(*v),
        Value::I64(v) => Json::from(*v),
        Value::U8(v) => Json::from(*v),
        Value::U16(v) => Json::from(*v),
        Value::U32(v) => Json::from(*v),
        Value::U64(v) => Json::from(*v),
        Value::F32(v) => float(f64::from(*v)),
        Value::F64(v) => float(*v),
        Value::String(v) => Json::String(v.clone()),
        Value::Bytes(v) => serde(v),
        Value::Json(v) => v.clone(),
        Value::Date(v) => serde(v),
        Value::Time(v) => serde(v),
        Value::DateTime(v) => serde(v),
        Value::Timestamp(v) => serde(v),
        Value::TimestampTz(v) => serde(v),
        Value::Uuid(v) => Json::String(v.to_string()),
        Value::Decimal(v) => Json::String(v.to_string()),
        // Enums (their text), vectors (arrays of numbers) and what later
        // versions add.
        other => other.to_json(),
    }
}

/// Whether two stored values of a column hold the same thing (what dirty
/// tracking asks): NULLs are equal whatever their kind, floats within a
/// small relative tolerance, decimals numerically (`12.5` and `12.50`),
/// date-times by instant, JSON structurally, anything else exactly.
///
/// ```
/// use navree_orm::{Decimal, Kind, Value, same_value};
///
/// assert!(same_value(&Value::F64(0.1 + 0.2), &Value::F64(0.3)));
/// assert!(same_value(
///     &Value::Decimal(Decimal::new(125, 1)),
///     &Value::Decimal(Decimal::new(1250, 2)),
/// ));
/// assert!(same_value(&Value::Null(Kind::String), &Value::Null(Kind::I64)));
/// assert!(!same_value(&Value::from("a"), &Value::from("b")));
/// ```
pub fn same_value(a: &Value, b: &Value) -> bool {
    match (is_null(a), is_null(b)) {
        (true, true) => return true,
        (true, false) | (false, true) => return false,
        (false, false) => {}
    }
    match (a, b) {
        (Value::F32(_) | Value::F64(_), Value::F32(_) | Value::F64(_)) => {
            match (float(a), float(b)) {
                (Ok(x), Ok(y)) => {
                    x == y || (x - y).abs() <= f64::EPSILON * 4.0 * x.abs().max(y.abs()).max(1.0)
                }
                _ => false,
            }
        }
        (Value::Decimal(x), Value::Decimal(y)) => x == y,
        (
            Value::Timestamp(_) | Value::TimestampTz(_),
            Value::Timestamp(_) | Value::TimestampTz(_),
        ) => match (date_time_utc(a.clone()), date_time_utc(b.clone())) {
            (Ok(x), Ok(y)) => x == y,
            _ => false,
        },
        _ => a == b,
    }
}

/// A short description of a value's type for error messages.
pub(crate) fn describe(value: &Value) -> &'static str {
    match value {
        Value::Null(_) => "NULL",
        Value::Bool(_) => "a boolean",
        Value::I8(_)
        | Value::I16(_)
        | Value::I32(_)
        | Value::I64(_)
        | Value::U8(_)
        | Value::U16(_)
        | Value::U32(_)
        | Value::U64(_) => "an integer",
        Value::F32(_) | Value::F64(_) => "a float",
        Value::Decimal(_) => "a decimal",
        Value::String(_) => "a string",
        Value::Bytes(_) => "bytes",
        Value::Json(_) => "JSON",
        Value::Date(_) => "a date",
        Value::Time(_) => "a time",
        Value::DateTime(_) | Value::Timestamp(_) | Value::TimestampTz(_) => "a date-time",
        Value::Uuid(_) => "a UUID",
        Value::Enum { .. } => "an enum value",
        Value::Vector(_) => "a vector",
        _ => "a value of another type",
    }
}

fn mismatch(value: &Value, wanted: &str) -> FieldError {
    FieldError::Invalid(format!("expected {wanted}, found {}", describe(value)))
}

fn null_check(value: &Value) -> Result<(), FieldError> {
    if is_null(value) {
        Err(FieldError::Null)
    } else {
        Ok(())
    }
}

/// An integer in `value`, from any integer, an integral float or decimal,
/// a boolean or a numeric string.
fn integer(value: &Value) -> Result<i128, FieldError> {
    null_check(value)?;
    let wanted = "an integer";
    match value {
        Value::Bool(b) => Ok(i128::from(*b)),
        Value::I8(v) => Ok(i128::from(*v)),
        Value::I16(v) => Ok(i128::from(*v)),
        Value::I32(v) => Ok(i128::from(*v)),
        Value::I64(v) => Ok(i128::from(*v)),
        Value::U8(v) => Ok(i128::from(*v)),
        Value::U16(v) => Ok(i128::from(*v)),
        Value::U32(v) => Ok(i128::from(*v)),
        Value::U64(v) => Ok(i128::from(*v)),
        Value::F32(v) => integral_float(f64::from(*v)),
        Value::F64(v) => integral_float(*v),
        Value::Decimal(v) if v.fract().is_zero() => v
            .to_i128()
            .ok_or_else(|| FieldError::Invalid(format!("{v} is out of range"))),
        Value::Decimal(v) => Err(FieldError::Invalid(format!("{v} is not a whole number"))),
        Value::String(s) => s
            .trim()
            .parse::<i128>()
            .map_err(|_| FieldError::Invalid(format!("`{s}` is not an integer"))),
        Value::Json(json) => json
            .as_i64()
            .map(i128::from)
            .ok_or_else(|| mismatch(value, wanted)),
        other => Err(mismatch(other, wanted)),
    }
}

fn integral_float(v: f64) -> Result<i128, FieldError> {
    if v.is_finite() && v.fract() == 0.0 && v.abs() < 1.7e38 {
        // In range and integral: the cast is exact.
        #[allow(clippy::cast_possible_truncation)]
        Ok(v as i128)
    } else {
        Err(FieldError::Invalid(format!("{v} is not a whole number")))
    }
}

/// A float in `value`, from any number or a numeric string.
fn float(value: &Value) -> Result<f64, FieldError> {
    null_check(value)?;
    match value {
        Value::F32(v) => Ok(f64::from(*v)),
        Value::F64(v) => Ok(*v),
        Value::Decimal(v) => v
            .to_f64()
            .ok_or_else(|| FieldError::Invalid(format!("{v} does not fit a float"))),
        Value::String(s) => s
            .trim()
            .parse::<f64>()
            .map_err(|_| FieldError::Invalid(format!("`{s}` is not a number"))),
        Value::Json(json) => json.as_f64().ok_or_else(|| mismatch(value, "a number")),
        other => {
            let int = integer(other).map_err(|_| mismatch(other, "a number"))?;
            // Integers from the database fit an f64 closely enough for a float field.
            #[allow(clippy::cast_precision_loss)]
            Ok(int as f64)
        }
    }
}

/// A decimal in `value`, from any number or a numeric string.
fn decimal(value: &Value) -> Result<Decimal, FieldError> {
    null_check(value)?;
    match value {
        Value::Decimal(v) => Ok(*v),
        Value::F32(v) => Decimal::from_f32(*v)
            .ok_or_else(|| FieldError::Invalid(format!("{v} does not fit a decimal"))),
        Value::F64(v) => Decimal::from_f64(*v)
            .ok_or_else(|| FieldError::Invalid(format!("{v} does not fit a decimal"))),
        Value::String(s) => Decimal::from_str(s.trim())
            .or_else(|_| Decimal::from_scientific(s.trim()))
            .map_err(|_| FieldError::Invalid(format!("`{s}` is not a decimal"))),
        other => {
            let int = integer(other).map_err(|_| mismatch(other, "a decimal"))?;
            Decimal::from_i128(int)
                .ok_or_else(|| FieldError::Invalid(format!("{int} does not fit a decimal")))
        }
    }
}

fn boolean(value: &Value) -> Result<bool, FieldError> {
    null_check(value)?;
    match value {
        Value::Bool(b) => Ok(*b),
        Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
            "true" | "t" | "1" | "yes" | "y" | "on" => Ok(true),
            "false" | "f" | "0" | "no" | "n" | "off" => Ok(false),
            _ => Err(FieldError::Invalid(format!("`{s}` is not a boolean"))),
        },
        Value::Json(json) => json.as_bool().ok_or_else(|| mismatch(value, "a boolean")),
        other => match integer(other) {
            Ok(0) => Ok(false),
            Ok(1) => Ok(true),
            Ok(n) => Err(FieldError::Invalid(format!(
                "{n} is not a boolean (0 or 1)"
            ))),
            Err(_) => Err(mismatch(other, "a boolean")),
        },
    }
}

fn string(value: Value) -> Result<String, FieldError> {
    null_check(&value)?;
    match value {
        Value::String(s) => Ok(s),
        Value::Uuid(u) => Ok(u.to_string()),
        Value::Enum { value, .. } => Ok(value),
        Value::Json(json) => match json {
            serde_json::Value::String(s) => Ok(s),
            other => Ok(other.to_string()),
        },
        Value::Bytes(bytes) => String::from_utf8(bytes)
            .map_err(|_| FieldError::Invalid("the bytes are not UTF-8 text".to_owned())),
        Value::Decimal(d) => Ok(d.to_string()),
        other => Err(mismatch(&other, "a string")),
    }
}

fn bytes(value: Value) -> Result<Vec<u8>, FieldError> {
    null_check(&value)?;
    match value {
        Value::Bytes(b) => Ok(b),
        Value::String(s) => Ok(s.into_bytes()),
        Value::Uuid(u) => Ok(u.as_bytes().to_vec()),
        other => Err(mismatch(&other, "bytes")),
    }
}

fn uuid(value: Value) -> Result<Uuid, FieldError> {
    null_check(&value)?;
    match value {
        Value::Uuid(u) => Ok(u),
        Value::String(s) => Uuid::parse_str(s.trim())
            .map_err(|_| FieldError::Invalid(format!("`{s}` is not a UUID"))),
        Value::Bytes(b) if b.len() == 16 => Uuid::from_slice(&b)
            .map_err(|_| FieldError::Invalid(format!("{} bytes are not a UUID", b.len()))),
        Value::Bytes(b) => std::str::from_utf8(&b)
            .ok()
            .and_then(|text| Uuid::parse_str(text.trim()).ok())
            .ok_or_else(|| FieldError::Invalid(format!("{} bytes are not a UUID", b.len()))),
        Value::Json(json) => match json.as_str() {
            Some(s) => {
                Uuid::parse_str(s).map_err(|_| FieldError::Invalid(format!("`{s}` is not a UUID")))
            }
            None => Err(FieldError::Invalid(
                "JSON that is not a UUID string".to_owned(),
            )),
        },
        other => Err(mismatch(&other, "a UUID")),
    }
}

fn json(value: Value) -> Result<serde_json::Value, FieldError> {
    null_check(&value)?;
    match value {
        Value::Json(json) => Ok(json),
        Value::String(s) => serde_json::from_str(&s)
            .map_err(|error| FieldError::Invalid(format!("the text is not JSON: {error}"))),
        Value::Bytes(b) => serde_json::from_slice(&b)
            .map_err(|error| FieldError::Invalid(format!("the bytes are not JSON: {error}"))),
        other => Ok(to_json(&other)),
    }
}

/// The text formats date-times come back in: RFC 3339, and the
/// space-separated form SQLite and MySQL use, with or without an offset.
const DATE_TIME_FORMATS: [&str; 2] = ["%Y-%m-%d %H:%M:%S%.f", "%Y-%m-%dT%H:%M:%S%.f"];

fn parse_offset_date_time(s: &str) -> Option<DateTime<FixedOffset>> {
    let s = s.trim();
    DateTime::parse_from_rfc3339(s)
        .ok()
        .or_else(|| DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f%:z").ok())
        .or_else(|| DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f%z").ok())
        .or_else(|| DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f %:z").ok())
}

fn parse_naive_date_time(s: &str) -> Option<NaiveDateTime> {
    let s = s.trim();
    DATE_TIME_FORMATS
        .iter()
        .find_map(|format| NaiveDateTime::parse_from_str(s, format).ok())
        .or_else(|| {
            NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .ok()
                .and_then(|date| date.and_hms_opt(0, 0, 0))
        })
}

fn date_time_utc(value: Value) -> Result<DateTime<Utc>, FieldError> {
    null_check(&value)?;
    match value {
        Value::Timestamp(v) => Ok(v),
        Value::TimestampTz(v) => Ok(v.with_timezone(&Utc)),
        Value::DateTime(v) => Ok(Utc.from_utc_datetime(&v)),
        Value::Date(v) => Ok(Utc.from_utc_datetime(&v.and_time(NaiveTime::MIN))),
        Value::String(s) => parse_offset_date_time(&s)
            .map(|v| v.with_timezone(&Utc))
            .or_else(|| parse_naive_date_time(&s).map(|v| Utc.from_utc_datetime(&v)))
            .ok_or_else(|| FieldError::Invalid(format!("`{s}` is not a date-time"))),
        // A boolean is never a moment in time.
        Value::Bool(_) => Err(mismatch(&value, "a date-time")),
        other => match integer(&other) {
            // Unix seconds (timestamps stored as integers).
            Ok(seconds) => i64::try_from(seconds)
                .ok()
                .and_then(|s| DateTime::from_timestamp(s, 0))
                .ok_or_else(|| FieldError::Invalid(format!("{seconds} is out of range"))),
            Err(_) => Err(mismatch(&other, "a date-time")),
        },
    }
}

fn date_time_offset(value: Value) -> Result<DateTime<FixedOffset>, FieldError> {
    null_check(&value)?;
    match value {
        Value::TimestampTz(v) => Ok(v),
        Value::String(s) => parse_offset_date_time(&s)
            .or_else(|| parse_naive_date_time(&s).map(|v| Utc.from_utc_datetime(&v).fixed_offset()))
            .ok_or_else(|| FieldError::Invalid(format!("`{s}` is not a date-time"))),
        other => date_time_utc(other).map(|v| v.fixed_offset()),
    }
}

fn naive_date_time(value: Value) -> Result<NaiveDateTime, FieldError> {
    null_check(&value)?;
    match value {
        Value::DateTime(v) => Ok(v),
        Value::Timestamp(v) => Ok(v.naive_utc()),
        Value::TimestampTz(v) => Ok(v.naive_utc()),
        Value::Date(v) => Ok(v.and_time(NaiveTime::MIN)),
        Value::String(s) => parse_naive_date_time(&s)
            .or_else(|| parse_offset_date_time(&s).map(|v| v.naive_utc()))
            .ok_or_else(|| FieldError::Invalid(format!("`{s}` is not a date-time"))),
        other => Err(mismatch(&other, "a date-time")),
    }
}

fn naive_date(value: Value) -> Result<NaiveDate, FieldError> {
    null_check(&value)?;
    match value {
        Value::Date(v) => Ok(v),
        Value::DateTime(v) => Ok(v.date()),
        Value::Timestamp(v) => Ok(v.date_naive()),
        Value::TimestampTz(v) => Ok(v.date_naive()),
        Value::String(s) => NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d")
            .ok()
            .or_else(|| parse_naive_date_time(&s).map(|v| v.date()))
            .ok_or_else(|| FieldError::Invalid(format!("`{s}` is not a date"))),
        other => Err(mismatch(&other, "a date")),
    }
}

fn naive_time(value: Value) -> Result<NaiveTime, FieldError> {
    null_check(&value)?;
    match value {
        Value::Time(v) => Ok(v),
        Value::DateTime(v) => Ok(v.time()),
        Value::String(s) => NaiveTime::parse_from_str(s.trim(), "%H:%M:%S%.f")
            .or_else(|_| NaiveTime::parse_from_str(s.trim(), "%H:%M"))
            .map_err(|_| FieldError::Invalid(format!("`{s}` is not a time"))),
        other => Err(mismatch(&other, "a time")),
    }
}

/// `Field` for a type read with a lenient conversion from values.
macro_rules! exact_field {
    ($ty:ty, $name:literal, $kind:expr, $null:expr, $from_value:expr) => {
        impl Field for $ty {
            const KIND: FieldKind = $kind;

            fn to_value(&self) -> Value {
                Value::from(self.clone())
            }

            fn null() -> Value {
                $null
            }

            fn from_value(value: Value) -> Result<Self, FieldError> {
                $from_value(value)
            }

            fn type_name() -> Cow<'static, str> {
                Cow::Borrowed($name)
            }
        }
    };
}

/// `Field` for an integer type: any integer column width (and whole
/// decimals, floats and numeric text), range-checked.
macro_rules! integer_field {
    ($ty:ty, $name:literal, $bits:literal, $signed:literal, $null:expr) => {
        impl Field for $ty {
            const KIND: FieldKind = FieldKind::Integer {
                bits: $bits,
                signed: $signed,
            };

            fn to_value(&self) -> Value {
                Value::from(*self)
            }

            fn null() -> Value {
                $null
            }

            fn from_value(value: Value) -> Result<Self, FieldError> {
                let int = integer(&value)?;
                <$ty>::try_from(int)
                    .map_err(|_| FieldError::Invalid(format!("{int} does not fit {}", $name)))
            }

            fn type_name() -> Cow<'static, str> {
                Cow::Borrowed($name)
            }
        }
    };
}

integer_field!(i8, "i8", 8, true, Value::Null(Kind::I8));
integer_field!(i16, "i16", 16, true, Value::Null(Kind::I16));
integer_field!(i32, "i32", 32, true, Value::Null(Kind::I32));
integer_field!(i64, "i64", 64, true, Value::Null(Kind::I64));
integer_field!(u8, "u8", 8, false, Value::Null(Kind::U8));
integer_field!(u16, "u16", 16, false, Value::Null(Kind::U16));
integer_field!(u32, "u32", 32, false, Value::Null(Kind::U32));
integer_field!(u64, "u64", 64, false, Value::Null(Kind::U64));

exact_field!(
    bool,
    "bool",
    FieldKind::Bool,
    Value::Null(Kind::Bool),
    |value: Value| boolean(&value)
);

impl Field for f32 {
    const KIND: FieldKind = FieldKind::Float { bits: 32 };

    fn to_value(&self) -> Value {
        Value::F32(*self)
    }

    fn null() -> Value {
        Value::Null(Kind::F32)
    }

    fn from_value(value: Value) -> Result<Self, FieldError> {
        // Narrowing to the field's own precision.
        #[allow(clippy::cast_possible_truncation)]
        float(&value).map(|v| v as f32)
    }

    fn type_name() -> Cow<'static, str> {
        Cow::Borrowed("f32")
    }
}

impl Field for f64 {
    const KIND: FieldKind = FieldKind::Float { bits: 64 };

    fn to_value(&self) -> Value {
        Value::F64(*self)
    }

    fn null() -> Value {
        Value::Null(Kind::F64)
    }

    fn from_value(value: Value) -> Result<Self, FieldError> {
        float(&value)
    }

    fn type_name() -> Cow<'static, str> {
        Cow::Borrowed("f64")
    }
}
exact_field!(
    String,
    "String",
    FieldKind::Text,
    Value::Null(Kind::String),
    string
);

/// Bytes: JSON input is an array of numbers (what serde writes for a
/// `Vec<u8>`) or a string (its UTF-8 bytes).
impl Field for Vec<u8> {
    const KIND: FieldKind = FieldKind::Bytes;

    fn to_value(&self) -> Value {
        Value::from(self.clone())
    }

    fn null() -> Value {
        Value::Null(Kind::Bytes)
    }

    fn from_value(value: Value) -> Result<Self, FieldError> {
        bytes(value)
    }

    fn from_json(json: serde_json::Value) -> Result<Self, FieldError> {
        match json {
            serde_json::Value::Array(items) => items
                .iter()
                .map(|item| {
                    item.as_u64()
                        .and_then(|n| u8::try_from(n).ok())
                        .ok_or_else(|| {
                            FieldError::Invalid(format!("{item} is not a byte (0 to 255)"))
                        })
                })
                .collect(),
            serde_json::Value::Null => Err(FieldError::Null),
            other => bytes(json_to_value(other)),
        }
    }

    fn type_name() -> Cow<'static, str> {
        Cow::Borrowed("Vec<u8>")
    }
}

/// `Uuid`: native on PostgreSQL, 16 bytes on SQLite and MySQL; also read
/// from text columns holding the hyphenated form (`CHAR(36)`).
impl Field for Uuid {
    const KIND: FieldKind = FieldKind::Uuid;

    fn to_value(&self) -> Value {
        Value::Uuid(*self)
    }

    fn null() -> Value {
        Value::Null(Kind::Uuid)
    }

    fn from_value(value: Value) -> Result<Self, FieldError> {
        uuid(value)
    }

    fn type_name() -> Cow<'static, str> {
        Cow::Borrowed("Uuid")
    }
}

/// `Field` for a type read as the column's own type, or parsed from text
/// when the column holds text (SQLite's date-times).
macro_rules! parsed_field {
    ($ty:ty, $name:literal, $kind:expr, $null:expr, $parse:expr) => {
        impl Field for $ty {
            const KIND: FieldKind = $kind;

            fn to_value(&self) -> Value {
                Value::from(self.clone())
            }

            fn null() -> Value {
                $null
            }

            fn from_value(value: Value) -> Result<Self, FieldError> {
                $parse(value)
            }

            fn type_name() -> Cow<'static, str> {
                Cow::Borrowed($name)
            }
        }
    };
}

/// JSON: read as it is from JSON columns, parsed from text columns; JSON
/// input is taken as it is (`null` included).
impl Field for serde_json::Value {
    const KIND: FieldKind = FieldKind::Json;

    fn to_value(&self) -> Value {
        Value::from(self.clone())
    }

    fn null() -> Value {
        Value::Null(Kind::Json)
    }

    fn from_value(value: Value) -> Result<Self, FieldError> {
        json(value)
    }

    fn from_json(json: serde_json::Value) -> Result<Self, FieldError> {
        Ok(json)
    }

    fn type_name() -> Cow<'static, str> {
        Cow::Borrowed("serde_json::Value")
    }
}

parsed_field!(
    NaiveDate,
    "NaiveDate",
    FieldKind::Date,
    Value::Null(Kind::Date),
    naive_date
);
parsed_field!(
    NaiveTime,
    "NaiveTime",
    FieldKind::Time,
    Value::Null(Kind::Time),
    naive_time
);
parsed_field!(
    NaiveDateTime,
    "NaiveDateTime",
    FieldKind::DateTime,
    Value::Null(Kind::DateTime),
    naive_date_time
);
parsed_field!(
    DateTime<Utc>,
    "DateTime<Utc>",
    FieldKind::DateTime,
    Value::Null(Kind::Timestamp),
    date_time_utc
);
parsed_field!(
    DateTime<FixedOffset>,
    "DateTime<FixedOffset>",
    FieldKind::DateTime,
    Value::Null(Kind::TimestampTz),
    date_time_offset
);

/// [`Decimal`]: exact on PostgreSQL and MySQL; on SQLite, whose `NUMERIC`
/// columns keep numbers as integers or floats, read from those or from
/// text.
impl Field for Decimal {
    const KIND: FieldKind = FieldKind::Decimal;

    fn to_value(&self) -> Value {
        Value::Decimal(*self)
    }

    fn null() -> Value {
        Value::Null(Kind::Decimal)
    }

    fn from_value(value: Value) -> Result<Self, FieldError> {
        decimal(&value)
    }

    /// A JSON number is read from its exact text (`12.10` stays `12.1`, not
    /// the nearest binary float).
    fn from_json(json: serde_json::Value) -> Result<Self, FieldError> {
        match json {
            serde_json::Value::Number(number) => decimal(&Value::String(number.to_string())),
            other => decimal(&json_to_value(other)),
        }
    }

    fn type_name() -> Cow<'static, str> {
        Cow::Borrowed("Decimal")
    }
}

impl<T: Field> Field for Option<T> {
    const KIND: FieldKind = T::KIND;

    fn to_value(&self) -> Value {
        match self {
            Some(value) => value.to_value(),
            None => T::null(),
        }
    }

    fn null() -> Value {
        T::null()
    }

    fn from_sql(row: &navree_db::Row, column: &str) -> Result<Self, FieldError> {
        match T::from_sql(row, column) {
            Ok(value) => Ok(Some(value)),
            Err(FieldError::Null) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn from_value(value: Value) -> Result<Self, FieldError> {
        if is_null(&value) {
            Ok(None)
        } else {
            T::from_value(value).map(Some)
        }
    }

    fn from_json(json: serde_json::Value) -> Result<Self, FieldError> {
        if json.is_null() {
            Ok(None)
        } else {
            T::from_json(json).map(Some)
        }
    }

    fn type_name() -> Cow<'static, str> {
        Cow::Owned(format!("Option<{}>", T::type_name()))
    }
}

/// A value a column holding `T` accepts in conditions and assignments
/// (`Deal::TITLE.eq("Renewal")`, `Company::COUNTRY.eq("US")`,
/// `Deal::AMOUNT_CENTS.gte(10_000)`).
///
/// Implemented for:
///
/// - `T` itself, and `T` for a nullable column (`Option<T>`);
/// - text (`&str`, `&String`, `Cow<str>`) for `String` and `Option<String>`
///   columns;
/// - lossless widenings of integers (`i32` for an `i64` column, ...) and
///   floats (`f32` for an `f64` column);
/// - everything a [`Value`] is built from, for untyped columns
///   ([`col`](crate::col)).
///
/// ```
/// use navree_orm::{Value, ValueFor};
///
/// let title: String = "Renewal".into_value_for();
/// let country: Option<String> = "US".into_value_for();
/// let cents: i64 = 12_i32.into_value_for();
/// let closed: Option<i64> = 7_i64.into_value_for();
/// let any: Value = 42_u8.into_value_for();
/// assert_eq!((title.as_str(), country.as_deref(), cents, closed), ("Renewal", Some("US"), 12, Some(7)));
/// assert_eq!(any, Value::U8(42));
/// ```
#[diagnostic::on_unimplemented(
    message = "a `{Self}` cannot be used for a column holding `{T}`",
    note = "pass a `{T}` (or, for a nullable column, its inner type)"
)]
pub trait ValueFor<T> {
    /// The value as the column's type.
    fn into_value_for(self) -> T;
}

impl<T> ValueFor<T> for T {
    fn into_value_for(self) -> T {
        self
    }
}

impl<T> ValueFor<Option<T>> for T {
    fn into_value_for(self) -> Option<T> {
        Some(self)
    }
}

impl ValueFor<String> for &str {
    fn into_value_for(self) -> String {
        self.to_owned()
    }
}

impl ValueFor<String> for &String {
    fn into_value_for(self) -> String {
        self.clone()
    }
}

impl ValueFor<String> for Cow<'_, str> {
    fn into_value_for(self) -> String {
        self.into_owned()
    }
}

impl ValueFor<Option<String>> for &str {
    fn into_value_for(self) -> Option<String> {
        Some(self.to_owned())
    }
}

impl ValueFor<Option<String>> for &String {
    fn into_value_for(self) -> Option<String> {
        Some(self.clone())
    }
}

impl ValueFor<Option<String>> for Option<&str> {
    fn into_value_for(self) -> Option<String> {
        self.map(str::to_owned)
    }
}

/// Lossless widenings: `$from` values for `$to` (and `Option<$to>`)
/// columns.
macro_rules! widening {
    ($($from:ty => $($to:ty),+;)*) => {$($(
        impl ValueFor<$to> for $from {
            fn into_value_for(self) -> $to {
                <$to>::from(self)
            }
        }

        impl ValueFor<Option<$to>> for $from {
            fn into_value_for(self) -> Option<$to> {
                Some(<$to>::from(self))
            }
        }
    )+)*};
}

widening! {
    i8 => i16, i32, i64;
    i16 => i32, i64;
    i32 => i64;
    u8 => i16, i32, i64, u16, u32, u64;
    u16 => i32, i64, u32, u64;
    u32 => i64, u64;
    f32 => f64;
}

/// Anything a [`Value`] is built from, for untyped columns.
macro_rules! into_value {
    ($($from:ty),* $(,)?) => {$(
        impl ValueFor<Value> for $from {
            fn into_value_for(self) -> Value {
                Value::from(self)
            }
        }
    )*};
}

into_value!(
    bool,
    i8,
    i16,
    i32,
    i64,
    u8,
    u16,
    u32,
    u64,
    f32,
    f64,
    char,
    &str,
    String,
    Vec<u8>,
    serde_json::Value,
    NaiveDate,
    NaiveTime,
    NaiveDateTime,
    DateTime<Utc>,
    DateTime<FixedOffset>,
    Uuid,
    Decimal,
    crate::Ulid,
);

impl ValueFor<Value> for &String {
    fn into_value_for(self) -> Value {
        Value::from(self.clone())
    }
}

/// A nullable value for an untyped column (a typed NULL for `None`).
impl<T> ValueFor<Value> for Option<T>
where
    Option<T>: Into<Value>,
{
    fn into_value_for(self) -> Value {
        self.into()
    }
}

/// A number a column can be incremented or decremented by
/// ([`Record::increment`](crate::Record::increment),
/// [`Builder::increment`](crate::Builder::increment)).
///
/// Implemented for every integer type, `f32`, `f64` and [`Decimal`].
#[diagnostic::on_unimplemented(
    message = "`{Self}` columns cannot be incremented",
    note = "increment integer, float or `Decimal` columns"
)]
pub trait Numeric: Field + Copy {
    /// `self + other`, `None` on overflow.
    fn checked_add(self, other: Self) -> Option<Self>;

    /// `self - other`, `None` on overflow.
    fn checked_sub(self, other: Self) -> Option<Self>;
}

macro_rules! integer_numeric {
    ($($ty:ty),*) => {$(
        impl Numeric for $ty {
            fn checked_add(self, other: Self) -> Option<Self> {
                <$ty>::checked_add(self, other)
            }

            fn checked_sub(self, other: Self) -> Option<Self> {
                <$ty>::checked_sub(self, other)
            }
        }
    )*};
}

integer_numeric!(i8, i16, i32, i64, u8, u16, u32, u64);

macro_rules! float_numeric {
    ($($ty:ty),*) => {$(
        impl Numeric for $ty {
            fn checked_add(self, other: Self) -> Option<Self> {
                Some(self + other).filter(|sum| sum.is_finite())
            }

            fn checked_sub(self, other: Self) -> Option<Self> {
                Some(self - other).filter(|difference| difference.is_finite())
            }
        }
    )*};
}

float_numeric!(f32, f64);

impl Numeric for Decimal {
    fn checked_add(self, other: Self) -> Option<Self> {
        Decimal::checked_add(self, other)
    }

    fn checked_sub(self, other: Self) -> Option<Self> {
        Decimal::checked_sub(self, other)
    }
}

/// Any column, decoded dynamically (the column's own type decides the
/// variant). For untyped columns and expressions.
impl Field for Value {
    fn to_value(&self) -> Value {
        self.clone()
    }

    fn null() -> Value {
        Value::Null(Kind::String)
    }

    fn from_value(value: Value) -> Result<Self, FieldError> {
        Ok(value)
    }

    fn type_name() -> Cow<'static, str> {
        Cow::Borrowed("Value")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers_convert_leniently_and_check_ranges() {
        assert_eq!(i32::from_value(Value::I64(7)).unwrap(), 7);
        assert_eq!(
            i64::from_value(Value::Decimal(Decimal::new(4200, 2))).unwrap(),
            42
        );
        assert_eq!(i64::from_value(Value::String(" 12 ".into())).unwrap(), 12);
        assert!(matches!(
            i8::from_value(Value::I64(300)),
            Err(FieldError::Invalid(message)) if message == "300 does not fit i8"
        ));
        assert_eq!(
            i64::from_value(Value::Null(Kind::I64)),
            Err(FieldError::Null)
        );
        assert!(i64::from_value(Value::Decimal(Decimal::new(425, 1))).is_err());
        assert_eq!(u64::from_value(Value::I64(5)).unwrap(), 5);
        assert!(u64::from_value(Value::I64(-5)).is_err());
    }

    #[test]
    fn booleans_read_integers_and_words() {
        assert!(bool::from_value(Value::I64(1)).unwrap());
        assert!(!bool::from_value(Value::I32(0)).unwrap());
        assert!(bool::from_value(Value::String("true".into())).unwrap());
        assert!(bool::from_value(Value::I64(2)).is_err());
    }

    #[test]
    fn floats_and_decimals() {
        assert_eq!(f64::from_value(Value::I64(3)).unwrap(), 3.0);
        assert_eq!(
            f64::from_value(Value::Decimal(Decimal::new(125, 2))).unwrap(),
            1.25
        );
        assert_eq!(
            Decimal::from_value(Value::String("12.50".into())).unwrap(),
            Decimal::new(1250, 2)
        );
        assert_eq!(
            Decimal::from_value(Value::I64(3)).unwrap(),
            Decimal::from(3)
        );
    }

    #[test]
    fn date_times_parse_the_text_forms_databases_return() {
        let expected = Utc.with_ymd_and_hms(2026, 10, 27, 8, 30, 0).unwrap();
        for text in [
            "2026-10-27T08:30:00Z",
            "2026-10-27 08:30:00+00:00",
            "2026-10-27 08:30:00",
            "2026-10-27T10:30:00+02:00",
        ] {
            assert_eq!(
                DateTime::<Utc>::from_value(Value::String(text.into())).unwrap(),
                expected,
                "{text}"
            );
        }
        assert_eq!(
            NaiveDate::from_value(Value::String("2026-10-27".into())).unwrap(),
            NaiveDate::from_ymd_opt(2026, 10, 27).unwrap()
        );
        assert_eq!(
            NaiveTime::from_value(Value::String("08:30:15".into())).unwrap(),
            NaiveTime::from_hms_opt(8, 30, 15).unwrap()
        );
        assert!(DateTime::<Utc>::from_value(Value::String("soon".into())).is_err());
    }

    #[test]
    fn options_map_nulls_to_none() {
        assert_eq!(<Option<String>>::null(), Value::Null(Kind::String));
        assert_eq!(
            <Option<String>>::from_value(Value::Null(Kind::String)).unwrap(),
            None
        );
        assert_eq!(
            <Option<String>>::from_value(Value::String("x".into())).unwrap(),
            Some("x".to_owned())
        );
        assert_eq!(
            <Option<Uuid>>::type_name(),
            Cow::<str>::Owned("Option<Uuid>".into())
        );
        assert_eq!(Some(3_i32).to_value(), Value::I32(3));
        assert_eq!(None::<i32>.to_value(), Value::Null(Kind::I32));
    }

    #[test]
    fn uuids_and_json() {
        let id = Uuid::now_v7();
        assert_eq!(Uuid::from_value(Value::String(id.to_string())).unwrap(), id);
        assert_eq!(
            Uuid::from_value(Value::Bytes(id.as_bytes().to_vec())).unwrap(),
            id
        );
        assert_eq!(
            serde_json::Value::from_value(Value::String(r#"{"a":1}"#.into())).unwrap(),
            serde_json::json!({"a": 1})
        );
    }

    #[test]
    fn nulls_of_every_type_are_detected() {
        assert!(is_null(&Value::Null(Kind::Uuid)));
        assert!(is_null(&Value::Null(Kind::Json)));
        assert!(!is_null(&Value::Bool(false)));
    }
}
