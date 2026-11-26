//! [`Zoned`]: an instant seen in a zone.

use std::cmp::Ordering;
use std::hash::{Hash, Hasher};
use std::ops::{Add, Sub};

use super::{SECS_PER_DAY, Zone, wall_second};
use crate::date::{MAX_YEAR, MIN_YEAR};
use crate::{Date, DateTime, Error, Offset, Result, Span, Time, Timestamp};

/// An instant seen in a zone: a [`Timestamp`], the [`Zone`] it is seen in,
/// and the offset the zone keeps at that instant, which says what its
/// calendar and its clock show.
///
/// [`Timestamp::in_zone`] makes one from an instant, and
/// [`Zone::to_timestamp`] goes the other way, from what a clock shows to
/// the instant.
///
/// There are two ways to move one, and they differ where the zone moves
/// its clocks. Adding a [`Span`] is exact: `zoned + Span::days(1)` is 24
/// hours later, whatever the clocks did in between. A step of the calendar
/// is a method with a name: [`add_days`](Zoned::add_days) and
/// [`add_months`](Zoned::add_months) keep the time the clock shows, so a
/// day is 23 or 25 hours long where the clocks change.
///
/// Two zoned values are equal when they are the same instant, and they
/// order as their instants do: the zone each is seen in plays no part, so
/// 17:30 in Kolkata equals 12:00 in UTC.
///
/// ```
/// use navree_datetime::{Date, Offset, Span, Time, Timestamp, Zone};
///
/// # fn main() -> navree_datetime::Result<()> {
/// let kolkata = Zone::named("Asia/Kolkata")?;
/// let sent = Timestamp::utc(2026, 11, 25, 20, 0, 0)?.in_zone(kolkata);
/// assert_eq!(sent.date(), Date::new(2026, 11, 26)?);
/// assert_eq!(sent.time(), Time::new(1, 30, 0)?);
/// assert_eq!(sent.offset(), Offset::east(19_800)?);
///
/// // The night the clocks of New York go forward: a day later on the
/// // clock is 23 hours later.
/// let new_york = Zone::named("America/New_York")?;
/// let evening = Timestamp::utc(2026, 3, 8, 1, 0, 0)?.in_zone(new_york); // 7 March, 20:00
/// let next_evening = evening.add_days(1)?;
/// assert_eq!(next_evening.time(), Time::new(20, 0, 0)?);
/// assert_eq!(next_evening.timestamp() - evening.timestamp(), Span::hours(23));
/// assert_eq!((evening + Span::days(1)).time(), Time::new(21, 0, 0)?);
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Copy)]
pub struct Zoned {
    timestamp: Timestamp,
    zone: Zone,
    /// What `zone` keeps at `timestamp`.
    offset: Offset,
}

impl Zoned {
    /// The instant `timestamp` seen in `zone`.
    pub fn new(timestamp: Timestamp, zone: Zone) -> Zoned {
        Zoned {
            timestamp,
            zone,
            offset: zone.offset_at(timestamp),
        }
    }

    /// The instant.
    pub const fn timestamp(self) -> Timestamp {
        self.timestamp
    }

    /// The zone the instant is seen in.
    pub const fn zone(self) -> Zone {
        self.zone
    }

    /// The offset from UTC the zone keeps at the instant.
    pub const fn offset(self) -> Offset {
        self.offset
    }

    /// The day the zone's calendar shows at the instant.
    ///
    /// In the first and the last hours of the range of instants a calendar
    /// away from UTC shows a day before [`Date::MIN`] or after
    /// [`Date::MAX`]. Such a day has its year, month and day to show, and
    /// is not to be stepped along the calendar.
    pub const fn date(self) -> Date {
        Date::from_epoch_days_unchecked(self.wall_second().div_euclid(SECS_PER_DAY))
    }

    /// The time of day the zone's clocks show at the instant.
    pub const fn time(self) -> Time {
        Time::from_second_of_day(
            self.wall_second().rem_euclid(SECS_PER_DAY) as u32,
            self.timestamp.subsec_nanos(),
        )
    }

    /// The day and the time of day the zone's calendar and clocks show at
    /// the instant.
    pub const fn date_time(self) -> DateTime {
        DateTime::new(self.date(), self.time())
    }

    /// The same instant seen in another zone.
    #[must_use]
    pub fn with_zone(self, zone: Zone) -> Zoned {
        Zoned::new(self.timestamp, zone)
    }

    /// The start of the day the zone's calendar shows at the instant:
    /// [`Date::start_of_day`] of [`date`](Zoned::date) in the zone.
    ///
    /// On the first day of the range of instants, whose midnight in a zone
    /// away from UTC can lie before [`Timestamp::MIN`], it is
    /// [`Timestamp::MIN`].
    #[must_use]
    pub fn start_of_day(self) -> Zoned {
        let wall = self.wall_second();
        let midnight = wall - wall.rem_euclid(SECS_PER_DAY);
        let secs = self
            .zone
            .second_of_wall(midnight)
            .clamp(Timestamp::MIN.as_secs(), Timestamp::MAX.as_secs());
        Zoned::new(Timestamp::from_parts_unchecked(secs, 0), self.zone)
    }

    /// The same time of day `days` days later on the zone's calendar, or
    /// earlier for a count below zero.
    ///
    /// It is a step of the calendar, not a length of time: across a change
    /// of the zone's clocks the day is an hour shorter or longer, and the
    /// clock still shows the same time. Where the clocks skip that time on
    /// the day reached, or show it twice, it is read by
    /// [`Disambiguation::Compatible`](crate::Disambiguation::Compatible).
    /// `zoned + Span::days(days)` is the exact one: so many times 24 hours.
    ///
    /// # Errors
    ///
    /// An instant before [`Timestamp::MIN`] or after [`Timestamp::MAX`] is
    /// out of range.
    pub fn add_days(self, days: i32) -> Result<Zoned> {
        self.at_wall_second(self.wall_second() + i64::from(days) * SECS_PER_DAY)
    }

    /// The same day of the month and the same time of day `months` months
    /// later on the zone's calendar, or earlier for a count below zero. A
    /// day the month does not have becomes the month's last day, as with
    /// [`Date::add_months`]. Where the clocks skip that time on the day
    /// reached, or show it twice, it is read by
    /// [`Disambiguation::Compatible`](crate::Disambiguation::Compatible).
    ///
    /// # Errors
    ///
    /// A day past [`Date::MIN`] or [`Date::MAX`], or an instant before
    /// [`Timestamp::MIN`] or after [`Timestamp::MAX`], is out of range.
    pub fn add_months(self, months: i32) -> Result<Zoned> {
        let local = self.date_time();
        let Some(date) = local.date().add_months(months) else {
            return Err(Error::out_of_range(format!(
                "{months} months from the year {} is outside the years {MIN_YEAR} to {MAX_YEAR}",
                local.date().year()
            )));
        };
        self.at_wall_second(wall_second(date.at(local.time())))
    }

    /// The instant `span` later, or earlier for a span below zero, seen in
    /// the same zone. `None` before [`Timestamp::MIN`] or after
    /// [`Timestamp::MAX`].
    #[must_use]
    pub fn checked_add(self, span: Span) -> Option<Zoned> {
        self.timestamp
            .checked_add(span)
            .map(|timestamp| Zoned::new(timestamp, self.zone))
    }

    /// The instant `span` earlier, or later for a span below zero, seen in
    /// the same zone. `None` before [`Timestamp::MIN`] or after
    /// [`Timestamp::MAX`].
    #[must_use]
    pub fn checked_sub(self, span: Span) -> Option<Zoned> {
        self.timestamp
            .checked_sub(span)
            .map(|timestamp| Zoned::new(timestamp, self.zone))
    }

    /// What the zone's calendar and clocks show at the instant, in seconds
    /// counted as though they were UTC's.
    const fn wall_second(self) -> i64 {
        self.timestamp.as_secs() + self.offset.seconds() as i64
    }

    /// The instant at which the zone's clocks show the wall-clock second
    /// `wall`, with this value's fraction of a second.
    fn at_wall_second(self, wall: i64) -> Result<Zoned> {
        let timestamp = Timestamp::from_secs_nanos(
            self.zone.second_of_wall(wall),
            self.timestamp.subsec_nanos(),
        )?;
        Ok(Zoned::new(timestamp, self.zone))
    }
}

impl Timestamp {
    /// The instant seen in `zone`: what the zone's calendar and clocks show
    /// at it.
    ///
    /// ```
    /// use navree_datetime::{Date, Time, Timestamp, Zone};
    ///
    /// # fn main() -> navree_datetime::Result<()> {
    /// let paid = Timestamp::utc(2026, 11, 25, 20, 0, 0)?;
    /// let in_kolkata = paid.in_zone(Zone::named("Asia/Kolkata")?);
    /// assert_eq!(in_kolkata.date(), Date::new(2026, 11, 26)?);
    /// assert_eq!(in_kolkata.time(), Time::new(1, 30, 0)?);
    /// assert_eq!(in_kolkata.timestamp(), paid);
    /// # Ok(())
    /// # }
    /// ```
    pub fn in_zone(self, zone: Zone) -> Zoned {
        Zoned::new(self, zone)
    }
}

impl Date {
    /// The start of the day in `zone`: the instant its clocks show
    /// midnight.
    ///
    /// Where the clocks skip midnight, which they do in a zone that goes
    /// forward at 00:00, the day starts at the first instant after the
    /// gap: midnight is read by
    /// [`Disambiguation::Compatible`](crate::Disambiguation::Compatible),
    /// which moves it forward by the length of the gap. Where the clocks
    /// show midnight twice, the day starts at the first of the two.
    ///
    /// The day ends where the next one starts, so a day is 23 or 25 hours
    /// long where the clocks change.
    ///
    /// ```
    /// use navree_datetime::{Date, Time, Timestamp, Zone};
    ///
    /// # fn main() -> navree_datetime::Result<()> {
    /// let kolkata = Zone::named("Asia/Kolkata")?;
    /// let day = Date::new(2026, 11, 25)?;
    /// assert_eq!(
    ///     day.start_of_day(kolkata)?.timestamp(),
    ///     Timestamp::utc(2026, 11, 24, 18, 30, 0)?
    /// );
    ///
    /// // Havana goes forward at midnight: 8 March 2026 starts at 01:00.
    /// let havana = Zone::named("America/Havana")?;
    /// let start = Date::new(2026, 3, 8)?.start_of_day(havana)?;
    /// assert_eq!(start.time(), Time::new(1, 0, 0)?);
    /// assert_eq!(start.timestamp(), Timestamp::utc(2026, 3, 8, 5, 0, 0)?);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// A start before [`Timestamp::MIN`] or after [`Timestamp::MAX`], which
    /// the first and the last day can have in a zone away from UTC, is out
    /// of range.
    pub fn start_of_day(self, zone: Zone) -> Result<Zoned> {
        let secs = zone.second_of_wall(self.epoch_days() * SECS_PER_DAY);
        Ok(Zoned::new(Timestamp::from_secs(secs)?, zone))
    }
}

/// The same instant, whatever zone each is seen in.
impl PartialEq for Zoned {
    fn eq(&self, other: &Zoned) -> bool {
        self.timestamp == other.timestamp
    }
}

impl Eq for Zoned {}

/// In the order of their instants.
impl PartialOrd for Zoned {
    fn partial_cmp(&self, other: &Zoned) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// In the order of their instants.
impl Ord for Zoned {
    fn cmp(&self, other: &Zoned) -> Ordering {
        self.timestamp.cmp(&other.timestamp)
    }
}

/// By the instant alone, as equality is.
impl Hash for Zoned {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.timestamp.hash(state);
    }
}

/// The instant `span` later in the same zone: exact time, so a day of a
/// span is 24 hours whatever the zone's clocks do.
///
/// # Panics
///
/// Panics when the instant is past the range;
/// [`checked_add`](Zoned::checked_add) answers `None` instead.
impl Add<Span> for Zoned {
    type Output = Zoned;

    #[track_caller]
    fn add(self, span: Span) -> Zoned {
        self.checked_add(span)
            .expect("`Zoned + Span` is past the range of `Timestamp`")
    }
}

/// The instant `span` earlier in the same zone: exact time.
///
/// # Panics
///
/// Panics when the instant is past the range;
/// [`checked_sub`](Zoned::checked_sub) answers `None` instead.
impl Sub<Span> for Zoned {
    type Output = Zoned;

    #[track_caller]
    fn sub(self, span: Span) -> Zoned {
        self.checked_sub(span)
            .expect("`Zoned - Span` is past the range of `Timestamp`")
    }
}
