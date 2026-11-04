//! Scales: where a value sits, and the ticks an axis writes.
//!
//! A number axis steps by 1, 2, 2.5 or 5 times a power of ten. A time axis steps on
//! the wall clock (minutes, hours, days, weeks, months, years), so ticks fall on
//! midnights and month starts whatever the zone did that day.

use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, Timelike, Weekday};

use crate::dates::DateLocale;

/// The largest magnitude a scale works with. Values past it are drawn at the edge;
/// tables still print them in full.
pub(crate) const LIMIT: f64 = 1e300;

/// A period a time series is counted in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Grain {
    /// One minute.
    Minute,
    /// One hour.
    Hour,
    /// One day.
    #[default]
    Day,
    /// One week, from the locale's first day.
    Week,
    /// One calendar month.
    Month,
    /// Three calendar months, from January, April, July and October.
    Quarter,
    /// One calendar year.
    Year,
}

impl Grain {
    /// Every grain, finest first.
    pub const ALL: [Self; 7] = [
        Self::Minute,
        Self::Hour,
        Self::Day,
        Self::Week,
        Self::Month,
        Self::Quarter,
        Self::Year,
    ];

    /// The name a URL carries: `hour`, `day`, `week`, `month`.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Minute => "minute",
            Self::Hour => "hour",
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "month",
            Self::Quarter => "quarter",
            Self::Year => "year",
        }
    }

    /// The grain called `key`.
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|grain| grain.key() == key)
    }

    /// About how long one period is, in seconds.
    pub const fn seconds(self) -> i64 {
        match self {
            Self::Minute => 60,
            Self::Hour => 3_600,
            Self::Day => 86_400,
            Self::Week => 604_800,
            Self::Month => 2_629_746,
            Self::Quarter => 7_889_238,
            Self::Year => 31_556_952,
        }
    }

    /// The grain whose periods are about `seconds` long.
    pub(crate) fn of_spacing(seconds: i64) -> Self {
        Self::ALL
            .into_iter()
            .rev()
            .find(|grain| seconds >= grain.seconds() * 9 / 10)
            .unwrap_or(Self::Minute)
    }
}

/// A linear scale from `lo` to `hi`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Linear {
    pub lo: f64,
    pub hi: f64,
}

impl Linear {
    /// Where `value` sits, from 0 at `lo` to 1 at `hi`, clamped.
    pub fn at(&self, value: f64) -> f64 {
        let span = self.hi - self.lo;
        if !(span > 0.0 && span.is_finite()) {
            return 0.5;
        }
        let value = value.clamp(-LIMIT, LIMIT);
        ((value - self.lo) / span).clamp(0.0, 1.0)
    }
}

/// An axis's ticks: the scale's ends and the values written.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Ticks {
    pub scale: Linear,
    pub values: Vec<f64>,
    /// The decimals every tick needs.
    pub places: u8,
}

/// Ticks over `[min, max]`: about `count` of them, at nice steps, with the scale
/// widened to the first and last tick. `whole` steps by one at least.
pub(crate) fn nice(min: f64, max: f64, count: usize, whole: bool) -> Ticks {
    let (mut min, mut max) = (min.clamp(-LIMIT, LIMIT), max.clamp(-LIMIT, LIMIT));
    if !min.is_finite() || !max.is_finite() {
        (min, max) = (0.0, 1.0);
    }
    if min > max {
        std::mem::swap(&mut min, &mut max);
    }
    if max - min < f64::MIN_POSITIVE * 1e10 || (max - min) <= max.abs().max(min.abs()) * 1e-12 {
        // One value: a scale around it, from zero when it is on one side of it.
        let size = max.abs().max(min.abs());
        if size == 0.0 {
            (min, max) = (0.0, 1.0);
        } else if min > 0.0 {
            (min, max) = (0.0, max);
        } else if max < 0.0 {
            (min, max) = (min, 0.0);
        } else {
            (min, max) = (min - size, max + size);
        }
    }
    let count = count.clamp(2, 12) as f64;
    let raw = (max - min) / count;
    let mut step = nice_step(raw);
    if whole && step < 1.0 {
        step = 1.0;
    }
    if !(step > 0.0 && step.is_finite()) {
        return Ticks {
            scale: Linear { lo: min, hi: max },
            values: vec![min, max],
            places: 0,
        };
    }
    let lo = (min / step).floor() * step;
    let hi = (max / step).ceil() * step;
    let places = places_of(step);
    let mut values = Vec::new();
    let mut at = 0.0_f64;
    loop {
        let value = round(lo + at * step, places);
        if value > hi + step * 1e-9 || values.len() >= 40 {
            break;
        }
        values.push(value);
        at += 1.0;
    }
    let (lo, hi) = (
        values.first().copied().unwrap_or(lo),
        values.last().copied().unwrap_or(hi),
    );
    Ticks {
        scale: Linear {
            lo: lo.min(min),
            hi: hi.max(max),
        },
        values,
        places,
    }
}

/// 1, 2, 2.5, 5 or 10 times the power of ten nearest `raw`.
fn nice_step(raw: f64) -> f64 {
    if !(raw > 0.0 && raw.is_finite()) {
        return 0.0;
    }
    let exponent = raw.log10().floor();
    let magnitude = 10f64.powf(exponent);
    if !(magnitude > 0.0 && magnitude.is_finite()) {
        return 0.0;
    }
    let norm = raw / magnitude;
    let nice = if norm <= 1.0 {
        1.0
    } else if norm <= 2.0 {
        2.0
    } else if norm <= 2.5 {
        2.5
    } else if norm <= 5.0 {
        5.0
    } else {
        10.0
    };
    nice * magnitude
}

/// The decimals a step needs: 0.25 needs 2, 50 needs none.
fn places_of(step: f64) -> u8 {
    let mut places = 0_u8;
    let mut scaled = step;
    while places < 12 && (scaled - scaled.round()).abs() > scaled.abs() * 1e-9 {
        scaled *= 10.0;
        places += 1;
    }
    places
}

/// `value` rounded to `places` decimals, so 0.1 + 0.2 is written 0.3.
fn round(value: f64, places: u8) -> f64 {
    if places == 0 {
        return value.round();
    }
    let factor = 10f64.powi(i32::from(places));
    let out = (value * factor).round() / factor;
    if out.is_finite() { out } else { value }
}

// ---- Time ----------------------------------------------------------------

/// A wall-clock time as seconds since 1970-01-01 00:00, with no zone.
pub(crate) fn seconds_of(at: NaiveDateTime) -> i64 {
    at.and_utc().timestamp()
}

/// The wall-clock time `seconds` stands for, when chrono holds it.
pub(crate) fn time_of(seconds: i64) -> Option<NaiveDateTime> {
    DateTime::from_timestamp(seconds, 0).map(|at| at.naive_utc())
}

/// One tick of a time axis.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TimeTick {
    pub at: i64,
    pub label: String,
}

/// A step of a time axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Unit {
    Second(i64),
    Minute(i64),
    Hour(i64),
    Day(i64),
    Week,
    Month(u32),
    Year(i32),
}

impl Unit {
    const LADDER: [Self; 27] = [
        Self::Second(1),
        Self::Second(5),
        Self::Second(15),
        Self::Second(30),
        Self::Minute(1),
        Self::Minute(5),
        Self::Minute(15),
        Self::Minute(30),
        Self::Hour(1),
        Self::Hour(3),
        Self::Hour(6),
        Self::Hour(12),
        Self::Day(1),
        Self::Day(2),
        Self::Week,
        Self::Month(1),
        Self::Month(3),
        Self::Month(6),
        Self::Year(1),
        Self::Year(2),
        Self::Year(5),
        Self::Year(10),
        Self::Year(25),
        Self::Year(50),
        Self::Year(100),
        Self::Year(500),
        Self::Year(1000),
    ];

    fn seconds(self) -> i64 {
        match self {
            Self::Second(n) => n,
            Self::Minute(n) => 60 * n,
            Self::Hour(n) => 3_600 * n,
            Self::Day(n) => 86_400 * n,
            Self::Week => 604_800,
            Self::Month(n) => 2_629_746 * i64::from(n),
            Self::Year(n) => 31_556_952 * i64::from(n),
        }
    }

    /// The first tick at or after `from`.
    fn first(self, from: NaiveDateTime, week_start: Weekday) -> Option<NaiveDateTime> {
        let fixed = |n: i64| {
            let s = seconds_of(from);
            let first = s.div_euclid(n) * n;
            time_of(if first < s { first + n } else { first })
        };
        match self {
            Self::Second(n) => fixed(n),
            Self::Minute(n) => fixed(60 * n),
            Self::Hour(n) => fixed(3_600 * n),
            Self::Day(1) => fixed(86_400),
            Self::Day(n) => {
                // Every other day counted from the month's start, so the 1st is a tick.
                let mut day = midnight_at_or_after(from)?;
                while (day.day() - 1) % u32::try_from(n).ok()? != 0 {
                    day = day.checked_add_signed(Duration::days(1))?;
                }
                Some(day)
            }
            Self::Week => {
                let mut day = midnight_at_or_after(from)?;
                for _ in 0..7 {
                    if day.weekday() == week_start {
                        return Some(day);
                    }
                    day = day.checked_add_signed(Duration::days(1))?;
                }
                None
            }
            Self::Month(n) => {
                let (mut year, mut month) = (from.year(), from.month());
                let start = NaiveDate::from_ymd_opt(year, month, 1)?.and_hms_opt(0, 0, 0)?;
                if start < from {
                    (year, month) = next_month(year, month)?;
                }
                while (month - 1) % n != 0 {
                    (year, month) = next_month(year, month)?;
                }
                NaiveDate::from_ymd_opt(year, month, 1)?.and_hms_opt(0, 0, 0)
            }
            Self::Year(n) => {
                let mut year = from.year();
                if NaiveDate::from_ymd_opt(year, 1, 1)?.and_hms_opt(0, 0, 0)? < from {
                    year += 1;
                }
                year = year.div_euclid(n).checked_mul(n)?;
                if year < from.year()
                    || NaiveDate::from_ymd_opt(year, 1, 1)?.and_hms_opt(0, 0, 0)? < from
                {
                    year = year.checked_add(n)?;
                }
                NaiveDate::from_ymd_opt(year, 1, 1)?.and_hms_opt(0, 0, 0)
            }
        }
    }

    /// The tick after `at`.
    fn next(self, at: NaiveDateTime) -> Option<NaiveDateTime> {
        match self {
            Self::Second(n) => at.checked_add_signed(Duration::seconds(n)),
            Self::Minute(n) => at.checked_add_signed(Duration::minutes(n)),
            Self::Hour(n) => at.checked_add_signed(Duration::hours(n)),
            Self::Day(1) => at.checked_add_signed(Duration::days(1)),
            Self::Day(n) => {
                let next = at.checked_add_signed(Duration::days(n))?;
                // A month of 31 days restarts at the 1st rather than drawing the 31st and the 1st.
                if next.month() != at.month() && next.day() != 1 {
                    let (year, month) = next_month(at.year(), at.month())?;
                    NaiveDate::from_ymd_opt(year, month, 1)?.and_hms_opt(0, 0, 0)
                } else {
                    Some(next)
                }
            }
            Self::Week => at.checked_add_signed(Duration::days(7)),
            Self::Month(n) => {
                let (mut year, mut month) = (at.year(), at.month());
                for _ in 0..n {
                    (year, month) = next_month(year, month)?;
                }
                NaiveDate::from_ymd_opt(year, month, 1)?.and_hms_opt(0, 0, 0)
            }
            Self::Year(n) => {
                NaiveDate::from_ymd_opt(at.year().checked_add(n)?, 1, 1)?.and_hms_opt(0, 0, 0)
            }
        }
    }
}

fn next_month(year: i32, month: u32) -> Option<(i32, u32)> {
    if month >= 12 {
        Some((year.checked_add(1)?, 1))
    } else {
        Some((year, month + 1))
    }
}

fn midnight_at_or_after(at: NaiveDateTime) -> Option<NaiveDateTime> {
    let midnight = at.date().and_hms_opt(0, 0, 0)?;
    if midnight < at {
        at.date().succ_opt()?.and_hms_opt(0, 0, 0)
    } else {
        Some(midnight)
    }
}

/// How a time reads at a grain: `14:00`, `Nov 3`, `Nov 2026`, `2026`.
pub(crate) fn time_label(
    at: NaiveDateTime,
    grain: Grain,
    locale: &DateLocale,
    with_year: bool,
) -> String {
    let date = at.date();
    match grain {
        Grain::Minute | Grain::Hour => {
            if at.time() == chrono::NaiveTime::MIN {
                locale.format_short(date, with_year)
            } else {
                locale.format_time(at.time(), false)
            }
        }
        Grain::Day | Grain::Week => locale.format_short(date, with_year),
        Grain::Month | Grain::Quarter => {
            if with_year {
                format!("{} {}", locale.month_short(date.month()), date.year())
            } else {
                locale.month_short(date.month()).to_owned()
            }
        }
        Grain::Year => date.year().to_string(),
    }
}

/// A time as a table row or a tooltip names it: the whole date, and the time when
/// the grain is finer than a day.
pub(crate) fn time_name(at: NaiveDateTime, grain: Grain, locale: &DateLocale) -> String {
    let date = at.date();
    match grain {
        Grain::Minute | Grain::Hour => format!(
            "{}, {}",
            locale.format_short(date, true),
            locale.format_time(at.time(), at.second() != 0)
        ),
        Grain::Day | Grain::Week => locale.format_short(date, true),
        Grain::Month => locale.format_caption(date.year(), date.month()),
        Grain::Quarter => format!("Q{} {}", (date.month() - 1) / 3 + 1, date.year()),
        Grain::Year => date.year().to_string(),
    }
}

/// Ticks for a time axis from `from` to `to` (wall-clock seconds), at most `most`.
pub(crate) fn time_ticks(
    from: i64,
    to: i64,
    most: usize,
    locale: &DateLocale,
    week_start: Weekday,
) -> Vec<TimeTick> {
    let (from, to) = (from.min(to), from.max(to));
    let (Some(start), Some(_)) = (time_of(from), time_of(to)) else {
        return Vec::new();
    };
    let span = to.saturating_sub(from).max(1);
    let most = i64::try_from(most.max(2)).unwrap_or(2);
    let unit = Unit::LADDER
        .into_iter()
        .find(|unit| span / unit.seconds() < most)
        .unwrap_or(Unit::Year(1000));
    let grain = match unit {
        Unit::Second(_) | Unit::Minute(_) => Grain::Minute,
        Unit::Hour(_) => Grain::Hour,
        Unit::Day(_) => Grain::Day,
        Unit::Week => Grain::Week,
        Unit::Month(_) => Grain::Month,
        Unit::Year(_) => Grain::Year,
    };
    let spans_years = time_of(from).map(|t| t.year()) != time_of(to).map(|t| t.year());
    let mut out = Vec::new();
    let mut at = unit.first(start, week_start);
    let mut last_year = None;
    while let Some(tick) = at {
        let seconds = seconds_of(tick);
        if seconds > to || out.len() >= 40 {
            break;
        }
        let with_year = spans_years
            && match grain {
                Grain::Month | Grain::Quarter => last_year != Some(tick.year()),
                Grain::Minute | Grain::Hour => false,
                _ => last_year != Some(tick.year()),
            };
        let mut label = time_label(tick, grain, locale, with_year);
        if let Unit::Second(_) = unit
            && tick.time() != chrono::NaiveTime::MIN
        {
            label = locale.format_time(tick.time(), true);
        }
        last_year = Some(tick.year());
        out.push(TimeTick { at: seconds, label });
        at = unit.next(tick);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(ticks: &Ticks) -> Vec<f64> {
        ticks.values.clone()
    }

    #[test]
    fn number_ticks_are_nice() {
        assert_eq!(
            values(&nice(0.0, 97.0, 5, false)),
            [0.0, 20.0, 40.0, 60.0, 80.0, 100.0]
        );
        assert_eq!(
            values(&nice(0.0, 1.0, 4, false)),
            [0.0, 0.25, 0.5, 0.75, 1.0]
        );
        assert_eq!(nice(0.0, 1.0, 4, false).places, 2);
        assert_eq!(
            values(&nice(-42.0, 17.0, 5, false)),
            [-60.0, -40.0, -20.0, 0.0, 20.0]
        );
        assert_eq!(values(&nice(0.0, 0.3, 3, false)), [0.0, 0.1, 0.2, 0.3]);
        assert_eq!(values(&nice(0.0, 2.0, 5, true)), [0.0, 1.0, 2.0]);
        assert_eq!(
            values(&nice(0.0, 1_000_000.0, 4, false)),
            [0.0, 250_000.0, 500_000.0, 750_000.0, 1_000_000.0]
        );
    }

    #[test]
    fn one_value_gets_a_scale_around_it() {
        assert_eq!(
            values(&nice(0.0, 0.0, 4, false)),
            [0.0, 0.25, 0.5, 0.75, 1.0]
        );
        let t = nice(5.0, 5.0, 4, false);
        assert!(t.scale.lo <= 0.0 && t.scale.hi >= 5.0, "{t:?}");
        let t = nice(-5.0, -5.0, 4, false);
        assert!(t.scale.lo <= -5.0 && t.scale.hi >= 0.0, "{t:?}");
    }

    #[test]
    fn extremes_give_a_scale_and_never_loop() {
        for (a, b) in [
            (f64::MIN, f64::MAX),
            (-1e308, 1e308),
            (0.0, 1e-300),
            (1e-9, 2e-9),
            (f64::NAN, 1.0),
            (1.0, f64::INFINITY),
            (5.0, -5.0),
            (1e15, 1e15 + 1.0),
            (0.1 + 0.2, 0.3),
        ] {
            let t = nice(a, b, 5, false);
            assert!(
                !t.values.is_empty() && t.values.len() <= 40,
                "{a} {b}: {t:?}"
            );
            assert!(t.scale.lo.is_finite() && t.scale.hi.is_finite(), "{a} {b}");
            assert!(t.scale.lo <= t.scale.hi, "{a} {b}");
            let at = t.scale.at(f64::MAX);
            assert!((0.0..=1.0).contains(&at));
        }
    }

    fn at(y: i32, m: u32, d: u32, h: u32) -> i64 {
        seconds_of(
            NaiveDate::from_ymd_opt(y, m, d)
                .unwrap()
                .and_hms_opt(h, 0, 0)
                .unwrap(),
        )
    }

    fn labels(from: i64, to: i64, most: usize) -> Vec<String> {
        time_ticks(from, to, most, &DateLocale::of("en-GB"), Weekday::Mon)
            .into_iter()
            .map(|t| t.label)
            .collect()
    }

    #[test]
    fn hours_fall_on_the_hour_and_midnight_names_the_day() {
        assert_eq!(
            labels(at(2026, 11, 3, 9) + 600, at(2026, 11, 4, 3), 6),
            ["12:00", "15:00", "18:00", "21:00", "4 Nov", "03:00"]
        );
    }

    #[test]
    fn days_weeks_months_and_years() {
        assert_eq!(
            labels(at(2026, 11, 1, 0), at(2026, 11, 7, 0), 8),
            [
                "1 Nov", "2 Nov", "3 Nov", "4 Nov", "5 Nov", "6 Nov", "7 Nov"
            ]
        );
        let weeks = labels(at(2026, 10, 1, 0), at(2026, 11, 30, 0), 10);
        assert_eq!(weeks[0], "5 Oct", "{weeks:?}");
        let months = labels(at(2025, 11, 15, 0), at(2026, 10, 1, 0), 12);
        assert_eq!(months[0], "Dec 2025", "{months:?}");
        assert!(months.contains(&"Jan 2026".to_owned()), "{months:?}");
        assert!(months.contains(&"Feb".to_owned()), "{months:?}");
        let years = labels(at(1990, 1, 1, 0), at(2026, 1, 1, 0), 8);
        assert_eq!(
            years,
            [
                "1990", "1995", "2000", "2005", "2010", "2015", "2020", "2025"
            ]
        );
    }

    #[test]
    fn time_ticks_never_panic_at_the_edges() {
        for (a, b) in [
            (i64::MIN, i64::MAX),
            (0, 0),
            (i64::MAX - 5, i64::MAX),
            (-62_000_000_000, 250_000_000_000),
            (5, -5),
        ] {
            let ticks = time_ticks(a, b, 6, &DateLocale::english(), Weekday::Sun);
            assert!(ticks.len() <= 40);
        }
    }

    #[test]
    fn grains_have_keys_and_spacings() {
        for grain in Grain::ALL {
            assert_eq!(Grain::from_key(grain.key()), Some(grain));
            assert_eq!(Grain::of_spacing(grain.seconds()), grain);
        }
        assert_eq!(
            Grain::of_spacing(3_600 * 23),
            Grain::Day,
            "a 23-hour day is a day"
        );
        assert_eq!(
            Grain::of_spacing(86_400 * 28),
            Grain::Month,
            "February is a month"
        );
    }
}
