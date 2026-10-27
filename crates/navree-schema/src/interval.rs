//! [`Interval`]: a span of time such as `7 days`, parsed once and rendered
//! for each database.

use std::fmt;
use std::str::FromStr;

use crate::error::SchemaError;

/// A unit of an [`Interval`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IntervalUnit {
    /// Seconds.
    Second,
    /// Minutes.
    Minute,
    /// Hours.
    Hour,
    /// Days.
    Day,
    /// Weeks.
    Week,
    /// Calendar months.
    Month,
    /// Calendar years.
    Year,
}

impl IntervalUnit {
    /// The unit's length in seconds; `None` for months and years, whose
    /// length depends on the calendar.
    pub fn seconds(self) -> Option<u64> {
        match self {
            IntervalUnit::Second => Some(1),
            IntervalUnit::Minute => Some(60),
            IntervalUnit::Hour => Some(3_600),
            IntervalUnit::Day => Some(86_400),
            IntervalUnit::Week => Some(604_800),
            IntervalUnit::Month | IntervalUnit::Year => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            IntervalUnit::Second => "second",
            IntervalUnit::Minute => "minute",
            IntervalUnit::Hour => "hour",
            IntervalUnit::Day => "day",
            IntervalUnit::Week => "week",
            IntervalUnit::Month => "month",
            IntervalUnit::Year => "year",
        }
    }

    fn parse(word: &str) -> Option<IntervalUnit> {
        let unit = match word.to_ascii_lowercase().as_str() {
            "s" | "sec" | "secs" | "second" | "seconds" => IntervalUnit::Second,
            "min" | "mins" | "minute" | "minutes" => IntervalUnit::Minute,
            "h" | "hr" | "hrs" | "hour" | "hours" => IntervalUnit::Hour,
            "d" | "day" | "days" => IntervalUnit::Day,
            "w" | "week" | "weeks" => IntervalUnit::Week,
            "mon" | "mons" | "month" | "months" => IntervalUnit::Month,
            "y" | "yr" | "yrs" | "year" | "years" => IntervalUnit::Year,
            _ => return None,
        };
        Some(unit)
    }
}

/// A span of time: a chunk size, a retention period, a compression delay,
/// a refresh window or a bucket width.
///
/// It parses `<number> <unit>` (`7 days`, `1 hour`, `15 minutes`,
/// `1 month`) and the short forms `7d`, `12h`, `30min`, `1mon`, `2w`, `1y`;
/// a bare `m` is refused because it could mean minutes or months.
///
/// ```
/// use navree_schema::{Interval, IntervalUnit};
///
/// let week: Interval = "1 week".parse().unwrap();
/// assert_eq!(week, Interval::new(1, IntervalUnit::Week));
/// assert_eq!(week.as_seconds(), Some(604_800));
/// assert_eq!(week.to_string(), "1 week");
/// assert_eq!("12h".parse::<Interval>().unwrap().to_string(), "12 hours");
/// assert!("7 dayz".parse::<Interval>().is_err());
/// assert_eq!("1 month".parse::<Interval>().unwrap().as_seconds(), None);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Interval {
    amount: u64,
    unit: IntervalUnit,
}

impl Interval {
    /// `amount` `unit`s.
    pub const fn new(amount: u64, unit: IntervalUnit) -> Interval {
        Interval { amount, unit }
    }

    /// Parses `text` (see the type's docs).
    pub fn parse(text: &str) -> Result<Interval, SchemaError> {
        let error = || SchemaError::Interval {
            input: text.to_owned(),
        };
        let trimmed = text.trim();
        let digits_end = trimmed
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(trimmed.len());
        if digits_end == 0 {
            return Err(error());
        }
        let amount: u64 = trimmed[..digits_end].parse().map_err(|_| error())?;
        let unit = IntervalUnit::parse(trimmed[digits_end..].trim()).ok_or_else(error)?;
        Ok(Interval { amount, unit })
    }

    /// The number of units.
    pub fn amount(&self) -> u64 {
        self.amount
    }

    /// The unit.
    pub fn unit(&self) -> IntervalUnit {
        self.unit
    }

    /// The length in seconds, when it does not depend on the calendar.
    pub fn as_seconds(&self) -> Option<u64> {
        self.unit.seconds().map(|s| s * self.amount)
    }

    /// The PostgreSQL literal: `INTERVAL '7 days'`.
    pub fn to_postgres(&self) -> String {
        format!("INTERVAL '{self}'")
    }
}

impl FromStr for Interval {
    type Err = SchemaError;

    fn from_str(text: &str) -> Result<Interval, SchemaError> {
        Interval::parse(text)
    }
}

impl fmt::Display for Interval {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let plural = if self.amount == 1 { "" } else { "s" };
        write!(f, "{} {}{plural}", self.amount, self.unit.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_long_and_short_forms() {
        for (text, amount, unit) in [
            ("7 days", 7, IntervalUnit::Day),
            ("1 day", 1, IntervalUnit::Day),
            ("7d", 7, IntervalUnit::Day),
            (" 90 DAYS ", 90, IntervalUnit::Day),
            ("15 minutes", 15, IntervalUnit::Minute),
            ("30min", 30, IntervalUnit::Minute),
            ("12h", 12, IntervalUnit::Hour),
            ("1 hour", 1, IntervalUnit::Hour),
            ("2w", 2, IntervalUnit::Week),
            ("1mon", 1, IntervalUnit::Month),
            ("3 months", 3, IntervalUnit::Month),
            ("1 year", 1, IntervalUnit::Year),
            ("45 seconds", 45, IntervalUnit::Second),
        ] {
            assert_eq!(
                Interval::parse(text).unwrap(),
                Interval::new(amount, unit),
                "{text}"
            );
        }
    }

    #[test]
    fn refuses_ambiguous_and_malformed_input() {
        for text in ["", "days", "7", "7 m", "7 dayz", "-1 day", "1.5 hours"] {
            let error = Interval::parse(text).unwrap_err().to_string();
            assert!(error.contains("is not an interval"), "{text}: {error}");
        }
    }

    #[test]
    fn renders_for_postgres() {
        assert_eq!(
            Interval::parse("7d").unwrap().to_postgres(),
            "INTERVAL '7 days'"
        );
        assert_eq!(
            Interval::parse("1 hour").unwrap().to_postgres(),
            "INTERVAL '1 hour'"
        );
    }
}
