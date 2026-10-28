//! [`CronExpression`]: Navree's own cron parser and next-run search.
//!
//! The accepted language is five fields (`minute hour day-of-month month
//! day-of-week`, separated by blanks) or one nickname:
//!
//! | Field | Values | Names | Also accepted |
//! |---|---|---|---|
//! | minute | 0-59 | | |
//! | hour | 0-23 | | |
//! | day of month | 1-31 | | `L`, `LW`, `15W`, `?` |
//! | month | 1-12 | `JAN`-`DEC` | |
//! | day of week | 0-7 (0 and 7 are Sunday) | `SUN`-`SAT` | `5L`, `5#2`, `5#L`, `?`, `+` prefix |
//!
//! Every field takes `*`, a value, a range `a-b`, a step `*/s` or `a-b/s`,
//! and comma lists of those. The nicknames are `@yearly`, `@annually`,
//! `@monthly`, `@weekly`, `@daily` and `@hourly`. Case does not matter.
//! A seconds field, a year field and everything else is rejected with an
//! error that names the field and the bad token.
//!
//! A day matches when its day of month **or** its day of week matches
//! (unless one of the two is `*`, which is then ignored). A `+` before the
//! day-of-week field (`0 0 1-7 * +MON`) asks for both.
//!
//! # Daylight saving time
//!
//! Expressions are read on a wall clock, and the rule for a clock change
//! depends on the expression.
//!
//! - **Fixed-time expressions** have no `*` in the minute or the hour field
//!   (`30 2 * * *`, `0 0 1 * *`, `@daily`; `*/n` counts as a `*`). A local
//!   time that is skipped when clocks go forward runs once, at the first
//!   minute after the gap. A local time that happens twice when clocks go
//!   back runs once, the first time.
//! - **Wildcard expressions** have a `*` in the minute or the hour field
//!   (`* * * * *`, `*/15 * * * *`, `0 * * * *`, `@hourly`). They follow the
//!   wall clock as it happens: every real minute whose local time matches is
//!   a run, so in a repeated hour they run in both passes, and a local time
//!   inside a gap does not exist, so nothing is caught up for it.
//!
//! [`CronExpression::is_due`] is the rule, and [`CronExpression::next_after`]
//! finds the instants it accepts, so `schedule:list` and real runs agree.
//!
//! # Bounds
//!
//! An expression is at most [`MAX_EXPRESSION_LEN`] bytes. The search for the
//! next run looks at most 400 years ahead (the Gregorian calendar repeats
//! every 400 years, so a longer search could not find anything new): an
//! expression that never matches, such as `0 0 30 2 *`, has no next run.

use std::fmt;
use std::str::FromStr;

use chrono::{
    DateTime, Datelike, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, Offset, TimeDelta,
    TimeZone, Timelike, Utc, Weekday,
};

/// The longest expression accepted, in bytes.
pub const MAX_EXPRESSION_LEN: usize = 256;

/// How many years ahead the search for a next run looks.
const SEARCH_YEARS: i32 = 400;

/// A parsed cron expression. See the [module documentation](self) for the
/// accepted language and the daylight saving time rules.
///
/// ```
/// use chrono::{TimeZone, Utc};
/// use navree_schedule::CronExpression;
///
/// let cron: CronExpression = "*/15 9-17 * * MON-FRI".parse().unwrap();
/// let after = Utc.with_ymd_and_hms(2026, 10, 26, 9, 7, 0).unwrap();
/// let next = cron.next_after(after, &Utc).unwrap();
/// assert_eq!(next, Utc.with_ymd_and_hms(2026, 10, 26, 9, 15, 0).unwrap());
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CronExpression {
    source: String,
    /// Bit `m` is set when minute `m` matches.
    minutes: u64,
    /// Bit `h` is set when hour `h` matches.
    hours: u32,
    /// Bit `m` is set when month `m` (1-12) matches.
    months: u16,
    dom: DayOfMonth,
    dow: DayOfWeek,
    /// A day must match both day fields (`+` before the day of week).
    both_days: bool,
    /// A `*` in the minute or the hour field: the expression follows the
    /// wall clock through clock changes.
    wildcard_time: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct DayOfMonth {
    /// The field is exactly `*` (or `?`).
    star: bool,
    /// Bit `d` is set when day `d` (1-31) matches.
    days: u32,
    /// `L`: the last day of the month.
    last: bool,
    /// `LW`: the last weekday (Monday to Friday) of the month.
    last_weekday: bool,
    /// Bit `d` is set for `dW`: the weekday nearest to day `d`.
    nearest_weekday: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct DayOfWeek {
    /// The field is exactly `*` (or `?`).
    star: bool,
    /// Bit `w` (0 = Sunday) is set when weekday `w` matches every week.
    every: u8,
    /// `nth[w]` has bit `n` (1-5) set for `w#n`.
    nth: [u8; 7],
    /// Bit `w` is set for `wL`: the last such weekday of the month.
    last: u8,
}

/// Why an expression was rejected: the field, the bad token and the reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CronError {
    field: &'static str,
    token: String,
    reason: String,
}

impl CronError {
    fn new(field: &'static str, token: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            field,
            token: token.into(),
            reason: reason.into(),
        }
    }

    /// The field that holds the bad token: `minute`, `hour`, `day of month`,
    /// `month`, `day of week`, or `expression` for problems with the
    /// expression as a whole.
    pub fn field(&self) -> &'static str {
        self.field
    }

    /// The bad token as written.
    pub fn token(&self) -> &str {
        &self.token
    }

    /// What is wrong with it.
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

impl fmt::Display for CronError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.token.is_empty() {
            write!(f, "{}: {}", self.field, self.reason)
        } else {
            write!(f, "{} field: `{}` {}", self.field, self.token, self.reason)
        }
    }
}

impl std::error::Error for CronError {}

const MINUTE: Field = Field {
    name: "minute",
    min: 0,
    max: 59,
    names: &[],
};
const HOUR: Field = Field {
    name: "hour",
    min: 0,
    max: 23,
    names: &[],
};
const DAY_OF_MONTH: Field = Field {
    name: "day of month",
    min: 1,
    max: 31,
    names: &[],
};
const MONTH: Field = Field {
    name: "month",
    min: 1,
    max: 12,
    names: &[
        ("JAN", 1),
        ("FEB", 2),
        ("MAR", 3),
        ("APR", 4),
        ("MAY", 5),
        ("JUN", 6),
        ("JUL", 7),
        ("AUG", 8),
        ("SEP", 9),
        ("OCT", 10),
        ("NOV", 11),
        ("DEC", 12),
    ],
};
const DAY_OF_WEEK: Field = Field {
    name: "day of week",
    min: 0,
    max: 7,
    names: &[
        ("SUN", 0),
        ("MON", 1),
        ("TUE", 2),
        ("WED", 3),
        ("THU", 4),
        ("FRI", 5),
        ("SAT", 6),
    ],
};

/// The bounds and names of one field.
struct Field {
    name: &'static str,
    min: u32,
    max: u32,
    names: &'static [(&'static str, u32)],
}

impl Field {
    /// The values of `token` (a number or a name). `range_end` makes `SUN`
    /// mean 7, so `FRI-SUN` is Friday to Sunday.
    fn value(&self, token: &str, range_end: bool) -> Result<u32, CronError> {
        if !token.is_empty() && token.bytes().all(|b| b.is_ascii_digit()) {
            let value = if token.len() <= 9 {
                token.parse::<u32>().ok()
            } else {
                None
            };
            return match value {
                Some(value) if (self.min..=self.max).contains(&value) => Ok(value),
                _ => Err(CronError::new(
                    self.name,
                    token,
                    format!("is out of range, use {} to {}", self.min, self.max),
                )),
            };
        }
        if let Some(&(_, value)) = self.names.iter().find(|(name, _)| *name == token) {
            return Ok(if range_end && self.max == 7 && value == 0 {
                7
            } else {
                value
            });
        }
        let expected = if self.names.is_empty() {
            "a number".to_owned()
        } else {
            let first = self.names.first().map_or("", |n| n.0);
            let last = self.names.last().map_or("", |n| n.0);
            format!("a number or a name ({first} to {last})")
        };
        Err(CronError::new(
            self.name,
            token,
            format!("is not {expected}"),
        ))
    }

    /// A step: digits, at least 1.
    fn step(&self, token: &str, item: &str) -> Result<u32, CronError> {
        let bad = |reason: &str| CronError::new(self.name, item, reason);
        if token.is_empty() || !token.bytes().all(|b| b.is_ascii_digit()) {
            return Err(bad("has a step that is not a number"));
        }
        match token.parse::<u16>() {
            Ok(0) => Err(bad("has a step of 0, use 1 or more")),
            Ok(step) => Ok(u32::from(step)),
            Err(_) => Err(bad("has a step that is too large")),
        }
    }

    /// The values of a plain item: `*`, `*/s`, `a`, `a-b` or `a-b/s`.
    fn plain(&self, item: &str) -> Result<Vec<u32>, CronError> {
        let (range, step) = match item.split_once('/') {
            Some((range, step)) => (range, Some(self.step(step, item)?)),
            None => (item, None),
        };
        let (start, end) = if range == "*" {
            (self.min, self.max)
        } else if let Some((start, end)) = range.split_once('-') {
            if end.contains('-') {
                return Err(CronError::new(
                    self.name,
                    item,
                    "has more than one `-`, a range is `a-b`",
                ));
            }
            let (start, end) = (self.value(start, false)?, self.value(end, true)?);
            if start > end {
                return Err(CronError::new(
                    self.name,
                    item,
                    format!(
                        "is a range that runs backwards, write it as two ranges such as {start}-{max},{min}-{end}",
                        max = self.max,
                        min = self.min,
                    ),
                ));
            }
            (start, end)
        } else if step.is_some() {
            return Err(CronError::new(
                self.name,
                item,
                format!(
                    "needs `*` or a range before `/`, such as `*/{s}` or `{a}-{b}/{s}`",
                    s = step.unwrap_or(1),
                    a = self.min,
                    b = self.max,
                ),
            ));
        } else {
            let value = self.value(range, false)?;
            return Ok(vec![value]);
        };
        let step = step.unwrap_or(1) as usize;
        Ok((start..=end).step_by(step).collect())
    }
}

/// Splits a field at commas, rejecting empty items.
fn items<'a>(field: &Field, text: &'a str) -> Result<impl Iterator<Item = &'a str>, CronError> {
    if text.split(',').any(str::is_empty) {
        return Err(CronError::new(
            field.name,
            text,
            "has an empty item, check for a stray comma",
        ));
    }
    if text.contains(',') && text.split(',').any(|item| item == "*" || item == "?") {
        return Err(CronError::new(
            field.name,
            text.split(',')
                .find(|i| *i == "*" || *i == "?")
                .unwrap_or("*"),
            "cannot be in a list, `*` already means every value",
        ));
    }
    Ok(text.split(','))
}

fn bits(values: impl IntoIterator<Item = u32>) -> u64 {
    values.into_iter().fold(0, |mask, v| mask | (1 << v))
}

impl FromStr for CronExpression {
    type Err = CronError;

    fn from_str(text: &str) -> Result<Self, CronError> {
        Self::parse(text)
    }
}

impl fmt::Display for CronExpression {
    /// The expression as it was written (trimmed).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.source)
    }
}

impl CronExpression {
    /// Parses `text`. Anything outside the accepted language (see the
    /// [module documentation](self)) is an error naming the field and the
    /// bad token.
    pub fn parse(text: &str) -> Result<Self, CronError> {
        let trimmed = text.trim_matches(is_blank);
        if trimmed.len() > MAX_EXPRESSION_LEN {
            return Err(CronError::new(
                "expression",
                "",
                format!(
                    "is {} bytes long, the limit is {MAX_EXPRESSION_LEN}",
                    trimmed.len()
                ),
            ));
        }
        if trimmed.is_empty() {
            return Err(CronError::new("expression", "", "is empty"));
        }
        if !trimmed.is_ascii() {
            let bad = trimmed.chars().find(|c| !c.is_ascii()).unwrap_or('?');
            return Err(CronError::new(
                "expression",
                bad.to_string(),
                "is not an ASCII character",
            ));
        }
        Self::parse_ascii(trimmed).map_err(|mut error| {
            // Report the token as the user wrote it.
            let upper = trimmed.to_ascii_uppercase();
            if let Some(at) = upper.find(&error.token) {
                error.token = trimmed[at..at + error.token.len()].to_owned();
            }
            error
        })
    }

    fn parse_ascii(trimmed: &str) -> Result<Self, CronError> {
        let upper = trimmed.to_ascii_uppercase();
        let tokens: Vec<&str> = upper.split(is_blank).filter(|t| !t.is_empty()).collect();
        if tokens.first().is_some_and(|t| t.starts_with('@')) {
            let [nickname] = tokens.as_slice() else {
                return Err(CronError::new(
                    "expression",
                    tokens.join(" "),
                    "has text after the nickname, a nickname stands alone",
                ));
            };
            let expanded = match *nickname {
                "@YEARLY" | "@ANNUALLY" => "0 0 1 1 *",
                "@MONTHLY" => "0 0 1 * *",
                "@WEEKLY" => "0 0 * * SUN",
                "@DAILY" => "0 0 * * *",
                "@HOURLY" => "0 * * * *",
                other => {
                    return Err(CronError::new(
                        "expression",
                        other,
                        "is not a nickname, use @yearly, @annually, @monthly, @weekly, @daily or @hourly",
                    ));
                }
            };
            let tokens: Vec<&str> = expanded.split(' ').collect();
            return Self::from_fields(trimmed, &tokens);
        }
        Self::from_fields(trimmed, &tokens)
    }

    fn from_fields(source: &str, tokens: &[&str]) -> Result<Self, CronError> {
        let [minute, hour, dom, month, dow] = tokens else {
            let detail = if tokens.len() == 6 || tokens.len() == 7 {
                ", a seconds or year field is not accepted"
            } else {
                ""
            };
            return Err(CronError::new(
                "expression",
                "",
                format!(
                    "has {} fields, expected 5 (minute hour day-of-month month day-of-week){detail}",
                    tokens.len()
                ),
            ));
        };
        let wildcard_time = minute.contains('*') || hour.contains('*');
        let minutes = Self::list(&MINUTE, minute)?;
        let hours = Self::list(&HOUR, hour)?;
        let months = Self::list(&MONTH, month)?;
        let dom = Self::day_of_month(dom)?;
        let (dow, both_days) = Self::day_of_week(dow)?;
        Ok(Self {
            source: source.to_owned(),
            minutes,
            hours: hours as u32,
            months: months as u16,
            dom,
            dow,
            both_days,
            wildcard_time,
        })
    }

    /// A field made of plain items only.
    fn list(field: &Field, text: &str) -> Result<u64, CronError> {
        let mut mask = 0;
        for item in items(field, text)? {
            mask |= bits(field.plain(item)?);
        }
        Ok(mask)
    }

    fn day_of_month(text: &str) -> Result<DayOfMonth, CronError> {
        let field = &DAY_OF_MONTH;
        let mut dom = DayOfMonth {
            star: text == "*" || text == "?",
            days: 0,
            last: false,
            last_weekday: false,
            nearest_weekday: 0,
        };
        if text == "?" {
            dom.days = (1..=31).fold(0, |mask, d| mask | (1 << d));
            return Ok(dom);
        }
        for item in items(field, text)? {
            if item == "L" {
                dom.last = true;
            } else if item == "LW" {
                dom.last_weekday = true;
            } else if let Some(day) = item.strip_suffix('W') {
                let day = field.value(day, false).map_err(|_| {
                    CronError::new(
                        field.name,
                        item,
                        "is not a day followed by W, such as 15W (the weekday nearest the 15th)",
                    )
                })?;
                dom.nearest_weekday |= 1 << day;
            } else {
                dom.days |= bits(field.plain(item)?) as u32;
            }
        }
        Ok(dom)
    }

    fn day_of_week(text: &str) -> Result<(DayOfWeek, bool), CronError> {
        let field = &DAY_OF_WEEK;
        let (text, both) = match text.strip_prefix('+') {
            Some(rest) => (rest, true),
            None => (text, false),
        };
        let mut dow = DayOfWeek {
            star: text == "*" || text == "?",
            every: 0,
            nth: [0; 7],
            last: 0,
        };
        if text == "?" {
            dow.every = 0x7f;
            return Ok((dow, both));
        }
        for item in items(field, text)? {
            if item == "L" {
                return Err(CronError::new(
                    field.name,
                    item,
                    "needs a weekday before it, such as 5L for the last Friday of the month",
                ));
            }
            if let Some((day, nth)) = item.split_once('#') {
                let weekday = field.value(day, false)? % 7;
                match nth {
                    "L" => dow.last |= 1 << weekday,
                    "1" | "2" | "3" | "4" | "5" => {
                        dow.nth[weekday as usize] |= 1 << (nth.as_bytes()[0] - b'0');
                    }
                    _ => {
                        return Err(CronError::new(
                            field.name,
                            item,
                            "needs 1 to 5 or L after `#`, such as MON#2 for the second Monday",
                        ));
                    }
                }
            } else if let Some(day) = item.strip_suffix('L') {
                let weekday = field.value(day, false).map_err(|_| {
                    CronError::new(
                        field.name,
                        item,
                        "is not a weekday followed by L, such as 5L for the last Friday",
                    )
                })? % 7;
                dow.last |= 1 << weekday;
            } else {
                for value in field.plain(item)? {
                    dow.every |= 1 << (value % 7);
                }
            }
        }
        Ok((dow, both))
    }

    /// Whether the wall-clock time `local` (seconds are ignored) matches
    /// the expression. This is the plain match; it knows nothing about
    /// daylight saving time.
    pub fn matches_local(&self, local: NaiveDateTime) -> bool {
        self.minutes >> local.minute() & 1 == 1
            && self.hours >> local.hour() & 1 == 1
            && self.day_matches(local.date())
    }

    fn day_matches(&self, date: NaiveDate) -> bool {
        if self.months >> date.month() & 1 == 0 {
            return false;
        }
        let day = date.day();
        let weekday = date.weekday().num_days_from_sunday();
        let month_length = days_in_month(date.year(), date.month());

        let dom = &self.dom;
        let dom_matches = dom.days >> day & 1 == 1
            || (dom.last && day == month_length)
            || (dom.last_weekday
                && day == last_weekday_of(date.year(), date.month(), month_length))
            || (dom.nearest_weekday != 0
                && (day.saturating_sub(2).max(1)..=(day + 2).min(month_length)).any(|pattern| {
                    dom.nearest_weekday >> pattern & 1 == 1
                        && nearest_weekday(date.year(), date.month(), month_length, pattern) == day
                }));

        let dow = &self.dow;
        let dow_matches = dow.every >> weekday & 1 == 1
            || dow.nth[weekday as usize] >> ((day - 1) / 7 + 1) & 1 == 1
            || (dow.last >> weekday & 1 == 1 && day + 7 > month_length);

        if self.dom.star || self.dow.star {
            // A `*` field is ignored.
            (dom_matches && self.dow.star) || (dow_matches && self.dom.star)
        } else if self.both_days {
            dom_matches && dow_matches
        } else {
            dom_matches || dow_matches
        }
    }

    /// The first wall-clock time at or after `from` (seconds are ignored)
    /// that matches, looking at most [`SEARCH_YEARS`] ahead.
    fn next_local(&self, from: NaiveDateTime) -> Option<NaiveDateTime> {
        let mut date = from.date();
        let last_year = date.year().checked_add(SEARCH_YEARS)?;
        let mut earliest = Some((from.hour(), from.minute()));
        while date.year() <= last_year {
            if self.months >> date.month() & 1 == 0 {
                date = first_of_next_month(date)?;
                earliest = None;
                continue;
            }
            if self.day_matches(date) {
                let (hour, minute) = earliest.unwrap_or((0, 0));
                if let Some((hour, minute)) = self.time_from(hour, minute) {
                    return Some(date.and_time(NaiveTime::from_hms_opt(hour, minute, 0)?));
                }
            }
            date = date.succ_opt()?;
            earliest = None;
        }
        None
    }

    /// The first matching `(hour, minute)` at or after `hour:minute`.
    fn time_from(&self, hour: u32, minute: u32) -> Option<(u32, u32)> {
        let in_hour = self.minutes >> minute << minute;
        if self.hours >> hour & 1 == 1 && in_hour != 0 {
            return Some((hour, in_hour.trailing_zeros()));
        }
        let later = (self.hours >> hour >> 1) << hour << 1;
        if later == 0 || self.minutes == 0 {
            return None;
        }
        Some((later.trailing_zeros(), self.minutes.trailing_zeros()))
    }

    /// Whether the expression follows the wall clock through clock changes
    /// (a `*` in the minute or the hour field) rather than running each
    /// local time once. See the [module documentation](self).
    pub fn follows_wall_clock(&self) -> bool {
        self.wildcard_time
    }

    /// The first run strictly after `after` on `zone`'s clock, as an
    /// instant, or `None` when the expression never matches within the next
    /// 400 years (for example `0 0 30 2 *`).
    ///
    /// Clock changes follow [`is_due`](Self::is_due): a fixed-time
    /// expression runs a skipped local time once, at the first minute after
    /// the gap, and a repeated one once, the first time; a wildcard
    /// expression runs at every real minute whose local time matches.
    pub fn next_after<Tz: TimeZone>(
        &self,
        after: DateTime<Utc>,
        zone: &Tz,
    ) -> Option<DateTime<Utc>> {
        let next = if self.wildcard_time {
            self.next_on_the_wall_clock(after, zone)
        } else {
            self.next_fixed_time(after, zone)
        };
        debug_assert!(next.is_none_or(|run| run > after && self.is_due(run, zone)));
        next
    }

    fn next_fixed_time<Tz: TimeZone>(
        &self,
        after: DateTime<Utc>,
        zone: &Tz,
    ) -> Option<DateTime<Utc>> {
        let local = after.with_timezone(zone).naive_local();
        let mut from = local
            .with_second(0)
            .and_then(|t| t.with_nanosecond(0))
            .unwrap_or(local);
        // Candidates before `after` exist only in the second pass of a
        // repeated hour or among times skipped by one gap, so a few
        // hundred steps are far more than ever needed.
        for _ in 0..4096 {
            let candidate = self.next_local(from)?;
            let instant = instant_of(zone, candidate)?;
            if instant > after {
                return Some(instant);
            }
            from = candidate.checked_add_signed(TimeDelta::minutes(1))?;
        }
        None
    }

    /// The first real minute after `after` whose local time matches. The
    /// search jumps to the next matching local time while the UTC offset
    /// stays the same, and looks at the minute where the offset changes,
    /// because the local clock jumps there.
    fn next_on_the_wall_clock<Tz: TimeZone>(
        &self,
        after: DateTime<Utc>,
        zone: &Tz,
    ) -> Option<DateTime<Utc>> {
        let offset_at = |seconds: i64| -> Option<i64> {
            let utc = DateTime::from_timestamp(seconds, 0)?.naive_utc();
            Some(i64::from(
                zone.offset_from_utc_datetime(&utc).fix().local_minus_utc(),
            ))
        };
        let mut t = after.timestamp().div_euclid(60) * 60 + 60;
        // A jump never crosses a day, so a clock change is never skipped.
        let day = 24 * 3600;
        // Each pass either returns, moves to a clock change, or gives up
        // after the 400 years `next_local` looks at.
        for _ in 0..1_000_000 {
            let offset = offset_at(t)?;
            let local = DateTime::from_timestamp(t + offset, 0)?.naive_utc();
            if self.matches_local(local) {
                return DateTime::from_timestamp(t, 0);
            }
            let target = self.next_local(local)?;
            let target_t = t + (target - local).num_seconds();
            let mut cursor = t;
            loop {
                let next = target_t.min(cursor + day);
                if offset_at(next)? != offset {
                    // A clock change: find its first minute and look there.
                    let (mut behind, mut ahead) = (cursor, next);
                    while ahead - behind > 60 {
                        let middle = behind + (ahead - behind) / 120 * 60;
                        if offset_at(middle)? == offset {
                            behind = middle;
                        } else {
                            ahead = middle;
                        }
                    }
                    t = ahead;
                    break;
                }
                if next == target_t {
                    return DateTime::from_timestamp(next, 0);
                }
                cursor = next;
            }
        }
        None
    }

    /// Whether the expression is due at `tick`, a whole minute (UTC), on
    /// `zone`'s clock. This is the rule for clock changes.
    ///
    /// A fixed-time expression (no `*` in the minute or hour field) is due
    /// once for a local time that happens twice (the first time) and once,
    /// at the first minute after the gap, for local times that are skipped.
    /// A wildcard expression is due at every real minute whose local time
    /// matches: in both passes of a repeated hour, and never for a time
    /// inside a gap.
    pub fn is_due<Tz: TimeZone>(&self, tick: DateTime<Utc>, zone: &Tz) -> bool {
        let local = tick.with_timezone(zone).naive_local();
        if self.wildcard_time {
            return self.matches_local(local);
        }
        if self.matches_local(local) {
            return match zone.from_local_datetime(&local) {
                LocalResult::Ambiguous(earliest, _) => earliest.to_utc() == tick,
                _ => true,
            };
        }
        let Some(before) = tick.checked_sub_signed(TimeDelta::minutes(1)) else {
            return false;
        };
        let previous = before.with_timezone(zone).naive_local();
        let mut skipped = previous + TimeDelta::minutes(1);
        while skipped < local {
            if self.matches_local(skipped) {
                return true;
            }
            skipped += TimeDelta::minutes(1);
        }
        false
    }
}

fn is_blank(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c')
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        _ => 28,
    }
}

fn first_of_next_month(date: NaiveDate) -> Option<NaiveDate> {
    if date.month() == 12 {
        NaiveDate::from_ymd_opt(date.year().checked_add(1)?, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(date.year(), date.month() + 1, 1)
    }
}

fn is_weekday(date: NaiveDate) -> bool {
    !matches!(date.weekday(), Weekday::Sat | Weekday::Sun)
}

/// The last Monday-to-Friday day of the month.
fn last_weekday_of(year: i32, month: u32, month_length: u32) -> u32 {
    let mut day = month_length;
    while let Some(date) = NaiveDate::from_ymd_opt(year, month, day) {
        if is_weekday(date) || day == 1 {
            break;
        }
        day -= 1;
    }
    day
}

/// The day `dW` resolves to: the weekday nearest to `day`, staying in the
/// month (a Saturday moves back to Friday, or on to Monday for the 1st; a
/// Sunday moves on to Monday, or back to Friday for the last day).
fn nearest_weekday(year: i32, month: u32, month_length: u32, day: u32) -> u32 {
    let Some(date) = NaiveDate::from_ymd_opt(year, month, day) else {
        return 0;
    };
    match date.weekday().num_days_from_sunday() {
        6 if day > 1 => day - 1,
        6 => day + 2,
        0 if day < month_length => day + 1,
        0 => day - 2,
        _ => day,
    }
}

/// The instant `local` happens on `zone`'s clock: the first one when it
/// happens twice, and the first minute after the gap when it is skipped.
fn instant_of<Tz: TimeZone>(zone: &Tz, local: NaiveDateTime) -> Option<DateTime<Utc>> {
    match zone.from_local_datetime(&local) {
        LocalResult::Single(at) => Some(at.to_utc()),
        LocalResult::Ambiguous(first, _) => Some(first.to_utc()),
        LocalResult::None => gap_end(zone, local),
    }
}

/// The first whole minute at which `zone`'s clock reads `local` or later,
/// for a `local` inside a gap. Binary search over instants: offsets stay
/// within 26 hours of zero, so the clock is behind `local` 26 hours before
/// `local` read as UTC, and ahead of it 26 hours after.
fn gap_end<Tz: TimeZone>(zone: &Tz, local: NaiveDateTime) -> Option<DateTime<Utc>> {
    let reads = |seconds: i64| -> Option<bool> {
        let utc = DateTime::from_timestamp(seconds, 0)?;
        Some(utc.with_timezone(zone).naive_local() >= local)
    };
    let centre = local.and_utc().timestamp();
    let (mut behind, mut ahead) = (centre - 26 * 3600, centre + 26 * 3600);
    if reads(behind)? || !reads(ahead)? {
        return None;
    }
    while ahead - behind > 1 {
        let middle = behind + (ahead - behind) / 2;
        if reads(middle)? {
            ahead = middle;
        } else {
            behind = middle;
        }
    }
    let whole_minute = ahead.div_euclid(60) * 60 + if ahead.rem_euclid(60) == 0 { 0 } else { 60 };
    DateTime::from_timestamp(whole_minute, 0)
}

#[cfg(test)]
mod tests;
