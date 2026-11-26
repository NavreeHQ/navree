//! Dispatch ledger types for the Amritsar grain mandi.

use navree::datetime::{
    Date, DateTime, Month, Offset, Precision, Span, Time, Timestamp, Weekday, Zoned,
};
use navree::time::Tz;

/// One weighbridge ticket in ₹.
#[derive(Clone, Debug)]
pub struct WeighTicket {
    /// Recorded instant in UTC.
    pub weighed_at: Timestamp,
    /// Gate pass wall clock in Asia/Kolkata.
    pub gate_at: Zoned,
    /// Settlement stamp with fixed offset.
    pub settled_at: Option<Zoned>,
    /// Market day and opening bell.
    pub mandi_day: Date,
    pub opens_at: Time,
    pub closes_local: DateTime,
    /// Grace window and hold span.
    pub grace: Span,
    pub hold: Span,
    /// Settlement day of week and harvest month.
    pub settlement_day: Weekday,
    pub harvest_month: Month,
    pub offset: Offset,
    pub precision: Precision,
}

/// Inline paths without imports stay qualified.
pub fn inline_stamp(
    at: navree::datetime::Timestamp,
    wait: navree::datetime::Span,
) -> (navree::datetime::Zoned, navree::datetime::Date) {
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
