//! Date-times stored as text in a format of the model's choosing
//! (`cast = "datetime_format(\"...\")"`) and as Unix timestamps
//! (`cast = "unix_timestamp"`, `cast = "unix_timestamp_ms"`).

use std::fmt::Write as _;
use std::marker::PhantomData;

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use navree_kernel::BoxFuture;

use crate::attributes::CastContext;
use crate::cast::{Cast, CastError};
use crate::value::{Field, FieldKind};

/// The format of a [`DateTimeFormat`] cast. `#[derive(Model)]` writes one
/// per `cast = "datetime_format(\"...\")"` field, after checking the
/// format at compile time.
pub trait DateTimeFormatSpec: 'static {
    /// A `chrono` format string, e.g. `"%d/%m/%Y %H:%M"`. With `%z` or
    /// `%:z`, the offset is stored too.
    const FORMAT: &'static str;
}

/// A date or time type a [`DateTimeFormat`] cast can store as text:
/// `DateTime<Utc>`, `DateTime<FixedOffset>`, `NaiveDateTime`, `NaiveDate`
/// and `NaiveTime`.
pub trait FormattedDateTime: Field + Sized {
    /// Formats `self` with `format`.
    fn format_with(&self, format: &str) -> Result<String, CastError>;

    /// Parses `text` written with `format`.
    fn parse_with(text: &str, format: &str) -> Option<Self>;
}

fn format_error(format: &str) -> CastError {
    CastError::new(format!("`{format}` is not a valid date-time format"))
}

/// Writes `item` with `format` (chrono reports an invalid format as a
/// formatting error, never a panic here).
fn write_formatted(item: impl std::fmt::Display) -> Option<String> {
    let mut text = String::new();
    write!(text, "{item}").ok().map(|()| text)
}

impl FormattedDateTime for DateTime<Utc> {
    fn format_with(&self, format: &str) -> Result<String, CastError> {
        write_formatted(self.format(format)).ok_or_else(|| format_error(format))
    }

    fn parse_with(text: &str, format: &str) -> Option<Self> {
        DateTime::parse_from_str(text, format)
            .map(|at| at.to_utc())
            .ok()
            .or_else(|| NaiveDateTime::parse_from_str(text, format).ok().map(|at| at.and_utc()))
            .or_else(|| {
                NaiveDate::parse_from_str(text, format)
                    .ok()
                    .map(|day| day.and_time(NaiveTime::MIN).and_utc())
            })
    }
}

impl FormattedDateTime for DateTime<FixedOffset> {
    fn format_with(&self, format: &str) -> Result<String, CastError> {
        write_formatted(self.format(format)).ok_or_else(|| format_error(format))
    }

    fn parse_with(text: &str, format: &str) -> Option<Self> {
        DateTime::parse_from_str(text, format).ok().or_else(|| {
            <DateTime<Utc> as FormattedDateTime>::parse_with(text, format).map(|at| at.fixed_offset())
        })
    }
}

impl FormattedDateTime for NaiveDateTime {
    fn format_with(&self, format: &str) -> Result<String, CastError> {
        write_formatted(self.format(format)).ok_or_else(|| format_error(format))
    }

    fn parse_with(text: &str, format: &str) -> Option<Self> {
        NaiveDateTime::parse_from_str(text, format)
            .ok()
            .or_else(|| {
                NaiveDate::parse_from_str(text, format)
                    .ok()
                    .map(|day| day.and_time(NaiveTime::MIN))
            })
            .or_else(|| DateTime::parse_from_str(text, format).ok().map(|at| at.naive_utc()))
    }
}

impl FormattedDateTime for NaiveDate {
    fn format_with(&self, format: &str) -> Result<String, CastError> {
        write_formatted(self.format(format)).ok_or_else(|| format_error(format))
    }

    fn parse_with(text: &str, format: &str) -> Option<Self> {
        NaiveDate::parse_from_str(text, format)
            .ok()
            .or_else(|| NaiveDateTime::parse_from_str(text, format).ok().map(|at| at.date()))
    }
}

impl FormattedDateTime for NaiveTime {
    fn format_with(&self, format: &str) -> Result<String, CastError> {
        write_formatted(self.format(format)).ok_or_else(|| format_error(format))
    }

    fn parse_with(text: &str, format: &str) -> Option<Self> {
        NaiveTime::parse_from_str(text, format).ok()
    }
}

/// A date or time stored as text in format `F`
/// (`#[model(cast = "datetime_format(\"%d/%m/%Y %H:%M\")")]`), for legacy
/// columns and fixed external formats. Formats with an offset (`%z`,
/// `%:z`) keep a `DateTime<FixedOffset>`'s zone on every database.
///
/// Values are stored with the format's precision (a format without
/// seconds drops them) and the record holds what is stored once it is
/// saved. Serialized records write the value in the same format. Input to
/// `fill` may be in the format or any form the field type accepts
/// (RFC 3339 text, Unix seconds).
///
/// ```
/// use chrono::{TimeZone, Utc};
/// use navree_orm::cast::{Cast, DateTimeFormat, DateTimeFormatSpec};
///
/// struct DayMonthYear;
/// impl DateTimeFormatSpec for DayMonthYear {
///     const FORMAT: &'static str = "%d/%m/%Y %H:%M";
/// }
///
/// let at = Utc.with_ymd_and_hms(2026, 10, 27, 8, 30, 15).unwrap();
/// let stored = DateTimeFormat::<DayMonthYear>::set(&at)?;
/// assert_eq!(stored, "27/10/2026 08:30");
/// let read: chrono::DateTime<Utc> = DateTimeFormat::<DayMonthYear>::get(stored)?;
/// assert_eq!(read, Utc.with_ymd_and_hms(2026, 10, 27, 8, 30, 0).unwrap());
/// # Ok::<(), navree_orm::cast::CastError>(())
/// ```
pub struct DateTimeFormat<F>(PhantomData<fn() -> F>);

impl<F: DateTimeFormatSpec, T: FormattedDateTime> Cast<T> for DateTimeFormat<F> {
    type Stored = String;

    const KIND: FieldKind = FieldKind::Text;

    fn get(stored: String) -> Result<T, CastError> {
        T::parse_with(&stored, F::FORMAT).ok_or_else(|| {
            CastError::new(format!(
                "`{stored}` does not match the format `{}` (the format must hold enough to read a {} back)",
                F::FORMAT,
                T::type_name()
            ))
        })
    }

    fn set(value: &T) -> Result<String, CastError> {
        value.format_with(F::FORMAT)
    }

    fn from_json(input: serde_json::Value, context: &CastContext) -> Result<T, CastError> {
        let _ = context;
        if let serde_json::Value::String(text) = &input
            && let Some(value) = T::parse_with(text, F::FORMAT)
        {
            return Ok(value);
        }
        T::from_json(input).map_err(CastError::from)
    }

    fn serialize(value: &T) -> Option<Result<serde_json::Value, CastError>> {
        Some(value.format_with(F::FORMAT).map(serde_json::Value::String))
    }

    /// The record keeps the value at the precision the format stores.
    fn prepare<'a>(value: &'a mut T, context: &'a CastContext) -> BoxFuture<'a, Result<(), CastError>> {
        let _ = context;
        let result = value.format_with(F::FORMAT).map(|text| {
            if let Some(stored) = T::parse_with(&text, F::FORMAT) {
                *value = stored;
            }
        });
        Box::pin(std::future::ready(result))
    }
}

/// A date-time stored as Unix seconds in an integer column
/// (`#[model(cast = "unix_timestamp")]`), on `DateTime<Utc>` and
/// `NaiveDateTime` (read as UTC) fields. Fractions of a second are not
/// stored (the record drops them when saved). Input to `fill` may be a
/// number of seconds or a date-time text.
///
/// ```
/// use chrono::{TimeZone, Utc};
/// use navree_orm::cast::{Cast, UnixTimestamp};
///
/// let at = Utc.with_ymd_and_hms(2026, 10, 27, 8, 30, 0).unwrap();
/// assert_eq!(UnixTimestamp::set(&at)?, 1_793_089_800);
/// let read: chrono::DateTime<Utc> = UnixTimestamp::get(1_793_089_800)?;
/// assert_eq!(read, at);
/// # Ok::<(), navree_orm::cast::CastError>(())
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct UnixTimestamp;

/// A date-time stored as Unix milliseconds in an integer column
/// (`#[model(cast = "unix_timestamp_ms")]`), on `DateTime<Utc>` and
/// `NaiveDateTime` fields; finer fractions are dropped.
#[derive(Clone, Copy, Debug, Default)]
pub struct UnixTimestampMs;

fn out_of_range(value: i64, unit: &str) -> CastError {
    CastError::new(format!("{value} {unit} is out of the range of date-times"))
}

/// Unix `seconds` (or milliseconds) from JSON input: a number, a numeric
/// string, or a date-time text.
fn timestamp_input(input: serde_json::Value, millis: bool) -> Result<DateTime<Utc>, CastError> {
    let number = match &input {
        serde_json::Value::Number(n) => n.as_i64(),
        serde_json::Value::String(s) => s.trim().parse::<i64>().ok(),
        _ => None,
    };
    match number {
        Some(n) if millis => DateTime::from_timestamp_millis(n).ok_or_else(|| out_of_range(n, "milliseconds")),
        Some(n) => DateTime::from_timestamp(n, 0).ok_or_else(|| out_of_range(n, "seconds")),
        None => DateTime::<Utc>::from_json(input).map_err(CastError::from),
    }
}

/// `at` without the fractions the cast does not store.
fn truncate(at: DateTime<Utc>, millis: bool) -> DateTime<Utc> {
    if millis {
        DateTime::from_timestamp_millis(at.timestamp_millis()).unwrap_or(at)
    } else {
        DateTime::from_timestamp(at.timestamp(), 0).unwrap_or(at)
    }
}

macro_rules! unix_casts {
    ($cast:ty, $millis:expr, $unit:literal, $to:ident, $from:expr) => {
        impl Cast<DateTime<Utc>> for $cast {
            type Stored = i64;

            const KIND: FieldKind = FieldKind::DateTime;

            fn get(stored: i64) -> Result<DateTime<Utc>, CastError> {
                ($from)(stored).ok_or_else(|| out_of_range(stored, $unit))
            }

            fn set(value: &DateTime<Utc>) -> Result<i64, CastError> {
                Ok(value.$to())
            }

            fn from_json(input: serde_json::Value, context: &CastContext) -> Result<DateTime<Utc>, CastError> {
                let _ = context;
                timestamp_input(input, $millis)
            }

            fn prepare<'a>(
                value: &'a mut DateTime<Utc>,
                context: &'a CastContext,
            ) -> BoxFuture<'a, Result<(), CastError>> {
                let _ = context;
                *value = truncate(*value, $millis);
                Box::pin(std::future::ready(Ok(())))
            }
        }

        impl Cast<NaiveDateTime> for $cast {
            type Stored = i64;

            const KIND: FieldKind = FieldKind::DateTime;

            fn get(stored: i64) -> Result<NaiveDateTime, CastError> {
                ($from)(stored)
                    .map(|at| at.naive_utc())
                    .ok_or_else(|| out_of_range(stored, $unit))
            }

            fn set(value: &NaiveDateTime) -> Result<i64, CastError> {
                Ok(value.and_utc().$to())
            }

            fn from_json(input: serde_json::Value, context: &CastContext) -> Result<NaiveDateTime, CastError> {
                let _ = context;
                timestamp_input(input, $millis).map(|at| at.naive_utc())
            }

            fn prepare<'a>(
                value: &'a mut NaiveDateTime,
                context: &'a CastContext,
            ) -> BoxFuture<'a, Result<(), CastError>> {
                let _ = context;
                *value = truncate(value.and_utc(), $millis).naive_utc();
                Box::pin(std::future::ready(Ok(())))
            }
        }
    };
}

unix_casts!(UnixTimestamp, false, "seconds", timestamp, |s| DateTime::from_timestamp(s, 0));
unix_casts!(UnixTimestampMs, true, "milliseconds", timestamp_millis, DateTime::from_timestamp_millis);

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use serde_json::json;

    use super::*;

    struct Iso;
    impl DateTimeFormatSpec for Iso {
        const FORMAT: &'static str = "%Y-%m-%dT%H:%M:%S%:z";
    }

    struct Day;
    impl DateTimeFormatSpec for Day {
        const FORMAT: &'static str = "%d.%m.%Y";
    }

    #[test]
    fn offsets_survive_a_format_that_stores_them() {
        let at = FixedOffset::east_opt(5 * 3600 + 1800)
            .unwrap()
            .with_ymd_and_hms(2026, 10, 27, 14, 0, 0)
            .unwrap();
        let stored = DateTimeFormat::<Iso>::set(&at).unwrap();
        assert_eq!(stored, "2026-10-27T14:00:00+05:30");
        let read: DateTime<FixedOffset> = DateTimeFormat::<Iso>::get(stored).unwrap();
        assert_eq!(read, at);
        assert_eq!(read.offset().local_minus_utc(), 19_800);
    }

    #[test]
    fn date_only_formats_read_midnight_and_reject_other_text() {
        let read: DateTime<Utc> = DateTimeFormat::<Day>::get("27.10.2026".into()).unwrap();
        assert_eq!(read, Utc.with_ymd_and_hms(2026, 10, 27, 0, 0, 0).unwrap());
        let day: NaiveDate = DateTimeFormat::<Day>::get("01.02.2026".into()).unwrap();
        assert_eq!(day, NaiveDate::from_ymd_opt(2026, 2, 1).unwrap());
        let error = <DateTimeFormat<Day> as Cast<NaiveDate>>::get("2026-02-01".into()).unwrap_err();
        assert_eq!(
            error.to_string(),
            "`2026-02-01` does not match the format `%d.%m.%Y` (the format must hold enough to read a NaiveDate back)"
        );
    }

    #[test]
    fn input_in_the_format_or_any_date_time_form() {
        let context = CastContext::new();
        let expected = Utc.with_ymd_and_hms(2026, 10, 27, 0, 0, 0).unwrap();
        let from_format: DateTime<Utc> =
            DateTimeFormat::<Day>::from_json(json!("27.10.2026"), &context).unwrap();
        assert_eq!(from_format, expected);
        let from_rfc: DateTime<Utc> =
            DateTimeFormat::<Day>::from_json(json!("2026-10-27T00:00:00Z"), &context).unwrap();
        assert_eq!(from_rfc, expected);
        assert_eq!(
            <DateTimeFormat<Day> as Cast<DateTime<Utc>>>::serialize(&expected)
                .unwrap()
                .unwrap(),
            json!("27.10.2026")
        );
    }

    #[test]
    fn invalid_formats_are_errors_not_panics() {
        struct Broken;
        impl DateTimeFormatSpec for Broken {
            const FORMAT: &'static str = "%Q";
        }
        let at = Utc.with_ymd_and_hms(2026, 10, 27, 0, 0, 0).unwrap();
        let error = DateTimeFormat::<Broken>::set(&at).unwrap_err();
        assert_eq!(error.to_string(), "`%Q` is not a valid date-time format");
    }

    #[test]
    fn unix_timestamps_in_seconds_and_milliseconds() {
        let at = Utc.with_ymd_and_hms(2026, 10, 27, 8, 30, 0).unwrap();
        assert_eq!(UnixTimestamp::set(&at).unwrap(), 1_793_089_800);
        assert_eq!(UnixTimestampMs::set(&at).unwrap(), 1_793_089_800_000);
        let read: DateTime<Utc> = UnixTimestampMs::get(1_793_089_800_123).unwrap();
        assert_eq!(read.timestamp_subsec_millis(), 123);
        let naive: NaiveDateTime = UnixTimestamp::get(0).unwrap();
        assert_eq!(naive, DateTime::<Utc>::UNIX_EPOCH.naive_utc());
        let error = <UnixTimestampMs as Cast<DateTime<Utc>>>::get(i64::MAX).unwrap_err();
        assert!(error.to_string().contains("out of the range"), "{error}");

        let context = CastContext::new();
        for input in [json!(1_793_089_800), json!("1793089800"), json!("2026-10-27T08:30:00Z")] {
            let read: DateTime<Utc> = UnixTimestamp::from_json(input.clone(), &context).unwrap();
            assert_eq!(read, at, "{input}");
        }
        assert!(<UnixTimestamp as Cast<DateTime<Utc>>>::from_json(json!(true), &context).is_err());
    }

    #[tokio::test]
    async fn preparation_drops_the_fractions_that_are_not_stored() {
        let mut at = Utc.with_ymd_and_hms(2026, 10, 27, 8, 30, 0).unwrap() + chrono::Duration::milliseconds(1_234);
        UnixTimestampMs::prepare(&mut at, &CastContext::new()).await.unwrap();
        assert_eq!(at.timestamp_subsec_millis(), 234);
        UnixTimestamp::prepare(&mut at, &CastContext::new()).await.unwrap();
        assert_eq!(at.timestamp_subsec_nanos(), 0);
    }
}
