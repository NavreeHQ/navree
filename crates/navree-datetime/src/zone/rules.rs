//! What a zone's clocks do: the offset they keep at an instant, and what
//! they make of a time read off them.
//!
//! A zone of the IANA database is a row of periods: stretches of time over
//! which its clocks keep one offset from UTC and one abbreviation. The
//! compiled [tables](super::tables) hold, for each zone, every change from
//! one period to the next up to the last one on record, and, for a zone
//! that still moves its clocks twice a year, the standing rule that says
//! when it does in every year after that.
//!
//! Two questions are answered here and nowhere else. Which period is in
//! force at an instant ([`Entry::span_at`]): a search of the recorded
//! changes, then the standing rule. And which periods show a time read off
//! the zone's clocks ([`Entry::local`]): one, as a rule; two where the
//! clocks went back over it (a fold); none where they went forward over it
//! (a gap).
//!
//! The types are public for navree-time's `Tz`, which stands on them; they
//! are no part of this crate's API (`__private` in the crate root).

use std::cmp::Ordering;

use super::Disambiguation;
use super::tables::{
    ABBREVS, BY_LOWER, DaySpec, ENTRIES, PERIODS, RuleEdge, STANDING_RULES, TRANSITIONS, TimeRef,
    UTC_INDEX, ZONES,
};
use crate::{ErrorKind, Timestamp, civil};

const SECS_PER_HOUR: i64 = 3_600;
const SECS_PER_DAY: i64 = 86_400;
/// How far either side of a wall clock's second the periods that may show
/// it are looked for: two days, more than any offset or any change of one.
const WINDOW: i64 = 2 * SECS_PER_DAY;
/// How many hours either side of a gap a time that exists is looked for.
const GAP_REACH_HOURS: i64 = 48;
/// Ten days: more than the searches below reach past any instant or wall
/// clock a value of this crate holds.
const MARGIN: i64 = 10 * SECS_PER_DAY;
/// The first second the standing rules are worked out for.
const FIRST_SECOND: i64 = Timestamp::MIN.as_secs() - MARGIN;
/// The last second the standing rules are worked out for.
const LAST_SECOND: i64 = Timestamp::MAX.as_secs() + MARGIN;

/// A zone or a link of the IANA database, under its own name:
/// `Asia/Kolkata`, the link `Asia/Calcutta`, `Factory`.
///
/// Entries order by exact name. [`Zone`](super::Zone) is the type to use: it
/// holds an entry that is a zone under its current name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Entry(u16);

/// The offset and the abbreviation a zone keeps over a stretch of time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Period(u16);

/// A stretch of time over which a zone keeps one [`Period`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeriodSpan {
    /// Its first second, counted from the epoch; `i64::MIN` for a stretch
    /// with no beginning.
    pub start: i64,
    /// The second after its last; `i64::MAX` for a stretch with no end.
    pub end: i64,
    /// What the zone keeps over it.
    pub period: Period,
}

/// What a zone's clocks make of a time read off them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalPeriods {
    /// They never show it: they went forward over it.
    Gap,
    /// They show it once, in this period.
    Single(Period),
    /// They show it more than once, having gone back over it: in the
    /// earliest of the periods that show it, and in the latest.
    Fold(Period, Period),
}

impl Entry {
    /// `UTC`.
    pub const UTC: Entry = Entry(UTC_INDEX);

    /// The zone or the link called `name`, in any ASCII case. The name is
    /// taken as it is: nothing is trimmed and no legacy name is replaced.
    pub fn find(name: &str) -> Option<Entry> {
        if name.is_empty() {
            return None;
        }
        let at = BY_LOWER
            .binary_search_by(|&entry| cmp_ignoring_ascii_case(ENTRIES[usize::from(entry)].0, name))
            .ok()?;
        Some(Entry(BY_LOWER[at]))
    }

    /// Every zone and link of the compiled release, in the order of their
    /// exact names.
    pub fn all() -> impl ExactSizeIterator<Item = Entry> + DoubleEndedIterator + Clone {
        (0..ENTRIES.len() as u16).map(Entry)
    }

    /// The entry's own name: a link keeps its name, not its target's.
    pub const fn name(self) -> &'static str {
        ENTRIES[self.0 as usize].0
    }

    /// The stretch of time that holds second `secs` from the epoch, and
    /// what the zone keeps over it: `start <= secs < end`.
    pub fn span_at(self, secs: i64) -> PeriodSpan {
        // Past either end of what this crate's values reach, the period in
        // force there goes on without end, so that a year never leaves the
        // calendar's arithmetic.
        if secs < FIRST_SECOND {
            return PeriodSpan {
                start: i64::MIN,
                ..self.span_in_reach(FIRST_SECOND)
            };
        }
        if secs > LAST_SECOND {
            return PeriodSpan {
                end: i64::MAX,
                ..self.span_in_reach(LAST_SECOND)
            };
        }
        self.span_in_reach(secs)
    }

    /// [`span_at`](Entry::span_at) for a second from `FIRST_SECOND` to
    /// `LAST_SECOND`.
    fn span_in_reach(self, secs: i64) -> PeriodSpan {
        let zone = &ZONES[usize::from(ENTRIES[usize::from(self.0)].1)];
        let transitions = &TRANSITIONS[zone.start as usize..zone.end as usize];
        let Some(&(last_at, last_period)) = transitions.last() else {
            return match zone.standing {
                Some(rule) => standing_span(secs, i64::MIN, rule),
                None => PeriodSpan {
                    start: i64::MIN,
                    end: i64::MAX,
                    period: Period(zone.initial),
                },
            };
        };

        if secs < last_at {
            let next = transitions.partition_point(|&(at, _)| at <= secs);
            let (start, period) = if next == 0 {
                (i64::MIN, zone.initial)
            } else {
                transitions[next - 1]
            };
            return PeriodSpan {
                start,
                end: transitions[next].0,
                period: Period(period),
            };
        }

        match zone.standing {
            Some(rule) => standing_span(secs, last_at, rule),
            None => PeriodSpan {
                start: last_at,
                end: i64::MAX,
                period: Period(last_period),
            },
        }
    }

    /// What the zone's clocks make of the wall-clock second `wall`: a time
    /// read off them, counted in seconds as though it were UTC's.
    ///
    /// Every period within two days of the second is tried: a period shows
    /// the time when the time, taken back by the period's offset, falls
    /// inside it.
    pub fn local(self, wall: i64) -> LocalPeriods {
        let until = wall.saturating_add(WINDOW);
        let mut shown = [Period(0); 4];
        let mut count = 0;
        let mut cursor = wall.saturating_sub(WINDOW);

        loop {
            let span = self.span_at(cursor);
            if let Some(at) = wall.checked_sub(i64::from(span.period.offset()))
                && at >= span.start
                && at < span.end
            {
                shown[count] = span.period;
                count += 1;
            }
            if span.end >= until || span.end == i64::MAX || count == shown.len() {
                break;
            }
            cursor = span.end;
        }

        match count {
            0 => LocalPeriods::Gap,
            1 => LocalPeriods::Single(shown[0]),
            _ => LocalPeriods::Fold(shown[0], shown[count - 1]),
        }
    }

    /// The offset, in seconds east of UTC, in force before the gap that
    /// holds the wall-clock second `wall`.
    ///
    /// The wall clock is stepped back an hour at a time, for up to 48
    /// hours, to the first time the zone's clocks show; where they show that
    /// time twice, it is the later showing that the gap follows. A zone
    /// that shows none of them counts as UTC.
    pub fn offset_before_gap(self, wall: i64) -> i32 {
        (1..=GAP_REACH_HOURS)
            .find_map(
                |hours| match self.local(wall.saturating_sub(hours * SECS_PER_HOUR)) {
                    LocalPeriods::Single(period) => Some(period.offset()),
                    LocalPeriods::Fold(_, later) => Some(later.offset()),
                    LocalPeriods::Gap => None,
                },
            )
            .unwrap_or(0)
    }

    /// The offset, in seconds east of UTC, in force after the gap that
    /// holds the wall-clock second `wall`. It is
    /// [`offset_before_gap`](Entry::offset_before_gap) the other way: the
    /// wall clock is stepped forward, and a time shown twice counts by its
    /// earlier showing.
    pub fn offset_after_gap(self, wall: i64) -> i32 {
        (1..=GAP_REACH_HOURS)
            .find_map(
                |hours| match self.local(wall.saturating_add(hours * SECS_PER_HOUR)) {
                    LocalPeriods::Single(period) => Some(period.offset()),
                    LocalPeriods::Fold(earlier, _) => Some(earlier.offset()),
                    LocalPeriods::Gap => None,
                },
            )
            .unwrap_or(0)
    }

    /// The offset, in seconds east of UTC, by which `how` reads the
    /// wall-clock second `wall`: the second less the offset is the instant.
    /// `Err` with [`ErrorKind::Gap`] or [`ErrorKind::Ambiguous`] where `how`
    /// refuses the time.
    pub(crate) fn offset_of_local(self, wall: i64, how: Disambiguation) -> Result<i32, ErrorKind> {
        match self.local(wall) {
            LocalPeriods::Single(period) => Ok(period.offset()),
            LocalPeriods::Fold(earlier, later) => match how {
                Disambiguation::Compatible | Disambiguation::Earlier => Ok(earlier.offset()),
                Disambiguation::Later => Ok(later.offset()),
                Disambiguation::Reject => Err(ErrorKind::Ambiguous),
            },
            // The offset from before the gap puts the time as far after the
            // gap as it was into it; the one from after it, as far before.
            LocalPeriods::Gap => match how {
                Disambiguation::Compatible | Disambiguation::Later => {
                    Ok(self.offset_before_gap(wall))
                }
                Disambiguation::Earlier => Ok(self.offset_after_gap(wall)),
                Disambiguation::Reject => Err(ErrorKind::Gap),
            },
        }
    }
}

impl Period {
    /// The zone's standard offset over the period, in seconds east of UTC.
    pub fn standard_offset(self) -> i32 {
        PERIODS[usize::from(self.0)].std_offset
    }

    /// What daylight saving adds to the standard offset over the period,
    /// in seconds: 0 in standard time, and below zero in a zone whose
    /// winter time is the saving one (`Europe/Dublin`).
    pub fn daylight_saving(self) -> i32 {
        PERIODS[usize::from(self.0)].dst_offset
    }

    /// The offset the zone's clocks keep over the period, in seconds east
    /// of UTC: the standard offset and the saving together.
    pub fn offset(self) -> i32 {
        let period = &PERIODS[usize::from(self.0)];
        period.std_offset + period.dst_offset
    }

    /// The zone's abbreviation over the period (`IST`, `EDT`, `+0530`).
    pub fn abbreviation(self) -> &'static str {
        ABBREVS[usize::from(PERIODS[usize::from(self.0)].abbrev)]
    }
}

/// The order of two names with their ASCII letters taken as lower case:
/// the order `BY_LOWER` and `ALIASES` are sorted in.
pub(super) fn cmp_ignoring_ascii_case(a: &str, b: &str) -> Ordering {
    a.bytes()
        .map(|byte| byte.to_ascii_lowercase())
        .cmp(b.bytes().map(|byte| byte.to_ascii_lowercase()))
}

/// The stretch that holds second `secs` under a standing rule, which is in
/// force from second `not_before` on.
///
/// A standing rule has two edges a year. Those of the year before, of the
/// year itself and of the year after are laid out in order, and the second
/// falls between two of them.
fn standing_span(secs: i64, not_before: i64, rule: u16) -> PeriodSpan {
    let rule = &STANDING_RULES[usize::from(rule)];
    let (year, _, _) = civil::civil_from_days_in_range(secs.div_euclid(SECS_PER_DAY));
    // An edge at a wall-clock time is on the clock the other edge left
    // behind, so it counts with the saving of the other edge's period.
    let saving_before_first = PERIODS[usize::from(rule.second.period)].dst_offset;
    let saving_before_second = PERIODS[usize::from(rule.first.period)].dst_offset;
    let first = |year: i32| {
        (
            edge_second(year, &rule.first, rule.std_offset, saving_before_first),
            rule.first.period,
        )
    };
    let second = |year: i32| {
        (
            edge_second(year, &rule.second, rule.std_offset, saving_before_second),
            rule.second.period,
        )
    };
    let edges: [(i64, u16); 6] = [
        first(year - 1),
        second(year - 1),
        first(year),
        second(year),
        first(year + 1),
        second(year + 1),
    ];

    let next = edges.partition_point(|&(at, _)| at <= secs);
    let (start, period) = if next == 0 {
        (not_before, rule.second.period)
    } else {
        (edges[next - 1].0.max(not_before), edges[next - 1].1)
    };
    let end = if next < edges.len() {
        edges[next].0
    } else {
        i64::MAX
    };
    PeriodSpan {
        start,
        end,
        period: Period(period),
    }
}

/// The second, from the epoch, at which an edge of a standing rule falls in
/// `year`, in a zone whose standard offset is `standard` and whose saving
/// before the edge is `saving`.
fn edge_second(year: i32, edge: &RuleEdge, standard: i32, saving: i32) -> i64 {
    let on_the_clock =
        edge_day(year, edge.month, edge.day) * SECS_PER_DAY + i64::from(edge.at_secs);
    match edge.at_ref {
        TimeRef::Utc => on_the_clock,
        TimeRef::Standard => on_the_clock - i64::from(standard),
        TimeRef::Wall => on_the_clock - i64::from(standard + saving),
    }
}

/// The day count of the day an edge of a standing rule falls on in `month`
/// of `year`. A weekday of the tables counts from Sunday, which is 0.
fn edge_day(year: i32, month: u8, day: DaySpec) -> i64 {
    match day {
        DaySpec::Dom(day) => civil::days_from_civil(year, month, day),
        DaySpec::LastWeekday(weekday) => {
            let last = civil::days_from_civil(year, month, civil::days_in_month(year, month));
            last - i64::from((days_from_sunday(last) + 7 - weekday) % 7)
        }
        DaySpec::OnOrAfter { weekday, dom } => {
            let from = civil::days_from_civil(year, month, dom);
            from + i64::from((weekday + 7 - days_from_sunday(from)) % 7)
        }
        DaySpec::OnOrBefore { weekday, dom } => {
            let from = civil::days_from_civil(year, month, dom);
            from - i64::from((days_from_sunday(from) + 7 - weekday) % 7)
        }
    }
}

/// How many days after Sunday a day count is: Sunday is 0.
fn days_from_sunday(days: i64) -> u8 {
    civil::weekday_from_days(days).days_from_sunday()
}
