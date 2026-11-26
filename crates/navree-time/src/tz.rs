//! [`Tz`] and [`TzOffset`]: Navree's own IANA time zone and offset types for
//! chrono, over the tables navree-datetime compiles and the one algorithm it
//! reads them with.

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

use chrono::{
    DateTime, FixedOffset, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, Offset, TimeDelta,
    TimeZone, Utc,
};
use navree_datetime::__private::{Entry, LocalPeriods, Period};
use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize, Serializer};

use crate::ZoneError;

/// An IANA time zone or link from the compiled tz database (`Asia/Kolkata`,
/// `America/New_York`, `UTC`, `Asia/Calcutta`, `Factory`).
///
/// Unlike [`Zone`](crate::Zone), which rejects `Factory` and normalizes legacy
/// aliases to the names every PostgreSQL accepts, `Tz` knows every zone and
/// link of the IANA release under its own name and implements
/// [`chrono::TimeZone`] directly.
///
/// ```
/// use chrono::{TimeZone, Utc};
/// use navree_time::Tz;
///
/// let kolkata: Tz = "Asia/Kolkata".parse()?;
/// let at = Utc.with_ymd_and_hms(2026, 1, 15, 12, 0, 0).unwrap();
/// assert_eq!(at.with_timezone(&kolkata).to_string(), "2026-01-15 17:30:00 IST");
/// assert_eq!(kolkata.abbreviation(at), "IST");
/// # Ok::<(), navree_time::ZoneError>(())
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Tz(Entry);

/// The UTC offset and abbreviation of a [`Tz`] at a particular instant.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct TzOffset {
    tz: Tz,
    period: Period,
}

impl Tz {
    /// Coordinated Universal Time (`UTC`).
    pub const UTC: Tz = Tz(Entry::UTC);

    /// Looks up a zone or link of the IANA database by name, without regard to
    /// ASCII case. Surrounding whitespace is not trimmed (unlike
    /// [`Zone::parse`](crate::Zone::parse), which trims whitespace, normalizes
    /// legacy aliases and rejects `Factory`).
    pub fn parse(name: &str) -> Result<Tz, ZoneError> {
        Entry::find(name)
            .map(Tz)
            .ok_or_else(|| ZoneError::new(name))
    }

    /// The zone or link's IANA name (`Asia/Kolkata`, `Asia/Calcutta`, `UTC`).
    pub const fn name(self) -> &'static str {
        self.0.name()
    }

    /// The zone's abbreviation at `at` (`IST`, `EST`, `EDT`, `+0530`, `UTC`).
    pub fn abbreviation(self, at: DateTime<Utc>) -> &'static str {
        self.offset_from_utc_datetime(&at.naive_utc())
            .abbreviation()
    }

    /// Every zone and link in the compiled tz database release, sorted by
    /// exact name.
    pub fn all() -> impl ExactSizeIterator<Item = Tz> + DoubleEndedIterator + Clone {
        Entry::all().map(Tz)
    }

    /// The `Tz` of a zone of the database; a fixed offset, which no zone of
    /// this crate is, has none.
    pub(crate) fn of(zone: navree_datetime::Zone) -> Option<Tz> {
        Entry::of(zone).map(Tz)
    }

    /// The offset, in seconds east of UTC, in force before the gap that
    /// holds the wall-clock second `wall` (a local time counted in seconds
    /// as though it were UTC's): navree-datetime's rule for a time the
    /// clocks skip.
    pub(crate) fn offset_before_gap(self, wall: i64) -> i32 {
        self.0.offset_before_gap(wall)
    }

    fn make_offset(self, period: Period) -> TzOffset {
        TzOffset { tz: self, period }
    }
}

impl TzOffset {
    /// The [`Tz`] this offset belongs to.
    pub const fn tz(self) -> Tz {
        self.tz
    }

    /// The zone's standard (non-DST) offset from UTC during this period.
    pub fn base_utc_offset(self) -> TimeDelta {
        TimeDelta::seconds(i64::from(self.period.standard_offset()))
    }

    /// The daylight saving adjustment added to [`Self::base_utc_offset`] during
    /// this period (`TimeDelta::zero()` in standard time; negative in zones
    /// with negative winter adjustments such as `Europe/Dublin`).
    pub fn dst_offset(self) -> TimeDelta {
        TimeDelta::seconds(i64::from(self.period.daylight_saving()))
    }

    /// The zone's abbreviation during this period (`IST`, `EDT`, `+0530`).
    pub fn abbreviation(self) -> &'static str {
        self.period.abbreviation()
    }

    pub(crate) fn total_seconds(self) -> i32 {
        self.period.offset()
    }
}

impl Offset for TzOffset {
    fn fix(&self) -> FixedOffset {
        FixedOffset::east_opt(self.total_seconds()).expect("tz database offset fits FixedOffset")
    }
}

impl fmt::Display for TzOffset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.abbreviation())
    }
}

impl fmt::Debug for TzOffset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.abbreviation())
    }
}

impl TimeZone for Tz {
    type Offset = TzOffset;

    fn from_offset(offset: &Self::Offset) -> Self {
        offset.tz
    }

    fn offset_from_utc_datetime(&self, utc: &NaiveDateTime) -> Self::Offset {
        self.make_offset(self.0.span_at(utc.and_utc().timestamp()).period)
    }

    fn offset_from_utc_date(&self, utc: &NaiveDate) -> Self::Offset {
        self.offset_from_utc_datetime(&utc.and_time(NaiveTime::MIN))
    }

    fn offset_from_local_datetime(&self, local: &NaiveDateTime) -> LocalResult<Self::Offset> {
        match self.0.local(local.and_utc().timestamp()) {
            LocalPeriods::Gap => LocalResult::None,
            LocalPeriods::Single(period) => LocalResult::Single(self.make_offset(period)),
            LocalPeriods::Fold(earliest, latest) => {
                LocalResult::Ambiguous(self.make_offset(earliest), self.make_offset(latest))
            }
        }
    }

    fn offset_from_local_date(&self, local: &NaiveDate) -> LocalResult<Self::Offset> {
        // Match chrono-tz's local-date semantics: resolve midnight at the start
        // and end of the local calendar day (`00:00:00` and `23:59:59`, or the
        // periods overlapping `[day_start, day_end]`).
        let day_start = local.and_time(NaiveTime::MIN).and_utc().timestamp();
        let day_end = day_start + 86_399;
        let w_start = day_start.saturating_sub(172_800);
        let w_end = day_end.saturating_add(172_800);

        let mut first: Option<TzOffset> = None;
        let mut last: Option<TzOffset> = None;
        let mut cursor = w_start;

        loop {
            let span = self.0.span_at(cursor);
            let (start, end) = (span.start, span.end);
            let offset = self.make_offset(span.period);
            let off = i128::from(offset.total_seconds());
            let local_lo = if start == i64::MIN {
                i128::MIN
            } else {
                i128::from(start) + off
            };
            let local_hi = if end == i64::MAX {
                i128::MAX
            } else {
                i128::from(end) - 1 + off
            };
            if local_lo <= i128::from(day_end) && local_hi >= i128::from(day_start) {
                if first.is_none() {
                    first = Some(offset);
                }
                last = Some(offset);
            }
            if end >= w_end || end == i64::MAX {
                break;
            }
            cursor = end;
        }

        match (first, last) {
            (None, _) => LocalResult::None,
            (Some(a), Some(b)) if a == b => LocalResult::Single(a),
            (Some(a), Some(b)) => LocalResult::Ambiguous(a, b),
            (Some(a), None) => LocalResult::Single(a),
        }
    }
}

impl Default for Tz {
    fn default() -> Self {
        Tz::UTC
    }
}

impl PartialOrd for Tz {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Tz {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp(&other.0)
    }
}

impl fmt::Display for Tz {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl fmt::Debug for Tz {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for Tz {
    type Err = ZoneError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Tz::parse(s)
    }
}

impl Serialize for Tz {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

impl<'de> Deserialize<'de> for Tz {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Tz::parse(&name).map_err(de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Zone;

    #[test]
    fn the_zone_type_is_small_and_copy() {
        fn assert_copy<T: Copy>() {}
        assert_copy::<Tz>();
        assert_copy::<TzOffset>();
        assert_copy::<Zone>();
        assert!(std::mem::size_of::<Tz>() <= 8);
        assert!(std::mem::size_of::<TzOffset>() <= 8);
        assert!(std::mem::size_of::<Zone>() <= 8);
    }

    #[test]
    fn every_zone_of_the_database_has_a_tz_under_the_same_name() {
        for zone in Zone::all() {
            assert_eq!(zone.tz().name(), zone.name());
            assert_eq!(Tz::parse(zone.name()), Ok(zone.tz()));
        }
        assert_eq!(Zone::UTC.tz(), Tz::UTC);
    }
}
