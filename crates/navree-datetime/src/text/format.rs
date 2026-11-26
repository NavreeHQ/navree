//! Writing a value by a pattern.

use std::fmt::{self, Write};

use super::display::{self, two_digits};
use super::pattern::{self, Fixed, Item, Items, Numeric, Pad};
use super::rfc3339::{self, Precision};
use super::{Fields, quarter, weeks_from};
use crate::{Date, DateTime, Error, Offset, Result, Time, Timestamp, Weekday, Zoned};

/// A value laid out by a pattern, ready to be written: what
/// `value.format(pattern)` gives.
///
/// It is text to be: [`Display`](fmt::Display) writes it, so it goes
/// straight into `format!`, `write!` and `to_string()`, padded and cut as
/// any text is.
///
/// ```
/// use navree_datetime::Date;
///
/// # fn main() -> navree_datetime::Result<()> {
/// let day = Date::new(2026, 11, 25)?;
/// assert_eq!(day.format("%d %b %Y").to_string(), "25 Nov 2026");
/// assert_eq!(format!("Due {}", day.format("%-d %B")), "Due 25 November");
/// assert_eq!(format!("[{:>12}]", day.format("%d/%m/%Y")), "[  25/11/2026]");
/// # Ok(())
/// # }
/// ```
///
/// # A pattern that cannot be written
///
/// Nothing is checked when it is made. Two things can be wrong when it is
/// written: the pattern is not one (`%Q`), or it asks the value for a part
/// it does not have (the hour of a [`Date`](crate::Date), the offset of a
/// [`DateTime`](crate::DateTime)). `Display` then returns an error, which
/// `to_string()` and `format!` turn into a panic.
///
/// A pattern written in the code is right or wrong once and for all, and a
/// test shows which. For a pattern that comes from outside (a setting, a
/// template), [`to_text`](Formatted::to_text) gives an [`Error`] in place
/// of the panic, and [`pattern::check`] says beforehand whether it is a
/// pattern at all.
///
/// ```
/// use navree_datetime::Date;
///
/// # fn main() -> navree_datetime::Result<()> {
/// let day = Date::new(2026, 11, 25)?;
/// assert_eq!(day.format("%d %b %Y").to_text()?, "25 Nov 2026");
/// assert!(day.format("%d %Q").to_text().is_err());
/// assert!(day.format("%d %b %Y at %H:%M").to_text().is_err());
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Copy)]
pub struct Formatted<'a> {
    fields: Fields<'a>,
    pattern: &'a str,
}

/// Why a value could not be written by a pattern.
enum Failure {
    /// The pattern is not one.
    Pattern,
    /// The pattern asks for a part the value does not have.
    Missing(&'static str),
    /// `%#z`, which only reads.
    ReadOnly,
    /// The text could not be written to.
    Write,
}

impl From<fmt::Error> for Failure {
    fn from(_: fmt::Error) -> Failure {
        Failure::Write
    }
}

impl<'a> Formatted<'a> {
    /// `fields` to be written as `pattern`.
    pub(crate) const fn new(fields: Fields<'a>, pattern: &'a str) -> Formatted<'a> {
        Formatted { fields, pattern }
    }

    /// The text, or an error where [`Display`](fmt::Display) would fail and
    /// `to_string()` panic.
    ///
    /// # Errors
    ///
    /// An error of the kind [`Parse`](crate::ErrorKind::Parse) when the
    /// pattern is not one, with the specifier that is wrong and where it
    /// is. An error of the kind [`Invalid`](crate::ErrorKind::Invalid) when
    /// the pattern asks for a part the value does not have: a time of day
    /// from a day, a day from a time of day, an offset or a zone from a
    /// value with none, or anything through `%#z`, which only reads.
    pub fn to_text(&self) -> Result<String> {
        let mut text = String::new();
        match self.write(&mut text) {
            Ok(()) => Ok(text),
            Err(Failure::Pattern) => Err(match pattern::check(self.pattern) {
                Err(error) => Error::from(error),
                // The pattern was refused a moment ago.
                Ok(()) => Error::parse("the pattern could not be read"),
            }),
            Err(Failure::Missing(part)) => Err(Error::invalid(format!(
                "the pattern asks for {part}, and the value has none"
            ))),
            Err(Failure::ReadOnly) => Err(Error::invalid(
                "`%#z` reads an offset and cannot write one: `%z` and `%:z` write it",
            )),
            Err(Failure::Write) => Err(Error::invalid("the text could not be written")),
        }
    }

    /// Writes every step of the pattern to `w`.
    fn write(&self, w: &mut String) -> Result<(), Failure> {
        for item in Items::new(self.pattern) {
            match item {
                Item::Literal(text) | Item::Space(text) => w.push_str(text),
                Item::Numeric(numeric, pad) => self.numeric(w, numeric, pad)?,
                Item::Fixed(fixed) => self.fixed(w, fixed)?,
                Item::Error => return Err(Failure::Pattern),
            }
        }
        Ok(())
    }

    fn date(&self) -> Result<Date, Failure> {
        self.fields.date.ok_or(Failure::Missing("a day"))
    }

    fn time(&self) -> Result<Time, Failure> {
        self.fields.time.ok_or(Failure::Missing("a time of day"))
    }

    fn offset(&self) -> Result<Offset, Failure> {
        self.fields
            .offset
            .ok_or(Failure::Missing("an offset from UTC"))
    }

    fn numeric(&self, w: &mut String, numeric: Numeric, pad: Pad) -> Result<(), Failure> {
        match numeric {
            Numeric::Year => write_year(w, self.date()?.year(), pad)?,
            // The century and the two digits are cut to a byte before they
            // are written, which is all a year of 0 to 9999 needs. Outside
            // those years `%C` writes what is left after the cut.
            Numeric::YearDiv100 => write_two(w, self.date()?.year().div_euclid(100) as u8, pad)?,
            Numeric::YearMod100 => write_two(w, self.date()?.year().rem_euclid(100) as u8, pad)?,
            Numeric::IsoYear => write_year(w, self.date()?.iso_week().0, pad)?,
            Numeric::IsoYearMod100 => {
                write_two(w, self.date()?.iso_week().0.rem_euclid(100) as u8, pad)?;
            }
            Numeric::Quarter => write_one(w, quarter(self.date()?) as u8)?,
            Numeric::Month => write_two(w, self.date()?.month_number(), pad)?,
            Numeric::Day => write_two(w, self.date()?.day(), pad)?,
            Numeric::WeekFromSun => {
                write_two(w, weeks_from(self.date()?, Weekday::Sunday) as u8, pad)?;
            }
            Numeric::WeekFromMon => {
                write_two(w, weeks_from(self.date()?, Weekday::Monday) as u8, pad)?;
            }
            Numeric::IsoWeek => write_two(w, self.date()?.iso_week().1, pad)?,
            Numeric::DaysFromSun => write_one(w, self.date()?.weekday().days_from_sunday())?,
            Numeric::WeekdayFromMon => write_one(w, self.date()?.weekday().number_from_monday())?,
            Numeric::Ordinal => write_n(w, 3, i64::from(self.date()?.ordinal()), pad, false)?,
            Numeric::Hour => write_two(w, self.time()?.hour(), pad)?,
            Numeric::Hour12 => write_two(w, hour12(self.time()?), pad)?,
            Numeric::Minute => write_two(w, self.time()?.minute(), pad)?,
            Numeric::Second => write_two(w, self.time()?.second(), pad)?,
            Numeric::Nanosecond => {
                write_n(w, 9, i64::from(self.time()?.nanosecond()), pad, false)?;
            }
            Numeric::Timestamp => {
                // The day and the time are on a clock `offset` from UTC,
                // or in UTC where the value has no offset.
                let local = self.date()?.at(self.time()?);
                let offset = self.fields.offset.map_or(0, Offset::seconds);
                let timestamp = local.in_utc().as_secs() - i64::from(offset);
                write_n(w, 9, timestamp, pad, false)?;
            }
        }
        Ok(())
    }

    fn fixed(&self, w: &mut String, fixed: Fixed) -> Result<(), Failure> {
        match fixed {
            Fixed::ShortMonthName => w.push_str(self.date()?.month().short_name()),
            Fixed::LongMonthName => w.push_str(self.date()?.month().name()),
            Fixed::ShortWeekdayName => w.push_str(self.date()?.weekday().short_name()),
            Fixed::LongWeekdayName => w.push_str(self.date()?.weekday().name()),
            Fixed::LowerAmPm => w.push_str(if self.time()?.hour() >= 12 {
                "pm"
            } else {
                "am"
            }),
            Fixed::UpperAmPm => w.push_str(if self.time()?.hour() >= 12 {
                "PM"
            } else {
                "AM"
            }),
            Fixed::Fraction => display::fraction(w, self.time()?.nanosecond())?,
            Fixed::Fraction3 => write!(w, ".{:03}", self.time()?.nanosecond() / 1_000_000)?,
            Fixed::Fraction6 => write!(w, ".{:06}", self.time()?.nanosecond() / 1_000)?,
            Fixed::Fraction9 => write!(w, ".{:09}", self.time()?.nanosecond())?,
            Fixed::Digits3 => write!(w, "{:03}", self.time()?.nanosecond() / 1_000_000)?,
            Fixed::Digits6 => write!(w, "{:06}", self.time()?.nanosecond() / 1_000)?,
            Fixed::Digits9 => write!(w, "{:09}", self.time()?.nanosecond())?,
            Fixed::ZoneName => match (self.fields.abbreviation, self.fields.offset) {
                (Some(abbreviation), _) => w.push_str(abbreviation),
                // A value with an offset and no zone: the offset is all the
                // name it has.
                (None, Some(offset)) => display::offset(w, offset)?,
                (None, None) => return Err(Failure::Missing("a zone")),
            },
            Fixed::Offset => {
                write_offset(w, self.offset()?, OffsetPrecision::Minutes, false, false)?;
            }
            Fixed::OffsetColon => {
                write_offset(w, self.offset()?, OffsetPrecision::Minutes, true, false)?;
            }
            Fixed::OffsetSeconds => {
                write_offset(w, self.offset()?, OffsetPrecision::Seconds, true, false)?;
            }
            Fixed::OffsetHours => {
                write_offset(w, self.offset()?, OffsetPrecision::Hours, false, false)?;
            }
            Fixed::OffsetPermissive => return Err(Failure::ReadOnly),
            Fixed::Rfc3339 => {
                let local = self.date()?.at(self.time()?);
                rfc3339::write(w, local, self.offset()?, Precision::Auto, false)?;
            }
        }
        Ok(())
    }
}

/// The text the pattern lays the value out as, padded and cut as text is.
///
/// # Errors
///
/// It fails, and `to_string()` panics, when the pattern is not one or asks
/// for a part the value does not have. [`to_text`](Formatted::to_text) says
/// which.
impl fmt::Display for Formatted<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut text = String::new();
        self.write(&mut text).map_err(|_| fmt::Error)?;
        f.pad(&text)
    }
}

impl fmt::Debug for Formatted<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Formatted")
            .field("pattern", &self.pattern)
            .field("date", &self.fields.date)
            .field("time", &self.fields.time)
            .field("offset", &self.fields.offset)
            .field("zone", &self.fields.zone)
            .field("abbreviation", &self.fields.abbreviation)
            .finish()
    }
}

/// The hour of a 12-hour clock: 12 for midnight and for noon.
fn hour12(time: Time) -> u8 {
    match time.hour() % 12 {
        0 => 12,
        hour => hour,
    }
}

/// One digit. It is for a number below 10.
fn write_one(w: &mut String, v: u8) -> fmt::Result {
    w.write_char((b'0' + v) as char)
}

/// A number of two digits, with `pad` before it where it has one. A number
/// of 100 or more, which no part of a date within the years 0 to 9999 is,
/// is written by its two last places.
fn write_two(w: &mut String, v: u8, pad: Pad) -> fmt::Result {
    let ones = b'0' + v % 10;
    match (v / 10, pad) {
        (0, Pad::None) => {}
        (0, Pad::Space) => w.write_char(' ')?,
        (tens, _) => w.write_char((b'0' + tens) as char)?,
    }
    w.write_char(ones as char)
}

/// A year, in four digits where it has fewer. The years 1000 to 9999 are
/// four digits whatever the padding. A year before 0 or after 9999 always
/// has its sign, which counts as one more place.
fn write_year(w: &mut String, year: i32, pad: Pad) -> fmt::Result {
    if (1000..=9999).contains(&year) {
        two_digits(w, (year / 100) as u8)?;
        two_digits(w, (year % 100) as u8)
    } else {
        write_n(w, 4, i64::from(year), pad, !(0..10_000).contains(&year))
    }
}

/// A number padded to `n` places, with its sign always there when
/// `always_sign`, in one more place.
fn write_n(w: &mut String, n: usize, v: i64, pad: Pad, always_sign: bool) -> fmt::Result {
    if always_sign {
        match pad {
            Pad::None => write!(w, "{v:+}"),
            Pad::Zero => write!(w, "{:+01$}", v, n + 1),
            Pad::Space => write!(w, "{:+1$}", v, n + 1),
        }
    } else {
        match pad {
            Pad::None => write!(w, "{v}"),
            Pad::Zero => write!(w, "{v:0n$}"),
            Pad::Space => write!(w, "{v:n$}"),
        }
    }
}

/// How much of an offset is written.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum OffsetPrecision {
    /// The hours alone. The minutes and the seconds are dropped.
    Hours,
    /// The hours and the minutes. Seconds are rounded to the nearest
    /// minute.
    Minutes,
    /// The hours, the minutes and the seconds.
    Seconds,
}

/// An offset as a sign and two digits of hours, then the minutes and the
/// seconds as far as `precision` goes, with a colon before each when
/// `colons`. `allow_z` writes an offset of exactly zero as `Z`.
pub(crate) fn write_offset(
    w: &mut impl Write,
    offset: Offset,
    precision: OffsetPrecision,
    colons: bool,
    allow_z: bool,
) -> fmt::Result {
    let off = offset.seconds();
    if allow_z && off == 0 {
        return w.write_char('Z');
    }
    let (sign, off) = if off < 0 { ('-', -off) } else { ('+', off) };

    let hours;
    let mut mins = 0;
    let mut secs = 0;
    match precision {
        OffsetPrecision::Hours => {
            hours = (off / 3_600) as u8;
        }
        OffsetPrecision::Minutes => {
            let minutes = (off + 30) / 60;
            mins = (minutes % 60) as u8;
            hours = (minutes / 60) as u8;
        }
        OffsetPrecision::Seconds => {
            let minutes = off / 60;
            secs = (off % 60) as u8;
            mins = (minutes % 60) as u8;
            hours = (minutes / 60) as u8;
        }
    }

    w.write_char(sign)?;
    two_digits(w, hours)?;
    if precision != OffsetPrecision::Hours {
        if colons {
            w.write_char(':')?;
        }
        two_digits(w, mins)?;
    }
    if precision == OffsetPrecision::Seconds {
        if colons {
            w.write_char(':')?;
        }
        two_digits(w, secs)?;
    }
    Ok(())
}

impl Date {
    /// The day laid out as `pattern`:
    /// [the specifiers](crate::pattern#the-specifiers) say what a pattern
    /// can hold.
    ///
    /// A day has no time of day and no offset, so a pattern that asks for
    /// one (`%H`, `%z`) cannot be written: see [`Formatted`].
    ///
    /// ```
    /// use navree_datetime::Date;
    ///
    /// # fn main() -> navree_datetime::Result<()> {
    /// let day = Date::new(2026, 11, 25)?;
    /// assert_eq!(day.format("%d/%m/%Y").to_string(), "25/11/2026");
    /// assert_eq!(day.format("%-d %b %Y").to_string(), "25 Nov 2026");
    /// assert_eq!(day.format("%A, week %V").to_string(), "Wednesday, week 48");
    /// # Ok(())
    /// # }
    /// ```
    pub const fn format(self, pattern: &str) -> Formatted<'_> {
        Formatted::new(Fields::date(self), pattern)
    }
}

impl Time {
    /// The time of day laid out as `pattern`:
    /// [the specifiers](crate::pattern#the-specifiers) say what a pattern
    /// can hold.
    ///
    /// A time of day has no day and no offset, so a pattern that asks for
    /// one (`%d`, `%z`) cannot be written: see [`Formatted`].
    ///
    /// ```
    /// use navree_datetime::Time;
    ///
    /// # fn main() -> navree_datetime::Result<()> {
    /// let closes = Time::new(17, 30, 0)?;
    /// assert_eq!(closes.format("%H:%M").to_string(), "17:30");
    /// assert_eq!(closes.format("%-I:%M %p").to_string(), "5:30 PM");
    /// assert_eq!(closes.format("%H:%M:%S%.3f").to_string(), "17:30:00.000");
    /// # Ok(())
    /// # }
    /// ```
    pub const fn format(self, pattern: &str) -> Formatted<'_> {
        Formatted::new(Fields::time(self), pattern)
    }
}

impl DateTime {
    /// The day and the time of day laid out as `pattern`:
    /// [the specifiers](crate::pattern#the-specifiers) say what a pattern
    /// can hold.
    ///
    /// A day and a time with no zone have no offset, so a pattern that asks
    /// for one (`%z`, `%Z`, `%+`) cannot be written: see [`Formatted`].
    /// `%s` counts the seconds as if the clock were in UTC.
    ///
    /// ```
    /// use navree_datetime::{Date, Time};
    ///
    /// # fn main() -> navree_datetime::Result<()> {
    /// let due = Date::new(2026, 11, 25)?.at(Time::new(17, 30, 0)?);
    /// assert_eq!(due.format("%d %b %Y, %H:%M").to_string(), "25 Nov 2026, 17:30");
    /// assert_eq!(due.format("%Y-%m-%dT%H:%M:%S%.f").to_string(), "2026-11-25T17:30:00");
    /// # Ok(())
    /// # }
    /// ```
    pub const fn format(self, pattern: &str) -> Formatted<'_> {
        Formatted::new(Fields::date_time(self), pattern)
    }
}

impl Timestamp {
    /// The instant laid out as `pattern`, as a clock in UTC shows it:
    /// [the specifiers](crate::pattern#the-specifiers) say what a pattern
    /// can hold.
    ///
    /// The offset is zero (`%z` is `+0000`) and the zone's name, `%Z`, is
    /// `UTC`.
    ///
    /// ```
    /// use navree_datetime::Timestamp;
    ///
    /// # fn main() -> navree_datetime::Result<()> {
    /// let sent = Timestamp::utc(2026, 11, 25, 12, 0, 0)?;
    /// assert_eq!(sent.format("%Y-%m-%d %H:%M %Z").to_string(), "2026-11-25 12:00 UTC");
    /// assert_eq!(sent.format("%Y%m%dT%H%M%SZ").to_string(), "20261125T120000Z");
    /// assert_eq!(sent.format("%s").to_string(), "1795608000");
    /// # Ok(())
    /// # }
    /// ```
    pub const fn format(self, pattern: &str) -> Formatted<'_> {
        Formatted::new(Fields::utc(self.date_time_utc()), pattern)
    }
}

impl Zoned {
    /// The instant seen in its zone laid out as `pattern`:
    /// [the specifiers](crate::pattern#the-specifiers) say what a pattern
    /// can hold.
    ///
    /// `%Z` writes the zone's short name at that instant (`IST`, `UTC`), or
    /// its offset (`+05:30`) where the zone is a fixed offset.
    ///
    /// ```
    /// use navree_datetime::{Offset, Timestamp, Zone};
    ///
    /// # fn main() -> navree_datetime::Result<()> {
    /// let sent = Timestamp::utc(2026, 11, 25, 12, 0, 0)?;
    /// let kolkata = Zone::named("Asia/Kolkata")?;
    /// assert_eq!(
    ///     sent.in_zone(kolkata).format("%d %b %Y, %H:%M %Z").to_string(),
    ///     "25 Nov 2026, 17:30 IST"
    /// );
    /// let fixed = Zone::fixed(Offset::east(19_800)?);
    /// assert_eq!(
    ///     sent.in_zone(fixed).format("%H:%M %Z").to_string(),
    ///     "17:30 +05:30"
    /// );
    /// # Ok(())
    /// # }
    /// ```
    pub fn format(self, pattern: &str) -> Formatted<'_> {
        Formatted::new(Fields::zoned(self), pattern)
    }
}
