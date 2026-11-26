//! Dispatch ledger types for the Amritsar grain mandi.

use chrono::{
    DateTime, Datelike, Duration, FixedOffset, Month, NaiveDate, NaiveDateTime, NaiveTime,
    SecondsFormat, TimeDelta, Timelike, Utc, Weekday,
};
use navree::time::Tz;

/// One weighbridge ticket in ₹.
#[derive(Clone, Debug)]
pub struct WeighTicket {
    /// Recorded instant in UTC.
    pub weighed_at: DateTime<Utc>,
    /// Gate pass wall clock in Asia/Kolkata.
    pub gate_at: DateTime<Tz>,
    /// Settlement stamp with fixed offset.
    pub settled_at: Option<DateTime<FixedOffset>>,
    /// Market day and opening bell.
    pub mandi_day: NaiveDate,
    pub opens_at: NaiveTime,
    pub closes_local: NaiveDateTime,
    /// Grace window and hold span.
    pub grace: Duration,
    pub hold: TimeDelta,
    /// Settlement day of week and harvest month.
    pub settlement_day: Weekday,
    pub harvest_month: Month,
    pub offset: FixedOffset,
    pub precision: SecondsFormat,
}

/// Inline paths without imports stay qualified.
pub fn inline_stamp(
    at: chrono::DateTime<chrono::Utc>,
    wait: chrono::TimeDelta,
) -> (chrono::DateTime<chrono::FixedOffset>, chrono::NaiveDate) {
    let _ = (at, wait);
    todo!()
}

pub mod timeouts {
    use std::time::Duration;

    /// Standard library `Duration` is never mistaken for `chrono::Duration`.
    pub fn socket_timeout() -> Duration {
        Duration::from_secs(5)
    }
}
