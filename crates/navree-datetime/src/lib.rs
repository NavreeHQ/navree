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
//! ## Text
//!
//! Every value has one fixed text, which `Display` writes and `to_string()`
//! gives. It is data as much as the value is: it is stored, sent and
//! compared, so it never depends on the machine's language or settings.
//!
//! | Type | `Display` | `Debug` | `str::parse` reads |
//! |---|---|---|---|
//! | [`Timestamp`] | `2026-11-25 12:00:00 UTC` | `2026-11-25T12:00:00Z` | RFC 3339, loosely: both of these and more |
//! | [`Zoned`] | `2026-11-25 17:30:00 IST` | `2026-11-25T17:30:00IST` | RFC 3339, loosely, at the offset written |
//! | [`Zone`] | `Asia/Kolkata`, `+05:30` | `Zone("Asia/Kolkata")` | a zone's name (`Asia/Kolkata`) |
//! | [`Date`] | `2026-11-25` | the same | `2026-11-25` |
//! | [`Time`] | `09:30:00` | the same | `09:30:00`, `09:30` |
//! | [`DateTime`] | `2026-11-25 09:30:00` | `2026-11-25T09:30:00` | the `Debug` form, with its `T` |
//! | [`Offset`] | `+05:30` | the same | `+05:30`, `+0530` |
//! | [`Span`] | `PT5400S` | its two numbers | |
//! | [`Weekday`] | `Wed` | the same | `Wed`, `Wednesday` |
//! | [`Month`] | `November` | the same | `Nov`, `November` |
//!
//! A fraction of a second is shown where there is one, in three, six or
//! nine digits, the fewest that lose nothing: `09:30:00.250`. A year before
//! 0 or after 9999 is shown with its sign: `-0044-03-15`.
//!
//! An instant is sent as RFC 3339: [`Timestamp::to_rfc3339`],
//! [`Timestamp::to_rfc3339_with`] and [`Zoned::to_rfc3339`] write it, and
//! [`Timestamp::parse_rfc3339`] and [`Zoned::parse_rfc3339`] read it.
//! [`Timestamp::to_rfc2822`], [`Zoned::to_rfc2822`],
//! [`Timestamp::parse_rfc2822`] and [`Zoned::parse_rfc2822`] are for the
//! dates of email and HTTP.
//!
//! For any other layout there are patterns, in the `strftime` style:
//! `value.format(pattern)` writes one and `Type::parse(pattern, text)`
//! reads one. The [`pattern`] module lists what a pattern can hold.
//!
//! ```
//! use navree_datetime::{Date, Precision, Timestamp};
//!
//! # fn main() -> navree_datetime::Result<()> {
//! let sent = Timestamp::utc(2026, 11, 25, 12, 0, 0)?;
//! assert_eq!(sent.to_string(), "2026-11-25 12:00:00 UTC");
//! assert_eq!(sent.to_rfc3339_with(Precision::Auto, true), "2026-11-25T12:00:00Z");
//! assert_eq!("2026-11-25T17:30:00+05:30".parse::<Timestamp>()?, sent);
//!
//! let day = Date::new(2026, 11, 25)?;
//! assert_eq!(day.format("%-d %b %Y").to_string(), "25 Nov 2026");
//! assert_eq!(Date::parse("%d/%m/%Y", "25/11/2026")?, day);
//! # Ok(())
//! # }
//! ```
//!
//! Text can name second 60, which a clock shows when a leap second is
//! added. A minute has 60 seconds here, so such a text is read as the last
//! nanosecond of its minute: `23:59:60`, with any fraction, is
//! `23:59:59.999999999`. No value is ever written with second 60.
//!
//! ## Errors
//!
//! Making a value from numbers that name no day or time is an [`Error`]
//! with a [kind](ErrorKind), never a panic: [`ErrorKind::Invalid`] for a
//! value that does not exist (30 February, hour 24) and
//! [`ErrorKind::OutOfRange`] for one past the ends of its type. Text that
//! cannot be read as the value asked for is [`ErrorKind::Parse`]. Arithmetic
//! that can leave the range has a form that answers `None`
//! ([`Timestamp::checked_add`], [`Date::add_days`], [`Span::checked_add`]).
//! The operators `+`, `-`, `*` and `/` are for values known to stay in the
//! range and panic when they do not, as the operators of the standard
//! library's times do.
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(feature = "bson")]
#[cfg_attr(docsrs, doc(cfg(feature = "bson")))]
pub mod bson;
pub mod civil;
mod date;
mod date_time;
mod error;
#[cfg(feature = "async-graphql")]
mod graphql;
mod month;
mod offset;
#[cfg(feature = "serde")]
mod serde;
mod span;
#[cfg(any(
    feature = "sqlx-postgres",
    feature = "sqlx-mysql",
    feature = "sqlx-sqlite"
))]
mod sqlx;
mod text;
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
pub use text::{Formatted, Precision, pattern};
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

    #[cfg(feature = "sqlx-mysql")]
    pub use crate::sqlx::mysql::{
        decode_date_bytes as decode_mysql_date_bytes,
        decode_date_time_bytes as decode_mysql_date_time_bytes,
        decode_timestamp_bytes as decode_mysql_timestamp_bytes,
    };
    #[cfg(feature = "sqlx-postgres")]
    pub use crate::sqlx::postgres::{
        decode_date_binary as decode_postgres_date_binary,
        decode_date_time_binary as decode_postgres_date_time_binary,
        decode_time_binary as decode_postgres_time_binary,
        decode_timestamp_binary as decode_postgres_timestamp_binary,
    };
    #[cfg(feature = "sqlx-sqlite")]
    pub use crate::sqlx::sqlite::{
        decode_date_text as decode_sqlite_date_text,
        decode_date_time_text as decode_sqlite_date_time_text,
        decode_time_text as decode_sqlite_time_text,
        decode_timestamp_text as decode_sqlite_timestamp_text,
        decode_zoned_text as decode_sqlite_zoned_text,
    };
}
