//! The text of dates and times: how each value is written and how it is
//! read.
//!
//! - [`display`]: `Display` and `Debug`, the fixed text of each value.
//! - [`rfc3339`] and [`rfc2822`]: the two standard forms of an instant.
//! - [`pattern`]: the specifiers of a pattern, in one table.
//! - [`format`]: writing a value by a pattern.
//! - [`parse`]: reading a value by a pattern and by `str::parse`, with
//!   [`parsed`] for what the parts of a text add up to and [`scan`] for the
//!   small scanners under all of them.
//!
//! # One set of fields
//!
//! Nothing here knows the types it serves. Writing works on [`Fields`]: a
//! day, a time of day, an offset, a zone's name and its short name, each of
//! which a value has or has not. A [`Date`] has only the first, a
//! [`DateTime`] the first two, an instant in UTC all but a zone of its own.
//! Reading ends in a day, a time of day, a day and a time, or an instant
//! with the offset its text was written with.
//!
//! A value seen in a zone is one more set of fields: its local day and
//! time, its offset at that instant, its zone's name, and the short name
//! the zone has then. It is written by handing those to
//! [`Formatted::new`], [`rfc3339::write`] and [`rfc2822::write`], shown by
//! [`display::date_time`] with the zone after it, and read by
//! [`rfc3339::parse`], [`rfc2822::parse`], [`parse::instant`] and
//! [`parse::instant_from_str`], which all give the instant and the offset.

pub(crate) mod display;
pub(crate) mod format;
mod names;
pub(crate) mod parse;
mod parsed;
pub mod pattern;
pub(crate) mod rfc2822;
pub(crate) mod rfc3339;
mod scan;

pub use format::Formatted;
pub use rfc3339::Precision;

use crate::{Date, DateTime, Offset, Time, Weekday, Zoned};

/// What a value has for a pattern to write.
///
/// A specifier that needs a part the value does not have cannot be written:
/// `%H` of a day, `%z` of a day and a time with no zone.
#[derive(Clone, Copy)]
pub(crate) struct Fields<'a> {
    /// The day, as a calendar where the value is shows it.
    pub(crate) date: Option<Date>,
    /// The time of day, as a clock where the value is shows it.
    pub(crate) time: Option<Time>,
    /// How far that calendar and clock are from UTC.
    pub(crate) offset: Option<Offset>,
    /// The name of the zone the value is seen in: `Asia/Kolkata`. No
    /// specifier writes it; it is here so that a value shows where it is.
    pub(crate) zone: Option<&'a str>,
    /// What `%Z` writes: the zone's short name at that instant (`IST`), or
    /// `UTC`. A value with an offset and no short name, as one seen at a
    /// fixed offset is, has `None` here, and `%Z` writes its offset
    /// (`+05:30`).
    pub(crate) abbreviation: Option<&'a str>,
}

impl Fields<'static> {
    /// A day, and nothing else.
    pub(crate) const fn date(date: Date) -> Fields<'static> {
        Fields {
            date: Some(date),
            time: None,
            offset: None,
            zone: None,
            abbreviation: None,
        }
    }

    /// A time of day, and nothing else.
    pub(crate) const fn time(time: Time) -> Fields<'static> {
        Fields {
            date: None,
            time: Some(time),
            offset: None,
            zone: None,
            abbreviation: None,
        }
    }

    /// A day and a time of day with no zone.
    pub(crate) const fn date_time(value: DateTime) -> Fields<'static> {
        Fields {
            date: Some(value.date()),
            time: Some(value.time()),
            offset: None,
            zone: None,
            abbreviation: None,
        }
    }

    /// An instant as a clock in UTC shows it: `utc` is its day and time
    /// there.
    pub(crate) const fn utc(utc: DateTime) -> Fields<'static> {
        Fields {
            date: Some(utc.date()),
            time: Some(utc.time()),
            offset: Some(Offset::UTC),
            zone: Some("UTC"),
            abbreviation: Some("UTC"),
        }
    }

    /// An instant seen in a zone: its local day and time, its offset there,
    /// and the zone's name and short name where the zone is one of the
    /// database.
    pub(crate) fn zoned(zoned: Zoned) -> Fields<'static> {
        let (zone, abbreviation) = if zoned.zone().is_fixed() {
            (None, None)
        } else {
            (
                Some(zoned.zone().name()),
                Some(zoned.zone().abbreviation_at(zoned.timestamp())),
            )
        };
        Fields {
            date: Some(zoned.date()),
            time: Some(zoned.time()),
            offset: Some(zoned.offset()),
            zone,
            abbreviation,
        }
    }
}

/// How many days `day` is after `other`, counted forward round the week: 0
/// to 6.
fn days_since(day: Weekday, other: Weekday) -> u32 {
    let day = u32::from(day.days_from_monday());
    let other = u32::from(other.days_from_monday());
    if day < other {
        7 + day - other
    } else {
        day - other
    }
}

/// The week of the year `date` is in, where week 1 starts on the year's
/// first `week_starts_on` and the days before it are week 0: 0 to 53.
fn weeks_from(date: Date, week_starts_on: Weekday) -> i32 {
    (i32::from(date.ordinal()) - days_since(date.weekday(), week_starts_on) as i32 + 6) / 7
}

/// The quarter of the year `date` is in: 1 to 4.
fn quarter(date: Date) -> u32 {
    (u32::from(date.month_number()) - 1) / 3 + 1
}
