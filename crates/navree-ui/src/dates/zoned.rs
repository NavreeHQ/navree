//! From a wall-clock time to an instant, and what can go wrong on the way.
//!
//! A [`TimePicker`](super::TimePicker) holds the time on a wall, with no date and no
//! zone. To store *when* something happens, combine it with a date and the zone it
//! was meant in, and store the UTC instant: [`resolve`] does that and says so when
//! the wall clock can't give a single answer. Twice a year in many zones it can't:
//! when clocks jump forward, some times never happen; when they go back, some
//! happen twice.

use chrono::{
    DateTime, Duration, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, Offset, TimeZone, Utc,
};

/// What a wall-clock time means in a zone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Resolved {
    /// Exactly one instant.
    Single(DateTime<Utc>),
    /// The clocks went back and this time happened twice. `first` is the earlier
    /// instant (before the change), `second` the later.
    Ambiguous {
        /// The first time the clock showed it.
        first: DateTime<Utc>,
        /// The second time the clock showed it.
        second: DateTime<Utc>,
    },
    /// The clocks jumped forward and this time never happened.
    Gap {
        /// The first instant after the jump: the clock reads the end of the gap.
        transition: DateTime<Utc>,
        /// The time read with the offset from before the jump: the same wall time
        /// as the clock shows it after the jump, an hour later in most zones.
        shifted: DateTime<Utc>,
    },
}

impl Resolved {
    /// One instant for callers that don't care to ask: the first of two, and the
    /// shifted time for a time that never happened (what most calendars do).
    pub fn instant(self) -> DateTime<Utc> {
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

fn offset_seconds<Tz: TimeZone>(tz: &Tz, at: &NaiveDateTime) -> i64 {
    i64::from(tz.offset_from_utc_datetime(at).fix().local_minus_utc())
}

/// The instant of `time` on `date` on the clocks of `tz`.
///
/// ```
/// use chrono::{NaiveDate, NaiveTime, Utc, TimeZone};
/// use navree_ui::dates::zoned::{resolve, Resolved};
///
/// let day = NaiveDate::from_ymd_opt(2026, 11, 3).unwrap();
/// let noon = NaiveTime::from_hms_opt(12, 0, 0).unwrap();
/// // UTC has no changes: always one answer.
/// assert!(matches!(resolve(day, noon, &Utc), Resolved::Single(_)));
/// ```
pub fn resolve<Tz: TimeZone>(date: NaiveDate, time: NaiveTime, tz: &Tz) -> Resolved {
    let local = NaiveDateTime::new(date, time);
    match tz.from_local_datetime(&local) {
        LocalResult::Single(at) => Resolved::Single(at.with_timezone(&Utc)),
        LocalResult::Ambiguous(first, second) => Resolved::Ambiguous {
            first: first.with_timezone(&Utc),
            second: second.with_timezone(&Utc),
        },
        LocalResult::None => gap(local, tz),
    }
}

/// A wall time the zone skipped: when the clocks jumped, and what the time reads with the
/// offset from before. Every sum is checked, so a time at the very edge of what chrono can
/// hold (where a zone with an offset may skip it for being out of reach) gets an answer
/// and not a panic: there the time itself stands for both.
fn gap<Tz: TimeZone>(local: NaiveDateTime, tz: &Tz) -> Resolved {
    let by = |seconds: i64| local.checked_add_signed(Duration::seconds(seconds));
    // The offsets a day and a half either side are the ones before and after.
    let day_and_a_half = 36 * 3600;
    let before = by(-day_and_a_half)
        .map_or_else(|| offset_seconds(tz, &local), |at| offset_seconds(tz, &at));
    let after = by(day_and_a_half).map_or(before, |at| offset_seconds(tz, &at));
    // The change happened in (local - after, local - before]: find the first second at the
    // new offset.
    let (Some(shifted), Some(mut low), Some(mut high)) = (by(-before), by(-after), by(-before))
    else {
        return Resolved::Gap {
            transition: local.and_utc(),
            shifted: local.and_utc(),
        };
    };
    while high - low > Duration::seconds(1) {
        let middle = low + (high - low) / 2;
        if offset_seconds(tz, &middle) == after {
            high = middle;
        } else {
            low = middle;
        }
    }
    Resolved::Gap {
        transition: high.and_utc(),
        shifted: shifted.and_utc(),
    }
}

#[cfg(test)]
mod tests {
    use chrono_tz::Tz;

    use super::*;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn time(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    fn utc(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, min, 0).unwrap()
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
                assert_eq!(shifted - transition, Duration::minutes(15));
            }
            other => panic!("{other:?}"),
        }
        // 2026-04-05: 02:00 (+11) falls back to 01:30 (+10:30); 01:45 happens twice.
        let got = resolve(day(2026, 4, 5), time(1, 45), &howe);
        match got {
            Resolved::Ambiguous { first, second } => {
                assert_eq!(second - first, Duration::minutes(30));
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
            resolve(day(2026, 11, 3), time(9, 0), &Utc),
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
                assert_eq!(shifted - transition, Duration::hours(12));
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
