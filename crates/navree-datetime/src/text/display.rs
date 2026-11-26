//! `Display` and `Debug`: what each value is shown as.
//!
//! The text of a date is data: it is stored in SQLite, compared as text in
//! SQL, sent in JSON and read by people. So each form here is fixed, and is
//! the one Navree has written since 1.0.
//!
//! The dates, the times, the offsets and the spans write themselves piece
//! by piece and do not look at a width, a fill or a precision:
//! `format!("{date:>12}")` is `format!("{date}")`. A [`Weekday`] and a
//! [`Month`] are a word, and are padded as a word is.

use std::fmt::{self, Write};

use crate::{Date, DateTime, Month, Offset, Span, Time, Timestamp, Weekday, Zone, Zoned};

/// `n` as two digits. It is for a number below 100.
pub(crate) fn two_digits(w: &mut impl Write, n: u8) -> fmt::Result {
    if n >= 100 {
        return Err(fmt::Error);
    }
    w.write_char((b'0' + n / 10) as char)?;
    w.write_char((b'0' + n % 10) as char)
}

/// The fraction of a second after its dot, in as many digits as `nano`
/// nanoseconds need of three, six and nine. Nothing, and no dot, for none.
pub(crate) fn fraction(w: &mut impl Write, nano: u32) -> fmt::Result {
    if nano == 0 {
        Ok(())
    } else if nano.is_multiple_of(1_000_000) {
        write!(w, ".{:03}", nano / 1_000_000)
    } else if nano.is_multiple_of(1_000) {
        write!(w, ".{:06}", nano / 1_000)
    } else {
        write!(w, ".{nano:09}")
    }
}

/// `2026-11-25`. A year before 0 or after 9999 has its sign and at least
/// four digits, as ISO 8601 asks: `-0044-03-15`, `+12026-01-01`.
pub(crate) fn date(w: &mut impl Write, date: Date) -> fmt::Result {
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
    two_digits(w, date.day())
}

/// `09:30:15`, and the fraction of the second where there is one:
/// `09:30:15.250`.
pub(crate) fn time(w: &mut impl Write, time: Time) -> fmt::Result {
    two_digits(w, time.hour())?;
    w.write_char(':')?;
    two_digits(w, time.minute())?;
    w.write_char(':')?;
    two_digits(w, time.second())?;
    fraction(w, time.nanosecond())
}

/// A day, `separator`, and a time of day: `2026-11-25 09:30:15` with a
/// space, `2026-11-25T09:30:15` with a `T`.
///
/// It is the first part of how an instant is shown, in UTC or in a zone:
/// what follows it is the zone.
pub(crate) fn date_time(w: &mut impl Write, value: DateTime, separator: char) -> fmt::Result {
    date(w, value.date())?;
    w.write_char(separator)?;
    time(w, value.time())
}

/// `+05:30`, and the seconds where the offset has any: `+05:53:28`.
pub(crate) fn offset(w: &mut impl Write, offset: Offset) -> fmt::Result {
    let seconds = offset.seconds();
    let (sign, seconds) = if seconds < 0 {
        ('-', -seconds)
    } else {
        ('+', seconds)
    };
    let second = seconds % 60;
    let minutes = seconds / 60;
    let minute = minutes % 60;
    let hour = minutes / 60;
    if second == 0 {
        write!(w, "{sign}{hour:02}:{minute:02}")
    } else {
        write!(w, "{sign}{hour:02}:{minute:02}:{second:02}")
    }
}

/// The day as `2026-11-25`: the year, the month and the day, each with
/// zeros before it. A year before 0 or after 9999 has its sign
/// (`-0044-03-15`, `+12026-01-01`).
///
/// A width, a fill or a precision in the format is not looked at.
///
/// ```
/// use navree_datetime::Date;
///
/// # fn main() -> navree_datetime::Result<()> {
/// assert_eq!(Date::new(2026, 11, 25)?.to_string(), "2026-11-25");
/// assert_eq!(Date::new(476, 9, 4)?.to_string(), "0476-09-04");
/// assert_eq!(Date::new(-44, 3, 15)?.to_string(), "-0044-03-15");
/// # Ok(())
/// # }
/// ```
impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        date(f, *self)
    }
}

/// The same text as [`Display`](fmt::Display): `2026-11-25`.
impl fmt::Debug for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        date(f, *self)
    }
}

/// The time of day as `09:30:15`, with the fraction of the second after a
/// dot where there is one, in three, six or nine digits, the fewest that
/// lose nothing: `09:30:15.250`, `09:30:15.000250`.
///
/// A width, a fill or a precision in the format is not looked at.
///
/// ```
/// use navree_datetime::Time;
///
/// # fn main() -> navree_datetime::Result<()> {
/// let opens = Time::new(9, 30, 0)?;
/// assert_eq!(opens.to_string(), "09:30:00");
/// assert_eq!(opens.with_nanos(250_000_000)?.to_string(), "09:30:00.250");
/// assert_eq!(opens.with_nanos(1)?.to_string(), "09:30:00.000000001");
/// # Ok(())
/// # }
/// ```
impl fmt::Display for Time {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        time(f, *self)
    }
}

/// The same text as [`Display`](fmt::Display): `09:30:15`.
impl fmt::Debug for Time {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        time(f, *self)
    }
}

/// The day and the time of day with a space between them:
/// `2026-11-25 09:30:15`, each as it is shown alone.
///
/// [`Debug`](fmt::Debug) and JSON have a `T` there, and that is the form
/// `str::parse` reads.
///
/// ```
/// use navree_datetime::{Date, Time};
///
/// # fn main() -> navree_datetime::Result<()> {
/// let due = Date::new(2026, 11, 25)?.at(Time::new(17, 30, 0)?);
/// assert_eq!(due.to_string(), "2026-11-25 17:30:00");
/// assert_eq!(format!("{due:?}"), "2026-11-25T17:30:00");
/// # Ok(())
/// # }
/// ```
impl fmt::Display for DateTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        date_time(f, *self, ' ')
    }
}

/// The day and the time of day with a `T` between them, as ISO 8601 writes
/// them: `2026-11-25T09:30:15`.
impl fmt::Debug for DateTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        date_time(f, *self, 'T')
    }
}

/// The instant as the day and the time a clock in UTC shows, and `UTC`:
/// `2026-11-25 12:00:00 UTC`, with the fraction of the second where there
/// is one.
///
/// It is for people to read. To store or send an instant, write RFC 3339
/// with [`to_rfc3339`](Timestamp::to_rfc3339) or
/// [`to_rfc3339_with`](Timestamp::to_rfc3339_with). `str::parse` reads both
/// back.
///
/// ```
/// use navree_datetime::{Span, Timestamp};
///
/// # fn main() -> navree_datetime::Result<()> {
/// let sent = Timestamp::utc(2026, 11, 25, 12, 0, 0)?;
/// assert_eq!(sent.to_string(), "2026-11-25 12:00:00 UTC");
/// assert_eq!((sent + Span::millis(250)).to_string(), "2026-11-25 12:00:00.250 UTC");
/// assert_eq!(format!("{sent:?}"), "2026-11-25T12:00:00Z");
/// # Ok(())
/// # }
/// ```
impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        date_time(f, self.date_time_utc(), ' ')?;
        f.write_str(" UTC")
    }
}

/// The instant as RFC 3339 in UTC with `Z`, and as much of the fraction of
/// the second as it has: `2026-11-25T12:00:00Z`.
impl fmt::Debug for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        date_time(f, self.date_time_utc(), 'T')?;
        f.write_char('Z')
    }
}

/// The offset as a sign, the hours and the minutes: `+05:30`, `-08:00`,
/// `+00:00`. An offset with seconds shows them too: `+05:53:28`.
///
/// ```
/// use navree_datetime::Offset;
///
/// # fn main() -> navree_datetime::Result<()> {
/// assert_eq!(Offset::east(19_800)?.to_string(), "+05:30");
/// assert_eq!(Offset::west(8 * 3_600)?.to_string(), "-08:00");
/// assert_eq!(Offset::UTC.to_string(), "+00:00");
/// assert_eq!(Offset::east(21_208)?.to_string(), "+05:53:28");
/// # Ok(())
/// # }
/// ```
impl fmt::Display for Offset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        offset(f, *self)
    }
}

/// The same text as [`Display`](fmt::Display): `+05:30`.
impl fmt::Debug for Offset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        offset(f, *self)
    }
}

/// The span as an ISO 8601 duration in seconds: `PT90S`, `PT1.5S`, and
/// `P0D` for no time at all. A span below zero has a minus before it
/// (`-PT0.5S`), which ISO 8601 itself has no way to write.
///
/// Minutes, hours and days are not broken out: a span is a count of
/// seconds, and that is what it shows.
///
/// ```
/// use navree_datetime::Span;
///
/// assert_eq!(Span::minutes(90).to_string(), "PT5400S");
/// assert_eq!(Span::millis(1_500).to_string(), "PT1.5S");
/// assert_eq!(Span::millis(-500).to_string(), "-PT0.5S");
/// assert_eq!(Span::ZERO.to_string(), "P0D");
/// ```
impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (abs, sign) = if self.is_negative() {
            (self.abs(), "-")
        } else {
            (*self, "")
        };
        let seconds = abs.secs_floor();
        let nanos = abs.nanos_above();

        write!(f, "{sign}P")?;
        if seconds == 0 && nanos == 0 {
            return f.write_str("0D");
        }

        write!(f, "T{seconds}")?;
        if nanos > 0 {
            // The fraction without the zeros at its end.
            let mut figures = 9usize;
            let mut digits = nanos;
            while digits.is_multiple_of(10) {
                digits /= 10;
                figures -= 1;
            }
            write!(f, ".{digits:0figures$}")?;
        }
        f.write_str("S")
    }
}

/// The first three letters of the day's English name: `Mon` to `Sun`.
///
/// It is padded and cut as text is: `format!("{day:<5}")` is `Wed` and two
/// spaces.
///
/// ```
/// use navree_datetime::Weekday;
///
/// assert_eq!(Weekday::Wednesday.to_string(), "Wed");
/// assert_eq!(format!("{:>5}|", Weekday::Wednesday), "  Wed|");
/// ```
impl fmt::Display for Weekday {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(self.short_name())
    }
}

/// The same three letters as [`Display`](fmt::Display), never padded:
/// `Wed`.
impl fmt::Debug for Weekday {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.short_name())
    }
}

/// The month's English name: `January` to `December`.
///
/// It is padded and cut as text is: `format!("{month:.3}")` is `Nov`.
///
/// ```
/// use navree_datetime::Month;
///
/// assert_eq!(Month::November.to_string(), "November");
/// assert_eq!(format!("{:.3}", Month::November), "Nov");
/// ```
impl fmt::Display for Month {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(self.name())
    }
}

/// The zone's current name in the IANA database (`Asia/Kolkata`, `UTC`), or
/// its offset (`+05:30`) where the zone is a fixed offset.
///
/// ```
/// use navree_datetime::{Offset, Zone};
///
/// # fn main() -> navree_datetime::Result<()> {
/// assert_eq!(Zone::named("Asia/Calcutta")?.to_string(), "Asia/Kolkata");
/// assert_eq!(Zone::UTC.to_string(), "UTC");
/// assert_eq!(Zone::fixed(Offset::east(19_800)?).to_string(), "+05:30");
/// # Ok(())
/// # }
/// ```
impl fmt::Display for Zone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_fixed() {
            offset(f, self.offset_at(Timestamp::UNIX_EPOCH))
        } else {
            f.write_str(self.name())
        }
    }
}

/// The day and the time the zone's calendar and clocks show at the instant,
/// a space, and the zone's short name then (`2026-11-25 17:30:00 IST`) or
/// its offset where the zone is a fixed offset (`2026-11-25 17:30:00 +05:30`).
///
/// A width, a fill or a precision in the format is not looked at.
///
/// ```
/// use navree_datetime::{Offset, Timestamp, Zone};
///
/// # fn main() -> navree_datetime::Result<()> {
/// let sent = Timestamp::utc(2026, 11, 25, 12, 0, 0)?;
/// let kolkata = Zone::named("Asia/Kolkata")?;
/// assert_eq!(sent.in_zone(kolkata).to_string(), "2026-11-25 17:30:00 IST");
/// assert_eq!(format!("{:?}", sent.in_zone(kolkata)), "2026-11-25T17:30:00IST");
///
/// let fixed = Zone::fixed(Offset::east(19_800)?);
/// assert_eq!(sent.in_zone(fixed).to_string(), "2026-11-25 17:30:00 +05:30");
/// assert_eq!(format!("{:?}", sent.in_zone(fixed)), "2026-11-25T17:30:00+05:30");
/// # Ok(())
/// # }
/// ```
impl fmt::Display for Zoned {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        date_time(f, self.date_time(), ' ')?;
        f.write_char(' ')?;
        if self.zone().is_fixed() {
            offset(f, self.offset())
        } else {
            f.write_str(self.zone().abbreviation_at(self.timestamp()))
        }
    }
}

/// The day and the time with a `T` between them, followed directly by the
/// zone's short name (`2026-11-25T17:30:00IST`) or by the offset where the
/// zone is a fixed offset (`2026-11-25T17:30:00+05:30`).
impl fmt::Debug for Zoned {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        date_time(f, self.date_time(), 'T')?;
        if self.zone().is_fixed() {
            offset(f, self.offset())
        } else {
            f.write_str(self.zone().abbreviation_at(self.timestamp()))
        }
    }
}
