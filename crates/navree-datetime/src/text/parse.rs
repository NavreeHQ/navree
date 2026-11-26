//! Reading a value from text: by a pattern, and by `str::parse`.

use std::str::FromStr;

use super::parsed::{Parsed, offset_within_a_day};
use super::pattern::{Fixed, Item, Items, Numeric, Pad};
use super::scan::{self, Reason, Scan};
use crate::{Date, DateTime, Error, Month, Offset, Result, Time, Timestamp, Weekday, Zone, Zoned};

/// Reads `s` by `items` into `parsed` and gives back what is left of it.
///
/// It reads as much as each step can take: a whole month's name where the
/// whole name is there, as many digits as a number's width allows. Padding
/// is not looked at: a number may have white space and zeros before it.
pub(super) fn read<'t, 'p>(
    parsed: &mut Parsed,
    mut s: &'t str,
    items: impl Iterator<Item = Item<'p>>,
) -> Scan<&'t str> {
    for item in items {
        match item {
            Item::Literal(prefix) => {
                if s.len() < prefix.len() {
                    return Err(Reason::TooShort);
                }
                if !s.starts_with(prefix) {
                    return Err(Reason::Invalid);
                }
                s = &s[prefix.len()..];
            }

            Item::Space(_) => {
                s = s.trim_start();
            }

            Item::Numeric(numeric, _) => {
                type Setter = fn(&mut Parsed, i64) -> Scan<()>;

                // The most digits it reads, whether it can have a sign, and
                // where it goes.
                let (width, signed, set): (usize, bool, Setter) = match numeric {
                    Numeric::Year => (4, true, Parsed::set_year),
                    Numeric::YearDiv100 => (2, false, Parsed::set_year_div_100),
                    Numeric::YearMod100 => (2, false, Parsed::set_year_mod_100),
                    Numeric::IsoYear => (4, true, Parsed::set_iso_year),
                    Numeric::IsoYearMod100 => (2, false, Parsed::set_iso_year_mod_100),
                    Numeric::Quarter => (1, false, Parsed::set_quarter),
                    Numeric::Month => (2, false, Parsed::set_month),
                    Numeric::Day => (2, false, Parsed::set_day),
                    Numeric::WeekFromSun => (2, false, Parsed::set_week_from_sun),
                    Numeric::WeekFromMon => (2, false, Parsed::set_week_from_mon),
                    Numeric::IsoWeek => (2, false, Parsed::set_iso_week),
                    Numeric::DaysFromSun => (1, false, Parsed::set_weekday_from_sunday),
                    Numeric::WeekdayFromMon => (1, false, Parsed::set_weekday_from_monday),
                    Numeric::Ordinal => (3, false, Parsed::set_ordinal),
                    Numeric::Hour => (2, false, Parsed::set_hour),
                    Numeric::Hour12 => (2, false, Parsed::set_hour12),
                    Numeric::Minute => (2, false, Parsed::set_minute),
                    Numeric::Second => (2, false, Parsed::set_second),
                    Numeric::Nanosecond => (9, false, Parsed::set_nanosecond),
                    Numeric::Timestamp => (usize::MAX, false, Parsed::set_timestamp),
                };

                s = s.trim_start();
                let (rest, v) = if signed {
                    if let Some(digits) = s.strip_prefix('-') {
                        let (rest, v) = scan::number(digits, 1, usize::MAX)?;
                        (rest, 0i64.checked_sub(v).ok_or(Reason::OutOfRange)?)
                    } else if let Some(digits) = s.strip_prefix('+') {
                        scan::number(digits, 1, usize::MAX)?
                    } else {
                        // With no sign the number keeps to its width.
                        scan::number(s, 1, width)?
                    }
                } else {
                    scan::number(s, 1, width)?
                };
                s = rest;
                set(parsed, v)?;
            }

            Item::Fixed(fixed) => match fixed {
                Fixed::ShortMonthName => {
                    let (rest, month0) = scan::short_month0(s)?;
                    s = rest;
                    parsed.set_month(i64::from(month0) + 1)?;
                }

                Fixed::LongMonthName => {
                    let (rest, month0) = scan::short_or_long_month0(s)?;
                    s = rest;
                    parsed.set_month(i64::from(month0) + 1)?;
                }

                Fixed::ShortWeekdayName => {
                    let (rest, weekday) = scan::short_weekday(s)?;
                    s = rest;
                    parsed.set_weekday(weekday)?;
                }

                Fixed::LongWeekdayName => {
                    let (rest, weekday) = scan::short_or_long_weekday(s)?;
                    s = rest;
                    parsed.set_weekday(weekday)?;
                }

                Fixed::LowerAmPm | Fixed::UpperAmPm => {
                    if s.len() < 2 {
                        return Err(Reason::TooShort);
                    }
                    let pm = match (s.as_bytes()[0] | 32, s.as_bytes()[1] | 32) {
                        (b'a', b'm') => false,
                        (b'p', b'm') => true,
                        _ => return Err(Reason::Invalid),
                    };
                    parsed.set_ampm(pm)?;
                    s = &s[2..];
                }

                Fixed::Fraction => {
                    if let Some(digits) = s.strip_prefix('.') {
                        let (rest, nano) = scan::nanosecond(digits)?;
                        s = rest;
                        parsed.set_nanosecond(i64::from(nano))?;
                    }
                }

                Fixed::Fraction3 | Fixed::Fraction6 | Fixed::Fraction9 => {
                    if let Some(digits) = s.strip_prefix('.') {
                        let (rest, nano) = scan::nanosecond_fixed(digits, fraction_digits(fixed))?;
                        s = rest;
                        parsed.set_nanosecond(nano)?;
                    }
                }

                Fixed::Digits3 | Fixed::Digits6 | Fixed::Digits9 => {
                    let digits = fraction_digits(fixed);
                    if s.len() < digits {
                        return Err(Reason::TooShort);
                    }
                    let (rest, nano) = scan::nanosecond_fixed(s, digits)?;
                    s = rest;
                    parsed.set_nanosecond(nano)?;
                }

                Fixed::ZoneName => {
                    s = s.trim_start_matches(|c: char| !c.is_whitespace());
                }

                Fixed::Offset | Fixed::OffsetColon | Fixed::OffsetSeconds | Fixed::OffsetHours => {
                    let (rest, offset) = scan::timezone_offset(
                        s.trim_start(),
                        scan::colon_or_space,
                        false,
                        false,
                        true,
                    )?;
                    s = rest;
                    parsed.set_offset(i64::from(offset))?;
                }

                Fixed::OffsetPermissive => {
                    let (rest, offset) = scan::timezone_offset(
                        s.trim_start(),
                        scan::colon_or_space,
                        true,
                        true,
                        true,
                    )?;
                    s = rest;
                    parsed.set_offset(i64::from(offset))?;
                }

                Fixed::Rfc3339 => {
                    s = relaxed_rfc3339(parsed, s)?;
                }
            },

            Item::Error => return Err(Reason::BadPattern),
        }
    }
    Ok(s)
}

/// How many digits a fixed fraction has.
fn fraction_digits(fixed: Fixed) -> usize {
    match fixed {
        Fixed::Fraction3 | Fixed::Digits3 => 3,
        Fixed::Fraction6 | Fixed::Digits6 => 6,
        _ => 9,
    }
}

/// Reads all of `s` by `items`: text left over is a refusal.
fn read_all<'p>(parsed: &mut Parsed, s: &str, items: impl Iterator<Item = Item<'p>>) -> Scan<()> {
    match read(parsed, s, items)? {
        "" => Ok(()),
        _ => Err(Reason::TooLong),
    }
}

const fn num0(numeric: Numeric) -> Item<'static> {
    Item::Numeric(numeric, Pad::Zero)
}

/// `2026-11-25`, with white space allowed before each number and before
/// each hyphen.
const DATE: &[Item<'static>] = &[
    num0(Numeric::Year),
    Item::Space(""),
    Item::Literal("-"),
    num0(Numeric::Month),
    Item::Space(""),
    Item::Literal("-"),
    num0(Numeric::Day),
];

/// `09:30`.
const HOUR_AND_MINUTE: &[Item<'static>] = &[
    num0(Numeric::Hour),
    Item::Space(""),
    Item::Literal(":"),
    num0(Numeric::Minute),
];

/// `:15` or `:15.25`, and any white space after it.
const SECOND_AND_FRACTION: &[Item<'static>] = &[
    Item::Space(""),
    Item::Literal(":"),
    num0(Numeric::Second),
    Item::Fixed(Fixed::Fraction),
    Item::Space(""),
];

const TRAILING_SPACE: &[Item<'static>] = &[Item::Space("")];

/// The loose form of RFC 3339 that `str::parse` and `%+` take for an
/// instant: the strict form, and also numbers of fewer digits, white space
/// between the parts, a year with a sign, a space or a small `t` for the
/// `T`, an offset with no colon, and `UTC` in any case for the offset.
pub(super) fn relaxed_rfc3339<'t>(parsed: &mut Parsed, s: &'t str) -> Scan<&'t str> {
    let s = read(parsed, s, DATE.iter().copied())?;

    let s = match s.as_bytes().first() {
        Some(&b't' | &b'T' | &b' ') => &s[1..],
        Some(_) => return Err(Reason::Invalid),
        None => return Err(Reason::TooShort),
    };

    let s = read(
        parsed,
        s,
        HOUR_AND_MINUTE.iter().chain(SECOND_AND_FRACTION).copied(),
    )?;
    let s = s.trim_start();
    let (s, offset) = if s.len() >= 3 && s.as_bytes()[..3].eq_ignore_ascii_case(b"UTC") {
        (&s[3..], 0)
    } else {
        scan::timezone_offset(s, scan::colon_or_space, true, false, true)?
    };
    parsed.set_offset(i64::from(offset))?;
    Ok(s)
}

/// The day `text` names, laid out as `pattern`.
pub(crate) fn date(pattern: &str, text: &str) -> Scan<Date> {
    let mut parsed = Parsed::default();
    read_all(&mut parsed, text, Items::new(pattern))?;
    parsed.to_date()
}

/// The time of day `text` names, laid out as `pattern`.
pub(crate) fn time(pattern: &str, text: &str) -> Scan<Time> {
    let mut parsed = Parsed::default();
    read_all(&mut parsed, text, Items::new(pattern))?;
    parsed.to_time().map(|(time, _)| time)
}

/// The day and the time of day `text` names, laid out as `pattern`.
pub(crate) fn date_time(pattern: &str, text: &str) -> Scan<DateTime> {
    let mut parsed = Parsed::default();
    read_all(&mut parsed, text, Items::new(pattern))?;
    parsed.to_date_time(0).map(|(date_time, _)| date_time)
}

/// The instant `text` names, laid out as `pattern`, and the offset from UTC
/// it was written with.
pub(crate) fn instant(pattern: &str, text: &str) -> Scan<(Timestamp, Offset)> {
    let mut parsed = Parsed::default();
    read_all(&mut parsed, text, Items::new(pattern))?;
    parsed.to_instant()
}

/// The instant `text` names in the loose form of RFC 3339, and the offset
/// from UTC it was written with.
pub(crate) fn instant_from_str(text: &str) -> Scan<(Timestamp, Offset)> {
    let mut parsed = Parsed::default();
    let rest = relaxed_rfc3339(&mut parsed, text)?;
    if !rest.trim_start().is_empty() {
        return Err(Reason::TooLong);
    }
    parsed.to_instant()
}

impl Date {
    /// Reads a day from `text` laid out as `pattern`:
    /// [the specifiers](crate::pattern#the-specifiers) say what a pattern
    /// can hold and [how it is read](crate::pattern#reading).
    ///
    /// The pattern has to name a day in full. What it reads of a time or an
    /// offset is dropped.
    ///
    /// ```
    /// use navree_datetime::Date;
    ///
    /// # fn main() -> navree_datetime::Result<()> {
    /// let day = Date::new(2026, 11, 25)?;
    /// assert_eq!(Date::parse("%d/%m/%Y", "25/11/2026")?, day);
    /// assert_eq!(Date::parse("%-d %B %Y", "25 November 2026")?, day);
    /// assert_eq!(Date::parse("%Y%m%d", "20261125")?, day);
    /// assert_eq!(Date::parse("%Y-%j", "2026-329")?, day);
    ///
    /// // The 25th was a Wednesday.
    /// assert!(Date::parse("%a %d/%m/%Y", "Mon 25/11/2026").is_err());
    /// assert!(Date::parse("%d/%m", "25/11").is_err());
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// An error of the kind [`Parse`](crate::ErrorKind::Parse) when the
    /// pattern is not one, the text is not laid out as the pattern, its
    /// parts name no day or contradict each other, or they do not name a
    /// day in full.
    pub fn parse(pattern: &str, text: &str) -> Result<Date> {
        Ok(date(pattern, text)?)
    }
}

impl Time {
    /// Reads a time of day from `text` laid out as `pattern`:
    /// [the specifiers](crate::pattern#the-specifiers) say what a pattern
    /// can hold and [how it is read](crate::pattern#reading).
    ///
    /// The pattern has to name an hour and a minute; a second it does not
    /// name is 0. What it reads of a day or an offset is dropped. Second 60
    /// is read as the last nanosecond of its minute.
    ///
    /// ```
    /// use navree_datetime::Time;
    ///
    /// # fn main() -> navree_datetime::Result<()> {
    /// assert_eq!(Time::parse("%H:%M", "09:30")?, Time::new(9, 30, 0)?);
    /// assert_eq!(Time::parse("%I:%M %p", "9:30 pm")?, Time::new(21, 30, 0)?);
    /// assert_eq!(
    ///     Time::parse("%H:%M:%S%.f", "09:30:15.25")?,
    ///     Time::new(9, 30, 15)?.with_nanos(250_000_000)?
    /// );
    /// assert_eq!(
    ///     Time::parse("%H:%M:%S", "23:59:60")?,
    ///     Time::new(23, 59, 59)?.with_nanos(999_999_999)?
    /// );
    /// assert!(Time::parse("%H:%M", "24:00").is_err());
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// An error of the kind [`Parse`](crate::ErrorKind::Parse) when the
    /// pattern is not one, the text is not laid out as the pattern, its
    /// parts name no time of day or contradict each other, or the hour or
    /// the minute is missing.
    pub fn parse(pattern: &str, text: &str) -> Result<Time> {
        Ok(time(pattern, text)?)
    }
}

impl DateTime {
    /// Reads a day and a time of day from `text` laid out as `pattern`:
    /// [the specifiers](crate::pattern#the-specifiers) say what a pattern
    /// can hold and [how it is read](crate::pattern#reading).
    ///
    /// The pattern has to name both, as [`Date::parse`] and [`Time::parse`]
    /// need them. An offset it reads is dropped: the day and the time are
    /// the ones written.
    ///
    /// ```
    /// use navree_datetime::{Date, DateTime, Time};
    ///
    /// # fn main() -> navree_datetime::Result<()> {
    /// let due = Date::new(2026, 11, 25)?.at(Time::new(17, 30, 0)?);
    /// assert_eq!(DateTime::parse("%d/%m/%Y %H:%M", "25/11/2026 17:30")?, due);
    /// assert_eq!(DateTime::parse("%Y-%m-%dT%H:%M:%S%.f", "2026-11-25T17:30:00")?, due);
    /// assert!(DateTime::parse("%d/%m/%Y", "25/11/2026").is_err());
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// An error of the kind [`Parse`](crate::ErrorKind::Parse) when the
    /// pattern is not one, the text is not laid out as the pattern, its
    /// parts name no day or no time or contradict each other, or they do
    /// not name both in full.
    pub fn parse(pattern: &str, text: &str) -> Result<DateTime> {
        Ok(date_time(pattern, text)?)
    }
}

impl Timestamp {
    /// Reads an instant from `text` laid out as `pattern`:
    /// [the specifiers](crate::pattern#the-specifiers) say what a pattern
    /// can hold and [how it is read](crate::pattern#reading).
    ///
    /// A day and a time of day are an instant only with an offset from UTC,
    /// so the pattern has to read one (`%z`, `%:z`, `%#z` or `%+`), or
    /// seconds since the epoch (`%s`). For text that is in UTC and does not
    /// say so, read a [`DateTime`] and take
    /// [`in_utc`](DateTime::in_utc).
    ///
    /// ```
    /// use navree_datetime::{DateTime, Timestamp};
    ///
    /// # fn main() -> navree_datetime::Result<()> {
    /// let sent = Timestamp::utc(2026, 11, 25, 12, 0, 0)?;
    /// assert_eq!(
    ///     Timestamp::parse("%d/%m/%Y %H:%M %z", "25/11/2026 17:30 +0530")?,
    ///     sent
    /// );
    /// assert_eq!(Timestamp::parse("%s", "1795608000")?, sent);
    ///
    /// // No offset: not an instant yet.
    /// assert!(Timestamp::parse("%Y-%m-%d %H:%M:%S", "2026-11-25 12:00:00").is_err());
    /// assert_eq!(
    ///     DateTime::parse("%Y-%m-%d %H:%M:%S", "2026-11-25 12:00:00")?.in_utc(),
    ///     sent
    /// );
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// An error of the kind [`Parse`](crate::ErrorKind::Parse) when the
    /// pattern is not one, the text is not laid out as the pattern, its
    /// parts name no instant or contradict each other, there is no offset,
    /// the offset is 24 hours or more, or the instant is before
    /// [`Timestamp::MIN`] or after [`Timestamp::MAX`].
    pub fn parse(pattern: &str, text: &str) -> Result<Timestamp> {
        Ok(instant(pattern, text)?.0)
    }
}

impl Zoned {
    /// Reads an instant and the fixed offset it was written with from `text`
    /// laid out as `pattern`:
    /// [the specifiers](crate::pattern#the-specifiers) say what a pattern
    /// can hold and [how it is read](crate::pattern#reading).
    ///
    /// As with [`Timestamp::parse`], the pattern has to read an offset
    /// (`%z`, `%:z`, `%#z` or `%+`) or seconds since the epoch (`%s`); the
    /// zone of the value returned is [`Zone::fixed`] at that offset (zero
    /// for `%s` alone).
    ///
    /// ```
    /// use navree_datetime::{Offset, Timestamp, Zoned};
    ///
    /// # fn main() -> navree_datetime::Result<()> {
    /// let read = Zoned::parse("%d/%m/%Y %H:%M %z", "25/11/2026 17:30 +0530")?;
    /// assert_eq!(read.timestamp(), Timestamp::utc(2026, 11, 25, 12, 0, 0)?);
    /// assert_eq!(read.offset(), Offset::east(19_800)?);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// An error of the kind [`Parse`](crate::ErrorKind::Parse) when the
    /// pattern is not one, the text is not laid out as the pattern, its
    /// parts name no instant or contradict each other, there is no offset,
    /// the offset is 24 hours or more, or the instant is before
    /// [`Timestamp::MIN`] or after [`Timestamp::MAX`].
    pub fn parse(pattern: &str, text: &str) -> Result<Zoned> {
        let (instant, offset) = instant(pattern, text)?;
        Ok(Zoned::new(instant, Zone::fixed(offset)))
    }
}

/// A day from `2026-11-25`, the text [`Display`](std::fmt::Display) writes.
///
/// The numbers may have fewer digits (`2026-1-5`) and white space around
/// them. A year before 0 or after 9999 has its sign (`-0044-03-15`,
/// `+12026-01-01`), and without a sign a year has at most four digits.
///
/// ```
/// use navree_datetime::Date;
///
/// # fn main() -> navree_datetime::Result<()> {
/// let day: Date = "2026-11-25".parse()?;
/// assert_eq!(day, Date::new(2026, 11, 25)?);
/// assert_eq!(day.to_string().parse::<Date>()?, day);
/// assert!("25/11/2026".parse::<Date>().is_err());
/// assert!("2026-02-30".parse::<Date>().is_err());
/// # Ok(())
/// # }
/// ```
impl FromStr for Date {
    type Err = Error;

    fn from_str(text: &str) -> Result<Date> {
        let mut parsed = Parsed::default();
        read_all(
            &mut parsed,
            text,
            DATE.iter().chain(TRAILING_SPACE).copied(),
        )?;
        Ok(parsed.to_date()?)
    }
}

/// A time of day from `09:30:15`, the text [`Display`](std::fmt::Display)
/// writes, with any fraction of the second after a dot (`09:30:15.25`).
///
/// The seconds may be missing (`09:30`), the numbers may have fewer digits
/// and white space around them. Second 60 is read as the last nanosecond of
/// its minute: `23:59:60` is `23:59:59.999999999`.
///
/// ```
/// use navree_datetime::Time;
///
/// # fn main() -> navree_datetime::Result<()> {
/// let opens: Time = "09:30".parse()?;
/// assert_eq!(opens, Time::new(9, 30, 0)?);
/// assert_eq!("09:30:00".parse::<Time>()?, opens);
/// assert_eq!(
///     "09:30:00.5".parse::<Time>()?,
///     opens.with_nanos(500_000_000)?
/// );
/// assert!("9.30".parse::<Time>().is_err());
/// assert!("24:00".parse::<Time>().is_err());
/// # Ok(())
/// # }
/// ```
impl FromStr for Time {
    type Err = Error;

    fn from_str(text: &str) -> Result<Time> {
        let mut parsed = Parsed::default();
        let rest = read(&mut parsed, text, HOUR_AND_MINUTE.iter().copied())?;
        // The seconds may be missing: where they cannot be read, what
        // follows the minute has to be white space.
        let rest = read(&mut parsed, rest, SECOND_AND_FRACTION.iter().copied()).unwrap_or(rest);
        read_all(&mut parsed, rest, TRAILING_SPACE.iter().copied())?;
        Ok(parsed.to_time()?.0)
    }
}

/// A day and a time of day from `2026-11-25T09:30:15`, with any fraction of
/// the second after a dot.
///
/// That is the text [`Debug`](std::fmt::Debug) writes and JSON holds, with
/// a capital `T` and the seconds. The text
/// [`Display`](std::fmt::Display) writes has a space there and is read by
/// `DateTime::parse("%Y-%m-%d %H:%M:%S%.f", text)`.
///
/// The numbers may have fewer digits and white space around them, and a
/// year before 0 or after 9999 has its sign. Second 60 is read as the last
/// nanosecond of its minute.
///
/// ```
/// use navree_datetime::{Date, DateTime, Time};
///
/// # fn main() -> navree_datetime::Result<()> {
/// let due: DateTime = "2026-11-25T17:30:00".parse()?;
/// assert_eq!(due, Date::new(2026, 11, 25)?.at(Time::new(17, 30, 0)?));
/// assert!("2026-11-25 17:30:00".parse::<DateTime>().is_err());
/// assert!("2026-11-25T17:30".parse::<DateTime>().is_err());
/// assert!("2026-11-25T17:30:00Z".parse::<DateTime>().is_err());
/// # Ok(())
/// # }
/// ```
impl FromStr for DateTime {
    type Err = Error;

    fn from_str(text: &str) -> Result<DateTime> {
        const T: &[Item<'static>] = &[Item::Space(""), Item::Literal("T")];

        let mut parsed = Parsed::default();
        read_all(
            &mut parsed,
            text,
            DATE.iter()
                .chain(T)
                .chain(HOUR_AND_MINUTE)
                .chain(SECOND_AND_FRACTION)
                .copied(),
        )?;
        Ok(parsed.to_date_time(0)?.0)
    }
}

/// An instant from RFC 3339 text (`2026-11-25T12:00:00Z`,
/// `2026-11-25T17:30:00+05:30`), read loosely.
///
/// Beyond what [`Timestamp::parse_rfc3339`] takes, the numbers may have
/// fewer digits and white space around them, the offset may have no colon
/// (`+0530`) or be `UTC` in any case, and a year before 0 or after 9999 is
/// read with its sign. So the text [`Debug`](std::fmt::Debug) and JSON
/// write is read back, for every instant there is, and so is the text
/// [`Display`](std::fmt::Display) writes (`2026-11-25 12:00:00 UTC`).
///
/// The text has to say its offset, which is then taken off: the instant is
/// the same one whatever offset it was written with. Second 60 is read as
/// the last nanosecond of its minute.
///
/// ```
/// use navree_datetime::Timestamp;
///
/// # fn main() -> navree_datetime::Result<()> {
/// let sent = Timestamp::utc(2026, 11, 25, 12, 0, 0)?;
/// assert_eq!("2026-11-25T12:00:00Z".parse::<Timestamp>()?, sent);
/// assert_eq!("2026-11-25T17:30:00+05:30".parse::<Timestamp>()?, sent);
/// assert_eq!("2026-11-25 17:30:00 +0530".parse::<Timestamp>()?, sent);
/// assert_eq!(sent.to_string().parse::<Timestamp>()?, sent);
/// assert_eq!(
///     format!("{:?}", Timestamp::MAX).parse::<Timestamp>()?,
///     Timestamp::MAX
/// );
/// assert!("2026-11-25T12:00:00".parse::<Timestamp>().is_err());
/// # Ok(())
/// # }
/// ```
impl FromStr for Timestamp {
    type Err = Error;

    fn from_str(text: &str) -> Result<Timestamp> {
        Ok(instant_from_str(text)?.0)
    }
}

/// An instant and the fixed offset it was written with, read from the loose
/// form of RFC 3339 that [`Timestamp`] reads (`2026-11-25T17:30:00+05:30`,
/// `2026-11-25 12:00:00 UTC`).
///
/// The zone of the value returned is [`Zone::fixed`] at the offset read.
/// Second 60 is read as the last nanosecond of its minute.
///
/// ```
/// use navree_datetime::{Offset, Timestamp, Zoned};
///
/// # fn main() -> navree_datetime::Result<()> {
/// let sent: Zoned = "2026-11-25T17:30:00+05:30".parse()?;
/// assert_eq!(sent.timestamp(), Timestamp::utc(2026, 11, 25, 12, 0, 0)?);
/// assert_eq!(sent.offset(), Offset::east(19_800)?);
/// # Ok(())
/// # }
/// ```
impl FromStr for Zoned {
    type Err = Error;

    fn from_str(text: &str) -> Result<Zoned> {
        let (instant, offset) = instant_from_str(text)?;
        Ok(Zoned::new(instant, Zone::fixed(offset)))
    }
}

/// A named zone of the IANA database from its name (`Asia/Kolkata`, `UTC`),
/// trimmed, in any case, with legacy names replaced by the current ones as
/// [`Zone::named`] reads them.
///
/// ```
/// use navree_datetime::Zone;
///
/// # fn main() -> navree_datetime::Result<()> {
/// let kolkata: Zone = "asia/calcutta".parse()?;
/// assert_eq!(kolkata.name(), "Asia/Kolkata");
/// assert!("+05:30".parse::<Zone>().is_err());
/// # Ok(())
/// # }
/// ```
impl FromStr for Zone {
    type Err = Error;

    fn from_str(text: &str) -> Result<Zone> {
        Zone::named(text).map_err(|error| Error::parse(error.to_string()))
    }
}

/// An offset from `+05:30` or `+0530`: a sign, two digits of hours and two
/// of minutes.
///
/// The sign is always there, and both the hours and the minutes: `+05` and
/// `Z` are refused. Reading stops after the minutes, and what follows them
/// is not looked at. An offset of 24 hours or more is refused: text holds
/// none, though an [`Offset`] made from a number of seconds can be a little
/// larger.
///
/// ```
/// use navree_datetime::Offset;
///
/// # fn main() -> navree_datetime::Result<()> {
/// let india: Offset = "+05:30".parse()?;
/// assert_eq!(india.seconds(), 19_800);
/// assert_eq!("-0800".parse::<Offset>()?.seconds(), -28_800);
/// assert!("05:30".parse::<Offset>().is_err());
/// assert!("+24:00".parse::<Offset>().is_err());
/// # Ok(())
/// # }
/// ```
impl FromStr for Offset {
    type Err = Error;

    fn from_str(text: &str) -> Result<Offset> {
        let (_, seconds_east) =
            scan::timezone_offset(text, scan::colon_or_space, false, false, true)?;
        Ok(offset_within_a_day(seconds_east)?)
    }
}

/// A weekday from its English name or the name's first three letters, in
/// any case: `Wednesday`, `wed`, `WED`. Nothing else is one: not `Weds`,
/// and not a name with anything after it.
///
/// ```
/// use navree_datetime::Weekday;
///
/// # fn main() -> navree_datetime::Result<()> {
/// assert_eq!("Wednesday".parse::<Weekday>()?, Weekday::Wednesday);
/// assert_eq!("wed".parse::<Weekday>()?, Weekday::Wednesday);
/// assert!("Weds".parse::<Weekday>().is_err());
/// # Ok(())
/// # }
/// ```
impl FromStr for Weekday {
    type Err = Error;

    fn from_str(text: &str) -> Result<Weekday> {
        match scan::short_or_long_weekday(text) {
            Ok(("", weekday)) => Ok(weekday),
            _ => Err(Error::parse(
                "a weekday is its English name or the name's first three letters",
            )),
        }
    }
}

/// A month from its English name or the name's first three letters, in any
/// case: `November`, `nov`, `NOV`. Nothing else is one: not `Sept`, and not
/// a name with anything after it.
///
/// ```
/// use navree_datetime::Month;
///
/// # fn main() -> navree_datetime::Result<()> {
/// assert_eq!("November".parse::<Month>()?, Month::November);
/// assert_eq!("nov".parse::<Month>()?, Month::November);
/// assert!("Sept".parse::<Month>().is_err());
/// # Ok(())
/// # }
/// ```
impl FromStr for Month {
    type Err = Error;

    fn from_str(text: &str) -> Result<Month> {
        match scan::short_or_long_month0(text) {
            Ok(("", month0)) => Month::try_from(month0 + 1),
            _ => Err(Error::parse(
                "a month is its English name or the name's first three letters",
            )),
        }
    }
}
