//! [`Value`], [`Field`] and the conversions between Rust types and column
//! values.
//!
//! [`Value`] is sea-query's value enum, so the ORM binds parameters exactly
//! as SeaORM does (typed NULLs included: PostgreSQL needs the type of a
//! NULL parameter). Its variant set grows with cargo features (PostgreSQL
//! arrays, for example), so code matching on it keeps a `_` arm.
//!
//! [`Field`] is implemented by every Rust type a model field can have
//! without a cast. Reading from an SQL row is exact (SeaORM's decoders for
//! that very type); reading from a [`Value`] (document rows, aggregates,
//! dynamic columns) is lenient between representations of the same thing:
//! `NUMERIC` 42 reads as an `i64`, the text `2026-10-27 08:00:00+00:00`
//! (SQLite's date-times) as a `DateTime<Utc>`.

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use sea_orm::{QueryResult, TryGetError, TryGetable};
use uuid::Uuid;

pub use rust_decimal::Decimal;
pub use sea_query::Value;

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
/// use navree_orm::{Field, Value};
///
/// assert_eq!(42_i64.to_value(), Value::BigInt(Some(42)));
/// assert_eq!(<Option<i64>>::null(), Value::BigInt(None));
/// // Lenient between representations of the same number:
/// assert_eq!(i64::from_value(Value::Double(Some(42.0))).unwrap(), 42);
/// assert!(i64::from_value(Value::Double(Some(4.2))).is_err());
/// assert_eq!(<Option<i64>>::from_value(Value::BigInt(None)).unwrap(), None);
/// ```
pub trait Field: Sized + Send + Sync + 'static {
    /// What the values are as JSON (for serialization and OpenAPI
    /// schemas); [`FieldKind::Any`] unless the type says.
    const KIND: FieldKind = FieldKind::Any;

    /// The column value of `self`.
    fn to_value(&self) -> Value;

    /// A NULL of this field's type.
    fn null() -> Value;

    /// Reads the field from column `column` of an SQL result row, with the
    /// database driver's decoder for this exact type.
    fn from_sql(row: &QueryResult, column: &str) -> Result<Self, FieldError>;

    /// Converts a value: a document field, a dynamically decoded column, an
    /// aggregate, an assignment.
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

/// JSON input as the [`Value`] it most naturally is: `null` a NULL,
/// booleans and strings themselves, integers `BIGINT` (or `BIGINT
/// UNSIGNED` above `i64`), other numbers `DOUBLE`, arrays and objects JSON.
pub(crate) fn json_to_value(json: serde_json::Value) -> Value {
    use serde_json::Value as Json;

    match json {
        Json::Null => Value::Json(None),
        Json::Bool(b) => Value::Bool(Some(b)),
        Json::Number(n) => match (n.as_i64(), n.as_u64(), n.as_f64()) {
            (Some(i), _, _) => Value::BigInt(Some(i)),
            (None, Some(u), _) => Value::BigUnsigned(Some(u)),
            (None, None, Some(f)) => Value::Double(Some(f)),
            (None, None, None) => Value::String(Some(n.to_string())),
        },
        Json::String(s) => Value::String(Some(s)),
        other => Value::Json(Some(Box::new(other))),
    }
}

/// Reads `T` from `column` with SeaORM's decoder, telling a NULL, a missing
/// column and a type mismatch apart.
pub(crate) fn sql_get<T: TryGetable>(row: &QueryResult, column: &str) -> Result<T, FieldError> {
    match T::try_get(row, "", column) {
        Ok(value) => Ok(value),
        Err(TryGetError::Null(_)) => Err(FieldError::Null),
        Err(TryGetError::DbErr(error)) => {
            if row.column_names().iter().any(|name| name == column) {
                Err(FieldError::Invalid(error.to_string()))
            } else {
                Err(FieldError::Missing)
            }
        }
    }
}

/// Whether `value` is a NULL (of any type).
///
/// ```
/// use navree_orm::{Value, is_null};
///
/// assert!(is_null(&Value::Uuid(None)));
/// assert!(!is_null(&Value::Bool(Some(false))));
/// ```
pub fn is_null(value: &Value) -> bool {
    match value {
        Value::Bool(v) => v.is_none(),
        Value::TinyInt(v) => v.is_none(),
        Value::SmallInt(v) => v.is_none(),
        Value::Int(v) => v.is_none(),
        Value::BigInt(v) => v.is_none(),
        Value::TinyUnsigned(v) => v.is_none(),
        Value::SmallUnsigned(v) => v.is_none(),
        Value::Unsigned(v) => v.is_none(),
        Value::BigUnsigned(v) => v.is_none(),
        Value::Float(v) => v.is_none(),
        Value::Double(v) => v.is_none(),
        Value::String(v) => v.is_none(),
        Value::Char(v) => v.is_none(),
        Value::Bytes(v) => v.is_none(),
        Value::Json(v) => v.is_none(),
        Value::ChronoDate(v) => v.is_none(),
        Value::ChronoTime(v) => v.is_none(),
        Value::ChronoDateTime(v) => v.is_none(),
        Value::ChronoDateTimeUtc(v) => v.is_none(),
        Value::ChronoDateTimeLocal(v) => v.is_none(),
        Value::ChronoDateTimeWithTimeZone(v) => v.is_none(),
        Value::Uuid(v) => v.is_none(),
        Value::Decimal(v) => v.is_none(),
        // Enums and the variants other cargo features add.
        other => sea_query::value::sea_value_to_json_value(other).is_null(),
    }
}

/// `value` as JSON, the way the field would serialize: numbers as numbers,
/// decimals as exact strings (`"12.50"`), UUIDs and date-times as their
/// serde text (RFC 3339), JSON as itself, bytes as an array of numbers,
/// NULL as `null`. Non-finite floats become `null`.
///
/// ```
/// use navree_orm::{Decimal, Value, to_json};
///
/// assert_eq!(to_json(&Value::from(42_i64)), serde_json::json!(42));
/// assert_eq!(to_json(&Value::Decimal(Some(Decimal::new(1250, 2)))), serde_json::json!("12.50"));
/// assert_eq!(to_json(&Value::String(None)), serde_json::Value::Null);
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
        Value::Bool(v) => v.map_or(Json::Null, Json::Bool),
        Value::TinyInt(v) => v.map_or(Json::Null, Json::from),
        Value::SmallInt(v) => v.map_or(Json::Null, Json::from),
        Value::Int(v) => v.map_or(Json::Null, Json::from),
        Value::BigInt(v) => v.map_or(Json::Null, Json::from),
        Value::TinyUnsigned(v) => v.map_or(Json::Null, Json::from),
        Value::SmallUnsigned(v) => v.map_or(Json::Null, Json::from),
        Value::Unsigned(v) => v.map_or(Json::Null, Json::from),
        Value::BigUnsigned(v) => v.map_or(Json::Null, Json::from),
        Value::Float(v) => v.map_or(Json::Null, |v| float(f64::from(v))),
        Value::Double(v) => v.map_or(Json::Null, float),
        Value::String(v) => v.as_ref().map_or(Json::Null, |s| Json::String(s.clone())),
        Value::Char(v) => v.map_or(Json::Null, |c| Json::String(c.to_string())),
        Value::Bytes(v) => v.as_ref().map_or(Json::Null, serde),
        Value::Json(v) => v.as_deref().cloned().unwrap_or(Json::Null),
        Value::ChronoDate(v) => v.as_ref().map_or(Json::Null, serde),
        Value::ChronoTime(v) => v.as_ref().map_or(Json::Null, serde),
        Value::ChronoDateTime(v) => v.as_ref().map_or(Json::Null, serde),
        Value::ChronoDateTimeUtc(v) => v.as_ref().map_or(Json::Null, serde),
        Value::ChronoDateTimeLocal(v) => v.as_ref().map_or(Json::Null, serde),
        Value::ChronoDateTimeWithTimeZone(v) => v.as_ref().map_or(Json::Null, serde),
        Value::Uuid(v) => v.map_or(Json::Null, |u| Json::String(u.to_string())),
        Value::Decimal(v) => v.map_or(Json::Null, |d| Json::String(d.to_string())),
        // Enums and the variants other cargo features add.
        other => sea_query::value::sea_value_to_json_value(other),
    }
}

/// Whether two stored values of a column hold the same thing (what dirty
/// tracking asks): NULLs are equal whatever their type, floats within a
/// small relative tolerance, decimals numerically (`12.5` and `12.50`),
/// date-times by instant, JSON structurally, anything else exactly.
///
/// ```
/// use navree_orm::{Decimal, Value, same_value};
///
/// assert!(same_value(&Value::Double(Some(0.1 + 0.2)), &Value::Double(Some(0.3))));
/// assert!(same_value(
///     &Value::Decimal(Some(Decimal::new(125, 1))),
///     &Value::Decimal(Some(Decimal::new(1250, 2))),
/// ));
/// assert!(same_value(&Value::String(None), &Value::BigInt(None)));
/// assert!(!same_value(&Value::from("a"), &Value::from("b")));
/// ```
pub fn same_value(a: &Value, b: &Value) -> bool {
    match (is_null(a), is_null(b)) {
        (true, true) => return true,
        (true, false) | (false, true) => return false,
        (false, false) => {}
    }
    match (a, b) {
        (Value::Float(_) | Value::Double(_), Value::Float(_) | Value::Double(_)) => {
            match (float(a), float(b)) {
                (Ok(x), Ok(y)) => {
                    x == y || (x - y).abs() <= f64::EPSILON * 4.0 * x.abs().max(y.abs()).max(1.0)
                }
                _ => false,
            }
        }
        (Value::Decimal(Some(x)), Value::Decimal(Some(y))) => x == y,
        (
            Value::ChronoDateTimeUtc(_)
            | Value::ChronoDateTimeWithTimeZone(_)
            | Value::ChronoDateTimeLocal(_),
            Value::ChronoDateTimeUtc(_)
            | Value::ChronoDateTimeWithTimeZone(_)
            | Value::ChronoDateTimeLocal(_),
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
        Value::Bool(_) => "a boolean",
        Value::TinyInt(_)
        | Value::SmallInt(_)
        | Value::Int(_)
        | Value::BigInt(_)
        | Value::TinyUnsigned(_)
        | Value::SmallUnsigned(_)
        | Value::Unsigned(_)
        | Value::BigUnsigned(_) => "an integer",
        Value::Float(_) | Value::Double(_) => "a float",
        Value::Decimal(_) => "a decimal",
        Value::String(_) | Value::Char(_) => "a string",
        Value::Bytes(_) => "bytes",
        Value::Json(_) => "JSON",
        Value::ChronoDate(_) => "a date",
        Value::ChronoTime(_) => "a time",
        Value::ChronoDateTime(_)
        | Value::ChronoDateTimeUtc(_)
        | Value::ChronoDateTimeLocal(_)
        | Value::ChronoDateTimeWithTimeZone(_) => "a date-time",
        Value::Uuid(_) => "a UUID",
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
        Value::Bool(Some(b)) => Ok(i128::from(*b)),
        Value::TinyInt(Some(v)) => Ok(i128::from(*v)),
        Value::SmallInt(Some(v)) => Ok(i128::from(*v)),
        Value::Int(Some(v)) => Ok(i128::from(*v)),
        Value::BigInt(Some(v)) => Ok(i128::from(*v)),
        Value::TinyUnsigned(Some(v)) => Ok(i128::from(*v)),
        Value::SmallUnsigned(Some(v)) => Ok(i128::from(*v)),
        Value::Unsigned(Some(v)) => Ok(i128::from(*v)),
        Value::BigUnsigned(Some(v)) => Ok(i128::from(*v)),
        Value::Float(Some(v)) => integral_float(f64::from(*v)),
        Value::Double(Some(v)) => integral_float(*v),
        Value::Decimal(Some(v)) if v.fract().is_zero() => v
            .to_i128()
            .ok_or_else(|| FieldError::Invalid(format!("{v} is out of range"))),
        Value::Decimal(Some(v)) => Err(FieldError::Invalid(format!("{v} is not a whole number"))),
        Value::String(Some(s)) => s
            .trim()
            .parse::<i128>()
            .map_err(|_| FieldError::Invalid(format!("`{s}` is not an integer"))),
        Value::Json(Some(json)) => json
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
        Value::Float(Some(v)) => Ok(f64::from(*v)),
        Value::Double(Some(v)) => Ok(*v),
        Value::Decimal(Some(v)) => v
            .to_f64()
            .ok_or_else(|| FieldError::Invalid(format!("{v} does not fit a float"))),
        Value::String(Some(s)) => s
            .trim()
            .parse::<f64>()
            .map_err(|_| FieldError::Invalid(format!("`{s}` is not a number"))),
        Value::Json(Some(json)) => json.as_f64().ok_or_else(|| mismatch(value, "a number")),
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
        Value::Decimal(Some(v)) => Ok(*v),
        Value::Float(Some(v)) => Decimal::from_f32(*v)
            .ok_or_else(|| FieldError::Invalid(format!("{v} does not fit a decimal"))),
        Value::Double(Some(v)) => Decimal::from_f64(*v)
            .ok_or_else(|| FieldError::Invalid(format!("{v} does not fit a decimal"))),
        Value::String(Some(s)) => Decimal::from_str(s.trim())
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
        Value::Bool(Some(b)) => Ok(*b),
        Value::String(Some(s)) => match s.trim().to_ascii_lowercase().as_str() {
            "true" | "t" | "1" | "yes" | "y" | "on" => Ok(true),
            "false" | "f" | "0" | "no" | "n" | "off" => Ok(false),
            _ => Err(FieldError::Invalid(format!("`{s}` is not a boolean"))),
        },
        Value::Json(Some(json)) => json.as_bool().ok_or_else(|| mismatch(value, "a boolean")),
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
        Value::String(Some(s)) => Ok(s),
        Value::Char(Some(c)) => Ok(c.to_string()),
        Value::Uuid(Some(u)) => Ok(u.to_string()),
        Value::Json(Some(json)) => match *json {
            serde_json::Value::String(s) => Ok(s),
            other => Ok(other.to_string()),
        },
        Value::Bytes(Some(bytes)) => String::from_utf8(bytes)
            .map_err(|_| FieldError::Invalid("the bytes are not UTF-8 text".to_owned())),
        Value::Decimal(Some(d)) => Ok(d.to_string()),
        other => Err(mismatch(&other, "a string")),
    }
}

fn bytes(value: Value) -> Result<Vec<u8>, FieldError> {
    null_check(&value)?;
    match value {
        Value::Bytes(Some(b)) => Ok(b),
        Value::String(Some(s)) => Ok(s.into_bytes()),
        Value::Uuid(Some(u)) => Ok(u.as_bytes().to_vec()),
        other => Err(mismatch(&other, "bytes")),
    }
}

fn uuid(value: Value) -> Result<Uuid, FieldError> {
    null_check(&value)?;
    match value {
        Value::Uuid(Some(u)) => Ok(u),
        Value::String(Some(s)) => Uuid::parse_str(s.trim())
            .map_err(|_| FieldError::Invalid(format!("`{s}` is not a UUID"))),
        Value::Bytes(Some(b)) if b.len() == 16 => Uuid::from_slice(&b)
            .map_err(|_| FieldError::Invalid(format!("{} bytes are not a UUID", b.len()))),
        Value::Bytes(Some(b)) => std::str::from_utf8(&b)
            .ok()
            .and_then(|text| Uuid::parse_str(text.trim()).ok())
            .ok_or_else(|| FieldError::Invalid(format!("{} bytes are not a UUID", b.len()))),
        Value::Json(Some(json)) => match json.as_str() {
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
        Value::Json(Some(json)) => Ok(*json),
        Value::String(Some(s)) => serde_json::from_str(&s)
            .map_err(|error| FieldError::Invalid(format!("the text is not JSON: {error}"))),
        Value::Bytes(Some(b)) => serde_json::from_slice(&b)
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
        Value::ChronoDateTimeUtc(Some(v)) => Ok(v),
        Value::ChronoDateTimeWithTimeZone(Some(v)) => Ok(v.with_timezone(&Utc)),
        Value::ChronoDateTimeLocal(Some(v)) => Ok(v.with_timezone(&Utc)),
        Value::ChronoDateTime(Some(v)) => Ok(Utc.from_utc_datetime(&v)),
        Value::ChronoDate(Some(v)) => Ok(Utc.from_utc_datetime(&v.and_time(NaiveTime::MIN))),
        Value::String(Some(s)) => parse_offset_date_time(&s)
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
        Value::ChronoDateTimeWithTimeZone(Some(v)) => Ok(v),
        Value::String(Some(s)) => parse_offset_date_time(&s)
            .or_else(|| parse_naive_date_time(&s).map(|v| Utc.from_utc_datetime(&v).fixed_offset()))
            .ok_or_else(|| FieldError::Invalid(format!("`{s}` is not a date-time"))),
        other => date_time_utc(other).map(|v| v.fixed_offset()),
    }
}

fn naive_date_time(value: Value) -> Result<NaiveDateTime, FieldError> {
    null_check(&value)?;
    match value {
        Value::ChronoDateTime(Some(v)) => Ok(v),
        Value::ChronoDateTimeUtc(Some(v)) => Ok(v.naive_utc()),
        Value::ChronoDateTimeWithTimeZone(Some(v)) => Ok(v.naive_utc()),
        Value::ChronoDateTimeLocal(Some(v)) => Ok(v.naive_utc()),
        Value::ChronoDate(Some(v)) => Ok(v.and_time(NaiveTime::MIN)),
        Value::String(Some(s)) => parse_naive_date_time(&s)
            .or_else(|| parse_offset_date_time(&s).map(|v| v.naive_utc()))
            .ok_or_else(|| FieldError::Invalid(format!("`{s}` is not a date-time"))),
        other => Err(mismatch(&other, "a date-time")),
    }
}

fn naive_date(value: Value) -> Result<NaiveDate, FieldError> {
    null_check(&value)?;
    match value {
        Value::ChronoDate(Some(v)) => Ok(v),
        Value::ChronoDateTime(Some(v)) => Ok(v.date()),
        Value::ChronoDateTimeUtc(Some(v)) => Ok(v.date_naive()),
        Value::ChronoDateTimeWithTimeZone(Some(v)) => Ok(v.date_naive()),
        Value::String(Some(s)) => NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d")
            .ok()
            .or_else(|| parse_naive_date_time(&s).map(|v| v.date()))
            .ok_or_else(|| FieldError::Invalid(format!("`{s}` is not a date"))),
        other => Err(mismatch(&other, "a date")),
    }
}

fn naive_time(value: Value) -> Result<NaiveTime, FieldError> {
    null_check(&value)?;
    match value {
        Value::ChronoTime(Some(v)) => Ok(v),
        Value::ChronoDateTime(Some(v)) => Ok(v.time()),
        Value::String(Some(s)) => NaiveTime::parse_from_str(s.trim(), "%H:%M:%S%.f")
            .or_else(|_| NaiveTime::parse_from_str(s.trim(), "%H:%M"))
            .map_err(|_| FieldError::Invalid(format!("`{s}` is not a time"))),
        other => Err(mismatch(&other, "a time")),
    }
}

/// `Field` for a type SeaORM decodes exactly, with a lenient conversion
/// from values.
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

            fn from_sql(row: &QueryResult, column: &str) -> Result<Self, FieldError> {
                sql_get::<$ty>(row, column)
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

/// Reads an integer column of any width (`SMALLINT`, `INTEGER`, `BIGINT`,
/// MySQL's unsigned types) as `T`, range-checked: the column's type need
/// not match the field's exactly (PostgreSQL's decoders are strict about
/// widths; SQLite has a single integer type).
fn sql_integer<T>(row: &QueryResult, column: &str, name: &str) -> Result<T, FieldError>
where
    T: TryGetable + TryFrom<i128>,
{
    let first = match sql_get::<T>(row, column) {
        Ok(value) => return Ok(value),
        Err(FieldError::Invalid(message)) => message,
        Err(other) => return Err(other),
    };
    let wide: i128 = if let Ok(v) = sql_get::<i64>(row, column) {
        i128::from(v)
    } else if let Ok(v) = sql_get::<i32>(row, column) {
        i128::from(v)
    } else if let Ok(v) = sql_get::<i16>(row, column) {
        i128::from(v)
    } else if let Ok(v) = sql_get_unsigned(row, column) {
        i128::from(v)
    } else if let Ok(v) = sql_get::<Decimal>(row, column) {
        // `SUM` of integers is a NUMERIC on PostgreSQL, a DECIMAL on MySQL.
        integer(&Value::Decimal(Some(v)))?
    } else if let Ok(v) = sql_get::<f64>(row, column) {
        integer(&Value::Double(Some(v)))?
    } else {
        return Err(FieldError::Invalid(first));
    };
    T::try_from(wide).map_err(|_| FieldError::Invalid(format!("{wide} does not fit {name}")))
}

/// Reads `column` with the driver's decoder for `T`, or, when the column
/// holds text (SQLite's date-times, an expression MySQL types as text),
/// parses the text with `parse`.
fn sql_or_text<T: TryGetable>(
    row: &QueryResult,
    column: &str,
    parse: fn(Value) -> Result<T, FieldError>,
) -> Result<T, FieldError> {
    match sql_get::<T>(row, column) {
        Ok(value) => Ok(value),
        Err(FieldError::Invalid(first)) => match sql_get::<String>(row, column) {
            Ok(text) => parse(Value::String(Some(text))),
            Err(_) => Err(FieldError::Invalid(first)),
        },
        Err(other) => Err(other),
    }
}

/// MySQL's `BIGINT UNSIGNED` (the only database with unsigned columns).
fn sql_get_unsigned(row: &QueryResult, column: &str) -> Result<u64, FieldError> {
    sql_get::<u64>(row, column)
}

/// Reads a floating-point, decimal or integer column as an `f64`.
fn sql_float(row: &QueryResult, column: &str) -> Result<f64, FieldError> {
    let first = match sql_get::<f64>(row, column) {
        Ok(value) => return Ok(value),
        Err(FieldError::Invalid(message)) => message,
        Err(other) => return Err(other),
    };
    if let Ok(v) = sql_get::<f32>(row, column) {
        return Ok(f64::from(v));
    }
    if let Ok(v) = sql_get::<Decimal>(row, column) {
        return float(&Value::Decimal(Some(v)));
    }
    match sql_integer::<i64>(row, column, "f64") {
        // Integers from the database fit an f64 closely enough for a float field.
        #[allow(clippy::cast_precision_loss)]
        Ok(v) => Ok(v as f64),
        Err(_) => Err(FieldError::Invalid(first)),
    }
}

/// `Field` for an integer type: any integer column width from SQL,
/// range-checked from values.
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

            fn from_sql(row: &QueryResult, column: &str) -> Result<Self, FieldError> {
                sql_integer::<$ty>(row, column, $name)
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

integer_field!(i8, "i8", 8, true, Value::TinyInt(None));
integer_field!(i16, "i16", 16, true, Value::SmallInt(None));
integer_field!(i32, "i32", 32, true, Value::Int(None));
integer_field!(i64, "i64", 64, true, Value::BigInt(None));
integer_field!(u8, "u8", 8, false, Value::TinyUnsigned(None));
integer_field!(u16, "u16", 16, false, Value::SmallUnsigned(None));
integer_field!(u32, "u32", 32, false, Value::Unsigned(None));
integer_field!(u64, "u64", 64, false, Value::BigUnsigned(None));

exact_field!(bool, "bool", FieldKind::Bool, Value::Bool(None), |value: Value| boolean(
    &value
));

impl Field for f32 {
    const KIND: FieldKind = FieldKind::Float { bits: 32 };

    fn to_value(&self) -> Value {
        Value::Float(Some(*self))
    }

    fn null() -> Value {
        Value::Float(None)
    }

    fn from_sql(row: &QueryResult, column: &str) -> Result<Self, FieldError> {
        match sql_get::<f32>(row, column) {
            Ok(value) => Ok(value),
            // Narrowing to the field's own precision.
            #[allow(clippy::cast_possible_truncation)]
            Err(FieldError::Invalid(_)) => sql_float(row, column).map(|v| v as f32),
            Err(other) => Err(other),
        }
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
        Value::Double(Some(*self))
    }

    fn null() -> Value {
        Value::Double(None)
    }

    fn from_sql(row: &QueryResult, column: &str) -> Result<Self, FieldError> {
        sql_float(row, column)
    }

    fn from_value(value: Value) -> Result<Self, FieldError> {
        float(&value)
    }

    fn type_name() -> Cow<'static, str> {
        Cow::Borrowed("f64")
    }
}
exact_field!(String, "String", FieldKind::Text, Value::String(None), string);

/// Bytes: JSON input is an array of numbers (what serde writes for a
/// `Vec<u8>`) or a string (its UTF-8 bytes).
impl Field for Vec<u8> {
    const KIND: FieldKind = FieldKind::Bytes;

    fn to_value(&self) -> Value {
        Value::from(self.clone())
    }

    fn null() -> Value {
        Value::Bytes(None)
    }

    fn from_sql(row: &QueryResult, column: &str) -> Result<Self, FieldError> {
        sql_get::<Vec<u8>>(row, column)
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
                        .ok_or_else(|| FieldError::Invalid(format!("{item} is not a byte (0 to 255)")))
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
        Value::Uuid(Some(*self))
    }

    fn null() -> Value {
        Value::Uuid(None)
    }

    fn from_sql(row: &QueryResult, column: &str) -> Result<Self, FieldError> {
        match sql_get::<Uuid>(row, column) {
            Ok(value) => Ok(value),
            Err(FieldError::Invalid(error)) => match sql_get::<String>(row, column) {
                Ok(text) => uuid(Value::String(Some(text))),
                Err(_) => match sql_get::<Vec<u8>>(row, column) {
                    Ok(bytes) => uuid(Value::Bytes(Some(bytes))),
                    Err(_) => Err(FieldError::Invalid(error)),
                },
            },
            Err(other) => Err(other),
        }
    }

    fn from_value(value: Value) -> Result<Self, FieldError> {
        uuid(value)
    }

    fn type_name() -> Cow<'static, str> {
        Cow::Borrowed("Uuid")
    }
}
/// `Field` for a type read exactly by the driver, or parsed from text when
/// the column holds text.
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

            fn from_sql(row: &QueryResult, column: &str) -> Result<Self, FieldError> {
                sql_or_text::<$ty>(row, column, $parse)
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

/// JSON: read exactly from JSON columns, parsed from text columns; JSON
/// input is taken as it is (`null` included).
impl Field for serde_json::Value {
    const KIND: FieldKind = FieldKind::Json;

    fn to_value(&self) -> Value {
        Value::from(self.clone())
    }

    fn null() -> Value {
        Value::Json(None)
    }

    fn from_sql(row: &QueryResult, column: &str) -> Result<Self, FieldError> {
        sql_or_text::<serde_json::Value>(row, column, json)
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
    Value::ChronoDate(None),
    naive_date
);
parsed_field!(
    NaiveTime,
    "NaiveTime",
    FieldKind::Time,
    Value::ChronoTime(None),
    naive_time
);
parsed_field!(
    NaiveDateTime,
    "NaiveDateTime",
    FieldKind::DateTime,
    Value::ChronoDateTime(None),
    naive_date_time
);
parsed_field!(
    DateTime<Utc>,
    "DateTime<Utc>",
    FieldKind::DateTime,
    Value::ChronoDateTimeUtc(None),
    date_time_utc
);
parsed_field!(
    DateTime<FixedOffset>,
    "DateTime<FixedOffset>",
    FieldKind::DateTime,
    Value::ChronoDateTimeWithTimeZone(None),
    date_time_offset
);

/// [`Decimal`]: exact on PostgreSQL and MySQL; on SQLite, whose `NUMERIC`
/// columns keep numbers as integers or floats, read from those or from
/// text.
impl Field for Decimal {
    const KIND: FieldKind = FieldKind::Decimal;

    fn to_value(&self) -> Value {
        Value::Decimal(Some(*self))
    }

    fn null() -> Value {
        Value::Decimal(None)
    }

    fn from_sql(row: &QueryResult, column: &str) -> Result<Self, FieldError> {
        match sql_get::<Decimal>(row, column) {
            Ok(value) => Ok(value),
            Err(FieldError::Invalid(error)) => {
                if let Ok(text) = sql_get::<String>(row, column) {
                    return decimal(&Value::String(Some(text)));
                }
                if let Ok(int) = sql_integer::<i64>(row, column, "Decimal") {
                    return Ok(Decimal::from(int));
                }
                match sql_get::<f64>(row, column) {
                    Ok(float) => decimal(&Value::Double(Some(float))),
                    Err(_) => Err(FieldError::Invalid(error)),
                }
            }
            Err(other) => Err(other),
        }
    }

    fn from_value(value: Value) -> Result<Self, FieldError> {
        decimal(&value)
    }

    /// A JSON number is read from its exact text (`12.10` stays `12.1`, not
    /// the nearest binary float).
    fn from_json(json: serde_json::Value) -> Result<Self, FieldError> {
        match json {
            serde_json::Value::Number(number) => {
                decimal(&Value::String(Some(number.to_string())))
            }
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

    fn from_sql(row: &QueryResult, column: &str) -> Result<Self, FieldError> {
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
/// assert_eq!(any, Value::TinyUnsigned(Some(42)));
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
        Value::String(None)
    }

    fn from_sql(row: &QueryResult, column: &str) -> Result<Self, FieldError> {
        crate::row::sniff(row, column)
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
        assert_eq!(i32::from_value(Value::BigInt(Some(7))).unwrap(), 7);
        assert_eq!(
            i64::from_value(Value::Decimal(Some(Decimal::new(4200, 2)))).unwrap(),
            42
        );
        assert_eq!(
            i64::from_value(Value::String(Some(" 12 ".into()))).unwrap(),
            12
        );
        assert!(matches!(
            i8::from_value(Value::BigInt(Some(300))),
            Err(FieldError::Invalid(message)) if message == "300 does not fit i8"
        ));
        assert_eq!(i64::from_value(Value::BigInt(None)), Err(FieldError::Null));
        assert!(i64::from_value(Value::Decimal(Some(Decimal::new(425, 1)))).is_err());
        assert_eq!(u64::from_value(Value::BigInt(Some(5))).unwrap(), 5);
        assert!(u64::from_value(Value::BigInt(Some(-5))).is_err());
    }

    #[test]
    fn booleans_read_integers_and_words() {
        assert!(bool::from_value(Value::BigInt(Some(1))).unwrap());
        assert!(!bool::from_value(Value::Int(Some(0))).unwrap());
        assert!(bool::from_value(Value::String(Some("true".into()))).unwrap());
        assert!(bool::from_value(Value::BigInt(Some(2))).is_err());
    }

    #[test]
    fn floats_and_decimals() {
        assert_eq!(f64::from_value(Value::BigInt(Some(3))).unwrap(), 3.0);
        assert_eq!(
            f64::from_value(Value::Decimal(Some(Decimal::new(125, 2)))).unwrap(),
            1.25
        );
        assert_eq!(
            Decimal::from_value(Value::String(Some("12.50".into()))).unwrap(),
            Decimal::new(1250, 2)
        );
        assert_eq!(
            Decimal::from_value(Value::BigInt(Some(3))).unwrap(),
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
                DateTime::<Utc>::from_value(Value::String(Some(text.into()))).unwrap(),
                expected,
                "{text}"
            );
        }
        assert_eq!(
            NaiveDate::from_value(Value::String(Some("2026-10-27".into()))).unwrap(),
            NaiveDate::from_ymd_opt(2026, 10, 27).unwrap()
        );
        assert_eq!(
            NaiveTime::from_value(Value::String(Some("08:30:15".into()))).unwrap(),
            NaiveTime::from_hms_opt(8, 30, 15).unwrap()
        );
        assert!(DateTime::<Utc>::from_value(Value::String(Some("soon".into()))).is_err());
    }

    #[test]
    fn options_map_nulls_to_none() {
        assert_eq!(<Option<String>>::null(), Value::String(None));
        assert_eq!(
            <Option<String>>::from_value(Value::String(None)).unwrap(),
            None
        );
        assert_eq!(
            <Option<String>>::from_value(Value::String(Some("x".into()))).unwrap(),
            Some("x".to_owned())
        );
        assert_eq!(
            <Option<Uuid>>::type_name(),
            Cow::<str>::Owned("Option<Uuid>".into())
        );
        assert_eq!(Some(3_i32).to_value(), Value::Int(Some(3)));
        assert_eq!(None::<i32>.to_value(), Value::Int(None));
    }

    #[test]
    fn uuids_and_json() {
        let id = Uuid::now_v7();
        assert_eq!(
            Uuid::from_value(Value::String(Some(id.to_string()))).unwrap(),
            id
        );
        assert_eq!(
            Uuid::from_value(Value::Bytes(Some(id.as_bytes().to_vec()))).unwrap(),
            id
        );
        assert_eq!(
            serde_json::Value::from_value(Value::String(Some(r#"{"a":1}"#.into()))).unwrap(),
            serde_json::json!({"a": 1})
        );
    }

    #[test]
    fn nulls_of_every_type_are_detected() {
        assert!(is_null(&Value::Uuid(None)));
        assert!(is_null(&Value::Json(None)));
        assert!(!is_null(&Value::Bool(Some(false))));
    }
}
