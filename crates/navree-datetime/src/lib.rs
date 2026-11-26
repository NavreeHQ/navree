//! # navree-datetime
//!
//! Navree's dates and times, written from the base: no third-party time
//! crate is under them.
//!
//! Each idea has one type, and a type says which idea a value is:
//!
//! | Type | What it is |
//! |---|---|
//! | [`Timestamp`] | an exact instant: seconds and nanoseconds from 1970-01-01T00:00:00Z, the same everywhere |
//! | [`Date`] | a day of the calendar: a year, a month and a day |
//! | [`Time`] | a time of day, to the nanosecond |
//! | [`DateTime`] | a day and a time of day with no zone: what a wall shows, somewhere |
//! | [`Offset`] | a fixed distance from UTC, in seconds |
//! | [`Zone`] | a time zone: a zone of the IANA database by its name, or one offset that never changes |
//! | [`Zoned`] | an instant seen in a zone: the day and the time of day its calendar and clocks show |
//! | [`Span`] | an exact, signed length of time, to the nanosecond |
//! | [`Weekday`], [`Month`] | the days of the week and the months of the year |
//!
//! ```
//! use navree_datetime::{Date, Span, Timestamp, Weekday};
//!
//! # fn main() -> navree_datetime::Result<()> {
//! // An instant, and exact time added to it.
//! let ordered = Timestamp::utc(2026, 11, 25, 4, 0, 0)?;
//! let due = ordered + Span::hours(36);
//! assert_eq!(due - ordered, Span::minutes(2_160));
//!
//! // A day, and steps along the calendar.
//! let day = Date::new(2026, 11, 25)?;
//! assert_eq!(day.weekday(), Weekday::Wednesday);
//! assert_eq!(
//!     Date::new(2026, 1, 31)?.add_months(1),
//!     Some(Date::new(2026, 2, 28)?)
//! );
//! # Ok(())
//! # }
//! ```
//!
//! ## Exact time and calendar time
//!
//! A [`Span`] is exact: `Span::days(1)` is 24 hours and `Span::weeks(1)` is
//! 7 of those. `timestamp + span` and `timestamp - timestamp` are the
//! arithmetic of instants and never look at a calendar.
//!
//! A step of the calendar is a method with a name, never a span, because it
//! is not a fixed length of time: [`Date::add_days`], [`Date::add_months`]
//! and [`Date::add_years`]. A month after 31 January is the last day of
//! February, and a year after 29 February is 28 February.
//!
//! ## Zones
//!
//! A [`Zone`] says which offset from UTC the clocks of a place keep at each
//! instant. The IANA time zone database is compiled into the crate, legacy
//! names and all ([`Zone::named`] reads `Asia/Calcutta` as `Asia/Kolkata`),
//! so no file of the system is read. [`Timestamp::in_zone`] gives a
//! [`Zoned`]: the instant, with the day and the time of day the zone shows
//! at it.
//!
//! The other way, from a day and a time on a zone's clocks to the instant,
//! has two hard cases: a time the clocks skip where they go forward, and a
//! time they show twice where they go back. [`Zone::to_timestamp`] takes a
//! [`Disambiguation`] that says what to make of them.
//!
//! ```
//! use navree_datetime::{Date, Disambiguation, Span, Time, Timestamp, Zone};
//!
//! # fn main() -> navree_datetime::Result<()> {
//! let kolkata = Zone::named("Asia/Kolkata")?;
//! let opens = Date::new(2026, 11, 25)?.at(Time::new(9, 30, 0)?);
//! let instant = kolkata.to_timestamp(opens, Disambiguation::Compatible)?;
//! assert_eq!(instant, Timestamp::utc(2026, 11, 25, 4, 0, 0)?);
//! assert_eq!(instant.in_zone(kolkata).date_time(), opens);
//!
//! // Exact time and calendar time stay apart on a zoned value too.
//! let new_york = Zone::named("America/New_York")?;
//! let evening = Timestamp::utc(2026, 3, 8, 1, 0, 0)?.in_zone(new_york);
//! assert_eq!(evening.time(), Time::new(20, 0, 0)?);
//! assert_eq!((evening + Span::days(1)).time(), Time::new(21, 0, 0)?);
//! assert_eq!(evening.add_days(1)?.time(), Time::new(20, 0, 0)?);
//! # Ok(())
//! # }
//! ```
//!
//! `zoned + Span::days(1)` is 24 hours later. [`Zoned::add_days`] and
//! [`Zoned::add_months`] are steps of the zone's calendar: the clock shows
//! the same time on the day reached, which is 23 or 25 hours away where the
//! clocks change in between.
//!
//! ## The calendar and the range
//!
//! The calendar is the proleptic Gregorian one: today's calendar carried
//! back before 1582 by its own rules, with a year 0 before year 1. A minute
//! always has 60 seconds; there is no leap second. The [`civil`] module is
//! the one place where a year, a month and a day become a count of days and
//! back.
//!
//! Days run from 1 January of the year -262,143 ([`Date::MIN`]) to
//! 31 December of the year 262,142 ([`Date::MAX`]), and instants from the
//! first moment of the first of those days in UTC to the last nanosecond of
//! the last ([`Timestamp::MIN`] and [`Timestamp::MAX`]). That is the range
//! of the chrono crate, which held Navree's dates from 1.0, so every value
//! an app has stored still fits, the last instant among them: Navree stores
//! [`Timestamp::MAX`] where a time is never to come.
//!
//! ## Errors
//!
//! Making a value from numbers that name no day or time is an [`Error`]
//! with a [kind](ErrorKind), never a panic: [`ErrorKind::Invalid`] for a
//! value that does not exist (30 February, hour 24) and
//! [`ErrorKind::OutOfRange`] for one past the ends of its type. Arithmetic
//! that can leave the range has a form that answers `None`
//! ([`Timestamp::checked_add`], [`Date::add_days`], [`Span::checked_add`]).
//! The operators `+`, `-`, `*` and `/` are for values known to stay in the
//! range and panic when they do not, as the operators of the standard
//! library's times do.
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod civil;
mod date;
mod date_time;
mod error;
mod month;
mod offset;
mod span;
mod time;
mod timestamp;
mod weekday;
mod zone;

pub use date::Date;
pub use date_time::DateTime;
pub use error::{Error, ErrorKind, Result};
pub use month::Month;
pub use offset::Offset;
pub use span::Span;
pub use time::Time;
pub use timestamp::Timestamp;
pub use weekday::Weekday;
pub use zone::{Disambiguation, Zone, Zoned};

/// Items Navree's own crates use; not public API.
#[doc(hidden)]
pub mod __private {
    // The IANA database as it is compiled: every zone and link under its
    // own name, and the offset each keeps over each stretch of time.
    // navree-time's `Tz`, a time zone for chrono, stands on them.
    pub use crate::zone::{Entry, LocalPeriods, Period, PeriodSpan};
}
