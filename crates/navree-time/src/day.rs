//! [`LocalDay`] and [`DayRange`]: calendar days counted on a zone's clock.

use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Datelike, Months, NaiveDate, NaiveTime, TimeDelta, Utc, Weekday};
use serde::de::{self, Deserializer, Unexpected};
use serde::{Deserialize, Serialize, Serializer};

use crate::Zone;

/// A calendar day, written `yyyy-MM-dd`, with no zone of its own: the
/// day of a report, a due date, a holiday. Pair it with a [`Zone`] to get
/// the instants it spans.
///
/// It parses and deserializes only from the exact `yyyy-MM-dd` form (a
/// valid date), serializes back to it, and orders by date.
///
/// ```
/// use navree_time::{LocalDay, Zone};
///
/// let day: LocalDay = "2026-10-27".parse()?;
/// let kolkata = Zone::parse("Asia/Kolkata").unwrap();
/// let (start, end) = day.utc_bounds(&kolkata);
/// assert_eq!(start.to_rfc3339(), "2026-10-26T18:30:00+00:00");
/// assert_eq!(end.to_rfc3339(), "2026-10-27T18:30:00+00:00");
/// # Ok::<(), navree_time::LocalDayError>(())
/// ```
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocalDay(NaiveDate);

/// Text that is not a day in the form `yyyy-MM-dd`.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` is not a day in the format yyyy-MM-dd")]
pub struct LocalDayError(String);

impl LocalDay {
    /// The day `year`-`month`-`day`, if it exists.
    pub fn new(year: i32, month: u32, day: u32) -> Option<LocalDay> {
        NaiveDate::from_ymd_opt(year, month, day).map(LocalDay)
    }

    /// Parses `yyyy-MM-dd` (four-digit year, two-digit month and day).
    pub fn parse(text: &str) -> Result<LocalDay, LocalDayError> {
        let invalid = || LocalDayError(text.to_owned());
        let bytes = text.as_bytes();
        let shaped = bytes.len() == 10
            && bytes[4] == b'-'
            && bytes[7] == b'-'
            && bytes
                .iter()
                .enumerate()
                .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit());
        if !shaped {
            return Err(invalid());
        }
        NaiveDate::parse_from_str(text, "%Y-%m-%d")
            .map(LocalDay)
            .map_err(|_| invalid())
    }

    /// The local day `at` falls on in `zone`.
    pub fn of(at: DateTime<Utc>, zone: &Zone) -> LocalDay {
        LocalDay(zone.local(at).date_naive())
    }

    /// The date.
    pub fn date(&self) -> NaiveDate {
        self.0
    }

    /// The year.
    pub fn year(&self) -> i32 {
        self.0.year()
    }

    /// The month, 1 to 12.
    pub fn month(&self) -> u32 {
        self.0.month()
    }

    /// The day of the month, 1 to 31.
    pub fn day(&self) -> u32 {
        self.0.day()
    }

    /// The day of the week.
    pub fn weekday(&self) -> Weekday {
        self.0.weekday()
    }

    /// The next day.
    pub fn succ(&self) -> LocalDay {
        LocalDay(self.0.succ_opt().unwrap_or(self.0))
    }

    /// The previous day.
    pub fn pred(&self) -> LocalDay {
        LocalDay(self.0.pred_opt().unwrap_or(self.0))
    }

    /// The day `days` days later (earlier when negative).
    pub fn add_days(&self, days: i64) -> LocalDay {
        LocalDay(
            self.0
                .checked_add_signed(TimeDelta::days(days))
                .unwrap_or(self.0),
        )
    }

    /// The first day of this day's month.
    pub fn first_of_month(&self) -> LocalDay {
        LocalDay(self.0.with_day(1).unwrap_or(self.0))
    }

    /// The last day of this day's month.
    pub fn last_of_month(&self) -> LocalDay {
        let first = self.first_of_month().0;
        LocalDay(
            first
                .checked_add_months(Months::new(1))
                .and_then(|next| next.pred_opt())
                .unwrap_or(self.0),
        )
    }

    /// The first instant of the day in `zone`: local midnight, or the first
    /// instant after it where clocks skip midnight.
    pub fn start_in(&self, zone: &Zone) -> DateTime<Utc> {
        zone.to_utc(self.0.and_time(NaiveTime::MIN))
    }

    /// The first instant of the next day in `zone` (the day's exclusive end).
    pub fn end_in(&self, zone: &Zone) -> DateTime<Utc> {
        self.succ().start_in(zone)
    }

    /// `(start, end)`: the half-open span of instants that fall on this day
    /// in `zone`. Usually 24 hours; 23 or 25 when clocks change.
    pub fn utc_bounds(&self, zone: &Zone) -> (DateTime<Utc>, DateTime<Utc>) {
        (self.start_in(zone), self.end_in(zone))
    }

    /// The instant this day's `time` is in `zone` (see [`Zone::to_utc`]
    /// for times skipped or repeated by DST changes).
    pub fn at(&self, time: NaiveTime, zone: &Zone) -> DateTime<Utc> {
        zone.to_utc(self.0.and_time(time))
    }
}

impl From<NaiveDate> for LocalDay {
    fn from(date: NaiveDate) -> Self {
        LocalDay(date)
    }
}

impl From<LocalDay> for NaiveDate {
    fn from(day: LocalDay) -> Self {
        day.0
    }
}

impl fmt::Display for LocalDay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.format("%Y-%m-%d"))
    }
}

impl fmt::Debug for LocalDay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LocalDay({self})")
    }
}

impl FromStr for LocalDay {
    type Err = LocalDayError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        LocalDay::parse(s)
    }
}

impl Serialize for LocalDay {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for LocalDay {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        LocalDay::parse(&text).map_err(|_| {
            de::Error::invalid_value(Unexpected::Str(&text), &"a date in the format yyyy-MM-dd")
        })
    }
}

impl navree_openapi::ToSchema for LocalDay {
    fn schema() -> navree_openapi::Schema {
        navree_openapi::Schema::string()
            .format("date")
            .examples(vec![serde_json::json!("2026-10-27")])
    }
}

/// Whole local days from `from` to `to`, both included: the window of a
/// report, counted in the tenant's days.
///
/// It deserializes from `{"from": "2026-10-01", "to": "2026-10-31"}` (a
/// query string or a body), and navree-validation checks that `to` is not
/// before `from` (its `time` feature, which the facade turns on), so
/// `Valid<Query<DayRange>>` takes it from a request.
/// [`utc_bounds`](DayRange::utc_bounds) turns it into the half-open span of
/// instants to filter rows by.
///
/// ```
/// use navree_time::{DayRange, LocalDay, Zone};
///
/// let october = DayRange::month_of("2026-10-15".parse::<LocalDay>()?);
/// let london = Zone::parse("Europe/London").unwrap();
/// let (from, to) = october.utc_bounds(&london);
/// assert_eq!(from.to_rfc3339(), "2026-09-30T23:00:00+00:00"); // BST
/// assert_eq!(to.to_rfc3339(), "2026-11-01T00:00:00+00:00"); // GMT
/// assert_eq!(october.day_count(), 31);
/// # Ok::<(), navree_time::LocalDayError>(())
/// ```
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, navree_openapi::ToSchema,
)]
pub struct DayRange {
    /// The first day.
    pub from: LocalDay,
    /// The last day (included).
    pub to: LocalDay,
}

/// A range whose last day comes before its first.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("the range ends ({to}) before it starts ({from})")]
pub struct DayRangeError {
    /// The first day.
    pub from: LocalDay,
    /// The last day.
    pub to: LocalDay,
}

impl DayRange {
    /// The days from `from` to `to`, both included.
    pub fn new(from: LocalDay, to: LocalDay) -> Result<DayRange, DayRangeError> {
        if to < from {
            Err(DayRangeError { from, to })
        } else {
            Ok(DayRange { from, to })
        }
    }

    /// One day.
    pub fn single(day: LocalDay) -> DayRange {
        DayRange { from: day, to: day }
    }

    /// The last `days` days in `zone`, today (at `now`) included; at least
    /// today.
    pub fn last_days(days: u32, zone: &Zone, now: DateTime<Utc>) -> DayRange {
        let to = zone.today(now);
        let from = to.add_days(1 - i64::from(days.max(1)));
        DayRange { from, to }
    }

    /// The whole month of `day`.
    pub fn month_of(day: LocalDay) -> DayRange {
        DayRange {
            from: day.first_of_month(),
            to: day.last_of_month(),
        }
    }

    /// How many days the range covers (1 for a single day; 0 when `to` is
    /// before `from`).
    pub fn day_count(&self) -> u32 {
        let days = (self.to.date() - self.from.date()).num_days() + 1;
        u32::try_from(days.max(0)).unwrap_or(u32::MAX)
    }

    /// Whether `day` is in the range.
    pub fn contains(&self, day: LocalDay) -> bool {
        self.from <= day && day <= self.to
    }

    /// Every day of the range, in order.
    pub fn days(&self) -> impl Iterator<Item = LocalDay> + use<> {
        let (from, to) = (self.from, self.to);
        std::iter::successors(Some(from), move |day| (*day < to).then(|| day.succ()))
            .take_while(move |day| *day <= to)
    }

    /// `(start, end)`: the half-open span of instants from the start of
    /// `from` to the end of `to`, both counted in `zone`.
    pub fn utc_bounds(&self, zone: &Zone) -> (DateTime<Utc>, DateTime<Utc>) {
        (self.from.start_in(zone), self.to.end_in(zone))
    }
}
