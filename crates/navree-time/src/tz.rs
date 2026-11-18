//! [`Tz`] and [`TzOffset`]: Navree's own IANA time zone and offset types over
//! the compiled tables in [`crate::tables`].

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

use chrono::{
    DateTime, FixedOffset, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, Offset, TimeDelta,
    TimeZone, Utc,
};
use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize, Serializer};

use crate::ZoneError;
use crate::tables::{
    ABBREVS, BY_LOWER, DaySpec, ENTRIES, PERIODS, RuleEdge, STANDING_RULES, TRANSITIONS, TimeRef,
    UTC_INDEX, ZONES,
};

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
pub struct Tz(u16);

/// The UTC offset and abbreviation of a [`Tz`] at a particular instant.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct TzOffset {
    tz: Tz,
    period: u16,
}

impl Tz {
    /// Coordinated Universal Time (`UTC`).
    pub const UTC: Tz = Tz(UTC_INDEX);

    /// Looks up a zone or link of the IANA database by name, without regard to
    /// ASCII case. Surrounding whitespace is not trimmed (unlike
    /// [`Zone::parse`](crate::Zone::parse), which trims whitespace, normalizes
    /// legacy aliases and rejects `Factory`).
    pub fn parse(name: &str) -> Result<Tz, ZoneError> {
        lookup_ignore_ascii_case(name).ok_or_else(|| ZoneError::new(name))
    }

    /// The zone or link's IANA name (`Asia/Kolkata`, `Asia/Calcutta`, `UTC`).
    pub const fn name(self) -> &'static str {
        ENTRIES[self.0 as usize].0
    }

    /// The zone's abbreviation at `at` (`IST`, `EST`, `EDT`, `+0530`, `UTC`).
    pub fn abbreviation(self, at: DateTime<Utc>) -> &'static str {
        self.offset_from_utc_datetime(&at.naive_utc())
            .abbreviation()
    }

    /// Every zone and link in the compiled tz database release, sorted by
    /// exact name.
    pub fn all() -> impl ExactSizeIterator<Item = Tz> + DoubleEndedIterator + Clone {
        (0..ENTRIES.len() as u16).map(Tz)
    }

    fn make_offset(self, period: u16) -> TzOffset {
        TzOffset { tz: self, period }
    }

    /// Returns `(start_utc, end_utc, period_idx)` for the UTC period active at
    /// `ts` seconds since `1970-01-01T00:00:00Z`, with `start_utc <= ts < end_utc`
    /// (`i64::MIN` and `i64::MAX` stand for unbounded ends).
    fn period_span_at_utc(self, ts: i64) -> (i64, i64, u16) {
        let z = &ZONES[usize::from(ENTRIES[usize::from(self.0)].1)];
        let transitions = &TRANSITIONS[z.start as usize..z.end as usize];
        let Some(&(last_at, last_period)) = transitions.last() else {
            if let Some(sr_idx) = z.standing {
                return eval_standing(ts, i64::MIN, sr_idx);
            }
            return (i64::MIN, i64::MAX, z.initial);
        };

        if ts < last_at {
            let idx = transitions.partition_point(|&(at, _)| at <= ts);
            let start = if idx == 0 {
                i64::MIN
            } else {
                transitions[idx - 1].0
            };
            let end = transitions[idx].0;
            let period = if idx == 0 {
                z.initial
            } else {
                transitions[idx - 1].1
            };
            return (start, end, period);
        }

        if let Some(sr_idx) = z.standing {
            eval_standing(ts, last_at, sr_idx)
        } else {
            (last_at, i64::MAX, last_period)
        }
    }
}

impl TzOffset {
    /// The [`Tz`] this offset belongs to.
    pub const fn tz(self) -> Tz {
        self.tz
    }

    /// The zone's standard (non-DST) offset from UTC during this period.
    pub fn base_utc_offset(self) -> TimeDelta {
        TimeDelta::seconds(i64::from(PERIODS[usize::from(self.period)].std_offset))
    }

    /// The daylight saving adjustment added to [`Self::base_utc_offset`] during
    /// this period (`TimeDelta::zero()` in standard time; negative in zones
    /// with negative winter adjustments such as `Europe/Dublin`).
    pub fn dst_offset(self) -> TimeDelta {
        TimeDelta::seconds(i64::from(PERIODS[usize::from(self.period)].dst_offset))
    }

    /// The zone's abbreviation during this period (`IST`, `EDT`, `+0530`).
    pub fn abbreviation(self) -> &'static str {
        ABBREVS[usize::from(PERIODS[usize::from(self.period)].abbrev)]
    }

    pub(crate) fn total_seconds(self) -> i32 {
        let p = &PERIODS[usize::from(self.period)];
        p.std_offset + p.dst_offset
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
        let (_, _, period) = self.period_span_at_utc(utc.and_utc().timestamp());
        self.make_offset(period)
    }

    fn offset_from_utc_date(&self, utc: &NaiveDate) -> Self::Offset {
        self.offset_from_utc_datetime(&utc.and_time(NaiveTime::MIN))
    }

    fn offset_from_local_datetime(&self, local: &NaiveDateTime) -> LocalResult<Self::Offset> {
        let local_secs = local.and_utc().timestamp();
        let w_start = local_secs.saturating_sub(172_800);
        let w_end = local_secs.saturating_add(172_800);

        let mut matches = [TzOffset {
            tz: *self,
            period: 0,
        }; 4];
        let mut count = 0;
        let mut cursor = w_start;

        loop {
            let (start, end, period) = self.period_span_at_utc(cursor);
            let offset = self.make_offset(period);
            if let Some(u) = local_secs.checked_sub(i64::from(offset.total_seconds()))
                && u >= start
                && u < end
            {
                matches[count] = offset;
                count += 1;
            }
            if end >= w_end || end == i64::MAX || count == matches.len() {
                break;
            }
            cursor = end;
        }

        match count {
            0 => LocalResult::None,
            1 => LocalResult::Single(matches[0]),
            _ => LocalResult::Ambiguous(matches[0], matches[count - 1]),
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
            let (start, end, period) = self.period_span_at_utc(cursor);
            let offset = self.make_offset(period);
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

fn lookup_ignore_ascii_case(name: &str) -> Option<Tz> {
    if name.is_empty() {
        return None;
    }
    let pos = BY_LOWER
        .binary_search_by(|&entry_idx| cmp_ascii_lower(ENTRIES[usize::from(entry_idx)].0, name))
        .ok()?;
    Some(Tz(BY_LOWER[pos]))
}

fn cmp_ascii_lower(a: &str, b: &str) -> Ordering {
    a.bytes()
        .map(|c| c.to_ascii_lowercase())
        .cmp(b.bytes().map(|c| c.to_ascii_lowercase()))
}

fn eval_standing(ts: i64, min_start: i64, sr_idx: u16) -> (i64, i64, u16) {
    let sr = &STANDING_RULES[usize::from(sr_idx)];
    let year = year_from_days(ts.div_euclid(86_400));
    let first_prev_dst = PERIODS[usize::from(sr.second.period)].dst_offset;
    let second_prev_dst = PERIODS[usize::from(sr.first.period)].dst_offset;

    let edges: [(i64, u16); 6] = [
        (
            edge_utc(year - 1, &sr.first, sr.std_offset, first_prev_dst),
            sr.first.period,
        ),
        (
            edge_utc(year - 1, &sr.second, sr.std_offset, second_prev_dst),
            sr.second.period,
        ),
        (
            edge_utc(year, &sr.first, sr.std_offset, first_prev_dst),
            sr.first.period,
        ),
        (
            edge_utc(year, &sr.second, sr.std_offset, second_prev_dst),
            sr.second.period,
        ),
        (
            edge_utc(year + 1, &sr.first, sr.std_offset, first_prev_dst),
            sr.first.period,
        ),
        (
            edge_utc(year + 1, &sr.second, sr.std_offset, second_prev_dst),
            sr.second.period,
        ),
    ];

    let pos = edges.partition_point(|&(at, _)| at <= ts);
    let start = if pos == 0 {
        min_start
    } else {
        edges[pos - 1].0.max(min_start)
    };
    let end = if pos < edges.len() {
        edges[pos].0
    } else {
        i64::MAX
    };
    let period = if pos == 0 {
        sr.second.period
    } else {
        edges[pos - 1].1
    };
    (start, end, period)
}

fn edge_utc(year: i64, edge: &RuleEdge, std_offset: i32, prev_dst: i32) -> i64 {
    let local = resolve_day(year, edge.month, edge.day) * 86_400 + i64::from(edge.at_secs);
    match edge.at_ref {
        TimeRef::Utc => local,
        TimeRef::Standard => local - i64::from(std_offset),
        TimeRef::Wall => local - i64::from(std_offset + prev_dst),
    }
}

fn resolve_day(year: i64, month: u8, spec: DaySpec) -> i64 {
    match spec {
        DaySpec::Dom(d) => days_from_civil(year, u32::from(month), u32::from(d)),
        DaySpec::LastWeekday(target) => {
            let last = days_from_civil(year, u32::from(month), days_in_month(year, month));
            let diff = i64::from((weekday_of_days(last) + 7 - target) % 7);
            last - diff
        }
        DaySpec::OnOrAfter { weekday, dom } => {
            let base = days_from_civil(year, u32::from(month), u32::from(dom));
            let diff = i64::from((weekday + 7 - weekday_of_days(base)) % 7);
            base + diff
        }
        DaySpec::OnOrBefore { weekday, dom } => {
            let base = days_from_civil(year, u32::from(month), u32::from(dom));
            let diff = i64::from((weekday_of_days(base) + 7 - weekday) % 7);
            base - diff
        }
    }
}

fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i64, month: u8) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => unreachable!(),
    }
}

fn weekday_of_days(days: i64) -> u8 {
    (days + 4).rem_euclid(7) as u8
}

fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let month = i64::from(month);
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn year_from_days(days: i64) -> i64 {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    yoe + era * 400 + i64::from(month <= 2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Zone;
    use crate::tables::TZDATA_VERSION;

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
    fn the_tables_name_the_tz_database_release_they_were_made_from() {
        assert_eq!(TZDATA_VERSION, crate::tzdata_version());
        assert!(TZDATA_VERSION.len() >= 5, "{TZDATA_VERSION}");
        assert!(
            TZDATA_VERSION.as_bytes()[..4]
                .iter()
                .all(u8::is_ascii_digit),
            "{TZDATA_VERSION}"
        );
        assert!(
            TZDATA_VERSION.as_bytes()[4..]
                .iter()
                .all(u8::is_ascii_lowercase),
            "{TZDATA_VERSION}"
        );
    }
}
