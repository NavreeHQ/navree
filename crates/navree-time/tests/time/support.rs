//! Helpers shared by the suites.

use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use navree_time::{LocalDay, Zone};

/// An RFC 3339 instant.
pub fn utc(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text)
        .unwrap_or_else(|e| panic!("bad instant `{text}`: {e}"))
        .with_timezone(&Utc)
}

/// A `yyyy-MM-dd` day.
pub fn day(text: &str) -> LocalDay {
    text.parse()
        .unwrap_or_else(|e| panic!("bad day `{text}`: {e}"))
}

/// A zone by name.
pub fn zone(name: &str) -> Zone {
    Zone::parse(name).unwrap_or_else(|e| panic!("bad zone `{name}`: {e}"))
}

/// A wall-clock time, `yyyy-MM-ddTHH:MM`.
pub fn local(text: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M")
        .unwrap_or_else(|e| panic!("bad local time `{text}`: {e}"))
}

/// A calendar date.
pub fn date(text: &str) -> NaiveDate {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
}
