//! [`Tz`], [`TzOffset`] and [`LocalTime`]: Navree's IANA time zone and offset
//! types over every zone and link in the compiled tz database.

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

use navree_datetime::__private::{Entry, LocalPeriods, Period};
use navree_datetime::{DateTime, Disambiguation, Error, ErrorKind, Offset, Span, Timestamp, Zoned};
use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize, Serializer};

use crate::{Zone, ZoneError};

/// An IANA time zone or link from the compiled tz database (`Asia/Kolkata`,
/// `America/New_York`, `UTC`, `Asia/Calcutta`, `Factory`).
///
/// Unlike [`Zone`], which rejects `Factory` and normalizes legacy aliases to
/// the names every PostgreSQL accepts, `Tz` knows every zone and link of the
/// IANA release under its own name.
///
/// ```
/// use navree_datetime::Timestamp;
/// use navree_time::Tz;
///
/// let kolkata: Tz = "Asia/Kolkata".parse()?;
/// let at = Timestamp::utc(2026, 1, 15, 12, 0, 0).unwrap();
/// assert_eq!(kolkata.local(at).to_string(), "2026-01-15 17:30:00 IST");
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

/// What a wall-clock time resolves to in a [`Tz`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LocalTime<T> {
    /// The clocks jumped forward over this time; no instant shows it.
    None,
    /// Exactly one instant shows this time.
    Single(T),
    /// The clocks went back over this time; holds `(earliest, latest)`.
    Ambiguous(T, T),
}

impl<T> LocalTime<T> {
    /// `Some(v)` only when the wall-clock time is unambiguous.
    pub fn single(self) -> Option<T> {
        match self {
            LocalTime::Single(value) => Some(value),
            LocalTime::None | LocalTime::Ambiguous(_, _) => None,
        }
    }

    /// `Some(v)` when unambiguous, or the earlier of two ambiguous values.
    pub fn earliest(self) -> Option<T> {
        match self {
            LocalTime::Single(value) | LocalTime::Ambiguous(value, _) => Some(value),
            LocalTime::None => None,
        }
    }

    /// `Some(v)` when unambiguous, or the later of two ambiguous values.
    pub fn latest(self) -> Option<T> {
        match self {
            LocalTime::Single(value) | LocalTime::Ambiguous(_, value) => Some(value),
            LocalTime::None => None,
        }
    }

    /// Transforms the inner value(s).
    pub fn map<U>(self, mut f: impl FnMut(T) -> U) -> LocalTime<U> {
        match self {
            LocalTime::None => LocalTime::None,
            LocalTime::Single(value) => LocalTime::Single(f(value)),
            LocalTime::Ambiguous(earliest, latest) => LocalTime::Ambiguous(f(earliest), f(latest)),
        }
    }
}

impl Tz {
    /// Coordinated Universal Time (`UTC`).
    pub const UTC: Tz = Tz(Entry::UTC);

    /// Looks up a zone or link of the IANA database by name, without regard to
    /// ASCII case. Surrounding whitespace is not trimmed (unlike
    /// [`Zone::parse`], which trims whitespace, normalizes legacy aliases and
    /// rejects `Factory`).
    pub fn parse(name: &str) -> Result<Tz, ZoneError> {
        Entry::find(name)
            .map(Tz)
            .ok_or_else(|| ZoneError::new(name))
    }

    /// The zone or link's IANA name (`Asia/Kolkata`, `Asia/Calcutta`, `UTC`).
    pub const fn name(self) -> &'static str {
        self.0.name()
    }

    /// The canonical [`Zone`] for this entry, or `None` for non-place entries
    /// such as `Factory`.
    pub fn zone(self) -> Option<Zone> {
        Zone::parse(self.name()).ok()
    }

    /// The [`TzOffset`] in force at `at`.
    pub fn offset_at(self, at: Timestamp) -> TzOffset {
        self.make_offset(self.0.span_at(at.as_secs()).period)
    }

    /// The zone's abbreviation at `at` (`IST`, `EST`, `EDT`, `+0530`, `UTC`).
    pub fn abbreviation(self, at: Timestamp) -> &'static str {
        self.offset_at(at).abbreviation()
    }

    /// `at` seen on this zone's clock.
    pub fn local(self, at: Timestamp) -> Zoned {
        let zone = navree_datetime::Zone::named(self.name())
            .unwrap_or_else(|_| navree_datetime::Zone::fixed(self.offset_at(at).offset()));
        at.in_zone(zone)
    }

    /// The offset(s) at which this zone's clocks show `local`.
    pub fn offset_of_local(self, local: DateTime) -> LocalTime<TzOffset> {
        match self.0.local(local.in_utc().as_secs()) {
            LocalPeriods::Gap => LocalTime::None,
            LocalPeriods::Single(period) => LocalTime::Single(self.make_offset(period)),
            LocalPeriods::Fold(earliest, latest) => {
                LocalTime::Ambiguous(self.make_offset(earliest), self.make_offset(latest))
            }
        }
    }

    /// Resolves the wall-clock time `local` to its UTC instant(s) and offset(s)
    /// in this zone.
    pub fn resolve_local(self, local: DateTime) -> LocalTime<(Timestamp, TzOffset)> {
        let wall = local.in_utc().as_secs();
        let nanos = local.time().nanosecond();
        let instant_at = |offset: TzOffset| -> Option<(Timestamp, TzOffset)> {
            let secs = wall.checked_sub(i64::from(offset.total_seconds()))?;
            Timestamp::from_secs_nanos(secs, nanos)
                .ok()
                .map(|at| (at, offset))
        };
        match self.offset_of_local(local) {
            LocalTime::None => LocalTime::None,
            LocalTime::Single(offset) => match instant_at(offset) {
                Some(pair) => LocalTime::Single(pair),
                None => LocalTime::None,
            },
            LocalTime::Ambiguous(earliest, latest) => {
                match (instant_at(earliest), instant_at(latest)) {
                    (Some(a), Some(b)) => LocalTime::Ambiguous(a, b),
                    (Some(a), None) | (None, Some(a)) => LocalTime::Single(a),
                    (None, None) => LocalTime::None,
                }
            }
        }
    }

    /// Resolves `local` on this zone's clocks using `how` for gaps and folds.
    pub fn to_timestamp(
        self,
        local: DateTime,
        how: Disambiguation,
    ) -> navree_datetime::Result<Timestamp> {
        let wall = local.in_utc().as_secs();
        let offset = match self.0.local(wall) {
            LocalPeriods::Single(period) => period.offset(),
            LocalPeriods::Fold(earlier, later) => match how {
                Disambiguation::Compatible | Disambiguation::Earlier => earlier.offset(),
                Disambiguation::Later => later.offset(),
                Disambiguation::Reject => {
                    return Err(Error::new(
                        ErrorKind::Ambiguous,
                        format!(
                            "{local} comes twice in {}: the clocks go back over it",
                            self.name()
                        ),
                    ));
                }
            },
            LocalPeriods::Gap => match how {
                Disambiguation::Compatible | Disambiguation::Later => {
                    self.0.offset_before_gap(wall)
                }
                Disambiguation::Earlier => self.0.offset_after_gap(wall),
                Disambiguation::Reject => {
                    return Err(Error::new(
                        ErrorKind::Gap,
                        format!(
                            "{local} does not exist in {}: the clocks go forward over it",
                            self.name()
                        ),
                    ));
                }
            },
        };
        Timestamp::from_secs_nanos(wall - i64::from(offset), local.time().nanosecond())
    }

    /// Resolves `local` on this zone's clocks with
    /// [`Disambiguation::Compatible`], matching [`Zone::to_utc`].
    pub fn to_utc(self, local: DateTime) -> Timestamp {
        self.to_timestamp(local, Disambiguation::Compatible)
            .unwrap_or_else(|_| local.in_utc())
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

    fn make_offset(self, period: Period) -> TzOffset {
        TzOffset { tz: self, period }
    }
}

impl TzOffset {
    /// The [`Tz`] this offset belongs to.
    pub const fn tz(self) -> Tz {
        self.tz
    }

    /// The total UTC offset during this period.
    pub fn offset(self) -> Offset {
        Offset::east(self.total_seconds()).expect("tz database offset fits Offset")
    }

    /// The total offset in seconds east of UTC.
    pub fn total_seconds(self) -> i32 {
        self.period.offset()
    }

    /// The zone's standard (non-DST) offset from UTC during this period.
    pub fn base_utc_offset(self) -> Span {
        Span::seconds(i64::from(self.period.standard_offset()))
    }

    /// The daylight saving adjustment added to [`Self::base_utc_offset`] during
    /// this period (`Span::ZERO` in standard time; negative in zones with
    /// negative winter adjustments such as `Europe/Dublin`).
    pub fn dst_offset(self) -> Span {
        Span::seconds(i64::from(self.period.daylight_saving()))
    }

    /// The zone's abbreviation during this period (`IST`, `EDT`, `+0530`).
    pub fn abbreviation(self) -> &'static str {
        self.period.abbreviation()
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
