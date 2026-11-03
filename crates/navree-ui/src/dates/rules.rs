//! Which days can be chosen, and the check that holds on the server.
//!
//! The same [`DateRules`] draws the calendar and checks every date that comes back
//! in a request, so what the page shows and what the server accepts can't drift.

use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;

use chrono::{Datelike, NaiveDate, Weekday};
use serde_json::{Value, json};

use super::grid::{self, add_months};
use super::locale::DateLocale;
use super::range::DateRange;

/// The most day ranges one calendar sends to the browser. The server still checks
/// every date against the whole of its rules.
pub const MAX_CLIENT_RANGES: usize = 1000;

/// The longest span a rule function is asked about a day at a time.
const RULE_SPAN_CAP: i64 = 40_000;

/// Why a date can't be chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DateError {
    /// Before the earliest date.
    TooEarly(NaiveDate),
    /// After the latest date.
    TooLate(NaiveDate),
    /// On a weekday that is turned off.
    Weekday(Weekday),
    /// On a date or in a range that is turned off, or the page's rule says no.
    Unavailable,
    /// Outside the years the calendar knows (1 to 9999).
    OutOfCalendar,
    /// A range that holds fewer days than the least allowed.
    RangeTooShort(u32),
    /// A range that holds more days than the most allowed.
    RangeTooLong(u32),
    /// A range that holds a day that can't be chosen.
    RangeBlocked,
}

impl DateError {
    /// The message for a person, in the locale's words. A range that is too short, too long
    /// or holds a day that is off says so; every other reason is "That date isn't
    /// available." ([`to_string`](ToString::to_string) says which, in English, for a log.)
    pub fn message(&self, locale: &DateLocale) -> String {
        let labels = &locale.labels;
        match self {
            Self::RangeTooShort(days) => {
                labels.range_too_short.replace("{days}", &days.to_string())
            }
            Self::RangeTooLong(days) => labels.range_too_long.replace("{days}", &days.to_string()),
            Self::RangeBlocked => labels.range_blocked.to_string(),
            _ => labels.unavailable_date.to_string(),
        }
    }
}

impl fmt::Display for DateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooEarly(min) => write!(f, "choose {min} or a later date"),
            Self::TooLate(max) => write!(f, "choose {max} or an earlier date"),
            Self::Weekday(day) => write!(f, "{day} can't be chosen"),
            Self::Unavailable => f.write_str("that date isn't available"),
            Self::OutOfCalendar => f.write_str("that year is outside the calendar"),
            Self::RangeTooShort(days) => write!(f, "choose at least {days} days"),
            Self::RangeTooLong(days) => write!(f, "choose at most {days} days"),
            Self::RangeBlocked => f.write_str("the range holds a day that isn't available"),
        }
    }
}

impl std::error::Error for DateError {}

type Rule = Arc<dyn Fn(NaiveDate) -> bool + Send + Sync>;

/// The limits of a range, written once: the days that can't be chosen, and the fewest and most
/// days a range may hold. Give it to a [`DateRangePicker`](super::DateRangePicker) with
/// [`limits`](super::DateRangePicker::limits) and the browser holds a person to it; give the
/// same value to [`DateRangeQuery::checked`](super::DateRangeQuery::checked) and the server
/// holds the request to it. Nothing is said twice, so the two can't differ.
///
/// ```
/// use navree_ui::dates::{DateRange, DateRangeQuery, DateRangePicker, DateRules, RangeRules};
/// use chrono::{NaiveDate, Weekday};
///
/// let limits = RangeRules::new(DateRules::new().disable_weekdays([Weekday::Sat, Weekday::Sun]))
///     .min_days(2)
///     .max_days(30);
/// let picker = DateRangePicker::new("from", "to").limits(limits.clone());
/// let query = DateRangeQuery { from: Some("2026-11-02".into()), to: Some("2026-11-04".into()) };
/// assert_eq!(query.range_within(&limits), DateRange::parse("2026-11-02", "2026-11-04"));
/// let too_short = DateRangeQuery { from: Some("2026-11-02".into()), to: Some("2026-11-02".into()) };
/// assert_eq!(too_short.range_within(&limits), None);
/// # let _ = picker;
/// ```
#[derive(Clone, Debug, Default)]
pub struct RangeRules {
    rules: DateRules,
    min_days: Option<u32>,
    max_days: Option<u32>,
}

impl RangeRules {
    /// Limits that hold a range to `rules`: both ends and every day between.
    pub fn new(rules: DateRules) -> Self {
        Self {
            rules,
            min_days: None,
            max_days: None,
        }
    }

    /// The fewest days a range may hold (one at least).
    #[must_use]
    pub fn min_days(mut self, days: u32) -> Self {
        self.min_days = Some(days.max(1));
        self
    }

    /// The most days a range may hold (one at least).
    #[must_use]
    pub fn max_days(mut self, days: u32) -> Self {
        self.max_days = Some(days.max(1));
        self
    }

    /// The days that can't be chosen.
    pub fn rules(&self) -> &DateRules {
        &self.rules
    }

    /// The fewest days a range may hold, when limited.
    pub fn least_days(&self) -> Option<u32> {
        self.min_days
    }

    /// The most days a range may hold, when limited.
    pub fn most_days(&self) -> Option<u32> {
        self.max_days
    }

    /// Why `range` can't be chosen, or `Ok`: the check the browser makes as a range is
    /// picked, made again by the server.
    pub fn check(&self, range: &DateRange) -> Result<(), DateError> {
        self.rules.check_range(range, self.min_days, self.max_days)
    }
}

impl From<DateRules> for RangeRules {
    fn from(rules: DateRules) -> Self {
        Self::new(rules)
    }
}

/// The days a calendar will not let a person choose.
///
/// ```
/// use navree_ui::dates::DateRules;
/// use chrono::{NaiveDate, Weekday};
///
/// let day = |d| NaiveDate::from_ymd_opt(2026, 12, d).unwrap();
/// let rules = DateRules::new()
///     .min(day(1))
///     .disable_weekdays([Weekday::Sat, Weekday::Sun])
///     .disable_dates([day(25)]);
/// assert!(rules.check(day(2)).is_ok());    // a Wednesday
/// assert!(rules.check(day(5)).is_err());   // a Saturday
/// assert!(rules.check(day(25)).is_err());  // listed
/// ```
#[derive(Clone, Default)]
pub struct DateRules {
    min: Option<NaiveDate>,
    max: Option<NaiveDate>,
    weekdays: u8,
    dates: BTreeSet<NaiveDate>,
    ranges: Vec<(NaiveDate, NaiveDate)>,
    rule: Option<Rule>,
}

impl fmt::Debug for DateRules {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DateRules")
            .field("min", &self.min)
            .field("max", &self.max)
            .field("weekdays", &self.weekdays)
            .field("dates", &self.dates.len())
            .field("ranges", &self.ranges.len())
            .field("rule", &self.rule.is_some())
            .finish()
    }
}

impl DateRules {
    /// Rules that allow every day.
    pub fn new() -> Self {
        Self::default()
    }

    /// The earliest day that can be chosen.
    #[must_use]
    pub fn min(mut self, day: NaiveDate) -> Self {
        self.min = Some(day);
        self
    }

    /// The latest day that can be chosen.
    #[must_use]
    pub fn max(mut self, day: NaiveDate) -> Self {
        self.max = Some(day);
        self
    }

    /// Turns weekdays off.
    #[must_use]
    pub fn disable_weekdays(mut self, days: impl IntoIterator<Item = Weekday>) -> Self {
        for day in days {
            self.weekdays |= 1 << day.num_days_from_sunday();
        }
        self
    }

    /// Turns single days off.
    #[must_use]
    pub fn disable_dates(mut self, days: impl IntoIterator<Item = NaiveDate>) -> Self {
        self.dates.extend(days);
        self
    }

    /// Turns a run of days off, both ends included. Given backwards, it is put right.
    #[must_use]
    pub fn disable_range(mut self, from: NaiveDate, to: NaiveDate) -> Self {
        self.ranges.push((from.min(to), from.max(to)));
        self
    }

    /// Turns off every day the function answers `true` for. The calendar asks it
    /// for the days it can show (three years around the month it opens on) and
    /// stops navigation at the edges of that window; the server asks it for each
    /// date that comes back.
    #[must_use]
    pub fn disable_if(mut self, rule: impl Fn(NaiveDate) -> bool + Send + Sync + 'static) -> Self {
        self.rule = Some(Arc::new(rule));
        self
    }

    /// The earliest day, when set.
    pub fn earliest(&self) -> Option<NaiveDate> {
        self.min
    }

    /// The latest day, when set.
    pub fn latest(&self) -> Option<NaiveDate> {
        self.max
    }

    /// Whether a rule function is set.
    pub fn has_rule(&self) -> bool {
        self.rule.is_some()
    }

    /// Why `day` can't be chosen, or `Ok` when it can.
    pub fn check(&self, day: NaiveDate) -> Result<(), DateError> {
        if day < grid::earliest() || day > grid::latest() {
            return Err(DateError::OutOfCalendar);
        }
        if let Some(min) = self.min
            && day < min
        {
            return Err(DateError::TooEarly(min));
        }
        if let Some(max) = self.max
            && day > max
        {
            return Err(DateError::TooLate(max));
        }
        if self.weekdays & (1 << day.weekday().num_days_from_sunday()) != 0 {
            return Err(DateError::Weekday(day.weekday()));
        }
        if self.dates.contains(&day)
            || self
                .ranges
                .iter()
                .any(|(from, to)| *from <= day && day <= *to)
            || self.rule.as_ref().is_some_and(|rule| rule(day))
        {
            return Err(DateError::Unavailable);
        }
        Ok(())
    }

    /// Whether `day` can be chosen.
    pub fn allows(&self, day: NaiveDate) -> bool {
        self.check(day).is_ok()
    }

    /// Why `range` can't be chosen: an end that is off, a day between that is off, or a
    /// length outside `min_days` and `max_days`. The same check the browser makes as a
    /// range is picked.
    ///
    /// ```
    /// use navree_ui::dates::{DateError, DateRange, DateRules};
    /// use chrono::NaiveDate;
    ///
    /// let day = |d| NaiveDate::from_ymd_opt(2026, 11, d).unwrap();
    /// let rules = DateRules::new().disable_dates([day(5)]);
    /// let range = DateRange::new(day(3), day(7));
    /// assert_eq!(rules.check_range(&range, None, None), Err(DateError::RangeBlocked));
    /// assert_eq!(rules.check_range(&DateRange::new(day(6), day(9)), Some(5), None), Err(DateError::RangeTooShort(5)));
    /// ```
    pub fn check_range(
        &self,
        range: &DateRange,
        min_days: Option<u32>,
        max_days: Option<u32>,
    ) -> Result<(), DateError> {
        self.check(range.from)?;
        self.check(range.to)?;
        let days = range.day_count();
        if let Some(least) = min_days
            && days < i64::from(least)
        {
            return Err(DateError::RangeTooShort(least));
        }
        if let Some(most) = max_days
            && days > i64::from(most)
        {
            return Err(DateError::RangeTooLong(most));
        }
        if self.blocks_span(range.from, range.to) {
            return Err(DateError::RangeBlocked);
        }
        Ok(())
    }

    /// Whether any day from `from` to `to` can't be chosen. The limits, the weekdays, the
    /// listed days and the listed runs are asked as ranges, so the cost is that of the rules
    /// and not of the days in the span. Only a rule function must be asked a day at a time,
    /// and for that a span of more than 40,000 days (about 110 years, more than any calendar
    /// the browser can show is given) counts as blocked.
    pub fn blocks_span(&self, from: NaiveDate, to: NaiveDate) -> bool {
        let (from, to) = (from.min(to), from.max(to));
        if from < grid::earliest() || to > grid::latest() {
            return true;
        }
        if self.min.is_some_and(|min| from < min) || self.max.is_some_and(|max| to > max) {
            return true;
        }
        if self.weekdays != 0 {
            // Seven days hold every weekday, so no more than seven need be looked at.
            let mut day = from;
            for _ in 0..=6 {
                if day > to {
                    break;
                }
                if self.weekdays & (1 << day.weekday().num_days_from_sunday()) != 0 {
                    return true;
                }
                let Some(next) = day.succ_opt() else { break };
                day = next;
            }
        }
        if self.dates.range(from..=to).next().is_some()
            || self
                .ranges
                .iter()
                .any(|(first, last)| *first <= to && from <= *last)
        {
            return true;
        }
        let Some(rule) = &self.rule else {
            return false;
        };
        if (to - from).num_days() > RULE_SPAN_CAP {
            return true;
        }
        let mut day = from;
        while day <= to {
            if rule(day) {
                return true;
            }
            let Some(next) = day.succ_opt() else { break };
            day = next;
        }
        false
    }

    /// What the browser needs: limits, weekdays, and the listed and ruled-out days as
    /// runs, for `window` (the days the calendar can reach). Navigation stops at the
    /// months in `nav` when a rule function limits it.
    pub(crate) fn client(&self, window: (NaiveDate, NaiveDate), limit_nav: bool) -> Value {
        let (start, end) = window;
        let mut runs: Vec<(NaiveDate, NaiveDate)> = Vec::new();
        let mut push = |from: NaiveDate, to: NaiveDate| {
            let (from, to) = (from.max(start), to.min(end));
            if from <= to {
                runs.push((from, to));
            }
        };
        // A window turned inside out (a least day after its end) holds no day, and a range
        // of a set with its ends the wrong way round is a panic.
        if start <= end {
            for day in self.dates.range(start..=end) {
                push(*day, *day);
            }
        }
        for (from, to) in &self.ranges {
            push(*from, *to);
        }
        if let Some(rule) = &self.rule {
            let mut run: Option<(NaiveDate, NaiveDate)> = None;
            let mut day = start;
            while day <= end {
                if rule(day) {
                    run = match run {
                        Some((from, _)) => Some((from, day)),
                        None => Some((day, day)),
                    };
                } else if let Some((from, to)) = run.take() {
                    push(from, to);
                }
                let Some(next) = day.succ_opt() else { break };
                day = next;
            }
            if let Some((from, to)) = run {
                push(from, to);
            }
        }
        runs.sort();
        let mut merged: Vec<(NaiveDate, NaiveDate)> = Vec::with_capacity(runs.len());
        for (from, to) in runs {
            let joins = merged
                .last()
                .is_some_and(|(_, last)| from <= last.succ_opt().unwrap_or(*last));
            match merged.last_mut() {
                Some(last) if joins => last.1 = last.1.max(to),
                _ => merged.push((from, to)),
            }
        }
        merged.truncate(MAX_CLIENT_RANGES);
        let weekdays: Vec<u32> = (0..7)
            .filter(|day| self.weekdays & (1 << day) != 0)
            .collect();
        let mut out = json!({});
        if let Some(min) = self.min {
            out["min"] = json!(min.to_string());
        }
        if let Some(max) = self.max {
            out["max"] = json!(max.to_string());
        }
        if !weekdays.is_empty() {
            out["wd"] = json!(weekdays);
        }
        if !merged.is_empty() {
            out["r"] = json!(
                merged
                    .iter()
                    .map(|(from, to)| json!([from.to_string(), to.to_string()]))
                    .collect::<Vec<_>>()
            );
        }
        if limit_nav {
            out["nav"] = json!([
                format!("{:04}-{:02}", start.year(), start.month()),
                format!("{:04}-{:02}", end.year(), end.month()),
            ]);
        }
        out
    }

    /// The days the browser can be told about, around the month `year`-`month`: the
    /// whole span between the limits when both are set and it is at most three years,
    /// else 18 months either side, held inside the limits.
    pub(crate) fn window(&self, year: i32, month: u32) -> (NaiveDate, NaiveDate) {
        let first_of = |delta: i32| {
            let (y, m) = add_months(year, month, delta).unwrap_or((year, month));
            NaiveDate::from_ymd_opt(y, m, 1).unwrap_or_else(grid::earliest)
        };
        let last_of = |delta: i32| {
            let (y, m) = add_months(year, month, delta).unwrap_or((year, month));
            NaiveDate::from_ymd_opt(y, m, grid::days_in_month(y, m)).unwrap_or_else(grid::latest)
        };
        let mut start = first_of(-18);
        let mut end = last_of(18);
        if let (Some(min), Some(max)) = (self.min, self.max)
            && (max - min).num_days() <= 3 * 366
        {
            start = min;
            end = max;
        }
        if let Some(min) = self.min {
            start = start.max(min);
        }
        if let Some(max) = self.max {
            end = end.min(max);
        }
        (start.max(grid::earliest()), end.min(grid::latest()))
    }
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use super::*;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn every_kind_of_rule_blocks() {
        let rules = DateRules::new()
            .min(day(2026, 1, 10))
            .max(day(2026, 12, 18))
            .disable_weekdays([Weekday::Sun])
            .disable_dates([day(2026, 6, 3)])
            .disable_range(day(2026, 8, 20), day(2026, 8, 10))
            .disable_if(|d| d.day() == 31);
        assert_eq!(
            rules.check(day(2026, 1, 9)),
            Err(DateError::TooEarly(day(2026, 1, 10)))
        );
        assert_eq!(
            rules.check(day(2026, 12, 19)),
            Err(DateError::TooLate(day(2026, 12, 18)))
        );
        assert_eq!(
            rules.check(day(2026, 11, 1)),
            Err(DateError::Weekday(Weekday::Sun))
        );
        assert_eq!(rules.check(day(2026, 6, 3)), Err(DateError::Unavailable));
        assert_eq!(rules.check(day(2026, 8, 15)), Err(DateError::Unavailable));
        assert_eq!(rules.check(day(2026, 8, 10)), Err(DateError::Unavailable));
        assert_eq!(rules.check(day(2026, 8, 20)), Err(DateError::Unavailable));
        assert_eq!(rules.check(day(2026, 8, 21)), Ok(()));
        assert_eq!(rules.check(day(2026, 10, 31)), Err(DateError::Unavailable));
        assert_eq!(rules.check(day(2026, 1, 10)), Ok(()));
        assert_eq!(rules.check(day(2026, 12, 18)), Ok(()));
    }

    #[test]
    fn a_range_is_held_to_the_rules_and_its_length() {
        let rules = DateRules::new()
            .min(day(2026, 11, 1))
            .max(day(2026, 11, 30))
            .disable_dates([day(2026, 11, 11)]);
        let range = |a, b| DateRange::new(day(2026, 11, a), day(2026, 11, b));
        assert_eq!(rules.check_range(&range(2, 6), Some(2), Some(7)), Ok(()));
        assert_eq!(
            rules.check_range(&range(9, 13), None, None),
            Err(DateError::RangeBlocked)
        );
        assert_eq!(
            rules.check_range(&range(11, 12), None, None),
            Err(DateError::Unavailable)
        );
        assert_eq!(
            rules.check_range(&range(2, 3), Some(3), None),
            Err(DateError::RangeTooShort(3))
        );
        assert_eq!(
            rules.check_range(&range(2, 9), None, Some(5)),
            Err(DateError::RangeTooLong(5))
        );
        assert!(matches!(
            rules.check_range(
                &DateRange::new(day(2026, 10, 30), day(2026, 11, 3)),
                None,
                None
            ),
            Err(DateError::TooEarly(_))
        ));
        // A one-day range is as long as one day.
        assert_eq!(rules.check_range(&range(4, 4), Some(1), Some(1)), Ok(()));
    }

    #[test]
    fn the_edges_are_included() {
        let rules = DateRules::new().min(day(2026, 5, 1)).max(day(2026, 5, 31));
        assert!(rules.allows(day(2026, 5, 1)));
        assert!(rules.allows(day(2026, 5, 31)));
        assert!(!rules.allows(day(2026, 4, 30)));
        assert!(!rules.allows(day(2026, 6, 1)));
    }

    #[test]
    fn days_outside_the_calendar_are_refused() {
        let rules = DateRules::new();
        assert_eq!(rules.check(day(0, 12, 31)), Err(DateError::OutOfCalendar));
        assert_eq!(
            rules.check(day(10_000, 1, 1)),
            Err(DateError::OutOfCalendar)
        );
        assert!(rules.allows(day(1, 1, 1)));
        assert!(rules.allows(day(9999, 12, 31)));
    }

    #[test]
    fn a_span_with_a_blocked_day_is_blocked() {
        let rules = DateRules::new().disable_dates([day(2026, 3, 5)]);
        assert!(!rules.blocks_span(day(2026, 3, 1), day(2026, 3, 4)));
        assert!(rules.blocks_span(day(2026, 3, 1), day(2026, 3, 9)));
        assert!(rules.blocks_span(day(2026, 3, 9), day(2026, 3, 1)));
        assert!(rules.blocks_span(day(1, 1, 1), day(9999, 12, 31)));
    }

    #[test]
    fn the_browser_gets_merged_runs() {
        let rules = DateRules::new()
            .disable_weekdays([Weekday::Sat, Weekday::Sun])
            .disable_dates([
                day(2026, 12, 1),
                day(2026, 12, 2),
                day(2026, 12, 3),
                day(2026, 12, 9),
            ])
            .disable_range(day(2026, 12, 3), day(2026, 12, 5))
            .min(day(2026, 11, 1));
        let json = rules.client((day(2026, 11, 1), day(2027, 1, 31)), false);
        assert_eq!(json["min"], "2026-11-01");
        assert_eq!(json["wd"], json!([0, 6]));
        assert_eq!(
            json["r"],
            json!([["2026-12-01", "2026-12-05"], ["2026-12-09", "2026-12-09"]])
        );
        assert!(json.get("nav").is_none());
    }

    #[test]
    fn a_rule_function_becomes_runs_and_limits_navigation() {
        let rules = DateRules::new().disable_if(|d| d.month() == 12 && d.day() >= 24);
        let json = rules.client((day(2026, 11, 1), day(2027, 1, 31)), true);
        assert_eq!(json["r"], json!([["2026-12-24", "2026-12-31"]]));
        assert_eq!(json["nav"], json!(["2026-11", "2027-01"]));
        // The server still asks the function itself.
        assert!(!rules.allows(day(2030, 12, 25)));
    }

    #[test]
    fn the_runs_are_capped() {
        let rules = DateRules::new()
            .disable_dates((0..3000).map(|n| day(2026, 1, 1) + Duration::days(n * 2)));
        let json = rules.client((day(2026, 1, 1), day(2035, 1, 1)), false);
        assert_eq!(json["r"].as_array().unwrap().len(), MAX_CLIENT_RANGES);
        assert!(!rules.allows(day(2026, 1, 1) + Duration::days(2 * 2999)));
    }

    #[test]
    fn the_window_follows_the_limits() {
        let open = DateRules::new();
        let (start, end) = open.window(2026, 11);
        assert_eq!(start, day(2025, 5, 1));
        assert_eq!(end, day(2028, 5, 31));
        let limited = DateRules::new()
            .min(day(2026, 10, 15))
            .max(day(2027, 2, 10));
        assert_eq!(
            limited.window(2026, 11),
            (day(2026, 10, 15), day(2027, 2, 10))
        );
        let early = DateRules::new().min(day(2026, 11, 20));
        assert_eq!(early.window(2026, 11).0, day(2026, 11, 20));
        // The edges of the calendar clamp rather than overflow.
        let (start, _) = open.window(1, 1);
        assert_eq!(start, day(1, 1, 1));
        let (_, end) = open.window(9999, 12);
        assert_eq!(end, day(9999, 12, 31));
    }
}
