//! Date-times stored as text in a format of the model's choosing
//! (`cast = "datetime_format(\"...\")"`) and as Unix timestamps
//! (`cast = "unix_timestamp"`, `cast = "unix_timestamp_ms"`).

use std::marker::PhantomData;

use navree_datetime::{Date, DateTime, Offset, Time, Timestamp, Zone, Zoned};
use navree_kernel::BoxFuture;

use crate::attributes::CastContext;
use crate::cast::{Cast, CastError};
use crate::value::{Field, FieldKind};

/// The format of a [`DateTimeFormat`] cast. `#[derive(Model)]` writes one
/// per `cast = "datetime_format(\"...\")"` field, after checking the
/// format at compile time.
pub trait DateTimeFormatSpec: 'static {
    /// A `strftime` pattern, e.g. `"%d/%m/%Y %H:%M"`. With `%z` or `%:z`,
    /// the offset is stored too.
    const FORMAT: &'static str;
}

/// A date or time type a [`DateTimeFormat`] cast can store as text:
/// [`Timestamp`], [`Zoned`], [`DateTime`], [`Date`] and [`Time`].
pub trait FormattedDateTime: Field + Sized {
    /// Formats `self` with `format`.
    fn format_with(&self, format: &str) -> Result<String, CastError>;

    /// Parses `text` written with `format`.
    fn parse_with(text: &str, format: &str) -> Option<Self>;
}

fn format_error(format: &str) -> CastError {
    CastError::new(format!("`{format}` is not a valid date-time format"))
}

impl FormattedDateTime for Timestamp {
    fn format_with(&self, format: &str) -> Result<String, CastError> {
        self.format(format)
            .to_text()
            .map_err(|_| format_error(format))
    }

    fn parse_with(text: &str, format: &str) -> Option<Self> {
        Zoned::parse(format, text)
            .ok()
            .map(|at| at.timestamp())
            .or_else(|| Timestamp::parse(format, text).ok())
            .or_else(|| DateTime::parse(format, text).ok().map(|at| at.in_utc()))
            .or_else(|| {
                Date::parse(format, text)
                    .ok()
                    .map(|day| day.at(Time::MIDNIGHT).in_utc())
            })
    }
}

impl FormattedDateTime for Zoned {
    fn format_with(&self, format: &str) -> Result<String, CastError> {
        self.format(format)
            .to_text()
            .map_err(|_| format_error(format))
    }

    fn parse_with(text: &str, format: &str) -> Option<Self> {
        Zoned::parse(format, text).ok().or_else(|| {
            <Timestamp as FormattedDateTime>::parse_with(text, format)
                .map(|at| at.in_zone(Zone::fixed(Offset::UTC)))
        })
    }
}

impl FormattedDateTime for DateTime {
    fn format_with(&self, format: &str) -> Result<String, CastError> {
        self.format(format)
            .to_text()
            .map_err(|_| format_error(format))
    }

    fn parse_with(text: &str, format: &str) -> Option<Self> {
        DateTime::parse(format, text)
            .ok()
            .or_else(|| {
                Date::parse(format, text)
                    .ok()
                    .map(|day| day.at(Time::MIDNIGHT))
            })
            .or_else(|| {
                Zoned::parse(format, text)
                    .ok()
                    .map(|at| at.timestamp().date_time_utc())
            })
    }
}

impl FormattedDateTime for Date {
    fn format_with(&self, format: &str) -> Result<String, CastError> {
        self.format(format)
            .to_text()
            .map_err(|_| format_error(format))
    }

    fn parse_with(text: &str, format: &str) -> Option<Self> {
        Date::parse(format, text)
            .ok()
            .or_else(|| DateTime::parse(format, text).ok().map(|at| at.date()))
    }
}

impl FormattedDateTime for Time {
    fn format_with(&self, format: &str) -> Result<String, CastError> {
        self.format(format)
            .to_text()
            .map_err(|_| format_error(format))
    }

    fn parse_with(text: &str, format: &str) -> Option<Self> {
        Time::parse(format, text).ok()
    }
}

/// A date or time stored as text in format `F`
/// (`#[model(cast = "datetime_format(\"%d/%m/%Y %H:%M\")")]`), for legacy
/// columns and fixed external formats. Formats with an offset (`%z`,
/// `%:z`) keep a [`Zoned`]'s offset on every database.
///
/// Values are stored with the format's precision (a format without
/// seconds drops them) and the record holds what is stored once it is
/// saved. Serialized records write the value in the same format. Input to
/// `fill` may be in the format or any form the field type accepts
/// (RFC 3339 text, Unix seconds).
///
/// ```
/// use navree_datetime::Timestamp;
/// use navree_orm::cast::{Cast, DateTimeFormat, DateTimeFormatSpec};
///
/// struct DayMonthYear;
/// impl DateTimeFormatSpec for DayMonthYear {
///     const FORMAT: &'static str = "%d/%m/%Y %H:%M";
/// }
///
/// let at = Timestamp::utc(2026, 10, 27, 8, 30, 15).unwrap();
/// let stored = DateTimeFormat::<DayMonthYear>::set(&at)?;
/// assert_eq!(stored, "27/10/2026 08:30");
/// let read: Timestamp = DateTimeFormat::<DayMonthYear>::get(stored)?;
/// assert_eq!(read, Timestamp::utc(2026, 10, 27, 8, 30, 0).unwrap());
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
    fn prepare<'a>(
        value: &'a mut T,
        context: &'a CastContext,
    ) -> BoxFuture<'a, Result<(), CastError>> {
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
/// (`#[model(cast = "unix_timestamp")]`), on [`Timestamp`] and
/// [`DateTime`] (read as UTC) fields. Fractions of a second are not
/// stored (the record drops them when saved). Input to `fill` may be a
/// number of seconds or a date-time text.
///
/// ```
/// use navree_datetime::Timestamp;
/// use navree_orm::cast::{Cast, UnixTimestamp};
///
/// let at = Timestamp::utc(2026, 10, 27, 8, 30, 0).unwrap();
/// assert_eq!(UnixTimestamp::set(&at)?, 1_793_089_800);
/// let read: Timestamp = UnixTimestamp::get(1_793_089_800)?;
/// assert_eq!(read, at);
/// # Ok::<(), navree_orm::cast::CastError>(())
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct UnixTimestamp;

/// A date-time stored as Unix milliseconds in an integer column
/// (`#[model(cast = "unix_timestamp_ms")]`), on [`Timestamp`] and
/// [`DateTime`] fields; finer fractions are dropped.
#[derive(Clone, Copy, Debug, Default)]
pub struct UnixTimestampMs;

fn out_of_range(value: i64, unit: &str) -> CastError {
    CastError::new(format!("{value} {unit} is out of the range of date-times"))
}

/// Unix `seconds` (or milliseconds) from JSON input: a number, a numeric
/// string, or a date-time text.
fn timestamp_input(input: serde_json::Value, millis: bool) -> Result<Timestamp, CastError> {
    let number = match &input {
        serde_json::Value::Number(n) => n.as_i64(),
        serde_json::Value::String(s) => s.trim().parse::<i64>().ok(),
        _ => None,
    };
    match number {
        Some(n) if millis => Timestamp::from_millis(n)
            .ok()
            .ok_or_else(|| out_of_range(n, "milliseconds")),
        Some(n) => Timestamp::from_secs(n)
            .ok()
            .ok_or_else(|| out_of_range(n, "seconds")),
        None => Timestamp::from_json(input).map_err(CastError::from),
    }
}

/// `at` without the fractions the cast does not store.
fn truncate(at: Timestamp, millis: bool) -> Timestamp {
    if millis {
        Timestamp::from_millis(at.as_millis()).ok().unwrap_or(at)
    } else {
        Timestamp::from_secs(at.as_secs()).ok().unwrap_or(at)
    }
}

macro_rules! unix_casts {
    ($cast:ty, $millis:expr, $unit:literal, $to:ident, $from:expr) => {
        impl Cast<Timestamp> for $cast {
            type Stored = i64;

            const KIND: FieldKind = FieldKind::DateTime;

            fn get(stored: i64) -> Result<Timestamp, CastError> {
                ($from)(stored).ok_or_else(|| out_of_range(stored, $unit))
            }

            fn set(value: &Timestamp) -> Result<i64, CastError> {
                Ok(value.$to())
            }

            fn from_json(
                input: serde_json::Value,
                context: &CastContext,
            ) -> Result<Timestamp, CastError> {
                let _ = context;
                timestamp_input(input, $millis)
            }

            fn prepare<'a>(
                value: &'a mut Timestamp,
                context: &'a CastContext,
            ) -> BoxFuture<'a, Result<(), CastError>> {
                let _ = context;
                *value = truncate(*value, $millis);
                Box::pin(std::future::ready(Ok(())))
            }
        }

        impl Cast<DateTime> for $cast {
            type Stored = i64;

            const KIND: FieldKind = FieldKind::DateTime;

            fn get(stored: i64) -> Result<DateTime, CastError> {
                ($from)(stored)
                    .map(|at: Timestamp| at.date_time_utc())
                    .ok_or_else(|| out_of_range(stored, $unit))
            }

            fn set(value: &DateTime) -> Result<i64, CastError> {
                Ok(value.in_utc().$to())
            }

            fn from_json(
                input: serde_json::Value,
                context: &CastContext,
            ) -> Result<DateTime, CastError> {
                let _ = context;
                timestamp_input(input, $millis).map(|at| at.date_time_utc())
            }

            fn prepare<'a>(
                value: &'a mut DateTime,
                context: &'a CastContext,
            ) -> BoxFuture<'a, Result<(), CastError>> {
                let _ = context;
                *value = truncate(value.in_utc(), $millis).date_time_utc();
                Box::pin(std::future::ready(Ok(())))
            }
        }
    };
}

unix_casts!(UnixTimestamp, false, "seconds", as_secs, |s| {
    Timestamp::from_secs(s).ok()
});
unix_casts!(UnixTimestampMs, true, "milliseconds", as_millis, |s| {
    Timestamp::from_millis(s).ok()
});

#[cfg(test)]
mod tests {
    use navree_datetime::{Date, DateTime, Timestamp, Zoned};
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
        let at = Zoned::parse_rfc3339("2026-10-27T14:00:00+05:30").unwrap();
        let stored = DateTimeFormat::<Iso>::set(&at).unwrap();
        assert_eq!(stored, "2026-10-27T14:00:00+05:30");
        let read: Zoned = DateTimeFormat::<Iso>::get(stored).unwrap();
        assert_eq!(read, at);
        assert_eq!(read.offset().seconds(), 19_800);
    }

    #[test]
    fn date_only_formats_read_midnight_and_reject_other_text() {
        let read: Timestamp = DateTimeFormat::<Day>::get("27.10.2026".into()).unwrap();
        assert_eq!(read, Timestamp::utc(2026, 10, 27, 0, 0, 0).unwrap());
        let day: Date = DateTimeFormat::<Day>::get("01.02.2026".into()).unwrap();
        assert_eq!(day, Date::new(2026, 2, 1).unwrap());
        let error = <DateTimeFormat<Day> as Cast<Date>>::get("2026-02-01".into()).unwrap_err();
        assert_eq!(
            error.to_string(),
            "`2026-02-01` does not match the format `%d.%m.%Y` (the format must hold enough to read a Date back)"
        );
    }

    #[test]
    fn input_in_the_format_or_any_date_time_form() {
        let context = CastContext::new();
        let expected = Timestamp::utc(2026, 10, 27, 0, 0, 0).unwrap();
        let from_format: Timestamp =
            DateTimeFormat::<Day>::from_json(json!("27.10.2026"), &context).unwrap();
        assert_eq!(from_format, expected);
        let from_rfc: Timestamp =
            DateTimeFormat::<Day>::from_json(json!("2026-10-27T00:00:00Z"), &context).unwrap();
        assert_eq!(from_rfc, expected);
        assert_eq!(
            <DateTimeFormat<Day> as Cast<Timestamp>>::serialize(&expected)
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
        let at = Timestamp::utc(2026, 10, 27, 0, 0, 0).unwrap();
        let error = DateTimeFormat::<Broken>::set(&at).unwrap_err();
        assert_eq!(error.to_string(), "`%Q` is not a valid date-time format");
    }

    #[test]
    fn unix_timestamps_in_seconds_and_milliseconds() {
        let at = Timestamp::utc(2026, 10, 27, 8, 30, 0).unwrap();
        assert_eq!(UnixTimestamp::set(&at).unwrap(), 1_793_089_800);
        assert_eq!(UnixTimestampMs::set(&at).unwrap(), 1_793_089_800_000);
        let read: Timestamp = UnixTimestampMs::get(1_793_089_800_123).unwrap();
        assert_eq!(read.subsec_nanos() / 1_000_000, 123);
        let naive: DateTime = UnixTimestamp::get(0).unwrap();
        assert_eq!(naive, Timestamp::UNIX_EPOCH.date_time_utc());
        let error = <UnixTimestampMs as Cast<Timestamp>>::get(i64::MAX).unwrap_err();
        assert!(error.to_string().contains("out of the range"), "{error}");

        let context = CastContext::new();
        for input in [
            json!(1_793_089_800),
            json!("1793089800"),
            json!("2026-10-27T08:30:00Z"),
        ] {
            let read: Timestamp = UnixTimestamp::from_json(input.clone(), &context).unwrap();
            assert_eq!(read, at, "{input}");
        }
        assert!(<UnixTimestamp as Cast<Timestamp>>::from_json(json!(true), &context).is_err());
    }

    #[tokio::test]
    async fn preparation_drops_the_fractions_that_are_not_stored() {
        let mut at =
            Timestamp::utc(2026, 10, 27, 8, 30, 0).unwrap() + navree_datetime::Span::millis(1_234);
        UnixTimestampMs::prepare(&mut at, &CastContext::new())
            .await
            .unwrap();
        assert_eq!(at.subsec_nanos() / 1_000_000, 234);
        UnixTimestamp::prepare(&mut at, &CastContext::new())
            .await
            .unwrap();
        assert_eq!(at.subsec_nanos(), 0);
    }
}
