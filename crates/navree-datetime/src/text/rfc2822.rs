//! RFC 2822: `Wed, 25 Nov 2026 12:00:00 +0000`, the form of the `Date`
//! header of an email and of the dates of HTTP.

use std::fmt::{self, Write};

use super::display::two_digits;
use super::format::{OffsetPrecision, write_offset};
use super::parsed::Parsed;
use super::scan::{self, Reason, Scan};
use crate::{DateTime, Error, Offset, Result, Timestamp, Zone, Zoned};

/// Writes `local`, a day and a time on a clock `offset` from UTC, as
/// RFC 2822: the weekday, the day with no zero before it, the month's short
/// name, the year, the time to the second and the offset to the minute.
///
/// # Errors
///
/// RFC 2822 has four digits for the year and no sign: a year before 0 or
/// after 9999 is out of range.
pub(crate) fn write(text: &mut String, local: DateTime, offset: Offset) -> Result<()> {
    let year = local.date().year();
    if !(0..=9999).contains(&year) {
        return Err(Error::out_of_range(format!(
            "RFC 2822 has no way to write the year {year}: its years are 0 to 9999"
        )));
    }
    // Writing to a `String` does not fail.
    let _ = write_parts(text, local, offset);
    Ok(())
}

/// [`write`] for a year of 0 to 9999.
fn write_parts(w: &mut impl Write, local: DateTime, offset: Offset) -> fmt::Result {
    let date = local.date();
    w.write_str(date.weekday().short_name())?;
    w.write_str(", ")?;
    let day = date.day();
    if day < 10 {
        w.write_char((b'0' + day) as char)?;
    } else {
        two_digits(w, day)?;
    }
    w.write_char(' ')?;
    w.write_str(date.month().short_name())?;
    w.write_char(' ')?;
    let year = date.year();
    two_digits(w, (year / 100) as u8)?;
    two_digits(w, (year % 100) as u8)?;
    w.write_char(' ')?;

    let time = local.time();
    two_digits(w, time.hour())?;
    w.write_char(':')?;
    two_digits(w, time.minute())?;
    w.write_char(':')?;
    two_digits(w, time.second())?;
    w.write_char(' ')?;
    write_offset(w, offset, OffsetPrecision::Minutes, false, false)
}

/// Reads the parts of an RFC 2822 date into `parsed` and gives back what is
/// left of `s` after it and its comments.
///
/// The form, after RFC 2822 sections 3.3 and 4.3, with `S` for white space:
///
/// ```text
/// date-time   = [ day-of-week "," ] date 1*S time *S *comment
/// day-of-week = *S day-name *S
/// day-name    = "Mon" / "Tue" / "Wed" / "Thu" / "Fri" / "Sat" / "Sun"
/// date        = day month year
/// day         = *S 1*2DIGIT *S
/// month       = 1*S month-name 1*S
/// month-name  = "Jan" / "Feb" / "Mar" / "Apr" / "May" / "Jun" /
///               "Jul" / "Aug" / "Sep" / "Oct" / "Nov" / "Dec"
/// year        = *S 2*DIGIT *S
/// time        = time-of-day 1*S zone
/// time-of-day = hour ":" minute [ ":" second ]
/// zone        = ( "+" / "-" ) 4DIGIT / "UT" / "GMT" / "Z" /
///               "EST" / "CST" / "MST" / "PST" / "EDT" / "CDT" / "MDT" / "PDT" /
///               one other letter
/// comment     = "(" *( comment / any character / "\" any character ) ")" *S
/// ```
///
/// - Names are read in any case.
/// - A year of two digits below 50 is in the 2000s; one of two digits from
///   50, or of three digits, is counted from 1900. A year of four digits or
///   more is itself.
/// - A weekday that is not the date's is a refusal.
fn read<'t>(parsed: &mut Parsed, mut s: &'t str) -> Scan<&'t str> {
    s = s.trim_start();

    if let Ok((rest, weekday)) = scan::short_weekday(s) {
        if !rest.starts_with(',') {
            return Err(Reason::Invalid);
        }
        s = &rest[1..];
        parsed.set_weekday(weekday)?;
    }

    s = s.trim_start();
    let (rest, day) = scan::number(s, 1, 2)?;
    parsed.set_day(day)?;
    s = scan::space(rest)?;
    let (rest, month0) = scan::short_month0(s)?;
    parsed.set_month(1 + i64::from(month0))?;
    s = scan::space(rest)?;

    // A year of two or three digits is short for one of four.
    let before = s.len();
    let (rest, mut year) = scan::number(s, 2, usize::MAX)?;
    let digits = before - rest.len();
    match (digits, year) {
        (2, 0..=49) => year += 2000,
        (2, 50..=99) => year += 1900,
        (3, _) => year += 1900,
        (_, _) => {}
    }
    parsed.set_year(year)?;

    s = scan::space(rest)?;
    let (rest, hour) = scan::number(s, 2, 2)?;
    parsed.set_hour(hour)?;
    s = scan::char(rest.trim_start(), b':')?.trim_start();
    let (rest, minute) = scan::number(s, 2, 2)?;
    parsed.set_minute(minute)?;
    s = rest;
    if let Ok(after_colon) = scan::char(s.trim_start(), b':') {
        let (rest, second) = scan::number(after_colon, 2, 2)?;
        parsed.set_second(second)?;
        s = rest;
    }

    s = scan::space(s)?;
    let (rest, offset) = scan::timezone_offset_2822(s)?;
    parsed.set_offset(i64::from(offset))?;
    s = rest;

    while let Ok(rest) = scan::comment_2822(s) {
        s = rest;
    }

    Ok(s)
}

/// The instant `text` is in RFC 2822, and the offset it was written with.
pub(crate) fn parse(text: &str) -> Scan<(Timestamp, Offset)> {
    let mut parsed = Parsed::default();
    if !read(&mut parsed, text)?.is_empty() {
        return Err(Reason::TooLong);
    }
    parsed.to_instant()
}

impl Timestamp {
    /// The instant as RFC 2822 text in UTC, to the second:
    /// `Wed, 25 Nov 2026 12:00:00 +0000`. A fraction of the second is
    /// dropped.
    ///
    /// ```
    /// use navree_datetime::Timestamp;
    ///
    /// # fn main() -> navree_datetime::Result<()> {
    /// let sent = Timestamp::utc(2026, 11, 5, 12, 0, 0)?;
    /// assert_eq!(sent.to_rfc2822()?, "Thu, 5 Nov 2026 12:00:00 +0000");
    /// assert!(Timestamp::MAX.to_rfc2822().is_err());
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// RFC 2822 has four digits for the year and no sign: an instant before
    /// the year 0 or after the year 9999 is out of range.
    pub fn to_rfc2822(self) -> Result<String> {
        let mut text = String::with_capacity(32);
        write(&mut text, self.date_time_utc(), Offset::UTC)?;
        Ok(text)
    }

    /// Reads an instant from RFC 2822 text:
    /// `Wed, 25 Nov 2026 12:00:00 +0000`.
    ///
    /// The weekday and the seconds may be missing. The zone is a sign and
    /// four digits, or one of the names the RFC keeps: `GMT`, `UT` and `Z`
    /// for UTC, and the North American `EST`, `EDT`, `CST`, `CDT`, `MST`,
    /// `MDT`, `PST` and `PDT`. Any other single letter is read as UTC, as
    /// the RFC says to. Names are read in any case, a year of two or three
    /// digits is counted as the RFC says (`26` is 2026 and `99` is 1999),
    /// and comments in round brackets may follow.
    ///
    /// The offset is taken off: the instant is the same one whatever zone
    /// it was written with. Second 60 is read as the last nanosecond of its
    /// minute.
    ///
    /// ```
    /// use navree_datetime::Timestamp;
    ///
    /// # fn main() -> navree_datetime::Result<()> {
    /// let sent = Timestamp::utc(2026, 11, 25, 12, 0, 0)?;
    /// assert_eq!(Timestamp::parse_rfc2822("Wed, 25 Nov 2026 12:00:00 +0000")?, sent);
    /// assert_eq!(Timestamp::parse_rfc2822("Wed, 25 Nov 2026 17:30:00 +0530")?, sent);
    /// assert_eq!(Timestamp::parse_rfc2822("25 Nov 2026 12:00 GMT")?, sent);
    ///
    /// // The 25th was a Wednesday.
    /// assert!(Timestamp::parse_rfc2822("Mon, 25 Nov 2026 12:00:00 +0000").is_err());
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// An error of the kind [`Parse`](crate::ErrorKind::Parse) when the
    /// text is not laid out as RFC 2822, names a day or a time that does
    /// not exist, names a weekday that is not the date's, or has an offset
    /// of 24 hours or more.
    pub fn parse_rfc2822(text: &str) -> Result<Timestamp> {
        Ok(parse(text)?.0)
    }
}

impl Zoned {
    /// The instant as RFC 2822 text at the offset the zone keeps then, to
    /// the second: `Wed, 25 Nov 2026 17:30:00 +0530`. A fraction of the
    /// second is dropped.
    ///
    /// ```
    /// use navree_datetime::{Timestamp, Zone, Zoned};
    ///
    /// # fn main() -> navree_datetime::Result<()> {
    /// let kolkata = Zone::named("Asia/Kolkata")?;
    /// let sent = Timestamp::utc(2026, 11, 5, 12, 0, 0)?.in_zone(kolkata);
    /// assert_eq!(sent.to_rfc2822()?, "Thu, 5 Nov 2026 17:30:00 +0530");
    /// assert_eq!(Zoned::parse_rfc2822(&sent.to_rfc2822()?)?, sent);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// RFC 2822 has four digits for the year and no sign: a local year
    /// before 0 or after 9999 is out of range.
    pub fn to_rfc2822(self) -> Result<String> {
        let mut text = String::with_capacity(32);
        write(&mut text, self.date_time(), self.offset())?;
        Ok(text)
    }

    /// Reads an instant and the fixed offset it was written with from
    /// RFC 2822 text: `Wed, 25 Nov 2026 17:30:00 +0530`.
    ///
    /// It takes everything [`Timestamp::parse_rfc2822`] takes; the zone of
    /// the value returned is [`Zone::fixed`] at the offset read. Second 60
    /// is read as the last nanosecond of its minute.
    ///
    /// # Errors
    ///
    /// An error of the kind [`Parse`](crate::ErrorKind::Parse) when the
    /// text is not laid out as RFC 2822, names a day or a time that does
    /// not exist, names a weekday that is not the date's, or has an offset
    /// of 24 hours or more.
    pub fn parse_rfc2822(text: &str) -> Result<Zoned> {
        let (instant, offset) = parse(text)?;
        Ok(Zoned::new(instant, Zone::fixed(offset)))
    }
}
