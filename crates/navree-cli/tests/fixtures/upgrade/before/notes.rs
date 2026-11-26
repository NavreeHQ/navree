//! Sites where human judgment chooses between calendar and exact time or disambiguation.

use chrono::{DateTime, Days, Duration, FixedOffset, LocalResult, NaiveDate, NaiveDateTime, TimeZone};
use navree::time::Tz;

pub fn advance_zoned(at: DateTime<FixedOffset>) -> DateTime<FixedOffset> {
    at + Duration::days(1)
}

pub fn add_calendar_days(day: NaiveDate) -> Option<NaiveDate> {
    day.checked_add_days(Days::new(30))
}

pub fn format_user_pattern(at: DateTime<FixedOffset>, user_pattern: &str) -> String {
    at.format(user_pattern).to_string()
}

pub fn resolve_wall_clock(zone: &Tz, local: NaiveDateTime) -> Option<DateTime<FixedOffset>> {
    match zone.from_local_datetime(&local) {
        LocalResult::Single(_) => None,
        LocalResult::Ambiguous(_, _) => None,
        LocalResult::None => None,
    }
}
