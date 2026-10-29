//! Columns and values: what a model's column stores ([`Shape`]), and
//! converting submitted JSON into the plain form a model field takes and
//! into the value a condition binds.

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use navree_orm::{ColumnMeta, FieldKind, Model, Value};
use serde_json::Value as Json;

/// How a date-time column holds its instant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Instant {
    /// `DateTime<Utc>`.
    Utc,
    /// `NaiveDateTime` (UTC by convention).
    Naive,
    /// `DateTime<FixedOffset>`.
    Offset,
}

/// What a column stores, read from the model's column metadata: the
/// conversions of form input and the values bound in conditions follow it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shape {
    Bool,
    Int { bits: u8, signed: bool },
    Float,
    Decimal,
    Text,
    Uuid,
    Date,
    Time,
    DateTime(Instant),
    Json,
    /// Anything else (casts without a known shape, bytes, vectors): input
    /// goes to the model as it is, and the model's conversion decides.
    Other,
}

impl Shape {
    /// The shape of column `meta`.
    pub(crate) fn of(meta: &ColumnMeta) -> Shape {
        match meta.field_kind() {
            FieldKind::Bool => Shape::Bool,
            FieldKind::Integer { bits, signed } => Shape::Int { bits, signed },
            FieldKind::Float { .. } => Shape::Float,
            FieldKind::Decimal => Shape::Decimal,
            FieldKind::Text | FieldKind::Url | FieldKind::Ulid => Shape::Text,
            FieldKind::Uuid => Shape::Uuid,
            FieldKind::Date => Shape::Date,
            FieldKind::Time => Shape::Time,
            FieldKind::DateTime => {
                let rust_type = meta.rust_type();
                Shape::DateTime(if rust_type.contains("NaiveDateTime") {
                    Instant::Naive
                } else if rust_type.contains("FixedOffset") {
                    Instant::Offset
                } else {
                    Instant::Utc
                })
            }
            FieldKind::Json => Shape::Json,
            _ => Shape::Other,
        }
    }

    /// Whether the column holds dates or date-times.
    pub(crate) fn is_temporal(self) -> bool {
        matches!(self, Shape::Date | Shape::DateTime(_))
    }
}

/// The column of `M` named `name`.
pub(crate) fn column<M: Model>(name: &str) -> Option<&'static ColumnMeta> {
    M::meta().column(name)
}

/// The shape of `M`'s column `name` (`Other` when there is none).
pub(crate) fn shape<M: Model>(name: &str) -> Shape {
    column::<M>(name).map_or(Shape::Other, Shape::of)
}

/// `column` qualified with `M`'s table (`deals.title`), so joins added by a
/// resource's query never make it ambiguous.
pub(crate) fn qualified<M: Model>(column: &str) -> String {
    format!("{}.{column}", M::meta().table())
}

/// Why a submitted value cannot be stored: a validation rule name for
/// navree-validation's messages (`integer`, `boolean`, `date`, ...).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Invalid(pub(crate) &'static str);

/// Whether `value` counts as empty: `null`, or a string with nothing but
/// whitespace.
pub(crate) fn is_blank(value: &Json) -> bool {
    match value {
        Json::Null => true,
        Json::String(text) => text.trim().is_empty(),
        _ => false,
    }
}

/// `input` in the plain form a field of `shape` takes from JSON
/// (`Model::set_attribute`): numbers as numbers, booleans as booleans,
/// dates as `YYYY-MM-DD`, date-times as RFC 3339 in UTC, UUIDs hyphenated.
/// A blank input is `null`, except for text and JSON columns. `tz_offset`
/// is the browser's offset from UTC (`Date.getTimezoneOffset()`), applied
/// to date-times without an offset (`datetime-local` inputs).
pub(crate) fn to_input(shape: Shape, input: &Json, tz_offset: Option<i32>) -> Result<Json, Invalid> {
    if input.is_null() || (is_blank(input) && !matches!(shape, Shape::Text | Shape::Json)) {
        return Ok(Json::Null);
    }
    Ok(match shape {
        Shape::Bool => Json::Bool(boolean(input).ok_or(Invalid("boolean"))?),
        Shape::Int { bits, signed } => Json::from(integer(input, bits, signed)?),
        Shape::Float => serde_json::Number::from_f64(number(input)?)
            .map(Json::Number)
            .ok_or(Invalid("numeric"))?,
        Shape::Decimal => {
            number(input)?;
            Json::String(text(input).ok_or(Invalid("numeric"))?.trim().to_owned())
        }
        Shape::Text => Json::String(text(input).ok_or(Invalid("string"))?),
        Shape::Uuid => Json::String(
            uuid(input)
                .ok_or(Invalid("uuid"))?
                .hyphenated()
                .to_string(),
        ),
        Shape::Date => Json::String(
            date(input)
                .ok_or(Invalid("date"))?
                .format("%Y-%m-%d")
                .to_string(),
        ),
        Shape::Time => Json::String(
            time(input)
                .ok_or(Invalid("date"))?
                .format("%H:%M:%S")
                .to_string(),
        ),
        Shape::DateTime(_) => Json::String(
            datetime(input, tz_offset)
                .ok_or(Invalid("date"))?
                .to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
        ),
        Shape::Json | Shape::Other => input.clone(),
    })
}

/// `input` as the value a condition on a column of `shape` binds (filters,
/// keys); a blank input is SQL `NULL`.
pub(crate) fn to_value(shape: Shape, input: &Json) -> Result<Value, Invalid> {
    if is_blank(input) && !matches!(shape, Shape::Text) {
        return Ok(Value::Null(navree_orm::Kind::String));
    }
    Ok(match shape {
        Shape::Bool => Value::Bool(boolean(input).ok_or(Invalid("boolean"))?),
        Shape::Int { bits, signed } => Value::I64(integer(input, bits, signed)?),
        Shape::Float => Value::F64(number(input)?),
        Shape::Decimal => {
            let text = text(input).ok_or(Invalid("numeric"))?;
            Value::Decimal(text.trim().parse().map_err(|_| Invalid("numeric"))?)
        }
        Shape::Uuid => Value::Uuid(uuid(input).ok_or(Invalid("uuid"))?),
        Shape::Date => Value::Date(date(input).ok_or(Invalid("date"))?),
        Shape::Time => Value::Time(time(input).ok_or(Invalid("date"))?),
        Shape::DateTime(instant) => {
            date_bound(Shape::DateTime(instant), datetime(input, None).ok_or(Invalid("date"))?)
        }
        Shape::Json => Value::Json(input.clone()),
        Shape::Text | Shape::Other => Value::String(text(input).ok_or(Invalid("string"))?),
    })
}

/// A boolean from JSON or a form field (`true`, `false`, `on`, `1`, `0`).
pub(crate) fn boolean(input: &Json) -> Option<bool> {
    match input {
        Json::Bool(b) => Some(*b),
        Json::Number(n) => match n.as_i64() {
            Some(1) => Some(true),
            Some(0) => Some(false),
            _ => None,
        },
        Json::String(s) => match s.trim().to_ascii_lowercase().as_str() {
            "true" | "on" | "1" | "yes" => Some(true),
            "false" | "off" | "0" | "no" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

/// An integer that fits a column of `bits` bits.
fn integer(input: &Json, bits: u8, signed: bool) -> Result<i64, Invalid> {
    let n = match input {
        Json::Number(n) => n.as_i64().ok_or(Invalid("integer"))?,
        Json::String(s) => s.trim().parse::<i64>().map_err(|_| Invalid("integer"))?,
        _ => return Err(Invalid("integer")),
    };
    let (min, max) = match (bits, signed) {
        (8, true) => (i64::from(i8::MIN), i64::from(i8::MAX)),
        (16, true) => (i64::from(i16::MIN), i64::from(i16::MAX)),
        (32, true) => (i64::from(i32::MIN), i64::from(i32::MAX)),
        (8, false) => (0, i64::from(u8::MAX)),
        (16, false) => (0, i64::from(u16::MAX)),
        (32, false) => (0, i64::from(u32::MAX)),
        (_, false) => (0, i64::MAX),
        _ => (i64::MIN, i64::MAX),
    };
    if (min..=max).contains(&n) {
        Ok(n)
    } else {
        Err(Invalid("integer"))
    }
}

/// A number from JSON or a form field.
pub(crate) fn number(input: &Json) -> Result<f64, Invalid> {
    match input {
        Json::Number(n) => n.as_f64().ok_or(Invalid("numeric")),
        Json::String(s) => s
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite())
            .ok_or(Invalid("numeric")),
        _ => Err(Invalid("numeric")),
    }
}

fn text(input: &Json) -> Option<String> {
    match input {
        Json::String(s) => Some(s.clone()),
        Json::Number(n) => Some(n.to_string()),
        Json::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn uuid(input: &Json) -> Option<uuid::Uuid> {
    input
        .as_str()
        .and_then(|s| uuid::Uuid::parse_str(s.trim()).ok())
}

fn time(input: &Json) -> Option<NaiveTime> {
    let s = input.as_str()?.trim();
    NaiveTime::parse_from_str(s, "%H:%M:%S")
        .or_else(|_| NaiveTime::parse_from_str(s, "%H:%M"))
        .ok()
}

/// A `YYYY-MM-DD` date (or the date part of an RFC 3339 date-time).
pub(crate) fn date(input: &Json) -> Option<NaiveDate> {
    let s = input.as_str()?.trim();
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok().or_else(|| {
        DateTime::parse_from_rfc3339(s)
            .ok()
            .map(|dt| dt.date_naive())
    })
}

/// An RFC 3339 date-time, or a `YYYY-MM-DDTHH:MM[:SS]` local time at
/// `tz_offset_minutes` (UTC when unknown), or a date at midnight.
pub(crate) fn datetime(input: &Json, tz_offset_minutes: Option<i32>) -> Option<DateTime<Utc>> {
    let s = input.as_str()?.trim();
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }
    let naive = [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M:%S%.f",
    ]
    .iter()
    .find_map(|format| NaiveDateTime::parse_from_str(s, format).ok())
    .or_else(|| {
        NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .ok()
            .and_then(|d| d.and_hms_opt(0, 0, 0))
    })?;
    // `getTimezoneOffset()` is UTC minus local time, in minutes.
    let offset = FixedOffset::west_opt(tz_offset_minutes.unwrap_or(0).saturating_mul(60))?;
    offset
        .from_local_datetime(&naive)
        .single()
        .map(|dt| dt.with_timezone(&Utc))
}

/// The date part of a stored date or date-time value, in UTC.
pub(crate) fn value_date(value: &Value) -> Option<NaiveDate> {
    match value {
        Value::Date(date) => Some(*date),
        Value::DateTime(dt) => Some(dt.date()),
        Value::Timestamp(dt) => Some(dt.date_naive()),
        Value::TimestampTz(dt) => Some(dt.with_timezone(&Utc).date_naive()),
        Value::String(s) => date(&Json::String(s.clone())).or_else(|| {
            NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%d %H:%M:%S%.f")
                .ok()
                .map(|dt| dt.date())
        }),
        _ => None,
    }
}

/// A stored number as `f64` (metrics): integers, floats, decimals and
/// numbers held as text; `None` for NULL and anything else.
pub(crate) fn value_number(value: &Value) -> Option<f64> {
    match value {
        Value::I8(n) => Some(f64::from(*n)),
        Value::I16(n) => Some(f64::from(*n)),
        Value::I32(n) => Some(f64::from(*n)),
        Value::I64(n) => Some(*n as f64),
        Value::U8(n) => Some(f64::from(*n)),
        Value::U16(n) => Some(f64::from(*n)),
        Value::U32(n) => Some(f64::from(*n)),
        Value::U64(n) => Some(*n as f64),
        Value::F32(n) => Some(f64::from(*n)),
        Value::F64(n) => Some(*n),
        Value::Decimal(d) => d.to_string().parse().ok(),
        Value::String(s) => s.trim().parse().ok(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

/// The instant `at` as a value of a date or date-time column of `shape`
/// (bounds of date filters and metric periods; timestamps Helm sets).
pub(crate) fn date_bound(shape: Shape, at: DateTime<Utc>) -> Value {
    match shape {
        Shape::Date => Value::Date(at.date_naive()),
        Shape::DateTime(Instant::Naive) => Value::DateTime(at.naive_utc()),
        Shape::DateTime(Instant::Offset) => Value::TimestampTz(at.fixed_offset()),
        Shape::Text | Shape::Other => Value::String(at.to_rfc3339()),
        _ => Value::Timestamp(at),
    }
}

/// The value of a primary key or foreign key given as text (a path
/// parameter or a form value), typed like the column; `None` when it cannot
/// be one.
pub(crate) fn key_from_str(shape: Shape, id: &str) -> Option<Value> {
    to_value(shape, &Json::String(id.to_owned()))
        .ok()
        .filter(|value| !matches!(value, Value::Null(_)))
}

/// A JSON value as the text of a key: strings as they are, other scalars
/// formatted.
pub(crate) fn key_text(value: &Json) -> Option<String> {
    match value {
        Json::String(s) => Some(s.clone()),
        Json::Number(n) => Some(n.to_string()),
        Json::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const INT: Shape = Shape::Int {
        bits: 32,
        signed: true,
    };

    #[test]
    fn input_converts_to_the_field_s_plain_form() {
        assert_eq!(to_input(INT, &json!("42"), None), Ok(json!(42)));
        assert_eq!(
            to_input(
                Shape::Int {
                    bits: 64,
                    signed: true
                },
                &json!(1_250_000),
                None
            ),
            Ok(json!(1_250_000))
        );
        assert_eq!(
            to_input(
                Shape::Int {
                    bits: 8,
                    signed: true
                },
                &json!(1000),
                None
            ),
            Err(Invalid("integer"))
        );
        assert_eq!(to_input(Shape::Bool, &json!("on"), None), Ok(json!(true)));
        assert_eq!(
            to_input(Shape::Bool, &json!("maybe"), None),
            Err(Invalid("boolean"))
        );
        assert_eq!(
            to_input(Shape::Text, &json!("Globex"), None),
            Ok(json!("Globex"))
        );
        assert_eq!(
            to_input(Shape::Uuid, &json!("nope"), None),
            Err(Invalid("uuid"))
        );
        assert_eq!(
            to_input(Shape::Date, &json!("2026-10-26"), None),
            Ok(json!("2026-10-26"))
        );
        assert_eq!(
            to_input(Shape::Float, &json!("x"), None),
            Err(Invalid("numeric"))
        );
        assert_eq!(
            to_input(Shape::DateTime(Instant::Utc), &json!("2026-10-26T14:30"), Some(-120)),
            Ok(json!("2026-10-26T12:30:00.000000Z"))
        );
    }

    #[test]
    fn blank_values_are_null_except_for_text() {
        assert_eq!(to_input(INT, &json!(""), None), Ok(Json::Null));
        assert_eq!(to_input(Shape::Text, &json!(""), None), Ok(json!("")));
        assert_eq!(to_input(Shape::Text, &Json::Null, None), Ok(Json::Null));
    }

    #[test]
    fn local_date_times_use_the_browser_offset() {
        // UTC+2 has `getTimezoneOffset() == -120`.
        let at = datetime(&json!("2026-10-26T14:30"), Some(-120)).unwrap();
        assert_eq!(at.to_rfc3339(), "2026-10-26T12:30:00+00:00");
        let utc = datetime(&json!("2026-10-26T14:30"), None).unwrap();
        assert_eq!(utc.to_rfc3339(), "2026-10-26T14:30:00+00:00");
        let explicit = datetime(&json!("2026-10-26T14:30:00+01:00"), Some(-120)).unwrap();
        assert_eq!(explicit.to_rfc3339(), "2026-10-26T13:30:00+00:00");
    }

    #[test]
    fn keys_parse_like_their_column() {
        let id = uuid::Uuid::now_v7();
        assert_eq!(
            key_from_str(Shape::Uuid, &id.to_string()),
            Some(Value::Uuid(id))
        );
        assert_eq!(key_from_str(Shape::Uuid, "12"), None);
        assert_eq!(
            key_from_str(
                Shape::Int {
                    bits: 64,
                    signed: true
                },
                "12"
            ),
            Some(Value::I64(12))
        );
    }

    #[test]
    fn stored_numbers_and_dates_read_back() {
        assert_eq!(value_number(&Value::I64(7)), Some(7.0));
        assert_eq!(value_number(&Value::Null(navree_orm::Kind::I64)), None);
        assert_eq!(
            value_date(&Value::String("2026-10-26 12:00:00".into())),
            NaiveDate::from_ymd_opt(2026, 10, 26)
        );
    }
}
