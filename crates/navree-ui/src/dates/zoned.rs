//! From a wall-clock time to an instant, and what can go wrong on the way.
//!
//! A [`TimePicker`](super::TimePicker) holds the time on a wall, with no date and no
//! zone. To store *when* something happens, combine it with a date and the zone it
//! was meant in, and store the UTC instant: [`resolve`] does that and says so when
//! the wall clock can't give a single answer. Twice a year in many zones it can't:
//! when clocks jump forward, some times never happen; when they go back, some
//! happen twice.

use navree_datetime::{
    Date, DateTime, Disambiguation, ErrorKind, Offset, Span, Time, Timestamp, Zone,
};

/// A time zone whose clocks a wall-clock date and time can be read against.
pub trait WallZone {
    /// Resolves `local` on this zone's clocks using `how` for gaps and folds.
    fn to_timestamp(
        &self,
        local: DateTime,
        how: Disambiguation,
    ) -> navree_datetime::Result<Timestamp>;

    /// The offset from UTC this zone keeps at `at`.
    fn offset_at(&self, at: Timestamp) -> Offset;

    /// `at` as a wall-clock date and time in this zone.
    fn local_date_time(&self, at: Timestamp) -> DateTime;
}

impl WallZone for Zone {
    fn to_timestamp(
        &self,
        local: DateTime,
        how: Disambiguation,
    ) -> navree_datetime::Result<Timestamp> {
        Zone::to_timestamp(*self, local, how)
    }

    fn offset_at(&self, at: Timestamp) -> Offset {
        Zone::offset_at(*self, at)
    }

    fn local_date_time(&self, at: Timestamp) -> DateTime {
        at.in_zone(*self).date_time()
    }
}

impl WallZone for Offset {
    fn to_timestamp(
        &self,
        local: DateTime,
        how: Disambiguation,
    ) -> navree_datetime::Result<Timestamp> {
        Zone::fixed(*self).to_timestamp(local, how)
    }

    fn offset_at(&self, _at: Timestamp) -> Offset {
        *self
    }

    fn local_date_time(&self, at: Timestamp) -> DateTime {
        at.in_zone(Zone::fixed(*self)).date_time()
    }
}

#[cfg(feature = "time")]
#[cfg_attr(docsrs, doc(cfg(feature = "time")))]
impl WallZone for navree_time::Tz {
    fn to_timestamp(
        &self,
        local: DateTime,
        how: Disambiguation,
    ) -> navree_datetime::Result<Timestamp> {
        navree_time::Tz::to_timestamp(*self, local, how)
    }

    fn offset_at(&self, at: Timestamp) -> Offset {
        navree_time::Tz::offset_at(*self, at).offset()
    }

    fn local_date_time(&self, at: Timestamp) -> DateTime {
        self.local(at).date_time()
    }
}

#[cfg(feature = "time")]
#[cfg_attr(docsrs, doc(cfg(feature = "time")))]
impl WallZone for navree_time::Zone {
    fn to_timestamp(
        &self,
        local: DateTime,
        how: Disambiguation,
    ) -> navree_datetime::Result<Timestamp> {
        navree_time::Zone::to_timestamp(self, local, how)
    }

    fn offset_at(&self, at: Timestamp) -> Offset {
        navree_time::Zone::offset_at(self, at)
    }

    fn local_date_time(&self, at: Timestamp) -> DateTime {
        self.local(at).date_time()
    }
}

/// What a wall-clock time means in a zone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Resolved {
    /// Exactly one instant.
    Single(Timestamp),
    /// The clocks went back and this time happened twice. `first` is the earlier
    /// instant (before the change), `second` the later.
    Ambiguous {
        /// The first time the clock showed it.
        first: Timestamp,
        /// The second time the clock showed it.
        second: Timestamp,
    },
    /// The clocks jumped forward and this time never happened.
    Gap {
        /// The first instant after the jump: the clock reads the end of the gap.
        transition: Timestamp,
        /// The time read with the offset from before the jump: the same wall time
        /// as the clock shows it after the jump, an hour later in most zones.
        shifted: Timestamp,
    },
}

impl Resolved {
    /// One instant for callers that don't care to ask: the first of two, and the
    /// shifted time for a time that never happened (what most calendars do).
    pub fn instant(self) -> Timestamp {
        match self {
            Self::Single(at) => at,
            Self::Ambiguous { first, .. } => first,
            Self::Gap { shifted, .. } => shifted,
        }
    }

    /// Whether the time is a single, certain instant.
    pub fn is_certain(self) -> bool {
        matches!(self, Self::Single(_))
    }
}

fn offset_seconds<Z: WallZone>(tz: &Z, at: DateTime) -> i64 {
    i64::from(tz.offset_at(at.in_utc()).seconds())
}

/// The instant of `time` on `date` on the clocks of `tz`.
///
/// ```
/// use navree_datetime::{Date, Time, Zone};
/// use navree_ui::dates::zoned::{Resolved, resolve};
///
/// let day = Date::new(2026, 11, 3).unwrap();
/// let noon = Time::new(12, 0, 0).unwrap();
/// // UTC has no changes: always one answer.
/// assert!(matches!(resolve(day, noon, &Zone::UTC), Resolved::Single(_)));
/// ```
pub fn resolve<Z: WallZone>(date: Date, time: Time, tz: &Z) -> Resolved {
    let local = DateTime::new(date, time);
    match tz.to_timestamp(local, Disambiguation::Reject) {
        Ok(at) => Resolved::Single(at),
        Err(err) if err.kind() == ErrorKind::Ambiguous => {
            let first = tz.to_timestamp(local, Disambiguation::Earlier).ok();
            let second = tz.to_timestamp(local, Disambiguation::Later).ok();
            match (first, second) {
                (Some(first), Some(second)) => Resolved::Ambiguous { first, second },
                (Some(at), None) | (None, Some(at)) => Resolved::Single(at),
                (None, None) => gap(local, tz),
            }
        }
        Err(_) => gap(local, tz),
    }
}

/// A wall time the zone skipped: when the clocks jumped, and what the time reads with the
/// offset from before. Every sum is checked, so a time at the very edge of what `Timestamp`
/// can hold (where a zone with an offset may skip it for being out of reach) gets an answer
/// and not a panic: there the time itself stands for both.
fn gap<Z: WallZone>(local: DateTime, tz: &Z) -> Resolved {
    let by = |seconds: i64| local.add(Span::seconds(seconds));
    // The offsets a day and a half either side are the ones before and after.
    let day_and_a_half = 36 * 3600;
    let before = by(-day_and_a_half)
        .map_or_else(|| offset_seconds(tz, local), |at| offset_seconds(tz, at));
    let after = by(day_and_a_half).map_or(before, |at| offset_seconds(tz, at));
    // The change happened in (local - after, local - before]: find the first second at the
    // new offset.
    let (Some(shifted), Some(mut low), Some(mut high)) = (by(-before), by(-after), by(-before))
    else {
        return Resolved::Gap {
            transition: local.in_utc(),
            shifted: local.in_utc(),
        };
    };
    while (high.in_utc() - low.in_utc()) > Span::seconds(1) {
        let half = (high.in_utc() - low.in_utc()).as_seconds() / 2;
        let Some(middle) = low.add(Span::seconds(half)) else {
            break;
        };
        if offset_seconds(tz, middle) == after {
            high = middle;
        } else {
            low = middle;
        }
    }
    Resolved::Gap {
        transition: high.in_utc(),
        shifted: shifted.in_utc(),
    }
}

#[cfg(all(test, feature = "time"))]
mod tests {
    use navree_time::Tz;

    use super::*;

    fn day(y: i32, m: u8, d: u8) -> Date {
        Date::new(y, m, d).unwrap()
    }

    fn time(h: u8, m: u8) -> Time {
        Time::new(h, m, 0).unwrap()
    }

    fn utc(y: i32, m: u8, d: u8, h: u8, min: u8) -> Timestamp {
        Timestamp::utc(y, m, d, h, min, 0).unwrap()
    }

    #[test]
    fn new_york_spring_forward_skips_two_thirty() {
        let ny: Tz = "America/New_York".parse().unwrap();
        // 2026-03-08: 02:00 EST jumps to 03:00 EDT (07:00 UTC).
        let got = resolve(day(2026, 3, 8), time(2, 30), &ny);
        assert_eq!(
            got,
            Resolved::Gap {
                transition: utc(2026, 3, 8, 7, 0),
                shifted: utc(2026, 3, 8, 7, 30),
            }
        );
        // Either side of the gap is certain.
        assert_eq!(
            resolve(day(2026, 3, 8), time(1, 59), &ny),
            Resolved::Single(utc(2026, 3, 8, 6, 59))
        );
        assert_eq!(
            resolve(day(2026, 3, 8), time(3, 0), &ny),
            Resolved::Single(utc(2026, 3, 8, 7, 0))
        );
    }

    #[test]
    fn new_york_fall_back_happens_twice() {
        let ny: Tz = "America/New_York".parse().unwrap();
        // 2026-11-01: 02:00 EDT falls back to 01:00 EST; 01:30 shows twice.
        assert_eq!(
            resolve(day(2026, 11, 1), time(1, 30), &ny),
            Resolved::Ambiguous {
                first: utc(2026, 11, 1, 5, 30),
                second: utc(2026, 11, 1, 6, 30),
            }
        );
        assert_eq!(
            resolve(day(2026, 11, 1), time(0, 59), &ny),
            Resolved::Single(utc(2026, 11, 1, 4, 59))
        );
        assert_eq!(
            resolve(day(2026, 11, 1), time(2, 0), &ny),
            Resolved::Single(utc(2026, 11, 1, 7, 0))
        );
    }

    #[test]
    fn london_changes_on_other_days_at_one_o_clock() {
        let london: Tz = "Europe/London".parse().unwrap();
        // 2026-03-29: 01:00 GMT jumps to 02:00 BST; 01:30 never happens.
        assert_eq!(
            resolve(day(2026, 3, 29), time(1, 30), &london),
            Resolved::Gap {
                transition: utc(2026, 3, 29, 1, 0),
                shifted: utc(2026, 3, 29, 1, 30),
            }
        );
        // 2026-10-25: 02:00 BST falls back to 01:00 GMT; 01:30 happens twice.
        assert_eq!(
            resolve(day(2026, 10, 25), time(1, 30), &london),
            Resolved::Ambiguous {
                first: utc(2026, 10, 25, 0, 30),
                second: utc(2026, 10, 25, 1, 30),
            }
        );
    }

    #[test]
    fn lord_howe_shifts_by_thirty_minutes() {
        let howe: Tz = "Australia/Lord_Howe".parse().unwrap();
        // 2026-10-04: 02:00 (+10:30) jumps to 02:30 (+11); 02:15 never happens.
        let got = resolve(day(2026, 10, 4), time(2, 15), &howe);
        match got {
            Resolved::Gap {
                transition,
                shifted,
            } => {
                assert_eq!(transition, utc(2026, 10, 3, 15, 30));
                assert_eq!(shifted - transition, Span::minutes(15));
            }
            other => panic!("{other:?}"),
        }
        // 2026-04-05: 02:00 (+11) falls back to 01:30 (+10:30); 01:45 happens twice.
        let got = resolve(day(2026, 4, 5), time(1, 45), &howe);
        match got {
            Resolved::Ambiguous { first, second } => {
                assert_eq!(second - first, Span::minutes(30));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn zones_without_changes_are_always_certain() {
        let kolkata: Tz = "Asia/Kolkata".parse().unwrap();
        for hour in 0..24 {
            assert!(resolve(day(2026, 3, 8), time(hour, 30), &kolkata).is_certain());
            assert!(resolve(day(2026, 11, 1), time(hour, 30), &kolkata).is_certain());
        }
        assert_eq!(
            resolve(day(2026, 11, 3), time(9, 0), &kolkata),
            Resolved::Single(utc(2026, 11, 3, 3, 30))
        );
        assert!(matches!(
            resolve(day(2026, 11, 3), time(9, 0), &Zone::UTC),
            Resolved::Single(_)
        ));
    }

    #[test]
    fn samoa_skipped_a_whole_day() {
        let apia: Tz = "Pacific/Apia".parse().unwrap();
        // 2011-12-30 did not exist in Samoa: the clocks went from 29 Dec 23:59 to 31 Dec.
        let got = resolve(day(2011, 12, 30), time(12, 0), &apia);
        match got {
            Resolved::Gap {
                transition,
                shifted,
            } => {
                assert_eq!(transition, utc(2011, 12, 30, 10, 0));
                assert_eq!(shifted - transition, Span::hours(12));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn far_zones_resolve() {
        let kiritimati: Tz = "Pacific/Kiritimati".parse().unwrap();
        assert_eq!(
            resolve(day(2026, 11, 3), time(0, 0), &kiritimati),
            Resolved::Single(utc(2026, 11, 2, 10, 0))
        );
        let baker: Tz = "Etc/GMT+12".parse().unwrap();
        assert_eq!(
            resolve(day(2026, 11, 3), time(0, 0), &baker),
            Resolved::Single(utc(2026, 11, 3, 12, 0))
        );
    }

    #[test]
    fn the_southern_summer_changes_the_other_way() {
        let sydney: Tz = "Australia/Sydney".parse().unwrap();
        // 2026-10-04: 02:00 AEST jumps to 03:00 AEDT.
        assert!(matches!(
            resolve(day(2026, 10, 4), time(2, 30), &sydney),
            Resolved::Gap { .. }
        ));
        // 2026-04-05: 03:00 AEDT falls back to 02:00 AEST.
        assert!(matches!(
            resolve(day(2026, 4, 5), time(2, 30), &sydney),
            Resolved::Ambiguous { .. }
        ));
    }

    #[test]
    fn instant_picks_the_first_or_the_shifted() {
        let ny: Tz = "America/New_York".parse().unwrap();
        assert_eq!(
            resolve(day(2026, 11, 1), time(1, 30), &ny).instant(),
            utc(2026, 11, 1, 5, 30)
        );
        assert_eq!(
            resolve(day(2026, 3, 8), time(2, 30), &ny).instant(),
            utc(2026, 3, 8, 7, 30)
        );
    }
}
