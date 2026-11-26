//! RFC 3339: `2026-11-25T12:00:00Z`, the form instants are sent in.

use std::fmt::{self, Write};

use super::display::{fraction, two_digits};
use super::format::{OffsetPrecision, write_offset};
use super::parsed::offset_within_a_day;
use super::scan::{self, Reason, Scan};
use crate::{Date, DateTime, Offset, Result, Time, Timestamp, Zone, Zoned};

/// How much of the fraction of a second RFC 3339 text shows.
///
/// ```
/// use navree_datetime::{Precision, Span, Timestamp};
///
/// # fn main() -> navree_datetime::Result<()> {
/// let at = Timestamp::utc(2026, 11, 25, 12, 0, 0)? + Span::millis(250);
/// assert_eq!(at.to_rfc3339_with(Precision::Secs, true), "2026-11-25T12:00:00Z");
/// assert_eq!(at.to_rfc3339_with(Precision::Millis, true), "2026-11-25T12:00:00.250Z");
/// assert_eq!(at.to_rfc3339_with(Precision::Micros, true), "2026-11-25T12:00:00.250000Z");
/// assert_eq!(at.to_rfc3339_with(Precision::Nanos, true), "2026-11-25T12:00:00.250000000Z");
/// assert_eq!(at.to_rfc3339_with(Precision::Auto, true), "2026-11-25T12:00:00.250Z");
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Precision {
    /// Whole seconds: no dot and no fraction. A fraction the value has is
    /// dropped.
    Secs,
    /// Three digits: milliseconds. The rest is dropped.
    Millis,
    /// Six digits: microseconds. The rest is dropped.
    Micros,
    /// Nine digits: nanoseconds, all of the fraction.
    Nanos,
    /// As many digits as the value needs, of none, three, six and nine:
    /// nothing is dropped and no group of three zeros is written at the
    /// end.
    Auto,
}

/// Writes `local`, a day and a time on a clock `offset` from UTC, as
/// RFC 3339. `use_z` writes an offset of exactly zero as `Z`, not `+00:00`.
///
/// The offset is written to the minute: one with seconds is rounded to the
/// nearest. A year before 0 or after 9999, which RFC 3339 itself has no
/// place for, is written with its sign.
pub(crate) fn write(
    w: &mut impl Write,
    local: DateTime,
    offset: Offset,
    precision: Precision,
    use_z: bool,
) -> fmt::Result {
    let date = local.date();
    let year = date.year();
    if (0..=9999).contains(&year) {
        two_digits(w, (year / 100) as u8)?;
        two_digits(w, (year % 100) as u8)?;
    } else {
        write!(w, "{year:+05}")?;
    }
    w.write_char('-')?;
    two_digits(w, date.month_number())?;
    w.write_char('-')?;
    two_digits(w, date.day())?;

    w.write_char('T')?;

    let time = local.time();
    two_digits(w, time.hour())?;
    w.write_char(':')?;
    two_digits(w, time.minute())?;
    w.write_char(':')?;
    two_digits(w, time.second())?;

    let nano = time.nanosecond();
    match precision {
        Precision::Secs => {}
        Precision::Millis => write!(w, ".{:03}", nano / 1_000_000)?,
        Precision::Micros => write!(w, ".{:06}", nano / 1_000)?,
        Precision::Nanos => write!(w, ".{nano:09}")?,
        Precision::Auto => fraction(w, nano)?,
    }

    write_offset(w, offset, OffsetPrecision::Minutes, true, use_z)
}

/// The instant `text` is in RFC 3339, and the offset it was written with.
///
/// It is strict about the layout: four digits of year, two of each other
/// number, `-` and `:` in their places, a fraction after a dot or none, and
/// `Z` or an offset with a colon. It takes a small `t` or a space for the
/// `T` and a small `z` for the `Z`, which the RFC allows.
pub(crate) fn parse(text: &str) -> Scan<(Timestamp, Offset)> {
    let bytes = text.as_bytes();
    if bytes.len() < 19 {
        return Err(Reason::TooShort);
    }

    let digit = |index: usize| -> Scan<u8> {
        if bytes[index].is_ascii_digit() {
            Ok(bytes[index] - b'0')
        } else {
            Err(Reason::Invalid)
        }
    };
    let year = u16::from(digit(0)?) * 1000
        + u16::from(digit(1)?) * 100
        + u16::from(digit(2)?) * 10
        + u16::from(digit(3)?);
    if bytes[4] != b'-' {
        return Err(Reason::Invalid);
    }

    let month = digit(5)? * 10 + digit(6)?;
    if bytes[7] != b'-' {
        return Err(Reason::Invalid);
    }

    let day = digit(8)? * 10 + digit(9)?;
    let date = Date::new(i32::from(year), month, day).map_err(|_| Reason::OutOfRange)?;

    if !matches!(bytes[10], b't' | b'T' | b' ') {
        return Err(Reason::Invalid);
    }

    let hour = digit(11)? * 10 + digit(12)?;
    if bytes[13] != b':' {
        return Err(Reason::Invalid);
    }

    let minute = digit(14)? * 10 + digit(15)?;
    if bytes[16] != b':' {
        return Err(Reason::Invalid);
    }

    let second = digit(17)? * 10 + digit(18)?;
    // Second 60 is the last nanosecond of its minute.
    let (second, sixty) = match second {
        60 => (59, true),
        _ => (second, false),
    };

    // The first 19 bytes are ASCII, so the text can be cut after them.
    let (rest, nano) = if bytes.get(19) == Some(&b'.') {
        scan::nanosecond(&text[20..])?
    } else {
        (&text[19..], 0)
    };
    let nano = if sixty { 999_999_999 } else { nano };

    let time = Time::new(hour, minute, second)
        .and_then(|time| time.with_nanos(nano))
        .map_err(|_| Reason::OutOfRange)?;

    let (rest, seconds_east) = scan::timezone_offset(rest, scan::colon, true, false, true)?;
    if !rest.is_empty() {
        return Err(Reason::TooLong);
    }
    let offset = offset_within_a_day(seconds_east)?;

    // A year of 0 to 9999 less than a day is far inside the range.
    let seconds = date.at(time).in_utc().as_secs() - i64::from(seconds_east);
    let instant = Timestamp::from_secs_nanos(seconds, nano).map_err(|_| Reason::OutOfRange)?;
    Ok((instant, offset))
}

impl Timestamp {
    /// The instant as RFC 3339 text in UTC, with `+00:00` for the offset
    /// and as much of the fraction of the second as it has, in none, three,
    /// six or nine digits: `2026-11-25T12:00:00+00:00`,
    /// `2026-11-25T12:00:00.250+00:00`.
    ///
    /// For the `Z` that JSON and most APIs use, and for a fixed number of
    /// digits, there is [`to_rfc3339_with`](Timestamp::to_rfc3339_with).
    ///
    /// An instant before the year 0 or after the year 9999, which RFC 3339
    /// itself has no place for, is written with the sign of its year
    /// (`+262142-12-31T23:59:59.999999999+00:00`) and read back by
    /// `str::parse`.
    ///
    /// ```
    /// use navree_datetime::Timestamp;
    ///
    /// # fn main() -> navree_datetime::Result<()> {
    /// let at = Timestamp::utc(2026, 11, 25, 12, 0, 0)?;
    /// assert_eq!(at.to_rfc3339(), "2026-11-25T12:00:00+00:00");
    /// assert_eq!(Timestamp::parse_rfc3339(&at.to_rfc3339())?, at);
    /// # Ok(())
    /// # }
    /// ```
    pub fn to_rfc3339(self) -> String {
        self.to_rfc3339_with(Precision::Auto, false)
    }

    /// The instant as RFC 3339 text in UTC, with `precision` digits of the
    /// fraction of the second, and `Z` for the offset when `use_z` is true
    /// or `+00:00` when it is not.
    ///
    /// ```
    /// use navree_datetime::{Precision, Timestamp};
    ///
    /// # fn main() -> navree_datetime::Result<()> {
    /// let at = Timestamp::from_millis(1_795_608_000_250)?;
    /// assert_eq!(at.to_rfc3339_with(Precision::Auto, true), "2026-11-25T12:00:00.250Z");
    /// assert_eq!(at.to_rfc3339_with(Precision::Secs, true), "2026-11-25T12:00:00Z");
    /// assert_eq!(
    ///     at.to_rfc3339_with(Precision::Micros, false),
    ///     "2026-11-25T12:00:00.250000+00:00"
    /// );
    /// # Ok(())
    /// # }
    /// ```
    pub fn to_rfc3339_with(self, precision: Precision, use_z: bool) -> String {
        let mut text = String::with_capacity(38);
        // Writing to a `String` does not fail.
        let _ = write(
            &mut text,
            self.date_time_utc(),
            Offset::UTC,
            precision,
            use_z,
        );
        text
    }

    /// Reads an instant from RFC 3339 text: `2026-11-25T12:00:00Z`,
    /// `2026-11-25T17:30:00.25+05:30`.
    ///
    /// The layout is the RFC's, and strict: four digits of year and two of
    /// each other number, a fraction of the second after a dot or none, and
    /// `Z` or an offset with its colon. A small `t` or a space may stand
    /// for the `T`, and a small `z` for the `Z`. A fraction of more than
    /// nine digits is cut to nine. `str::parse` reads more loosely.
    ///
    /// The offset is taken off: the instant is the same one whatever offset
    /// it was written with. Second 60 is read as the last nanosecond of its
    /// minute, since a minute has 60 seconds here.
    ///
    /// ```
    /// use navree_datetime::Timestamp;
    ///
    /// # fn main() -> navree_datetime::Result<()> {
    /// let sent = Timestamp::utc(2026, 11, 25, 12, 0, 0)?;
    /// assert_eq!(Timestamp::parse_rfc3339("2026-11-25T12:00:00Z")?, sent);
    /// assert_eq!(Timestamp::parse_rfc3339("2026-11-25T17:30:00+05:30")?, sent);
    /// assert_eq!(Timestamp::parse_rfc3339("2026-11-25 12:00:00.000z")?, sent);
    ///
    /// assert!(Timestamp::parse_rfc3339("2026-11-25T12:00:00").is_err());
    /// assert!(Timestamp::parse_rfc3339("2026-11-25T17:30:00+0530").is_err());
    /// assert!(Timestamp::parse_rfc3339("2026-11-25").is_err());
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// An error of the kind [`Parse`](crate::ErrorKind::Parse) when the
    /// text is not laid out as RFC 3339, names a day or a time that does
    /// not exist, or has an offset of 24 hours or more.
    pub fn parse_rfc3339(text: &str) -> Result<Timestamp> {
        Ok(parse(text)?.0)
    }
}

impl Zoned {
    /// The instant as RFC 3339 text at the offset the zone keeps then, with
    /// `+00:00` where that offset is zero and as much of the fraction of the
    /// second as the instant has: `2026-11-25T17:30:00+05:30`.
    ///
    /// ```
    /// use navree_datetime::{Timestamp, Zone, Zoned};
    ///
    /// # fn main() -> navree_datetime::Result<()> {
    /// let kolkata = Zone::named("Asia/Kolkata")?;
    /// let sent = Timestamp::utc(2026, 11, 25, 12, 0, 0)?.in_zone(kolkata);
    /// assert_eq!(sent.to_rfc3339(), "2026-11-25T17:30:00+05:30");
    /// assert_eq!(Zoned::parse_rfc3339(&sent.to_rfc3339())?, sent);
    /// # Ok(())
    /// # }
    /// ```
    pub fn to_rfc3339(self) -> String {
        self.to_rfc3339_with(Precision::Auto, false)
    }

    /// The instant as RFC 3339 text at the offset the zone keeps then, with
    /// `precision` digits of the fraction of the second, and `Z` where the
    /// offset is zero and `use_z` is true.
    ///
    /// ```
    /// use navree_datetime::{Precision, Timestamp, Zone};
    ///
    /// # fn main() -> navree_datetime::Result<()> {
    /// let sent = Timestamp::from_millis(1_795_608_000_250)?;
    /// let kolkata = Zone::named("Asia/Kolkata")?;
    /// assert_eq!(
    ///     sent.in_zone(kolkata).to_rfc3339_with(Precision::Secs, true),
    ///     "2026-11-25T17:30:00+05:30"
    /// );
    /// assert_eq!(
    ///     sent.in_zone(Zone::UTC).to_rfc3339_with(Precision::Auto, true),
    ///     "2026-11-25T12:00:00.250Z"
    /// );
    /// # Ok(())
    /// # }
    /// ```
    pub fn to_rfc3339_with(self, precision: Precision, use_z: bool) -> String {
        let mut text = String::with_capacity(38);
        // Writing to a `String` does not fail.
        let _ = write(&mut text, self.date_time(), self.offset(), precision, use_z);
        text
    }

    /// Reads an instant and the fixed offset it was written with from
    /// RFC 3339 text: `2026-11-25T17:30:00+05:30`, `2026-11-25T12:00:00Z`.
    ///
    /// The layout is the strict one of [`Timestamp::parse_rfc3339`]; the
    /// zone of the value returned is [`Zone::fixed`] at the offset read.
    /// Second 60 is read as the last nanosecond of its minute.
    ///
    /// # Errors
    ///
    /// An error of the kind [`Parse`](crate::ErrorKind::Parse) when the
    /// text is not laid out as RFC 3339, names a day or a time that does
    /// not exist, or has an offset of 24 hours or more.
    pub fn parse_rfc3339(text: &str) -> Result<Zoned> {
        let (instant, offset) = parse(text)?;
        Ok(Zoned::new(instant, Zone::fixed(offset)))
    }
}
