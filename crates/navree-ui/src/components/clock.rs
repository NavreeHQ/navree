//! Calendar dates and clock times from unix seconds, for the timeline and the feed.
//!
//! Civil date fields come from [`navree_datetime::civil::civil_from_days`]. The
//! functions take any `i64` and never panic: a time far outside `i32` years clamps
//! to the calendar's ends rather than failing.

use std::fmt::Write as _;

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

const WEEKDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// The longest offset from UTC that exists is under a day.
const MOST_OFFSET: i32 = 24 * 3600 - 1;

/// A moment as the wall clock of one offset shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Wall {
    year: i64,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
    weekday: u8,
    offset: i32,
}

impl Wall {
    /// The wall clock at `unix` seconds in a zone `offset` seconds east of UTC.
    pub(crate) fn at(unix: i64, offset: i32) -> Self {
        let offset = offset.clamp(-MOST_OFFSET, MOST_OFFSET);
        let local = unix.saturating_add(i64::from(offset));
        let days = local.div_euclid(86_400);
        let seconds = local.rem_euclid(86_400);
        let (year, month, day) = civil_from_days(days);
        // 1970-01-01 was a Thursday.
        let weekday = u8::try_from((days + 4).rem_euclid(7)).unwrap_or(0);
        Self {
            year,
            month,
            day,
            hour: u8::try_from(seconds / 3600).unwrap_or(0),
            minute: u8::try_from(seconds % 3600 / 60).unwrap_or(0),
            second: u8::try_from(seconds % 60).unwrap_or(0),
            weekday,
            offset,
        }
    }

    /// `2026-11-02`: the day, to tell one day from the next.
    pub(crate) fn day_key(self) -> (i64, u8, u8) {
        (self.year, self.month, self.day)
    }

    /// `Monday, November 2, 2026`.
    pub(crate) fn day_label(self) -> String {
        format!(
            "{}, {} {}, {}",
            WEEKDAYS[usize::from(self.weekday)],
            MONTHS[usize::from(self.month - 1)],
            self.day,
            self.year
        )
    }

    /// `09:41:07`.
    pub(crate) fn clock(self) -> String {
        format!("{:02}:{:02}:{:02}", self.hour, self.minute, self.second)
    }

    /// `2026-11-02T09:41:07+05:30`: for a `<time datetime>`.
    pub(crate) fn iso(self) -> String {
        let mut out = String::with_capacity(25);
        if (0..=9999).contains(&self.year) {
            let _ = write!(out, "{:04}", self.year);
        } else {
            let _ = write!(out, "{:+}", self.year);
        }
        let _ = write!(
            out,
            "-{:02}-{:02}T{}{}",
            self.month,
            self.day,
            self.clock(),
            offset_iso(self.offset)
        );
        out
    }
}

/// `+05:30`, `-08:00`, `+00:00`.
fn offset_iso(offset: i32) -> String {
    let sign = if offset < 0 { '-' } else { '+' };
    let all = offset.unsigned_abs();
    format!("{sign}{:02}:{:02}", all / 3600, all % 3600 / 60)
}

/// `GMT+05:30`, `GMT-08:00`.
pub(crate) fn offset_label(offset: i32) -> String {
    format!("GMT{}", offset_iso(offset.clamp(-MOST_OFFSET, MOST_OFFSET)))
}

/// Year, month and day of a count of days since 1970-01-01 (the proleptic
/// Gregorian calendar).
fn civil_from_days(days: i64) -> (i64, u8, u8) {
    match navree_datetime::civil::civil_from_days(days) {
        Some((year, month, day)) => (i64::from(year), month, day),
        None if days < 0 => (i64::from(i32::MIN), 1, 1),
        None => (i64::from(i32::MAX), 12, 31),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_epoch_is_a_thursday() {
        let wall = Wall::at(0, 0);
        assert_eq!(wall.day_label(), "Thursday, January 1, 1970");
        assert_eq!(wall.clock(), "00:00:00");
    }

    #[test]
    fn a_known_day_in_kolkata() {
        // 2026-11-02 04:11:07 UTC is 09:41:07 in Asia/Kolkata (+05:30).
        let wall = Wall::at(1_793_592_667, 19_800);
        assert_eq!(wall.day_label(), "Monday, November 2, 2026");
        assert_eq!(wall.clock(), "09:41:07");
        assert_eq!(wall.iso(), "2026-11-02T09:41:07+05:30");
    }

    #[test]
    fn midnight_and_the_day_boundary_follow_the_offset() {
        // 2026-11-02 18:29:59 UTC is still Monday in UTC and Monday 23:59:59 in Kolkata;
        // one second later it is Tuesday in Kolkata.
        let before = Wall::at(1_793_644_199, 19_800);
        let after = Wall::at(1_793_644_200, 19_800);
        assert_eq!(before.clock(), "23:59:59");
        assert_eq!(after.clock(), "00:00:00");
        assert_ne!(before.day_key(), after.day_key());
        assert_eq!(Wall::at(1_793_644_200, 0).day_key(), before.day_key());
    }

    #[test]
    fn a_leap_day_exists_and_a_century_year_does_not_have_one() {
        // 2028-02-29 12:00:00 UTC.
        assert_eq!(
            Wall::at(1_835_438_400, 0).day_label(),
            "Tuesday, February 29, 2028"
        );
        // 2100 is not a leap year: 2100-03-01 follows 2100-02-28.
        assert_eq!(
            Wall::at(4_107_542_400, 0).day_label(),
            "Monday, March 1, 2100"
        );
    }

    #[test]
    fn times_before_1970_work() {
        assert_eq!(Wall::at(-1, 0).day_label(), "Wednesday, December 31, 1969");
        assert_eq!(Wall::at(-1, 0).clock(), "23:59:59");
    }

    #[test]
    fn negative_and_zero_offsets_are_labelled() {
        assert_eq!(offset_label(-28_800), "GMT-08:00");
        assert_eq!(offset_label(0), "GMT+00:00");
        assert_eq!(offset_label(19_800), "GMT+05:30");
        assert_eq!(offset_label(i32::MAX), "GMT+23:59");
    }

    #[test]
    fn nothing_panics_at_the_edges() {
        for unix in [i64::MIN, i64::MIN + 1, -1, 0, i64::MAX - 1, i64::MAX] {
            for offset in [i32::MIN, -86_400, 0, 19_800, i32::MAX] {
                let wall = Wall::at(unix, offset);
                assert!(!wall.day_label().is_empty());
                assert_eq!(wall.clock().len(), 8);
                assert!(wall.iso().contains('T'));
            }
        }
    }
}
