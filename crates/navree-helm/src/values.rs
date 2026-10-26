//! Columns and values: finding an entity's columns by name, and converting
//! submitted JSON into the `sea_orm::Value` variant a column holds.

use std::collections::HashMap;

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use sea_orm::sea_query::Value as SqlValue;
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, ColumnType, EntityTrait, IdenStatic, Iterable,
    PrimaryKeyToColumn,
};
use serde_json::Value;

/// The column of `E` named `name`.
pub(crate) fn column<E: EntityTrait>(name: &str) -> Option<E::Column> {
    E::Column::iter().find(|column| column.as_str() == name)
}

/// The names of `E`'s columns, in declaration order.
pub(crate) fn column_names<E: EntityTrait>() -> Vec<&'static str> {
    E::Column::iter().map(|column| column.as_str()).collect()
}

/// `E`'s single-column primary key.
pub(crate) fn primary_key<E: EntityTrait>() -> Result<E::Column, String> {
    let mut keys = E::PrimaryKey::iter();
    match (keys.next(), keys.next()) {
        (Some(key), None) => Ok(key.into_column()),
        _ => Err(format!(
            "Helm needs a single-column primary key on `{}`",
            E::default().table_name()
        )),
    }
}

/// The empty value of each column of `E`, in the variant the column's Rust
/// type converts from: what [`ActiveModelTrait::default_values`] sets, or a
/// variant derived from the column type for types without a `Default`
/// (string-backed enums).
pub(crate) fn templates<E: EntityTrait>() -> HashMap<&'static str, SqlValue> {
    let defaults = <E::ActiveModel as ActiveModelTrait>::default_values();
    E::Column::iter()
        .map(|column| {
            let template = match defaults.get(column) {
                ActiveValue::Set(value) | ActiveValue::Unchanged(value) => value.as_null(),
                ActiveValue::NotSet => from_column_type(column.def().get_column_type()),
            };
            (column.as_str(), template)
        })
        .collect()
}

/// The empty value of a column type, for columns whose Rust type has no
/// `Default`.
fn from_column_type(column_type: &ColumnType) -> SqlValue {
    match column_type {
        ColumnType::Boolean => SqlValue::Bool(None),
        ColumnType::TinyInteger => SqlValue::TinyInt(None),
        ColumnType::SmallInteger => SqlValue::SmallInt(None),
        ColumnType::Integer => SqlValue::Int(None),
        ColumnType::BigInteger => SqlValue::BigInt(None),
        ColumnType::TinyUnsigned => SqlValue::TinyUnsigned(None),
        ColumnType::SmallUnsigned => SqlValue::SmallUnsigned(None),
        ColumnType::Unsigned => SqlValue::Unsigned(None),
        ColumnType::BigUnsigned => SqlValue::BigUnsigned(None),
        ColumnType::Float => SqlValue::Float(None),
        ColumnType::Double => SqlValue::Double(None),
        ColumnType::Uuid => SqlValue::Uuid(None),
        ColumnType::Date => SqlValue::ChronoDate(None),
        ColumnType::Time => SqlValue::ChronoTime(None),
        ColumnType::DateTime | ColumnType::Timestamp => SqlValue::ChronoDateTime(None),
        ColumnType::TimestampWithTimeZone => SqlValue::ChronoDateTimeUtc(None),
        ColumnType::Json | ColumnType::JsonBinary => SqlValue::Json(None),
        _ => SqlValue::String(None),
    }
}

/// Why a submitted value cannot be stored: a validation rule name for
/// navree-validation's messages (`integer`, `boolean`, `date`, ...).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Invalid(pub(crate) &'static str);

/// Whether `value` counts as empty: `null`, or a string with nothing but
/// whitespace.
pub(crate) fn is_blank(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(text) => text.trim().is_empty(),
        _ => false,
    }
}

/// Converts `input` into the variant of `template` (see [`templates`]).
/// A blank input is the variant's `NULL`. `tz_offset_minutes` is the
/// browser's offset from UTC (`Date.getTimezoneOffset()`), applied to
/// date-times without an offset (`datetime-local` inputs).
pub(crate) fn to_sql(
    template: &SqlValue,
    input: &Value,
    tz_offset_minutes: Option<i32>,
) -> Result<SqlValue, Invalid> {
    if is_blank(input) && !matches!(template, SqlValue::String(_) | SqlValue::Json(_)) {
        return Ok(template.as_null());
    }
    if input.is_null() {
        return Ok(template.as_null());
    }
    let value = match template {
        SqlValue::Bool(_) => SqlValue::Bool(Some(boolean(input).ok_or(Invalid("boolean"))?)),
        SqlValue::TinyInt(_) => SqlValue::TinyInt(Some(int(input)?)),
        SqlValue::SmallInt(_) => SqlValue::SmallInt(Some(int(input)?)),
        SqlValue::Int(_) => SqlValue::Int(Some(int(input)?)),
        SqlValue::BigInt(_) => SqlValue::BigInt(Some(int(input)?)),
        SqlValue::TinyUnsigned(_) => SqlValue::TinyUnsigned(Some(int(input)?)),
        SqlValue::SmallUnsigned(_) => SqlValue::SmallUnsigned(Some(int(input)?)),
        SqlValue::Unsigned(_) => SqlValue::Unsigned(Some(int(input)?)),
        SqlValue::BigUnsigned(_) => SqlValue::BigUnsigned(Some(int(input)?)),
        SqlValue::Float(_) => SqlValue::Float(Some(number(input)? as f32)),
        SqlValue::Double(_) => SqlValue::Double(Some(number(input)?)),
        SqlValue::String(_) => SqlValue::String(Some(text(input).ok_or(Invalid("string"))?)),
        SqlValue::Char(_) => {
            let text = text(input).ok_or(Invalid("string"))?;
            let mut chars = text.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => SqlValue::Char(Some(c)),
                _ => return Err(Invalid("string")),
            }
        }
        SqlValue::Uuid(_) => SqlValue::Uuid(Some(
            input
                .as_str()
                .and_then(|s| uuid::Uuid::parse_str(s.trim()).ok())
                .ok_or(Invalid("uuid"))?,
        )),
        SqlValue::ChronoDate(_) => SqlValue::ChronoDate(Some(date(input).ok_or(Invalid("date"))?)),
        SqlValue::ChronoTime(_) => SqlValue::ChronoTime(Some(
            input
                .as_str()
                .and_then(|s| {
                    NaiveTime::parse_from_str(s.trim(), "%H:%M:%S")
                        .or_else(|_| NaiveTime::parse_from_str(s.trim(), "%H:%M"))
                        .ok()
                })
                .ok_or(Invalid("date"))?,
        )),
        SqlValue::ChronoDateTime(_) => SqlValue::ChronoDateTime(Some(
            datetime(input, tz_offset_minutes)
                .ok_or(Invalid("date"))?
                .naive_utc(),
        )),
        SqlValue::ChronoDateTimeUtc(_) => SqlValue::ChronoDateTimeUtc(Some(
            datetime(input, tz_offset_minutes).ok_or(Invalid("date"))?,
        )),
        SqlValue::ChronoDateTimeWithTimeZone(_) => SqlValue::ChronoDateTimeWithTimeZone(Some(
            datetime(input, tz_offset_minutes)
                .ok_or(Invalid("date"))?
                .fixed_offset(),
        )),
        SqlValue::Json(_) => SqlValue::Json(Some(Box::new(input.clone()))),
        _ => return Err(Invalid("invalid")),
    };
    Ok(value)
}

/// A boolean from JSON or a form field (`true`, `false`, `on`, `1`, `0`).
pub(crate) fn boolean(input: &Value) -> Option<bool> {
    match input {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => match n.as_i64() {
            Some(1) => Some(true),
            Some(0) => Some(false),
            _ => None,
        },
        Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
            "true" | "on" | "1" | "yes" => Some(true),
            "false" | "off" | "0" | "no" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn int<T: TryFrom<i64> + std::str::FromStr>(input: &Value) -> Result<T, Invalid> {
    match input {
        Value::Number(n) => n
            .as_i64()
            .and_then(|n| T::try_from(n).ok())
            .ok_or(Invalid("integer")),
        Value::String(s) => s.trim().parse::<T>().map_err(|_| Invalid("integer")),
        _ => Err(Invalid("integer")),
    }
}

/// A number from JSON or a form field.
pub(crate) fn number(input: &Value) -> Result<f64, Invalid> {
    match input {
        Value::Number(n) => n.as_f64().ok_or(Invalid("numeric")),
        Value::String(s) => s
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite())
            .ok_or(Invalid("numeric")),
        _ => Err(Invalid("numeric")),
    }
}

fn text(input: &Value) -> Option<String> {
    match input {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// A `YYYY-MM-DD` date (or the date part of an RFC 3339 date-time).
pub(crate) fn date(input: &Value) -> Option<NaiveDate> {
    let s = input.as_str()?.trim();
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok().or_else(|| {
        DateTime::parse_from_rfc3339(s)
            .ok()
            .map(|dt| dt.date_naive())
    })
}

/// An RFC 3339 date-time, or a `YYYY-MM-DDTHH:MM[:SS]` local time at
/// `tz_offset_minutes` (UTC when unknown), or a date at midnight.
pub(crate) fn datetime(input: &Value, tz_offset_minutes: Option<i32>) -> Option<DateTime<Utc>> {
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
pub(crate) fn sql_date(value: &SqlValue) -> Option<NaiveDate> {
    match value {
        SqlValue::ChronoDate(Some(date)) => Some(*date),
        SqlValue::ChronoDateTime(Some(dt)) => Some(dt.date()),
        SqlValue::ChronoDateTimeUtc(Some(dt)) => Some(dt.date_naive()),
        SqlValue::ChronoDateTimeLocal(Some(dt)) => Some(dt.with_timezone(&Utc).date_naive()),
        SqlValue::ChronoDateTimeWithTimeZone(Some(dt)) => Some(dt.with_timezone(&Utc).date_naive()),
        SqlValue::String(Some(s)) => date(&Value::String(s.clone())),
        _ => None,
    }
}

/// A stored number as `f64` (for metrics computed in Rust).
pub(crate) fn sql_number(value: &SqlValue) -> Option<f64> {
    match value {
        SqlValue::TinyInt(Some(n)) => Some(f64::from(*n)),
        SqlValue::SmallInt(Some(n)) => Some(f64::from(*n)),
        SqlValue::Int(Some(n)) => Some(f64::from(*n)),
        SqlValue::BigInt(Some(n)) => Some(*n as f64),
        SqlValue::TinyUnsigned(Some(n)) => Some(f64::from(*n)),
        SqlValue::SmallUnsigned(Some(n)) => Some(f64::from(*n)),
        SqlValue::Unsigned(Some(n)) => Some(f64::from(*n)),
        SqlValue::BigUnsigned(Some(n)) => Some(*n as f64),
        SqlValue::Float(Some(n)) => Some(f64::from(*n)),
        SqlValue::Double(Some(n)) => Some(*n),
        _ => None,
    }
}

/// A bound for comparing a date or date-time column with the instant `at`,
/// typed like the column.
pub(crate) fn date_bound(template: &SqlValue, at: DateTime<Utc>) -> SqlValue {
    match template {
        SqlValue::ChronoDate(_) => SqlValue::ChronoDate(Some(at.date_naive())),
        SqlValue::ChronoDateTime(_) => SqlValue::ChronoDateTime(Some(at.naive_utc())),
        SqlValue::ChronoDateTimeWithTimeZone(_) => {
            SqlValue::ChronoDateTimeWithTimeZone(Some(at.fixed_offset()))
        }
        SqlValue::String(_) => SqlValue::String(Some(at.to_rfc3339())),
        _ => SqlValue::ChronoDateTimeUtc(Some(at)),
    }
}

/// Whether the column holds dates or date-times.
pub(crate) fn is_temporal(template: &SqlValue) -> bool {
    matches!(
        template,
        SqlValue::ChronoDate(_)
            | SqlValue::ChronoDateTime(_)
            | SqlValue::ChronoDateTimeUtc(_)
            | SqlValue::ChronoDateTimeLocal(_)
            | SqlValue::ChronoDateTimeWithTimeZone(_)
    )
}

/// The value of a primary key or foreign key given as text (a path
/// parameter or a form value), typed like the column; `None` when it cannot
/// be one.
pub(crate) fn key_from_str(template: &SqlValue, id: &str) -> Option<SqlValue> {
    to_sql(template, &Value::String(id.to_owned()), None)
        .ok()
        .filter(|value| !is_sql_null(value))
}

/// Whether a value is SQL `NULL`.
pub(crate) fn is_sql_null(value: &SqlValue) -> bool {
    *value == value.as_null()
}

/// A JSON value as the text of a key: strings as they are, other scalars
/// formatted.
pub(crate) fn key_text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn values_convert_into_the_template_variant() {
        assert_eq!(
            to_sql(&SqlValue::Int(None), &json!("42"), None),
            Ok(SqlValue::Int(Some(42)))
        );
        assert_eq!(
            to_sql(&SqlValue::BigInt(None), &json!(1_250_000), None),
            Ok(SqlValue::BigInt(Some(1_250_000)))
        );
        assert_eq!(
            to_sql(&SqlValue::TinyInt(None), &json!(1000), None),
            Err(Invalid("integer"))
        );
        assert_eq!(
            to_sql(&SqlValue::Bool(None), &json!("on"), None),
            Ok(SqlValue::Bool(Some(true)))
        );
        assert_eq!(
            to_sql(&SqlValue::Bool(None), &json!("maybe"), None),
            Err(Invalid("boolean"))
        );
        assert_eq!(
            to_sql(&SqlValue::String(None), &json!("Globex"), None),
            Ok(SqlValue::String(Some("Globex".into())))
        );
        assert_eq!(
            to_sql(&SqlValue::Uuid(None), &json!("nope"), None),
            Err(Invalid("uuid"))
        );
        assert_eq!(
            to_sql(&SqlValue::ChronoDate(None), &json!("2026-10-26"), None),
            Ok(SqlValue::ChronoDate(NaiveDate::from_ymd_opt(2026, 10, 26)))
        );
        assert_eq!(
            to_sql(&SqlValue::Double(None), &json!("x"), None),
            Err(Invalid("numeric"))
        );
    }

    #[test]
    fn blank_values_are_null_except_for_text() {
        assert_eq!(
            to_sql(&SqlValue::Int(None), &json!(""), None),
            Ok(SqlValue::Int(None))
        );
        assert_eq!(
            to_sql(&SqlValue::String(None), &json!(""), None),
            Ok(SqlValue::String(Some(String::new())))
        );
        assert_eq!(
            to_sql(&SqlValue::String(None), &Value::Null, None),
            Ok(SqlValue::String(None))
        );
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
            key_from_str(&SqlValue::Uuid(None), &id.to_string()),
            Some(SqlValue::Uuid(Some(id)))
        );
        assert_eq!(key_from_str(&SqlValue::Uuid(None), "12"), None);
        assert_eq!(
            key_from_str(&SqlValue::BigInt(None), "12"),
            Some(SqlValue::BigInt(Some(12)))
        );
    }
}
