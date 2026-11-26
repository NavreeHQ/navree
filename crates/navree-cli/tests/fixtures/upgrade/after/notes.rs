//! Sites where human judgment chooses between calendar and exact time or disambiguation.

use chrono::{Days, LocalResult, TimeZone};
use navree::datetime::{Date, DateTime, Span, Zoned};
use navree::time::Tz;

pub fn advance_zoned(at: Zoned) -> Zoned {
    // navree upgrade: confirm exact 24h (Span::days) vs calendar wall-clock step (Zoned::add_days)
    at + Span::days(1)
}

pub fn add_calendar_days(day: Date) -> Option<Date> {
    // navree upgrade: choose calendar step (Date::add_days / Date::add_months or Zoned::add_days / Zoned::add_months) vs exact Span
    day.checked_add_days(Days::new(30))
}

pub fn format_user_pattern(at: Zoned, user_pattern: &str) -> String {
    // navree upgrade: runtime pattern — use Formatted::to_text() or pattern::check() if untrusted (Display panics on invalid specifiers)
    at.format(user_pattern).to_string()
}

pub fn resolve_wall_clock(zone: &Tz, local: DateTime) -> Option<Zoned> {
    // navree upgrade: choose Disambiguation (Compatible, Earlier, Later or Reject) for Zone::to_timestamp
    match zone.from_local_datetime(&local) {
        LocalResult::Single(_) => None,
        LocalResult::Ambiguous(_, _) => None,
        LocalResult::None => None,
    }
}
