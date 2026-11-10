//! A window of whole days: the value of a range picker, the presets that fill it,
//! and what a query needs from it.

use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveTime, TimeZone, Utc, Weekday};
use serde::Deserialize;

use super::grid::{self, add_months, days_in_month};
use super::locale::DateLocale;
use super::rules::{DateError, RangeRules};
use super::zoned::{Resolved, resolve};

/// From one day to another, both included. `from` is never after `to`.
///
/// A range is whole days. To ask a database for it, turn it into instants with
/// [`utc_bounds`](Self::utc_bounds) and write `>= start AND < end`: that counts the
/// last day whole, and the 23-hour and 25-hour days of a clock change too.
///
/// ```
/// use navree_ui::dates::DateRange;
/// use chrono::NaiveDate;
///
/// let day = |d| NaiveDate::from_ymd_opt(2026, 11, d).unwrap();
/// let range = DateRange::new(day(3), day(1));
/// assert_eq!((range.from, range.to), (day(1), day(3)));
/// assert_eq!(range.day_count(), 3);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DateRange {
    /// The first day.
    pub from: NaiveDate,
    /// The last day.
    pub to: NaiveDate,
}

impl DateRange {
    /// A range between two days, given in either order.
    pub fn new(a: NaiveDate, b: NaiveDate) -> Self {
        Self {
            from: a.min(b),
            to: a.max(b),
        }
    }

    /// A range of one day.
    pub fn single(day: NaiveDate) -> Self {
        Self::new(day, day)
    }

    /// How many days it holds.
    pub fn day_count(&self) -> i64 {
        (self.to - self.from).num_days() + 1
    }

    /// Whether `day` is in it.
    pub fn contains(&self, day: NaiveDate) -> bool {
        self.from <= day && day <= self.to
    }

    /// Reads two `YYYY-MM-DD` texts. One that doesn't parse gives `None`; a pair in
    /// the wrong order is put right.
    pub fn parse(from: &str, to: &str) -> Option<Self> {
        let from = read_day(from)?;
        let to = read_day(to)?;
        Some(Self::new(from, to))
    }

    /// The instants the range spans in a zone, as `[start, end)`: the first moment
    /// of `from` and the first moment of the day after `to`. A day whose midnight
    /// the zone skipped starts when the clocks jump.
    ///
    /// ```
    /// use navree_ui::dates::DateRange;
    /// use chrono::{NaiveDate, Utc};
    ///
    /// let day = |d| NaiveDate::from_ymd_opt(2026, 11, d).unwrap();
    /// let (start, end) = DateRange::new(day(1), day(3)).utc_bounds(&Utc);
    /// assert_eq!((end - start).num_hours(), 72);
    /// ```
    pub fn utc_bounds<Tz: TimeZone>(&self, zone: &Tz) -> (DateTime<Utc>, DateTime<Utc>) {
        let start = start_of_day(self.from, zone);
        // The last day chrono holds has no day after it: the range runs to the end of time.
        let end = self
            .to
            .succ_opt()
            .map_or(DateTime::<Utc>::MAX_UTC, |after| start_of_day(after, zone));
        (start, end)
    }

    /// Every day in the range, in order.
    pub fn days(&self) -> impl Iterator<Item = NaiveDate> + use<> {
        let to = self.to;
        std::iter::successors(Some(self.from), move |day| {
            day.succ_opt().filter(|next| *next <= to)
        })
    }
}

/// A day as a request writes it: `YYYY-MM-DD`, and in the years a calendar draws, 1 to
/// 9999. Another year is no day here, though chrono could hold it.
fn read_day(text: &str) -> Option<NaiveDate> {
    let day: NaiveDate = text.trim().parse().ok()?;
    (grid::earliest()..=grid::latest())
        .contains(&day)
        .then_some(day)
}

/// Reads the dates a calendar of several days posts: ISO dates joined by commas. More
/// than `max` entries, or an entry that isn't a day, gives `None`; the same day twice
/// counts once, and the days come back in order.
///
/// ```
/// use navree_ui::dates::parse_list;
/// use chrono::NaiveDate;
///
/// let day = |d| NaiveDate::from_ymd_opt(2026, 11, d).unwrap();
/// assert_eq!(parse_list("2026-11-12,2026-11-03,2026-11-03", 10), Some(vec![day(3), day(12)]));
/// assert_eq!(parse_list("2026-11-12,soon", 10), None);
/// assert_eq!(parse_list("", 10), Some(vec![]));
/// ```
pub fn parse_list(text: &str, max: usize) -> Option<Vec<NaiveDate>> {
    let text = text.trim();
    if text.is_empty() {
        return Some(Vec::new());
    }
    // No list of days is this long: refuse before reading any of it.
    if text.len() > 64 * 1024 {
        return None;
    }
    let mut days = std::collections::BTreeSet::new();
    for part in text.split(',') {
        days.insert(read_day(part)?);
        if days.len() > max {
            return None;
        }
    }
    Some(days.into_iter().collect())
}

/// The first instant of a day on a zone's clocks.
fn start_of_day<Tz: TimeZone>(day: NaiveDate, zone: &Tz) -> DateTime<Utc> {
    match resolve(day, NaiveTime::MIN, zone) {
        Resolved::Single(at) => at,
        Resolved::Ambiguous { first, .. } => first,
        Resolved::Gap { transition, .. } => transition,
    }
}

/// A range as it arrives in a query string, before it is checked. Axum-style
/// extractors fill it from `?from=2026-11-01&to=2026-11-03`.
///
/// ```
/// use navree_ui::dates::DateRangeQuery;
///
/// let query: DateRangeQuery = serde_json::from_str(r#"{"from":"2026-11-03","to":"2026-11-01"}"#).unwrap();
/// let range = query.range().unwrap();
/// assert!(range.from < range.to); // put right
/// let bad: DateRangeQuery = serde_json::from_str(r#"{"from":"soon"}"#).unwrap();
/// assert!(bad.range().is_none());
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DateRangeQuery {
    /// The first day.
    pub from: Option<String>,
    /// The last day.
    pub to: Option<String>,
}

/// A key that comes twice (`?from=a&from=b`) is not an error: **the first one stands**, and
/// any other key is ignored. A request can't make the extractor fail by repeating itself.
/// Note that an extractor that reads the query into a map (`Query<HashMap<String, String>>`)
/// keeps **the last** value of a repeated key, so a page that reads the same address both
/// ways would see two windows: read it one way.
impl<'de> Deserialize<'de> for DateRangeQuery {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Reader;
        impl<'de> serde::de::Visitor<'de> for Reader {
            type Value = DateRangeQuery;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a query with `from` and `to`")
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<DateRangeQuery, A::Error> {
                let mut out = DateRangeQuery::default();
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "from" => {
                            let value = map.next_value::<Option<String>>()?;
                            out.from = out.from.or(value);
                        }
                        "to" => {
                            let value = map.next_value::<Option<String>>()?;
                            out.to = out.to.or(value);
                        }
                        _ => {
                            map.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(out)
            }
        }
        deserializer.deserialize_map(Reader)
    }
}

impl DateRangeQuery {
    /// The range, when both ends are dates. A pair in the wrong order is put right;
    /// anything else is no range, never an error.
    pub fn range(&self) -> Option<DateRange> {
        DateRange::parse(self.from.as_deref()?, self.to.as_deref()?)
    }

    /// The range held to `limits`: `Ok(None)` when there is no range (an end is missing or
    /// not a date), `Ok(Some(range))` when there is one the limits allow, and the reason
    /// in `Err` when there is one they refuse. The same [`RangeRules`] the picker was
    /// given, so what the browser held a person to is what the server holds the request to.
    pub fn checked(&self, limits: &RangeRules) -> Result<Option<DateRange>, DateError> {
        match self.range() {
            None => Ok(None),
            Some(range) => limits.check(&range).map(|()| Some(range)),
        }
    }

    /// The range, when it is one the limits allow; no range when it is not (a page that
    /// would rather show its default window than an error).
    pub fn range_within(&self, limits: &RangeRules) -> Option<DateRange> {
        self.checked(limits).ok().flatten()
    }
}

/// A range the picker offers by name, counted from a day the app names (its idea of
/// today, in the zone that matters). *This week*, *this month*, *this quarter* and
/// *this year* run up to today; the *last* ones are whole periods; *last 7 days*
/// and the like end today and count it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DatePreset {
    /// Today only.
    Today,
    /// Yesterday only.
    Yesterday,
    /// The start of this week to today.
    ThisWeek,
    /// The whole of last week.
    LastWeek,
    /// The last 7 days, today included.
    Last7Days,
    /// The last 14 days, today included.
    Last14Days,
    /// The last 30 days, today included.
    Last30Days,
    /// The first of this month to today.
    ThisMonth,
    /// The whole of last month.
    LastMonth,
    /// The start of this quarter to today.
    ThisQuarter,
    /// The whole of last quarter.
    LastQuarter,
    /// 1 January to today.
    ThisYear,
    /// The whole of last year.
    LastYear,
    /// From the picker's earliest day to today. Needs a `min`.
    AllTime,
}

impl DatePreset {
    /// The presets in the order a list shows them.
    pub const ALL: [Self; 14] = [
        Self::Today,
        Self::Yesterday,
        Self::ThisWeek,
        Self::LastWeek,
        Self::Last7Days,
        Self::Last14Days,
        Self::Last30Days,
        Self::ThisMonth,
        Self::LastMonth,
        Self::ThisQuarter,
        Self::LastQuarter,
        Self::ThisYear,
        Self::LastYear,
        Self::AllTime,
    ];

    /// Where the preset stands in [`DatePreset::ALL`].
    pub(super) const fn index(self) -> usize {
        match self {
            Self::Today => 0,
            Self::Yesterday => 1,
            Self::ThisWeek => 2,
            Self::LastWeek => 3,
            Self::Last7Days => 4,
            Self::Last14Days => 5,
            Self::Last30Days => 6,
            Self::ThisMonth => 7,
            Self::LastMonth => 8,
            Self::ThisQuarter => 9,
            Self::LastQuarter => 10,
            Self::ThisYear => 11,
            Self::LastYear => 12,
            Self::AllTime => 13,
        }
    }

    /// A stable name for the preset.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Today => "today",
            Self::Yesterday => "yesterday",
            Self::ThisWeek => "this_week",
            Self::LastWeek => "last_week",
            Self::Last7Days => "last_7_days",
            Self::Last14Days => "last_14_days",
            Self::Last30Days => "last_30_days",
            Self::ThisMonth => "this_month",
            Self::LastMonth => "last_month",
            Self::ThisQuarter => "this_quarter",
            Self::LastQuarter => "last_quarter",
            Self::ThisYear => "this_year",
            Self::LastYear => "last_year",
            Self::AllTime => "all_time",
        }
    }

    /// The preset with this key.
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|preset| preset.key() == key)
    }

    /// The words on its button, in English. The picker writes them in its locale's
    /// words ([`DateLabels::preset`](super::DateLabels::preset)); a page sets its own with
    /// [`DateRangePicker::preset_label`](super::DateRangePicker::preset_label).
    pub const fn label(self) -> &'static str {
        match self {
            Self::Today => "Today",
            Self::Yesterday => "Yesterday",
            Self::ThisWeek => "This week",
            Self::LastWeek => "Last week",
            Self::Last7Days => "Last 7 days",
            Self::Last14Days => "Last 14 days",
            Self::Last30Days => "Last 30 days",
            Self::ThisMonth => "This month",
            Self::LastMonth => "Last month",
            Self::ThisQuarter => "This quarter",
            Self::LastQuarter => "Last quarter",
            Self::ThisYear => "This year",
            Self::LastYear => "Last year",
            Self::AllTime => "All time",
        }
    }

    /// The range, counted from `today`. `week_start` is the first day of the week;
    /// `earliest` is where *all time* begins (`None` makes *all time* no range).
    pub fn range(
        self,
        today: NaiveDate,
        week_start: Weekday,
        earliest: Option<NaiveDate>,
    ) -> Option<DateRange> {
        self.counted(today, week_start, earliest)
            .filter(|range| range.from >= grid::earliest() && range.to <= grid::latest())
    }

    fn counted(
        self,
        today: NaiveDate,
        week_start: Weekday,
        earliest: Option<NaiveDate>,
    ) -> Option<DateRange> {
        // Every step back is checked: a `today` at the edge of what chrono holds has no day
        // a week before it, and then there is no range, not a panic.
        let back = |day: NaiveDate, n: i64| day.checked_sub_signed(Duration::days(n));
        let days_ago = |n: i64| back(today, n);
        let week_begin = |day: NaiveDate| {
            let since =
                (day.weekday().num_days_from_sunday() + 7 - week_start.num_days_from_sunday()) % 7;
            back(day, i64::from(since))
        };
        let month_range = |year: i32, month: u32, months: u32| -> Option<DateRange> {
            let from = NaiveDate::from_ymd_opt(year, month, 1)?;
            let (last_year, last_month) = add_months(year, month, i32::try_from(months).ok()? - 1)?;
            let to = NaiveDate::from_ymd_opt(
                last_year,
                last_month,
                days_in_month(last_year, last_month),
            )?;
            Some(DateRange::new(from, to))
        };
        let quarter_first = |year: i32, month: u32| (year, (month - 1) / 3 * 3 + 1);
        Some(match self {
            Self::Today => DateRange::single(today),
            Self::Yesterday => DateRange::single(days_ago(1)?),
            Self::ThisWeek => DateRange::new(week_begin(today)?, today),
            Self::LastWeek => {
                let this = week_begin(today)?;
                DateRange::new(back(this, 7)?, back(this, 1)?)
            }
            Self::Last7Days => DateRange::new(days_ago(6)?, today),
            Self::Last14Days => DateRange::new(days_ago(13)?, today),
            Self::Last30Days => DateRange::new(days_ago(29)?, today),
            Self::ThisMonth => DateRange::new(today.with_day(1)?, today),
            Self::LastMonth => {
                let (year, month) = add_months(today.year(), today.month(), -1)?;
                month_range(year, month, 1)?
            }
            Self::ThisQuarter => {
                let (year, month) = quarter_first(today.year(), today.month());
                DateRange::new(NaiveDate::from_ymd_opt(year, month, 1)?, today)
            }
            Self::LastQuarter => {
                let (year, month) = quarter_first(today.year(), today.month());
                let (year, month) = add_months(year, month, -3)?;
                month_range(year, month, 3)?
            }
            Self::ThisYear => DateRange::new(NaiveDate::from_ymd_opt(today.year(), 1, 1)?, today),
            Self::LastYear => DateRange::new(
                NaiveDate::from_ymd_opt(today.year() - 1, 1, 1)?,
                NaiveDate::from_ymd_opt(today.year() - 1, 12, 31)?,
            ),
            Self::AllTime => DateRange::new(earliest?, today),
        })
    }
}

impl DateLocale {
    /// A range as a chip reads it: `Nov 1 – 3`, `Nov 28 – Dec 3`, `Dec 28, 2025 –
    /// Jan 4, 2026`. The year is left out when it is the year of `today` and the
    /// range stays inside it. One day reads as that day alone.
    ///
    /// ```
    /// use navree_ui::dates::{DateLocale, DateRange};
    /// use chrono::NaiveDate;
    ///
    /// let day = |m, d| NaiveDate::from_ymd_opt(2026, m, d).unwrap();
    /// let en = DateLocale::of("en");
    /// let today = day(11, 3);
    /// assert_eq!(en.format_range(&DateRange::new(day(11, 1), day(11, 3)), today), "Nov 1 – 3");
    /// assert_eq!(en.format_range(&DateRange::new(day(10, 28), day(11, 3)), today), "Oct 28 – Nov 3");
    /// ```
    pub fn format_range(&self, range: &DateRange, today: NaiveDate) -> String {
        let same_year = range.from.year() == range.to.year();
        let with_year = !same_year || range.from.year() != today.year();
        if range.from == range.to {
            return self.format_short(range.from, with_year);
        }
        if range.from.year() == range.to.year() && range.from.month() == range.to.month() {
            let (first, last) = (range.from.day(), range.to.day());
            let month = self.month_short(range.from.month());
            let year = range.from.year();
            match (
                self.short_pattern.as_ref(),
                self.short_year_pattern.as_ref(),
                with_year,
            ) {
                ("{M} {d}", "{M} {d}, {y}", false) => return format!("{month} {first} – {last}"),
                ("{M} {d}", "{M} {d}, {y}", true) => {
                    return format!("{month} {first} – {last}, {year}");
                }
                ("{d} {M}", "{d} {M} {y}", false) => return format!("{first} – {last} {month}"),
                ("{d} {M}", "{d} {M} {y}", true) => {
                    return format!("{first} – {last} {month} {year}");
                }
                _ => {}
            }
        }
        format!(
            "{} – {}",
            self.format_short(range.from, with_year),
            self.format_short(range.to, with_year)
        )
    }
}

#[cfg(test)]
mod tests {
    use chrono_tz::Tz;

    use super::*;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn a_preset_knows_its_place_in_the_list() {
        for (at, preset) in DatePreset::ALL.into_iter().enumerate() {
            assert_eq!(preset.index(), at, "{preset:?}");
        }
    }

    #[test]
    fn a_list_of_days_is_read_whole_or_not_at_all() {
        let day = |d| day(2026, 11, d);
        assert_eq!(
            parse_list(" 2026-11-12 , 2026-11-03 ", 5),
            Some(vec![day(3), day(12)])
        );
        assert_eq!(parse_list("2026-11-03,2026-11-03", 1), Some(vec![day(3)]));
        assert_eq!(parse_list("2026-11-03,2026-11-04", 1), None, "over the cap");
        assert_eq!(parse_list("2026-11-03,", 5), None, "an empty entry");
        assert_eq!(parse_list("2026-02-30", 5), None);
        assert_eq!(
            parse_list("2026-11-3", 5),
            Some(vec![day(3)]),
            "a day without its zero"
        );
        // A hostile list is refused before it is read: no work, no allocation of any size.
        let huge = vec!["2026-11-03"; 100_000].join(",");
        assert_eq!(parse_list(&huge, 366), None);
        let many: Vec<String> = (0..100_000)
            .map(|n| (day(1) + chrono::Duration::days(n)).to_string())
            .collect();
        assert_eq!(parse_list(&many.join(","), 366), None);
    }

    #[test]
    fn a_range_is_put_in_order() {
        let range = DateRange::new(day(2026, 11, 3), day(2026, 11, 1));
        assert_eq!(range.from, day(2026, 11, 1));
        assert_eq!(range.to, day(2026, 11, 3));
        assert_eq!(range.day_count(), 3);
        assert_eq!(DateRange::single(day(2026, 2, 28)).day_count(), 1);
        assert!(range.contains(day(2026, 11, 3)));
        assert!(!range.contains(day(2026, 11, 4)));
        assert_eq!(range.days().count(), 3);
        assert_eq!(DateRange::single(day(9999, 12, 31)).days().count(), 1);
    }

    #[test]
    fn query_values_are_dropped_one_at_a_time() {
        assert_eq!(
            DateRange::parse("2026-11-03", "2026-11-01"),
            Some(DateRange::new(day(2026, 11, 1), day(2026, 11, 3)))
        );
        assert_eq!(DateRange::parse("2026-11-03", "nope"), None);
        assert_eq!(DateRange::parse("2026-02-30", "2026-03-01"), None);
        assert_eq!(DateRange::parse("", ""), None);
        let query = DateRangeQuery {
            from: Some("2026-11-01".into()),
            to: None,
        };
        assert_eq!(query.range(), None);
    }

    #[test]
    fn utc_bounds_count_the_last_day_whole() {
        let (start, end) = DateRange::new(day(2026, 11, 1), day(2026, 11, 3)).utc_bounds(&Utc);
        assert_eq!(start, Utc.with_ymd_and_hms(2026, 11, 1, 0, 0, 0).unwrap());
        assert_eq!(end, Utc.with_ymd_and_hms(2026, 11, 4, 0, 0, 0).unwrap());
        // One day is one day.
        let (start, end) = DateRange::single(day(2026, 11, 3)).utc_bounds(&Utc);
        assert_eq!((end - start).num_hours(), 24);
    }

    #[test]
    fn a_23_hour_day_and_a_25_hour_day_are_counted_whole() {
        let ny: Tz = "America/New_York".parse().unwrap();
        let (start, end) = DateRange::single(day(2026, 3, 8)).utc_bounds(&ny);
        assert_eq!((end - start).num_hours(), 23);
        let (start, end) = DateRange::single(day(2026, 11, 1)).utc_bounds(&ny);
        assert_eq!((end - start).num_hours(), 25);
        // A range over both counts the 48 hours of ordinary days plus the odd ones.
        let (start, end) = DateRange::new(day(2026, 3, 7), day(2026, 3, 9)).utc_bounds(&ny);
        assert_eq!((end - start).num_hours(), 24 + 23 + 24);
    }

    #[test]
    fn unusual_offsets_still_start_at_local_midnight() {
        let kolkata: Tz = "Asia/Kolkata".parse().unwrap();
        let (start, end) = DateRange::single(day(2026, 11, 3)).utc_bounds(&kolkata);
        assert_eq!(start, Utc.with_ymd_and_hms(2026, 11, 2, 18, 30, 0).unwrap());
        assert_eq!(end, Utc.with_ymd_and_hms(2026, 11, 3, 18, 30, 0).unwrap());
        let kiritimati: Tz = "Pacific/Kiritimati".parse().unwrap();
        let (start, _) = DateRange::single(day(2026, 11, 3)).utc_bounds(&kiritimati);
        assert_eq!(start, Utc.with_ymd_and_hms(2026, 11, 2, 10, 0, 0).unwrap());
        let baker: Tz = "Etc/GMT+12".parse().unwrap();
        let (start, _) = DateRange::single(day(2026, 11, 3)).utc_bounds(&baker);
        assert_eq!(start, Utc.with_ymd_and_hms(2026, 11, 3, 12, 0, 0).unwrap());
    }

    #[test]
    fn a_zone_that_skips_midnight_starts_the_day_when_the_clocks_jump() {
        // Brazil's old summer time began at midnight: 2018-11-04 00:00 did not exist
        // in São Paulo; the day began at 01:00 (03:00 UTC).
        let sao_paulo: Tz = "America/Sao_Paulo".parse().unwrap();
        let (start, _) = DateRange::single(day(2018, 11, 4)).utc_bounds(&sao_paulo);
        assert_eq!(start, Utc.with_ymd_and_hms(2018, 11, 4, 3, 0, 0).unwrap());
    }

    #[test]
    fn presets_count_from_the_day_given() {
        let today = day(2026, 11, 3); // a Tuesday
        let r = |p: DatePreset, ws| p.range(today, ws, None).unwrap();
        assert_eq!(r(DatePreset::Today, Weekday::Mon), DateRange::single(today));
        assert_eq!(
            r(DatePreset::Yesterday, Weekday::Mon),
            DateRange::single(day(2026, 11, 2))
        );
        assert_eq!(
            r(DatePreset::ThisWeek, Weekday::Mon),
            DateRange::new(day(2026, 11, 2), today)
        );
        assert_eq!(
            r(DatePreset::ThisWeek, Weekday::Sun),
            DateRange::new(day(2026, 11, 1), today)
        );
        assert_eq!(
            r(DatePreset::ThisWeek, Weekday::Sat),
            DateRange::new(day(2026, 10, 31), today)
        );
        assert_eq!(
            r(DatePreset::LastWeek, Weekday::Mon),
            DateRange::new(day(2026, 10, 26), day(2026, 11, 1))
        );
        assert_eq!(
            r(DatePreset::Last7Days, Weekday::Mon),
            DateRange::new(day(2026, 10, 28), today)
        );
        assert_eq!(r(DatePreset::Last7Days, Weekday::Mon).day_count(), 7);
        assert_eq!(r(DatePreset::Last14Days, Weekday::Mon).day_count(), 14);
        assert_eq!(r(DatePreset::Last30Days, Weekday::Mon).day_count(), 30);
        assert_eq!(
            r(DatePreset::ThisMonth, Weekday::Mon),
            DateRange::new(day(2026, 11, 1), today)
        );
        assert_eq!(
            r(DatePreset::LastMonth, Weekday::Mon),
            DateRange::new(day(2026, 10, 1), day(2026, 10, 31))
        );
        assert_eq!(
            r(DatePreset::ThisQuarter, Weekday::Mon),
            DateRange::new(day(2026, 10, 1), today)
        );
        assert_eq!(
            r(DatePreset::LastQuarter, Weekday::Mon),
            DateRange::new(day(2026, 7, 1), day(2026, 9, 30))
        );
        assert_eq!(
            r(DatePreset::ThisYear, Weekday::Mon),
            DateRange::new(day(2026, 1, 1), today)
        );
        assert_eq!(
            r(DatePreset::LastYear, Weekday::Mon),
            DateRange::new(day(2025, 1, 1), day(2025, 12, 31))
        );
        assert_eq!(DatePreset::AllTime.range(today, Weekday::Mon, None), None);
        assert_eq!(
            DatePreset::AllTime.range(today, Weekday::Mon, Some(day(2020, 5, 17))),
            Some(DateRange::new(day(2020, 5, 17), today))
        );
    }

    #[test]
    fn presets_cross_the_year_and_the_leap_day() {
        let jan = day(2026, 1, 15);
        assert_eq!(
            DatePreset::LastMonth
                .range(jan, Weekday::Mon, None)
                .unwrap(),
            DateRange::new(day(2025, 12, 1), day(2025, 12, 31))
        );
        assert_eq!(
            DatePreset::LastQuarter
                .range(jan, Weekday::Mon, None)
                .unwrap(),
            DateRange::new(day(2025, 10, 1), day(2025, 12, 31))
        );
        let mar = day(2024, 3, 10);
        assert_eq!(
            DatePreset::LastMonth
                .range(mar, Weekday::Mon, None)
                .unwrap(),
            DateRange::new(day(2024, 2, 1), day(2024, 2, 29))
        );
        assert_eq!(
            DatePreset::Last30Days
                .range(mar, Weekday::Mon, None)
                .unwrap()
                .from,
            day(2024, 2, 10)
        );
        // The edge of the calendar returns nothing rather than overflow.
        assert_eq!(
            DatePreset::LastYear.range(day(1, 6, 1), Weekday::Mon, None),
            None
        );
    }

    #[test]
    fn preset_keys_round_trip() {
        for preset in DatePreset::ALL {
            assert_eq!(DatePreset::from_key(preset.key()), Some(preset));
            assert!(!preset.label().is_empty());
        }
        assert_eq!(DatePreset::from_key("later"), None);
    }

    #[test]
    fn chips_read_short() {
        let en = DateLocale::of("en");
        let today = day(2026, 11, 3);
        let f = |a: NaiveDate, b: NaiveDate| en.format_range(&DateRange::new(a, b), today);
        assert_eq!(f(day(2026, 11, 3), day(2026, 11, 3)), "Nov 3");
        assert_eq!(f(day(2026, 11, 1), day(2026, 11, 3)), "Nov 1 – 3");
        assert_eq!(f(day(2026, 10, 28), day(2026, 11, 3)), "Oct 28 – Nov 3");
        assert_eq!(
            f(day(2025, 12, 28), day(2026, 1, 4)),
            "Dec 28, 2025 – Jan 4, 2026"
        );
        assert_eq!(f(day(2025, 3, 1), day(2025, 3, 9)), "Mar 1 – 9, 2025");
        assert_eq!(f(day(2025, 3, 9), day(2025, 3, 9)), "Mar 9, 2025");
        let gb = DateLocale::of("en-GB");
        assert_eq!(
            gb.format_range(&DateRange::new(day(2026, 11, 1), day(2026, 11, 3)), today),
            "1 – 3 Nov"
        );
        let ja = DateLocale::of("ja");
        assert_eq!(
            ja.format_range(&DateRange::new(day(2026, 11, 1), day(2026, 11, 3)), today),
            "11月1日 – 11月3日"
        );
    }
}
