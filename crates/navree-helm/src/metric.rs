//! [`Metric`]: the numbers that matter, as cards above a resource's list
//! and on the dashboard.

use std::fmt;
use std::marker::PhantomData;

use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Utc};
use navree_orm::{ColumnName, Model};
use serde::Serialize;

use crate::slug;

/// The kinds of metric.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MetricKind {
    Value,
    Trend,
    Partition,
}

impl MetricKind {
    pub(crate) fn name(self) -> &'static str {
        match self {
            MetricKind::Value => "value",
            MetricKind::Trend => "trend",
            MetricKind::Partition => "partition",
        }
    }
}

/// What a metric computes over its rows.
#[derive(Clone, Debug)]
pub(crate) enum Aggregate<C> {
    Count,
    Sum(C),
    Average(C),
    Min(C),
    Max(C),
}

impl<C> Aggregate<C> {
    pub(crate) fn column(&self) -> Option<&C> {
        match self {
            Aggregate::Count => None,
            Aggregate::Sum(c) | Aggregate::Average(c) | Aggregate::Min(c) | Aggregate::Max(c) => {
                Some(c)
            }
        }
    }

    /// Folds the numbers of a bucket (`None` for an empty bucket, except
    /// counts and sums, which are 0).
    pub(crate) fn fold(&self, values: &[f64]) -> Option<f64> {
        match self {
            Aggregate::Count => Some(values.len() as f64),
            Aggregate::Sum(_) => Some(values.iter().sum()),
            Aggregate::Average(_) if values.is_empty() => None,
            Aggregate::Average(_) => Some(values.iter().sum::<f64>() / values.len() as f64),
            Aggregate::Min(_) => values.iter().copied().reduce(f64::min),
            Aggregate::Max(_) => values.iter().copied().reduce(f64::max),
        }
    }
}

/// The name of a column given to a metric.
fn name(column: impl ColumnName) -> String {
    column.column_name().into_owned()
}

/// The buckets of a trend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Interval {
    Day,
    Week,
    Month,
}

/// One metric of a resource: a single value (with the change against the
/// previous period), a trend over time, or a breakdown by a column. Each is
/// one query over the rows the user may read.
///
/// ```ignore
/// Metric::value("Open pipeline")
///     .sum(Deal::AMOUNT_CENTS)
///     .filter(Deal::CLOSED_AT.is_null())
///     .money("USD")
///     .on_dashboard()
/// Metric::trend("New deals").count().by_day(Deal::CREATED_AT, 30)
/// Metric::partition("Deals by stage").count().group_by(Deal::STAGE)
/// ```
///
/// Columns are the model's typed columns (`Deal::AMOUNT_CENTS`) or column
/// names (`"amount_cents"`); the panel checks them when it boots.
pub struct Metric<M: Model> {
    _model: PhantomData<fn() -> M>,
    pub(crate) label: String,
    pub(crate) slug: String,
    pub(crate) kind: MetricKind,
    pub(crate) aggregate: Aggregate<String>,
    pub(crate) conditions: Vec<navree_orm::Filter>,
    pub(crate) money: Option<String>,
    pub(crate) on_dashboard: bool,
    pub(crate) over: Option<String>,
    pub(crate) trend: Option<(String, Interval, u32)>,
    pub(crate) group_by: Option<String>,
}

impl<M: Model> Metric<M> {
    fn new(label: &str, kind: MetricKind) -> Self {
        Self {
            _model: PhantomData,
            label: label.to_owned(),
            slug: slug(label),
            kind,
            aggregate: Aggregate::Count,
            conditions: Vec::new(),
            money: None,
            on_dashboard: false,
            over: None,
            trend: None,
            group_by: None,
        }
    }

    /// One number. With [`over`](Metric::over) it gets a range picker and
    /// the change against the period before.
    pub fn value(label: &str) -> Self {
        Self::new(label, MetricKind::Value)
    }

    /// A line over time: an aggregate, then [`by_day`](Metric::by_day),
    /// [`by_week`](Metric::by_week) or [`by_month`](Metric::by_month).
    pub fn trend(label: &str) -> Self {
        Self::new(label, MetricKind::Trend)
    }

    /// A breakdown by a column: an aggregate, then
    /// [`group_by`](Metric::group_by).
    pub fn partition(label: &str) -> Self {
        Self::new(label, MetricKind::Partition)
    }

    /// Counts the rows (the default).
    pub fn count(mut self) -> Self {
        self.aggregate = Aggregate::Count;
        self
    }

    /// Adds up `column`.
    pub fn sum(mut self, column: impl ColumnName) -> Self {
        self.aggregate = Aggregate::Sum(name(column));
        self
    }

    /// Averages `column`.
    pub fn average(mut self, column: impl ColumnName) -> Self {
        self.aggregate = Aggregate::Average(name(column));
        self
    }

    /// The smallest `column`.
    pub fn min(mut self, column: impl ColumnName) -> Self {
        self.aggregate = Aggregate::Min(name(column));
        self
    }

    /// The largest `column`.
    pub fn max(mut self, column: impl ColumnName) -> Self {
        self.aggregate = Aggregate::Max(name(column));
        self
    }

    /// Only the rows matching `condition` (a navree-orm condition, such as
    /// `Deal::CLOSED_AT.is_null()`); calls add up.
    pub fn filter(mut self, condition: impl Into<navree_orm::Filter>) -> Self {
        self.conditions.push(condition.into());
        self
    }

    /// Shows the value as an amount in minor units of `currency`.
    pub fn money(mut self, currency: &str) -> Self {
        self.money = Some(currency.trim().to_ascii_uppercase());
        self
    }

    /// Also shows the metric on the panel's home page.
    pub fn on_dashboard(mut self) -> Self {
        self.on_dashboard = true;
        self
    }

    /// A value metric over the date column `column`: a range picker (7, 30
    /// or 90 days, this month, this year) and the change against the
    /// period before.
    pub fn over(mut self, column: impl ColumnName) -> Self {
        self.over = Some(name(column));
        self
    }

    /// A trend of the last `days` days, by day of `column`.
    pub fn by_day(mut self, column: impl ColumnName, days: u32) -> Self {
        self.trend = Some((name(column), Interval::Day, days.max(1)));
        self
    }

    /// A trend of the last `weeks` weeks (from Monday), by week of `column`.
    pub fn by_week(mut self, column: impl ColumnName, weeks: u32) -> Self {
        self.trend = Some((name(column), Interval::Week, weeks.max(1)));
        self
    }

    /// A trend of the last `months` months, by month of `column`.
    pub fn by_month(mut self, column: impl ColumnName, months: u32) -> Self {
        self.trend = Some((name(column), Interval::Month, months.max(1)));
        self
    }

    /// A breakdown by the values of `column`.
    pub fn group_by(mut self, column: impl ColumnName) -> Self {
        self.group_by = Some(name(column));
        self
    }

    /// The metric's name in URLs: its label in kebab-case.
    pub fn slug(&self) -> &str {
        &self.slug
    }
}

impl<M: Model> Clone for Metric<M> {
    fn clone(&self) -> Self {
        Self {
            _model: PhantomData,
            label: self.label.clone(),
            slug: self.slug.clone(),
            kind: self.kind,
            aggregate: self.aggregate.clone(),
            conditions: self.conditions.clone(),
            money: self.money.clone(),
            on_dashboard: self.on_dashboard,
            over: self.over.clone(),
            trend: self.trend.clone(),
            group_by: self.group_by.clone(),
        }
    }
}

impl<M: Model> fmt::Debug for Metric<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Metric")
            .field("label", &self.label)
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

/// `[start, end)` of a period.
pub(crate) type Period = (DateTime<Utc>, DateTime<Utc>);

/// The period of a value metric with [`Metric::over`]: `range=7`, `30`
/// (the default), `90`, `month` or `year`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MetricRange {
    /// The last 7 days.
    Days7,
    /// The last 30 days.
    #[default]
    Days30,
    /// The last 90 days.
    Days90,
    /// This month, so far.
    Month,
    /// This year, so far.
    Year,
}

impl MetricRange {
    /// Every range, in picker order.
    pub const ALL: [MetricRange; 5] = [
        MetricRange::Days7,
        MetricRange::Days30,
        MetricRange::Days90,
        MetricRange::Month,
        MetricRange::Year,
    ];

    /// The range named `text` (`7`, `30`, `90`, `month`, `year`).
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text.trim() {
            "7" => MetricRange::Days7,
            "30" => MetricRange::Days30,
            "90" => MetricRange::Days90,
            "month" | "mtd" => MetricRange::Month,
            "year" | "ytd" => MetricRange::Year,
            _ => return None,
        })
    }

    /// The range's name in URLs.
    pub fn as_str(self) -> &'static str {
        match self {
            MetricRange::Days7 => "7",
            MetricRange::Days30 => "30",
            MetricRange::Days90 => "90",
            MetricRange::Month => "month",
            MetricRange::Year => "year",
        }
    }

    /// The range's label in the picker.
    pub fn label(self) -> &'static str {
        match self {
            MetricRange::Days7 => "7 days",
            MetricRange::Days30 => "30 days",
            MetricRange::Days90 => "90 days",
            MetricRange::Month => "This month",
            MetricRange::Year => "This year",
        }
    }

    /// `[start, end)` of the current period ending at `now`, and the period
    /// of the same length before it.
    pub(crate) fn periods(self, now: DateTime<Utc>) -> (Period, Period) {
        let start = match self {
            MetricRange::Days7 => now - Duration::days(7),
            MetricRange::Days30 => now - Duration::days(30),
            MetricRange::Days90 => now - Duration::days(90),
            MetricRange::Month => midnight(
                NaiveDate::from_ymd_opt(now.year(), now.month(), 1).unwrap_or(now.date_naive()),
            ),
            MetricRange::Year => {
                midnight(NaiveDate::from_ymd_opt(now.year(), 1, 1).unwrap_or(now.date_naive()))
            }
        };
        let length = now - start;
        ((start, now), (start - length, start))
    }
}

/// Midnight UTC of `date`.
pub(crate) fn midnight(date: NaiveDate) -> DateTime<Utc> {
    Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0).unwrap_or_default())
}

/// The buckets of a trend ending at `today`: (label, first day, day after
/// the last), oldest first.
pub(crate) fn buckets(
    interval: Interval,
    count: u32,
    today: NaiveDate,
) -> Vec<(String, NaiveDate, NaiveDate)> {
    let mut out = Vec::new();
    match interval {
        Interval::Day => {
            for back in (0..count).rev() {
                let day = today - Duration::days(i64::from(back));
                out.push((
                    day.format("%Y-%m-%d").to_string(),
                    day,
                    day + Duration::days(1),
                ));
            }
        }
        Interval::Week => {
            let monday = today - Duration::days(i64::from(today.weekday().num_days_from_monday()));
            for back in (0..count).rev() {
                let start = monday - Duration::weeks(i64::from(back));
                out.push((
                    start.format("%Y-%m-%d").to_string(),
                    start,
                    start + Duration::weeks(1),
                ));
            }
        }
        Interval::Month => {
            let mut months = Vec::new();
            let (mut year, mut month) = (today.year(), today.month());
            for _ in 0..count {
                months.push((year, month));
                if month == 1 {
                    year -= 1;
                    month = 12;
                } else {
                    month -= 1;
                }
            }
            for (year, month) in months.into_iter().rev() {
                let Some(start) = NaiveDate::from_ymd_opt(year, month, 1) else {
                    continue;
                };
                let end = if month == 12 {
                    NaiveDate::from_ymd_opt(year + 1, 1, 1)
                } else {
                    NaiveDate::from_ymd_opt(year, month + 1, 1)
                }
                .unwrap_or(start);
                out.push((start.format("%Y-%m").to_string(), start, end));
            }
        }
    }
    out
}

/// A metric's answer: `GET {path}/api/resources/{resource}/metrics/{metric}`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct MetricResult {
    pub(crate) label: String,
    pub(crate) slug: String,
    pub(crate) kind: &'static str,
    /// The range of a value metric with `over`.
    pub(crate) range: Option<&'static str>,
    pub(crate) ranges: Vec<RangeOption>,
    /// The value (value metrics).
    pub(crate) value: Option<f64>,
    /// The previous period's value (value metrics with `over`).
    pub(crate) previous: Option<f64>,
    /// `(value - previous) / previous`, when both are known and previous is
    /// not 0.
    pub(crate) change: Option<f64>,
    /// The value as text (`$12,500.00`, `1,204`).
    pub(crate) display: String,
    pub(crate) currency: Option<String>,
    /// Trend points, oldest first.
    pub(crate) points: Vec<Point>,
    /// Partition groups, largest first.
    pub(crate) groups: Vec<Point>,
}

/// One range of the picker.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct RangeOption {
    pub(crate) value: &'static str,
    pub(crate) label: &'static str,
}

/// A trend point or a partition group.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct Point {
    /// The bucket (`2026-10-26`, `2026-10`) or the group's value.
    pub(crate) key: String,
    /// The label on screen.
    pub(crate) label: String,
    pub(crate) value: f64,
    pub(crate) display: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_parse_and_compute_their_periods() {
        assert_eq!(MetricRange::parse("7"), Some(MetricRange::Days7));
        assert_eq!(MetricRange::parse("mtd"), Some(MetricRange::Month));
        assert_eq!(MetricRange::parse("week"), None);
        let now = Utc.with_ymd_and_hms(2026, 10, 26, 12, 0, 0).unwrap();
        let ((start, end), (previous_start, previous_end)) = MetricRange::Month.periods(now);
        assert_eq!(start, Utc.with_ymd_and_hms(2026, 10, 1, 0, 0, 0).unwrap());
        assert_eq!(end, now);
        assert_eq!(previous_end, start);
        assert_eq!(previous_start, start - (now - start));
        let ((start, _), _) = MetricRange::Days7.periods(now);
        assert_eq!(start, Utc.with_ymd_and_hms(2026, 10, 19, 12, 0, 0).unwrap());
    }

    #[test]
    fn trend_buckets_end_today() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 28).unwrap(); // a Wednesday
        let days = buckets(Interval::Day, 3, today);
        let labels: Vec<&str> = days.iter().map(|b| b.0.as_str()).collect();
        assert_eq!(labels, ["2026-10-26", "2026-10-27", "2026-10-28"]);
        let weeks = buckets(Interval::Week, 2, today);
        assert_eq!(weeks[1].1, NaiveDate::from_ymd_opt(2026, 10, 26).unwrap());
        assert_eq!(weeks[0].1, NaiveDate::from_ymd_opt(2026, 10, 19).unwrap());
        let months = buckets(
            Interval::Month,
            3,
            NaiveDate::from_ymd_opt(2026, 1, 15).unwrap(),
        );
        let labels: Vec<&str> = months.iter().map(|b| b.0.as_str()).collect();
        assert_eq!(labels, ["2025-11", "2025-12", "2026-01"]);
        assert_eq!(months[2].2, NaiveDate::from_ymd_opt(2026, 2, 1).unwrap());
    }

    #[test]
    fn aggregates_fold_buckets() {
        let values = [1.0, 5.0, 3.0];
        assert_eq!(Aggregate::<()>::Count.fold(&values), Some(3.0));
        assert_eq!(Aggregate::Sum(()).fold(&values), Some(9.0));
        assert_eq!(Aggregate::Average(()).fold(&values), Some(3.0));
        assert_eq!(Aggregate::Min(()).fold(&values), Some(1.0));
        assert_eq!(Aggregate::Max(()).fold(&values), Some(5.0));
        assert_eq!(Aggregate::Average(()).fold(&[]), None);
        assert_eq!(Aggregate::Sum(()).fold(&[]), Some(0.0));
    }
}
