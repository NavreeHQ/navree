//! [`DateTime`]: a day and a time of day with no zone.

use crate::{Date, Span, Time, Timestamp};

/// A day and a time of day with no zone: what a calendar and a clock on a
/// wall show, somewhere.
///
/// It is not an instant until a zone says where the wall is.
/// [`in_utc`](DateTime::in_utc) reads it as UTC's own wall.
///
/// Values compare in the order of the calendar and the clock.
///
/// ```
/// use navree_datetime::{Date, DateTime, Span, Time};
///
/// # fn main() -> navree_datetime::Result<()> {
/// let closes = Date::new(2026, 11, 25)?.at(Time::new(23, 30, 0)?);
/// assert_eq!(closes, DateTime::new(closes.date(), closes.time()));
///
/// let later = closes.add(Span::minutes(45)).expect("a day in the range");
/// assert_eq!(later, Date::new(2026, 11, 26)?.at(Time::new(0, 15, 0)?));
/// assert_eq!(closes.in_utc().as_secs(), 1_795_649_400);
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DateTime {
    date: Date,
    time: Time,
}

impl DateTime {
    /// `time` on `date`.
    pub const fn new(date: Date, time: Time) -> DateTime {
        DateTime { date, time }
    }

    /// The day.
    pub const fn date(self) -> Date {
        self.date
    }

    /// The time of day.
    pub const fn time(self) -> Time {
        self.time
    }

    /// The instant at which a clock in UTC shows this day and time.
    pub const fn in_utc(self) -> Timestamp {
        Timestamp::from_parts_unchecked(
            self.date.epoch_days() * 86_400 + self.time.second_of_day() as i64,
            self.time.nanosecond(),
        )
    }

    /// The day and time `span` later, or earlier for a span below zero, on
    /// a clock that never jumps: a minute is 60 seconds and a day is 24
    /// hours. `None` past the first or the last day a [`Date`] holds.
    // The name says what it does to the wall clock; `+` is kept for
    // instants, where adding a span has one meaning.
    #[allow(clippy::should_implement_trait)]
    #[must_use]
    pub const fn add(self, span: Span) -> Option<DateTime> {
        match self.in_utc().checked_add(span) {
            Some(instant) => Some(instant.date_time_utc()),
            None => None,
        }
    }
}
