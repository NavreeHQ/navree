//! [`Report`]: a window of days from the URL, held to the same rules as the range
//! picker that wrote it, cut into periods of a grain in the report's zone.

use chrono::{
    DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc, Weekday,
};
use serde::Deserialize;

use super::scale::{Grain, seconds_of};
use super::{Chart, Series};
use crate::dates::{DateError, DatePreset, DateRange, DateRangePicker, DateRangeQuery, RangeRules};
use crate::node::Node;

/// The most periods a report draws. A grain asked for in the URL that would make
/// more is not used.
pub const MAX_BUCKETS: usize = 400;

/// What a report reads from its URL: `from` and `to` (the window, both days included)
/// and `grain` (`hour`, `day`, `week`, `month`). A key that comes twice keeps its
/// first value; anything else is ignored.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReportQuery {
    /// The first day, `YYYY-MM-DD`.
    pub from: Option<String>,
    /// The last day.
    pub to: Option<String>,
    /// The grain asked for.
    pub grain: Option<String>,
}

impl<'de> Deserialize<'de> for ReportQuery {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Reader;
        impl<'de> serde::de::Visitor<'de> for Reader {
            type Value = ReportQuery;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a query with `from`, `to` and `grain`")
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<ReportQuery, A::Error> {
                let mut out = ReportQuery::default();
                while let Some(key) = map.next_key::<String>()? {
                    let slot = match key.as_str() {
                        "from" => &mut out.from,
                        "to" => &mut out.to,
                        "grain" => &mut out.grain,
                        _ => {
                            map.next_value::<serde::de::IgnoredAny>()?;
                            continue;
                        }
                    };
                    let value = map.next_value::<Option<String>>()?;
                    if slot.is_none() {
                        *slot = value.map(|v| v.chars().take(32).collect());
                    }
                }
                Ok(out)
            }
        }
        deserializer.deserialize_map(Reader)
    }
}

impl ReportQuery {
    /// The window, as the range picker's query reads it.
    pub fn window(&self) -> DateRangeQuery {
        DateRangeQuery {
            from: self.from.clone(),
            to: self.to.clone(),
        }
    }
}

/// One period of a report: the instants it covers, `[start, end)`, and its start on
/// the zone's wall clock, which names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bucket {
    /// The first instant counted.
    pub start: DateTime<Utc>,
    /// The first instant not counted.
    pub end: DateTime<Utc>,
    /// The period's start on the report zone's clock: its name.
    pub local: NaiveDateTime,
}

/// A report's window and periods.
///
/// The window comes from the URL when the range picker's rules allow it, and from a
/// preset counted from the app's today when not. The grain comes from the window's
/// length: hours up to two days, days up to 92, weeks up to a year and a little,
/// months up to ten years, then years. A grain in the URL that would draw between 2 and
/// [`MAX_BUCKETS`] periods replaces it. The periods are wall-clock periods in the
/// report's zone, so a day is a day across a clock change and a month is a month.
///
/// ```
/// use navree_ui::charts::{Grain, Report, ReportQuery};
/// use navree_ui::dates::{DatePreset, DateRules, RangeRules};
/// use chrono::{NaiveDate, Utc};
///
/// let today = NaiveDate::from_ymd_opt(2026, 11, 4).unwrap();
/// let limits = RangeRules::new(DateRules::new().max(today)).max_days(3660);
/// let query = ReportQuery { from: Some("2026-10-01".into()), to: Some("2026-10-31".into()), grain: None };
/// let report = Report::from_query(&query, &limits, today, DatePreset::Last30Days, &Utc);
/// assert_eq!(report.grain(), Grain::Day);
/// assert_eq!(report.buckets().len(), 31);
/// let refused = ReportQuery { from: Some("2026-10-01".into()), to: Some("2027-01-01".into()), grain: None };
/// let report = Report::from_query(&refused, &limits, today, DatePreset::Last30Days, &Utc);
/// assert!(report.refused().is_some(), "after the last day the rules allow");
/// assert_eq!(report.range().day_count(), 30, "the default window instead");
/// ```
#[derive(Clone, Debug)]
pub struct Report {
    range: DateRange,
    grain: Grain,
    auto: Grain,
    buckets: Vec<Bucket>,
    refused: Option<DateError>,
    from_url: bool,
    week_start: Weekday,
    zone: Option<String>,
    today: NaiveDate,
    limits: Option<RangeRules>,
    default: Option<DatePreset>,
}

impl Report {
    /// A report over `range` at `grain`, in `zone`.
    pub fn new<Tz: TimeZone>(range: DateRange, grain: Grain, zone: &Tz) -> Self {
        Self::build(
            range,
            grain,
            Grain::for_days(range.day_count()),
            Weekday::Mon,
            zone,
        )
    }

    fn build<Tz: TimeZone>(
        range: DateRange,
        grain: Grain,
        auto: Grain,
        week_start: Weekday,
        zone: &Tz,
    ) -> Self {
        Self {
            buckets: buckets(range, grain, week_start, zone),
            range,
            grain,
            auto,
            refused: None,
            from_url: false,
            week_start,
            zone: None,
            today: range.to,
            limits: None,
            default: None,
        }
    }

    /// The report a request asks for: its window when `limits` allow it, `default`
    /// counted from `today` when the URL has none or one they refuse (which
    /// [`refused`](Self::refused) then says), and its grain when it fits.
    pub fn from_query<Tz: TimeZone>(
        query: &ReportQuery,
        limits: &RangeRules,
        today: NaiveDate,
        default: DatePreset,
        zone: &Tz,
    ) -> Self {
        Self::from_query_starting(query, limits, today, default, Weekday::Mon, zone)
    }

    /// [`from_query`](Self::from_query) with weeks starting on `week_start`.
    pub fn from_query_starting<Tz: TimeZone>(
        query: &ReportQuery,
        limits: &RangeRules,
        today: NaiveDate,
        default: DatePreset,
        week_start: Weekday,
        zone: &Tz,
    ) -> Self {
        let fallback = default
            .range(today, week_start, limits.rules().earliest())
            .or_else(|| DatePreset::Last30Days.range(today, week_start, None))
            .unwrap_or(DateRange::single(today));
        let (range, refused, from_url) = match query.window().checked(limits) {
            Ok(Some(range)) => (range, None, true),
            Ok(None) => (fallback, None, false),
            Err(error) => (fallback, Some(error), false),
        };
        let auto = Grain::for_days(range.day_count());
        let asked = query
            .grain
            .as_deref()
            .and_then(Grain::from_key)
            .filter(|grain| fits(range, *grain));
        let mut report = Self::build(range, asked.unwrap_or(auto), auto, week_start, zone);
        report.refused = refused;
        report.from_url = from_url;
        report.today = today;
        report.limits = Some(limits.clone());
        report.default = Some(default);
        report
    }

    /// Names the zone the periods are in (`Asia/Kolkata`), which TimescaleDB is given
    /// to bucket in that zone. Without a name a report on a zone other than UTC
    /// buckets with bound comparisons, which are as exact.
    pub fn zone_name(mut self, name: impl Into<String>) -> Self {
        self.zone = Some(name.into()).filter(|n| valid_zone_name(n));
        self
    }

    /// The zone's name, when given.
    pub fn zone(&self) -> Option<&str> {
        self.zone.as_deref()
    }

    /// The window.
    pub fn range(&self) -> DateRange {
        self.range
    }

    /// The period the window is cut into.
    pub fn grain(&self) -> Grain {
        self.grain
    }

    /// The grain the window's length picks.
    pub fn auto_grain(&self) -> Grain {
        self.auto
    }

    /// The first day of a week.
    pub fn week_start(&self) -> Weekday {
        self.week_start
    }

    /// The periods, in order.
    pub fn buckets(&self) -> &[Bucket] {
        &self.buckets
    }

    /// The instants the window covers, `[start, end)`: what a query filters on.
    pub fn bounds(&self) -> (DateTime<Utc>, DateTime<Utc>) {
        match (self.buckets.first(), self.buckets.last()) {
            (Some(a), Some(b)) => (a.start, b.end),
            _ => (DateTime::<Utc>::MIN_UTC, DateTime::<Utc>::MIN_UTC),
        }
    }

    /// Why the window in the URL was not used, when it was refused.
    pub fn refused(&self) -> Option<&DateError> {
        self.refused.as_ref()
    }

    /// Whether the window came from the URL (not the default).
    pub fn from_url(&self) -> bool {
        self.from_url
    }

    /// The grains a person may switch to: those that draw between 2 and
    /// [`MAX_BUCKETS`] periods of this window.
    pub fn grains(&self) -> Vec<Grain> {
        [
            Grain::Hour,
            Grain::Day,
            Grain::Week,
            Grain::Month,
            Grain::Quarter,
            Grain::Year,
        ]
        .into_iter()
        .filter(|g| *g == self.grain || fits(self.range, *g))
        .collect()
    }

    /// The URL's parameters for this window and grain: what a link keeps. The grain is
    /// left out when it is the one the window picks by itself.
    pub fn params(&self) -> Vec<(&'static str, String)> {
        let mut out = vec![
            ("from", self.range.from.to_string()),
            ("to", self.range.to.to_string()),
        ];
        if self.grain != self.auto {
            out.push(("grain", self.grain.key().to_owned()));
        }
        out
    }

    /// `path` with this report's parameters, and `grain` instead of its own when given.
    pub fn href(&self, path: &str, grain: Option<Grain>) -> String {
        let grain = grain.unwrap_or(self.grain);
        let mut out = format!("{path}?from={}&to={}", self.range.from, self.range.to);
        if grain != self.auto {
            out.push_str("&grain=");
            out.push_str(grain.key());
        }
        out
    }

    /// The range picker for this window, named `from` and `to`, with the report's
    /// rules and today: put it in a GET form with the grain as a hidden field.
    pub fn picker(&self) -> DateRangePicker {
        let mut picker = DateRangePicker::new("from", "to")
            .today(self.today)
            .week_start(self.week_start)
            .value(self.range)
            .not_clearable();
        if let Some(limits) = &self.limits {
            picker = picker.limits(limits.clone());
        }
        picker
    }

    /// The periods' starts, for [`Chart::times`].
    pub fn times(&self) -> impl Iterator<Item = NaiveDateTime> + '_ {
        self.buckets.iter().map(|b| b.local)
    }

    /// A chart over this report's periods: `chart` with its times and grain set.
    pub fn chart(&self, chart: Chart) -> Chart {
        chart
            .times(self.times())
            .grain(self.grain)
            .week_start(self.week_start)
    }

    /// The period an instant falls in.
    pub fn bucket_of(&self, at: DateTime<Utc>) -> Option<usize> {
        let found = self.buckets.partition_point(|b| b.start <= at);
        let index = found.checked_sub(1)?;
        (at < self.buckets[index].end).then_some(index)
    }

    /// How many of `instants` fall in each period: for data already in memory.
    pub fn count(&self, instants: impl IntoIterator<Item = DateTime<Utc>>) -> Vec<u64> {
        let mut out = vec![0; self.buckets.len()];
        for at in instants {
            if let Some(i) = self.bucket_of(at) {
                out[i] += 1;
            }
        }
        out
    }

    /// The sum of the values in each period: for data already in memory.
    pub fn sum(&self, values: impl IntoIterator<Item = (DateTime<Utc>, f64)>) -> Vec<f64> {
        let mut out = vec![0.0; self.buckets.len()];
        for (at, value) in values {
            if let Some(i) = self.bucket_of(at)
                && value.is_finite()
            {
                out[i] += value;
            }
        }
        out
    }

    /// A series over the periods, from counts.
    pub fn series(&self, name: impl Into<String>, counts: &[u64]) -> Series {
        Series::new(name, counts.iter().copied())
    }

    /// The grain switch: a link per grain this window can take, the current one marked.
    pub fn grain_links(&self, path: &str, label: &str) -> Node {
        use crate::node::el;
        let items: Vec<Node> = self
            .grains()
            .into_iter()
            .map(|grain| {
                let current = grain == self.grain;
                el("a")
                    .slot("chart-grain")
                    .href(self.href(path, Some(grain)))
                    .attr_opt("aria-current", current.then_some("true"))
                    .child(grain_name(grain))
                    .into()
            })
            .collect();
        el("nav")
            .slot("chart-grains")
            .attr("aria-label", label.to_owned())
            .child(items)
            .into()
    }
}

/// The English name of a grain on a switch.
pub(crate) fn grain_name(grain: Grain) -> &'static str {
    match grain {
        Grain::Minute => "Minute",
        Grain::Hour => "Hour",
        Grain::Day => "Day",
        Grain::Week => "Week",
        Grain::Month => "Month",
        Grain::Quarter => "Quarter",
        Grain::Year => "Year",
    }
}

impl Grain {
    /// The grain a window of `days` days is drawn in.
    pub fn for_days(days: i64) -> Self {
        match days {
            ..=2 => Self::Hour,
            3..=92 => Self::Day,
            93..=400 => Self::Week,
            401..=3660 => Self::Month,
            _ => Self::Year,
        }
    }
}

/// Whether `grain` cuts `range` into between 2 and [`MAX_BUCKETS`] periods.
fn fits(range: DateRange, grain: Grain) -> bool {
    if grain == Grain::Minute {
        return false;
    }
    let seconds = range.day_count().saturating_mul(86_400);
    let count = seconds / grain.seconds().max(1);
    (1..=MAX_BUCKETS as i64).contains(&count)
        && (count >= 2 || grain == Grain::for_days(range.day_count()))
}

/// Whether `name` looks like a zone name a database can be given: `Area/City`,
/// letters, digits, `_+-/`.
fn valid_zone_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '-' | '/'))
}

/// The first instant of a day in a zone.
fn start_of<Tz: TimeZone>(day: NaiveDate, zone: &Tz) -> DateTime<Utc> {
    DateRange::single(day).utc_bounds(zone).0
}

/// The periods of `range` at `grain` in `zone`. Each is clipped to the window, and
/// named by its start on the zone's clock.
fn buckets<Tz: TimeZone>(
    range: DateRange,
    grain: Grain,
    week_start: Weekday,
    zone: &Tz,
) -> Vec<Bucket> {
    let (start, end) = range.utc_bounds(zone);
    let mut out = Vec::new();
    let local = |at: DateTime<Utc>| at.with_timezone(zone).naive_local();
    match grain {
        Grain::Minute | Grain::Hour => {
            // Real hours from the window's first instant: a 23-hour day has 23, a
            // 25-hour day 25, and a zone half an hour off UTC stays on its own hours.
            let step = Duration::seconds(grain.seconds());
            let mut at = start;
            while at < end && out.len() < MAX_BUCKETS * 60 {
                let next = (at + step).min(end);
                out.push(Bucket {
                    start: at,
                    end: next,
                    local: local(at),
                });
                at = next;
            }
        }
        Grain::Day | Grain::Week | Grain::Month | Grain::Quarter | Grain::Year => {
            let first = period_start(range.from, grain, week_start);
            let mut day = first;
            while day <= range.to && out.len() < 100_000 {
                let Some(next) = period_next(day, grain) else {
                    break;
                };
                let a = start_of(day.max(range.from), zone);
                let b = match next.checked_sub_signed(Duration::days(1)) {
                    Some(last) if last < range.to => start_of(next, zone),
                    _ => end,
                };
                out.push(Bucket {
                    start: a,
                    end: b,
                    local: day.and_time(NaiveTime::MIN),
                });
                day = next;
            }
        }
    }
    out
}

/// The first day of the period `day` is in.
fn period_start(day: NaiveDate, grain: Grain, week_start: Weekday) -> NaiveDate {
    match grain {
        Grain::Week => {
            let back =
                (day.weekday().num_days_from_monday() + 7 - week_start.num_days_from_monday()) % 7;
            day.checked_sub_signed(Duration::days(i64::from(back)))
                .unwrap_or(day)
        }
        Grain::Month => day.with_day(1).unwrap_or(day),
        Grain::Quarter => {
            NaiveDate::from_ymd_opt(day.year(), (day.month() - 1) / 3 * 3 + 1, 1).unwrap_or(day)
        }
        Grain::Year => NaiveDate::from_ymd_opt(day.year(), 1, 1).unwrap_or(day),
        Grain::Minute | Grain::Hour | Grain::Day => day,
    }
}

/// The first day of the period after the one starting on `day`.
fn period_next(day: NaiveDate, grain: Grain) -> Option<NaiveDate> {
    let months = |n: u32| {
        let total = day.year() * 12 + day.month0() as i32 + n as i32;
        NaiveDate::from_ymd_opt(total.div_euclid(12), total.rem_euclid(12) as u32 + 1, 1)
    };
    match grain {
        Grain::Week => day.checked_add_signed(Duration::days(7)),
        Grain::Month => months(1),
        Grain::Quarter => months(3),
        Grain::Year => NaiveDate::from_ymd_opt(day.year().checked_add(1)?, 1, 1),
        Grain::Minute | Grain::Hour | Grain::Day => day.succ_opt(),
    }
}

/// Seconds of a wall-clock time, for tests and charts.
#[allow(dead_code)]
pub(crate) fn wall(at: NaiveDateTime) -> i64 {
    seconds_of(at)
}

#[cfg(test)]
mod tests {
    use chrono_tz::Tz;

    use super::*;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn the_grain_follows_the_window() {
        assert_eq!(Grain::for_days(1), Grain::Hour);
        assert_eq!(Grain::for_days(2), Grain::Hour);
        assert_eq!(Grain::for_days(7), Grain::Day);
        assert_eq!(Grain::for_days(92), Grain::Day);
        assert_eq!(Grain::for_days(365), Grain::Week);
        assert_eq!(Grain::for_days(730), Grain::Month);
        assert_eq!(Grain::for_days(20_000), Grain::Year);
    }

    #[test]
    fn days_are_whole_across_a_clock_change() {
        let ny: Tz = "America/New_York".parse().unwrap();
        let report = Report::new(
            DateRange::new(day(2026, 3, 7), day(2026, 3, 9)),
            Grain::Day,
            &ny,
        );
        let hours: Vec<i64> = report
            .buckets()
            .iter()
            .map(|b| (b.end - b.start).num_hours())
            .collect();
        assert_eq!(hours, [24, 23, 24]);
        let report = Report::new(DateRange::single(day(2026, 11, 1)), Grain::Hour, &ny);
        assert_eq!(report.buckets().len(), 25, "the fall-back day has 25 hours");
        let labels: Vec<String> = report
            .buckets()
            .iter()
            .map(|b| b.local.format("%H").to_string())
            .collect();
        assert_eq!(
            labels.iter().filter(|h| *h == "01").count(),
            2,
            "{labels:?}"
        );
    }

    #[test]
    fn hours_stay_on_the_zones_own_hours() {
        let kolkata: Tz = "Asia/Kolkata".parse().unwrap();
        let report = Report::new(DateRange::single(day(2026, 11, 3)), Grain::Hour, &kolkata);
        assert_eq!(report.buckets().len(), 24);
        assert!(
            report
                .buckets()
                .iter()
                .all(|b| b.local.format("%M").to_string() == "00")
        );
    }

    #[test]
    fn weeks_and_months_are_clipped_to_the_window() {
        let report = Report::new(
            DateRange::new(day(2026, 11, 4), day(2026, 11, 30)),
            Grain::Week,
            &Utc,
        );
        assert_eq!(
            report.buckets()[0].local.date(),
            day(2026, 11, 2),
            "named by the week's Monday"
        );
        assert_eq!(
            report.buckets()[0].start,
            start_of(day(2026, 11, 4), &Utc),
            "counted from the window"
        );
        assert_eq!(report.bounds().1, start_of(day(2026, 12, 1), &Utc));
        let months = Report::new(
            DateRange::new(day(2025, 12, 15), day(2026, 3, 1)),
            Grain::Month,
            &Utc,
        );
        let names: Vec<String> = months
            .buckets()
            .iter()
            .map(|b| b.local.format("%Y-%m").to_string())
            .collect();
        assert_eq!(names, ["2025-12", "2026-01", "2026-02", "2026-03"]);
        assert!(
            months.buckets().windows(2).all(|w| w[0].end == w[1].start),
            "no gap, no overlap"
        );
    }

    #[test]
    fn instants_land_in_their_period() {
        let report = Report::new(
            DateRange::new(day(2026, 11, 1), day(2026, 11, 3)),
            Grain::Day,
            &Utc,
        );
        let at = |d, h| Utc.with_ymd_and_hms(2026, 11, d, h, 0, 0).unwrap();
        assert_eq!(
            report.count([at(1, 0), at(1, 23), at(2, 5), at(4, 0), at(3, 23)]),
            [2, 1, 1]
        );
        assert_eq!(
            report.bucket_of(Utc.with_ymd_and_hms(2026, 10, 31, 23, 0, 0).unwrap()),
            None
        );
    }

    #[test]
    fn the_url_grain_must_fit() {
        let limits = RangeRules::new(crate::dates::DateRules::new());
        let today = day(2026, 11, 4);
        let query = |from: &str, to: &str, grain: &str| ReportQuery {
            from: Some(from.into()),
            to: Some(to.into()),
            grain: Some(grain.into()),
        };
        let r = Report::from_query(
            &query("2026-01-01", "2026-12-31", "hour"),
            &limits,
            today,
            DatePreset::Last30Days,
            &Utc,
        );
        assert_eq!(r.grain(), Grain::Week, "8,760 hours is too many");
        let r = Report::from_query(
            &query("2026-01-01", "2026-12-31", "month"),
            &limits,
            today,
            DatePreset::Last30Days,
            &Utc,
        );
        assert_eq!(r.grain(), Grain::Month);
        assert_eq!(r.params().last().unwrap(), &("grain", "month".to_owned()));
        let r = Report::from_query(
            &query("2026-11-01", "2026-11-03", "nonsense"),
            &limits,
            today,
            DatePreset::Last30Days,
            &Utc,
        );
        assert_eq!(r.grain(), Grain::Day);
        assert!(r.grains().contains(&Grain::Hour));
        let r = Report::from_query(
            &query("soon", "later", "day"),
            &limits,
            today,
            DatePreset::Last7Days,
            &Utc,
        );
        assert!(!r.from_url());
        assert_eq!(r.range().day_count(), 7);
    }

    #[test]
    fn the_first_value_of_a_repeated_key_stands() {
        let q: ReportQuery =
            serde_urlencoded::from_str("grain=day&grain=hour&from=2026-11-01&x=1").unwrap();
        assert_eq!(q.grain.as_deref(), Some("day"));
        let long: ReportQuery =
            serde_urlencoded::from_str(&format!("grain={}", "a".repeat(10_000))).unwrap();
        assert_eq!(long.grain.unwrap().len(), 32);
    }

    #[test]
    fn edges_of_the_calendar_do_not_panic() {
        for (a, b, grain) in [
            (day(1, 1, 1), day(1, 1, 3), Grain::Hour),
            (day(9999, 12, 1), day(9999, 12, 31), Grain::Week),
            (day(9999, 1, 1), day(9999, 12, 31), Grain::Year),
            (day(1, 1, 1), day(9999, 12, 31), Grain::Year),
        ] {
            let report = Report::new(DateRange::new(a, b), grain, &Utc);
            assert!(!report.buckets().is_empty());
        }
    }
}
