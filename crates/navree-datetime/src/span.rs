//! [`Span`]: an exact, signed length of time.

use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};
use std::time::Duration;

use crate::{Error, Result};

const NANOS_PER_SEC: i32 = 1_000_000_000;
const NANOS_PER_MILLI: i32 = 1_000_000;
const NANOS_PER_MICRO: i32 = 1_000;
const MICROS_PER_SEC: i64 = 1_000_000;
const MILLIS_PER_SEC: i64 = 1_000;
const SECS_PER_MINUTE: i64 = 60;
const SECS_PER_HOUR: i64 = 3_600;
const SECS_PER_DAY: i64 = 86_400;
const SECS_PER_WEEK: i64 = 604_800;

/// An exact, signed length of time, to the nanosecond.
///
/// A span is a count of seconds and nanoseconds and nothing else: a day is
/// 24 hours and a week is 7 of those, whatever a clock on a wall did in
/// between. Steps of the calendar, which are not exact (a month is 28 to 31
/// days, a local day 23 to 25 hours), are methods with names of their own,
/// such as [`Date::add_months`](crate::Date::add_months).
///
/// A span holds up to [`Span::MAX`], `i64::MAX` milliseconds (about 292
/// million years), either way. The functions that make one from a number
/// ([`seconds`](Span::seconds), [`days`](Span::days) and the rest) panic past
/// that, which a number written in the code never is; for a number that
/// comes from outside, the `try_` forms answer `None`.
///
/// ```
/// use navree_datetime::Span;
///
/// const RETENTION: Span = Span::days(7);
///
/// assert_eq!(RETENTION, Span::hours(168));
/// assert_eq!(RETENTION.as_days(), 7);
/// assert_eq!(Span::millis(1_500).as_seconds(), 1);
/// assert_eq!((Span::minutes(90) - Span::hours(2)).as_minutes(), -30);
/// assert_eq!(Span::try_days(i64::MAX), None);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Span {
    /// Whole seconds, counted down for a span below zero: with `nanos` on
    /// top, a span of -0.25 seconds is -1 second and 750,000,000
    /// nanoseconds.
    secs: i64,
    /// Nanoseconds above `secs`: 0 to 999,999,999.
    nanos: i32,
}

impl Span {
    /// No time at all.
    pub const ZERO: Span = Span { secs: 0, nanos: 0 };

    /// The longest span: `i64::MAX` milliseconds.
    pub const MAX: Span = Span {
        secs: i64::MAX / MILLIS_PER_SEC,
        nanos: (i64::MAX % MILLIS_PER_SEC) as i32 * NANOS_PER_MILLI,
    };

    /// The longest span below zero: `-i64::MAX` milliseconds, so every span
    /// has its opposite.
    pub const MIN: Span = Span {
        secs: -i64::MAX / MILLIS_PER_SEC - 1,
        nanos: NANOS_PER_SEC + (-i64::MAX % MILLIS_PER_SEC) as i32 * NANOS_PER_MILLI,
    };

    /// A span of `weeks` weeks of 7 days of 24 hours.
    ///
    /// # Panics
    ///
    /// Panics past [`Span::MAX`] or [`Span::MIN`];
    /// [`try_weeks`](Span::try_weeks) does not.
    #[track_caller]
    pub const fn weeks(weeks: i64) -> Span {
        match Span::try_weeks(weeks) {
            Some(span) => span,
            None => panic!("`Span::weeks` is past the range of `Span`"),
        }
    }

    /// A span of `weeks` weeks, or `None` past the range.
    pub const fn try_weeks(weeks: i64) -> Option<Span> {
        match weeks.checked_mul(SECS_PER_WEEK) {
            Some(seconds) => Span::try_seconds(seconds),
            None => None,
        }
    }

    /// A span of `days` days of 24 hours each.
    ///
    /// # Panics
    ///
    /// Panics past [`Span::MAX`] or [`Span::MIN`];
    /// [`try_days`](Span::try_days) does not.
    #[track_caller]
    pub const fn days(days: i64) -> Span {
        match Span::try_days(days) {
            Some(span) => span,
            None => panic!("`Span::days` is past the range of `Span`"),
        }
    }

    /// A span of `days` days, or `None` past the range.
    pub const fn try_days(days: i64) -> Option<Span> {
        match days.checked_mul(SECS_PER_DAY) {
            Some(seconds) => Span::try_seconds(seconds),
            None => None,
        }
    }

    /// A span of `hours` hours.
    ///
    /// # Panics
    ///
    /// Panics past [`Span::MAX`] or [`Span::MIN`];
    /// [`try_hours`](Span::try_hours) does not.
    #[track_caller]
    pub const fn hours(hours: i64) -> Span {
        match Span::try_hours(hours) {
            Some(span) => span,
            None => panic!("`Span::hours` is past the range of `Span`"),
        }
    }

    /// A span of `hours` hours, or `None` past the range.
    pub const fn try_hours(hours: i64) -> Option<Span> {
        match hours.checked_mul(SECS_PER_HOUR) {
            Some(seconds) => Span::try_seconds(seconds),
            None => None,
        }
    }

    /// A span of `minutes` minutes.
    ///
    /// # Panics
    ///
    /// Panics past [`Span::MAX`] or [`Span::MIN`];
    /// [`try_minutes`](Span::try_minutes) does not.
    #[track_caller]
    pub const fn minutes(minutes: i64) -> Span {
        match Span::try_minutes(minutes) {
            Some(span) => span,
            None => panic!("`Span::minutes` is past the range of `Span`"),
        }
    }

    /// A span of `minutes` minutes, or `None` past the range.
    pub const fn try_minutes(minutes: i64) -> Option<Span> {
        match minutes.checked_mul(SECS_PER_MINUTE) {
            Some(seconds) => Span::try_seconds(seconds),
            None => None,
        }
    }

    /// A span of `seconds` seconds.
    ///
    /// # Panics
    ///
    /// Panics past [`Span::MAX`] or [`Span::MIN`], which is more than
    /// `i64::MAX / 1000` seconds either way;
    /// [`try_seconds`](Span::try_seconds) does not.
    #[track_caller]
    pub const fn seconds(seconds: i64) -> Span {
        match Span::try_seconds(seconds) {
            Some(span) => span,
            None => panic!("`Span::seconds` is past the range of `Span`"),
        }
    }

    /// A span of `seconds` seconds, or `None` past the range.
    pub const fn try_seconds(seconds: i64) -> Option<Span> {
        Span::from_parts(seconds, 0)
    }

    /// A span of `millis` milliseconds.
    ///
    /// # Panics
    ///
    /// Panics for `i64::MIN`, the one number of milliseconds past the range;
    /// [`try_millis`](Span::try_millis) does not.
    #[track_caller]
    pub const fn millis(millis: i64) -> Span {
        match Span::try_millis(millis) {
            Some(span) => span,
            None => panic!("`Span::millis` is past the range of `Span`"),
        }
    }

    /// A span of `millis` milliseconds, or `None` for `i64::MIN`, the one
    /// number of milliseconds past the range.
    pub const fn try_millis(millis: i64) -> Option<Span> {
        if millis < -i64::MAX {
            return None;
        }
        Some(Span {
            secs: millis.div_euclid(MILLIS_PER_SEC),
            nanos: millis.rem_euclid(MILLIS_PER_SEC) as i32 * NANOS_PER_MILLI,
        })
    }

    /// A span of `micros` microseconds. Every `i64` is in the range.
    pub const fn micros(micros: i64) -> Span {
        Span {
            secs: micros.div_euclid(MICROS_PER_SEC),
            nanos: micros.rem_euclid(MICROS_PER_SEC) as i32 * NANOS_PER_MICRO,
        }
    }

    /// A span of `micros` microseconds: always `Some`, since every `i64` is
    /// in the range. It is here so that every unit has a `try_` form.
    pub const fn try_micros(micros: i64) -> Option<Span> {
        Some(Span::micros(micros))
    }

    /// A span of `nanos` nanoseconds. Every `i64` is in the range.
    pub const fn nanos(nanos: i64) -> Span {
        Span {
            secs: nanos.div_euclid(NANOS_PER_SEC as i64),
            nanos: nanos.rem_euclid(NANOS_PER_SEC as i64) as i32,
        }
    }

    /// A span of `nanos` nanoseconds: always `Some`, since every `i64` is in
    /// the range. It is here so that every unit has a `try_` form.
    pub const fn try_nanos(nanos: i64) -> Option<Span> {
        Some(Span::nanos(nanos))
    }

    /// The whole days of 24 hours in the span, the rest dropped: -1 for a
    /// span of -36 hours.
    pub const fn as_days(self) -> i64 {
        self.as_seconds() / SECS_PER_DAY
    }

    /// The whole hours in the span, the rest dropped.
    pub const fn as_hours(self) -> i64 {
        self.as_seconds() / SECS_PER_HOUR
    }

    /// The whole minutes in the span, the rest dropped.
    pub const fn as_minutes(self) -> i64 {
        self.as_seconds() / SECS_PER_MINUTE
    }

    /// The whole seconds in the span, the rest dropped: -1 for a span of
    /// -1.5 seconds.
    pub const fn as_seconds(self) -> i64 {
        if self.secs < 0 && self.nanos > 0 {
            self.secs + 1
        } else {
            self.secs
        }
    }

    /// The whole milliseconds in the span, the rest dropped. Every span
    /// fits.
    pub const fn as_millis(self) -> i64 {
        self.as_seconds() * MILLIS_PER_SEC + (self.subsec_nanos() / NANOS_PER_MILLI) as i64
    }

    /// The whole microseconds in the span, the rest dropped, or `None` for a
    /// span of more than `i64::MAX` microseconds (about 292,000 years)
    /// either way.
    pub const fn as_micros(self) -> Option<i64> {
        match self.as_seconds().checked_mul(MICROS_PER_SEC) {
            Some(micros) => micros.checked_add((self.subsec_nanos() / NANOS_PER_MICRO) as i64),
            None => None,
        }
    }

    /// The span in seconds as a floating-point number, which rounds a long
    /// span to what an `f64` holds.
    pub const fn as_seconds_f64(self) -> f64 {
        self.secs as f64 + self.nanos as f64 / NANOS_PER_SEC as f64
    }

    /// The nanoseconds beyond [`as_seconds`](Span::as_seconds), with the
    /// sign of the span: -500,000,000 for a span of -1.5 seconds.
    pub const fn subsec_nanos(self) -> i32 {
        if self.secs < 0 && self.nanos > 0 {
            self.nanos - NANOS_PER_SEC
        } else {
            self.nanos
        }
    }

    /// The span without its sign.
    #[must_use]
    pub const fn abs(self) -> Span {
        if self.secs < 0 { self.negated() } else { self }
    }

    /// Whether the span is no time at all.
    pub const fn is_zero(self) -> bool {
        self.secs == 0 && self.nanos == 0
    }

    /// Whether the span is below zero.
    pub const fn is_negative(self) -> bool {
        self.secs < 0
    }

    /// The sum of two spans, or `None` past the range.
    #[must_use]
    pub const fn checked_add(self, other: Span) -> Option<Span> {
        // Both are in the range, so the sums stay far inside an `i64`.
        let mut secs = self.secs + other.secs;
        let mut nanos = self.nanos + other.nanos;
        if nanos >= NANOS_PER_SEC {
            nanos -= NANOS_PER_SEC;
            secs += 1;
        }
        Span::from_parts(secs, nanos as u32)
    }

    /// The span less `other`, or `None` past the range.
    #[must_use]
    pub const fn checked_sub(self, other: Span) -> Option<Span> {
        let mut secs = self.secs - other.secs;
        let mut nanos = self.nanos - other.nanos;
        if nanos < 0 {
            nanos += NANOS_PER_SEC;
            secs -= 1;
        }
        Span::from_parts(secs, nanos as u32)
    }

    /// The span `times` times over, or `None` past the range.
    #[must_use]
    pub const fn checked_mul(self, times: i32) -> Option<Span> {
        let nanos = self.nanos as i64 * times as i64;
        let secs =
            self.secs as i128 * times as i128 + nanos.div_euclid(NANOS_PER_SEC as i64) as i128;
        if secs < i64::MIN as i128 || secs > i64::MAX as i128 {
            return None;
        }
        Span::from_parts(secs as i64, nanos.rem_euclid(NANOS_PER_SEC as i64) as u32)
    }

    /// The span divided into `parts`, or `None` for 0 parts.
    ///
    /// The seconds and the nanoseconds are each divided and what is left of
    /// the seconds is carried into the nanoseconds, so a span that does not
    /// divide evenly loses less than a nanosecond.
    #[must_use]
    pub const fn checked_div(self, parts: i32) -> Option<Span> {
        if parts == 0 {
            return None;
        }
        let secs = self.secs / parts as i64;
        let carry = self.secs % parts as i64;
        let nanos = self.nanos / parts + (carry * NANOS_PER_SEC as i64 / parts as i64) as i32;
        Some(if nanos < 0 {
            Span {
                secs: secs - 1,
                nanos: nanos + NANOS_PER_SEC,
            }
        } else if nanos >= NANOS_PER_SEC {
            Span {
                secs: secs + 1,
                nanos: nanos - NANOS_PER_SEC,
            }
        } else {
            Span { secs, nanos }
        })
    }

    /// The span as the standard library's length of time, which has no sign:
    /// `None` for a span below zero.
    pub const fn to_std(self) -> Option<Duration> {
        if self.secs < 0 {
            return None;
        }
        Some(Duration::new(self.secs as u64, self.nanos as u32))
    }

    /// A span from its two parts: whole seconds counted down, and 0 to
    /// 999,999,999 nanoseconds on top of them. `None` past the range or for
    /// a nanosecond count of a second or more.
    pub(crate) const fn from_parts(secs: i64, nanos: u32) -> Option<Span> {
        if secs < Span::MIN.secs
            || secs > Span::MAX.secs
            || nanos >= NANOS_PER_SEC as u32
            || (secs == Span::MAX.secs && nanos > Span::MAX.nanos as u32)
            || (secs == Span::MIN.secs && nanos < Span::MIN.nanos as u32)
        {
            return None;
        }
        Some(Span {
            secs,
            nanos: nanos as i32,
        })
    }

    /// The whole seconds of the span, counted down for a span below zero:
    /// with [`nanos_above`](Span::nanos_above), the two parts it is kept as.
    pub(crate) const fn secs_floor(self) -> i64 {
        self.secs
    }

    /// The nanoseconds above [`secs_floor`](Span::secs_floor): 0 to
    /// 999,999,999.
    pub(crate) const fn nanos_above(self) -> u32 {
        self.nanos as u32
    }

    /// The opposite span. Every span has one, since the range is the same
    /// both ways.
    const fn negated(self) -> Span {
        if self.nanos == 0 {
            Span {
                secs: -self.secs,
                nanos: 0,
            }
        } else {
            Span {
                secs: -self.secs - 1,
                nanos: NANOS_PER_SEC - self.nanos,
            }
        }
    }
}

impl Neg for Span {
    type Output = Span;

    fn neg(self) -> Span {
        self.negated()
    }
}

/// # Panics
///
/// Panics when the sum is past the range of a span;
/// [`checked_add`](Span::checked_add) answers `None` instead.
impl Add for Span {
    type Output = Span;

    #[track_caller]
    fn add(self, other: Span) -> Span {
        self.checked_add(other)
            .expect("`Span + Span` is past the range of `Span`")
    }
}

/// # Panics
///
/// Panics when the difference is past the range of a span;
/// [`checked_sub`](Span::checked_sub) answers `None` instead.
impl Sub for Span {
    type Output = Span;

    #[track_caller]
    fn sub(self, other: Span) -> Span {
        self.checked_sub(other)
            .expect("`Span - Span` is past the range of `Span`")
    }
}

impl AddAssign for Span {
    #[track_caller]
    fn add_assign(&mut self, other: Span) {
        *self = *self + other;
    }
}

impl SubAssign for Span {
    #[track_caller]
    fn sub_assign(&mut self, other: Span) {
        *self = *self - other;
    }
}

/// # Panics
///
/// Panics when the product is past the range of a span;
/// [`checked_mul`](Span::checked_mul) answers `None` instead.
impl Mul<i32> for Span {
    type Output = Span;

    #[track_caller]
    fn mul(self, times: i32) -> Span {
        self.checked_mul(times)
            .expect("`Span * i32` is past the range of `Span`")
    }
}

/// # Panics
///
/// Panics when dividing by zero; [`checked_div`](Span::checked_div) answers
/// `None` instead.
impl Div<i32> for Span {
    type Output = Span;

    #[track_caller]
    fn div(self, parts: i32) -> Span {
        self.checked_div(parts)
            .expect("a `Span` cannot be divided by zero")
    }
}

/// A span from the standard library's length of time, which holds more: one
/// longer than [`Span::MAX`] is an error.
impl TryFrom<Duration> for Span {
    type Error = Error;

    fn try_from(duration: Duration) -> Result<Span> {
        i64::try_from(duration.as_secs())
            .ok()
            .and_then(|secs| Span::from_parts(secs, duration.subsec_nanos()))
            .ok_or_else(|| {
                Error::out_of_range(format!(
                    "{} seconds is longer than a span holds",
                    duration.as_secs()
                ))
            })
    }
}
