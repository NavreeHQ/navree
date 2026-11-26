//! [`Timestamp`]: an exact instant.

use std::ops::{Add, AddAssign, Sub, SubAssign};
use std::time::SystemTime;

use crate::date::{MAX_YEAR, MIN_YEAR};
use crate::{Date, DateTime, Error, Result, Span, Time, civil};

const SECS_PER_DAY: i64 = 86_400;
const NANOS_PER_SEC: u32 = 1_000_000_000;
/// The second [`Timestamp::MIN`] is.
const MIN_SECS: i64 = civil::days_from_civil(MIN_YEAR, 1, 1) * SECS_PER_DAY;
/// The second [`Timestamp::MAX`] is in.
const MAX_SECS: i64 = civil::days_from_civil(MAX_YEAR, 12, 31) * SECS_PER_DAY + SECS_PER_DAY - 1;

/// An exact instant: a count of seconds and nanoseconds from the Unix
/// epoch, 1970-01-01T00:00:00Z. It is the same instant everywhere and has
/// no zone.
///
/// Instants run from [`Timestamp::MIN`], the first moment of the year
/// -262,143 in UTC, to [`Timestamp::MAX`], the last nanosecond of the year
/// 262,142. A minute always has 60 seconds: there is no leap second.
///
/// Instants compare in the order they happen. A [`Span`] added to one gives
/// a later one, and one instant taken from another gives the span between
/// them.
///
/// ```
/// use navree_datetime::{Span, Timestamp};
///
/// # fn main() -> navree_datetime::Result<()> {
/// let sent = Timestamp::utc(2026, 11, 25, 4, 0, 0)?;
/// let expires = sent + Span::hours(36);
/// assert_eq!(expires, Timestamp::utc(2026, 11, 26, 16, 0, 0)?);
/// assert_eq!(expires - sent, Span::minutes(2_160));
/// assert!(sent < expires);
///
/// assert_eq!(sent.as_secs(), 1_795_579_200);
/// assert_eq!(Timestamp::from_millis(sent.as_millis())?, sent);
/// assert_eq!(Timestamp::MAX.checked_add(Span::nanos(1)), None);
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp {
    /// Whole seconds from the epoch, counted down before it: with `nanos`
    /// on top, a quarter of a second before the epoch is second -1 and
    /// 750,000,000 nanoseconds.
    secs: i64,
    /// Nanoseconds after `secs`: 0 to 999,999,999.
    nanos: u32,
}

impl Timestamp {
    /// The Unix epoch: 1970-01-01T00:00:00Z.
    pub const UNIX_EPOCH: Timestamp = Timestamp { secs: 0, nanos: 0 };

    /// The first instant: the start of 1 January of the year -262,143 in
    /// UTC.
    pub const MIN: Timestamp = Timestamp {
        secs: MIN_SECS,
        nanos: 0,
    };

    /// The last instant: the last nanosecond of 31 December of the year
    /// 262,142 in UTC. Navree stores it as a sentinel where a time is never to
    /// come; columns that order instants as SQLite `TEXT` or store them in
    /// MySQL `DATETIME` need four-digit years (`<= 9999-12-31T23:59:59Z`).
    pub const MAX: Timestamp = Timestamp {
        secs: MAX_SECS,
        nanos: NANOS_PER_SEC - 1,
    };

    /// The instant the system's clock shows now.
    pub fn now() -> Timestamp {
        Timestamp::from(SystemTime::now())
    }

    /// The instant at which a clock in UTC shows `hour`:`minute`:`second`
    /// on `day` of `month` (1 to 12) of `year`.
    ///
    /// # Errors
    ///
    /// The errors of [`Date::new`] and of [`Time::new`]: a year out of
    /// range, or a month, a day, an hour, a minute or a second that does not
    /// exist.
    pub fn utc(
        year: i32,
        month: u8,
        day: u8,
        hour: u8,
        minute: u8,
        second: u8,
    ) -> Result<Timestamp> {
        Ok(Date::new(year, month, day)?
            .at(Time::new(hour, minute, second)?)
            .in_utc())
    }

    /// The instant `secs` seconds after the epoch, or before it for a count
    /// below zero.
    ///
    /// # Errors
    ///
    /// An instant before [`Timestamp::MIN`] or after [`Timestamp::MAX`] is
    /// out of range.
    pub fn from_secs(secs: i64) -> Result<Timestamp> {
        Timestamp::from_secs_nanos(secs, 0)
    }

    /// The instant `nanos` nanoseconds after second `secs` from the epoch.
    /// Seconds before the epoch count down and the nanoseconds still count
    /// up from them: a quarter of a second before the epoch is second -1
    /// and 750,000,000 nanoseconds.
    ///
    /// # Errors
    ///
    /// A nanosecond count of 1,000,000,000 or more, a whole second, is
    /// invalid. An instant before [`Timestamp::MIN`] or after
    /// [`Timestamp::MAX`] is out of range.
    pub fn from_secs_nanos(secs: i64, nanos: u32) -> Result<Timestamp> {
        if nanos >= NANOS_PER_SEC {
            return Err(Error::invalid(format!(
                "a second has no nanosecond {nanos}"
            )));
        }
        if !(MIN_SECS..=MAX_SECS).contains(&secs) {
            return Err(Error::out_of_range(format!(
                "second {secs} from 1970-01-01T00:00:00Z is outside the years {MIN_YEAR} to {MAX_YEAR}"
            )));
        }
        Ok(Timestamp { secs, nanos })
    }

    /// The instant `millis` milliseconds after the epoch, or before it for
    /// a count below zero.
    ///
    /// # Errors
    ///
    /// An instant before [`Timestamp::MIN`] or after [`Timestamp::MAX`] is
    /// out of range.
    pub fn from_millis(millis: i64) -> Result<Timestamp> {
        Timestamp::from_secs_nanos(
            millis.div_euclid(1_000),
            millis.rem_euclid(1_000) as u32 * 1_000_000,
        )
    }

    /// The instant `micros` microseconds after the epoch, or before it for
    /// a count below zero.
    ///
    /// # Errors
    ///
    /// An instant before [`Timestamp::MIN`] or after [`Timestamp::MAX`] is
    /// out of range.
    pub fn from_micros(micros: i64) -> Result<Timestamp> {
        Timestamp::from_secs_nanos(
            micros.div_euclid(1_000_000),
            micros.rem_euclid(1_000_000) as u32 * 1_000,
        )
    }

    /// Whole seconds from the epoch to the instant's second, below zero
    /// before the epoch. A fraction of a second is dropped towards the
    /// past: a quarter of a second before the epoch is in second -1.
    pub const fn as_secs(self) -> i64 {
        self.secs
    }

    /// Whole milliseconds from the epoch, a fraction dropped towards the
    /// past.
    pub const fn as_millis(self) -> i64 {
        self.secs * 1_000 + (self.nanos / 1_000_000) as i64
    }

    /// Whole microseconds from the epoch, a fraction dropped towards the
    /// past.
    pub const fn as_micros(self) -> i64 {
        self.secs * 1_000_000 + (self.nanos / 1_000) as i64
    }

    /// The nanoseconds after the instant's second: 0 to 999,999,999.
    pub const fn subsec_nanos(self) -> u32 {
        self.nanos
    }

    /// The instant `span` later, or earlier for a span below zero. `None`
    /// before [`Timestamp::MIN`] or after [`Timestamp::MAX`].
    #[must_use]
    pub const fn checked_add(self, span: Span) -> Option<Timestamp> {
        let mut secs = match self.secs.checked_add(span.secs_floor()) {
            Some(secs) => secs,
            None => return None,
        };
        let mut nanos = self.nanos + span.nanos_above();
        if nanos >= NANOS_PER_SEC {
            nanos -= NANOS_PER_SEC;
            secs = match secs.checked_add(1) {
                Some(secs) => secs,
                None => return None,
            };
        }
        if secs < MIN_SECS || secs > MAX_SECS {
            return None;
        }
        Some(Timestamp { secs, nanos })
    }

    /// The instant `span` earlier, or later for a span below zero. `None`
    /// before [`Timestamp::MIN`] or after [`Timestamp::MAX`].
    #[must_use]
    pub const fn checked_sub(self, span: Span) -> Option<Timestamp> {
        // Every span has its opposite.
        match Span::ZERO.checked_sub(span) {
            Some(opposite) => self.checked_add(opposite),
            None => None,
        }
    }

    /// The span from `earlier` to this instant: below zero when `earlier`
    /// is in fact later. Any two instants are a span apart that fits.
    pub const fn duration_since(self, earlier: Timestamp) -> Span {
        let mut secs = self.secs - earlier.secs;
        let mut nanos = self.nanos as i64 - earlier.nanos as i64;
        if nanos < 0 {
            nanos += NANOS_PER_SEC as i64;
            secs -= 1;
        }
        match Span::from_parts(secs, nanos as u32) {
            Some(span) => span,
            // The whole range of instants is some 525,000 years and a span
            // holds 292 million.
            None => Span::ZERO,
        }
    }

    /// The instant with its fraction of a second cut to whole microseconds,
    /// which is what PostgreSQL and MySQL keep: an instant cut this way
    /// equals the one read back from them.
    #[must_use]
    pub const fn trunc_micros(self) -> Timestamp {
        Timestamp {
            secs: self.secs,
            nanos: self.nanos - self.nanos % 1_000,
        }
    }

    /// The day and the time of day a clock in UTC shows at the instant.
    pub const fn date_time_utc(self) -> DateTime {
        DateTime::new(
            Date::from_epoch_days_unchecked(self.secs.div_euclid(SECS_PER_DAY)),
            Time::from_second_of_day(self.secs.rem_euclid(SECS_PER_DAY) as u32, self.nanos),
        )
    }

    /// The instant `nanos` nanoseconds (below a second) after second `secs`
    /// from the epoch. Neither is checked: `secs` is in the range.
    pub(crate) const fn from_parts_unchecked(secs: i64, nanos: u32) -> Timestamp {
        Timestamp { secs, nanos }
    }

    /// The instant `nanos` nanoseconds (below a second) after second
    /// `secs`, or the nearest end of the range for a second outside it.
    const fn clamped(secs: i64, nanos: u32) -> Timestamp {
        if secs < MIN_SECS {
            Timestamp::MIN
        } else if secs > MAX_SECS {
            Timestamp::MAX
        } else {
            Timestamp { secs, nanos }
        }
    }
}

/// The Unix epoch.
impl Default for Timestamp {
    fn default() -> Timestamp {
        Timestamp::UNIX_EPOCH
    }
}

/// The instant a system clock's time is. A time outside the range of
/// instants, which no clock shows, becomes the nearest end of the range.
impl From<SystemTime> for Timestamp {
    fn from(time: SystemTime) -> Timestamp {
        match time.duration_since(SystemTime::UNIX_EPOCH) {
            Ok(after) => match i64::try_from(after.as_secs()) {
                Ok(secs) => Timestamp::clamped(secs, after.subsec_nanos()),
                Err(_) => Timestamp::MAX,
            },
            Err(before) => {
                let before = before.duration();
                match i64::try_from(before.as_secs()) {
                    Ok(secs) if before.subsec_nanos() == 0 => Timestamp::clamped(-secs, 0),
                    // A fraction before the epoch is the second below, and
                    // the rest of that second above it.
                    Ok(secs) => {
                        Timestamp::clamped(-secs - 1, NANOS_PER_SEC - before.subsec_nanos())
                    }
                    Err(_) => Timestamp::MIN,
                }
            }
        }
    }
}

/// # Panics
///
/// Panics when the instant is past the range;
/// [`checked_add`](Timestamp::checked_add) answers `None` instead.
impl Add<Span> for Timestamp {
    type Output = Timestamp;

    #[track_caller]
    fn add(self, span: Span) -> Timestamp {
        self.checked_add(span)
            .expect("`Timestamp + Span` is past the range of `Timestamp`")
    }
}

/// # Panics
///
/// Panics when the instant is past the range;
/// [`checked_sub`](Timestamp::checked_sub) answers `None` instead.
impl Sub<Span> for Timestamp {
    type Output = Timestamp;

    #[track_caller]
    fn sub(self, span: Span) -> Timestamp {
        self.checked_sub(span)
            .expect("`Timestamp - Span` is past the range of `Timestamp`")
    }
}

/// The span from the second instant to the first: below zero when the
/// second is the later one.
impl Sub for Timestamp {
    type Output = Span;

    fn sub(self, earlier: Timestamp) -> Span {
        self.duration_since(earlier)
    }
}

impl AddAssign<Span> for Timestamp {
    #[track_caller]
    fn add_assign(&mut self, span: Span) {
        *self = *self + span;
    }
}

impl SubAssign<Span> for Timestamp {
    #[track_caller]
    fn sub_assign(&mut self, span: Span) {
        *self = *self - span;
    }
}
