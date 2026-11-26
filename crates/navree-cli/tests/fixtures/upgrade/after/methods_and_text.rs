//! Method rewrites, RFC 3339 and pattern argument order for Jaipur invoices.

use navree::datetime::{Date, DateTime, Precision, Span, Time, Timestamp, Zoned};

pub fn inspect_instant(at: Timestamp, earlier: Timestamp) -> (i64, i64, i64, u32) {
    let rounded = at.trunc_micros();
    let delta = rounded.duration_since(earlier);
    assert_eq!(
        (
            delta.as_days(),
            delta.as_hours(),
            delta.as_minutes(),
            delta.as_seconds(),
            delta.as_millis(),
            delta.as_micros(),
        ),
        (0, 0, 0, 0, 0, Some(0))
    );
    (
        rounded.as_secs(),
        rounded.as_millis(),
        rounded.as_micros(),
        rounded.subsec_nanos(),
    )
}

pub fn shift_and_convert(
    at: Timestamp,
    step: Span,
    day: Date,
    opens: Time,
) -> Option<(Timestamp, DateTime, Date, i64)> {
    let moved = at.checked_add(step)?.checked_sub(step)?;
    let wall = day.at(opens).in_utc();
    let noon = day.at(Time::new(12, 0, 0).unwrap());
    let next_day = day.tomorrow()?.yesterday()?;
    let offset_from_mon = i64::from(next_day.weekday().days_from_monday());
    let _ = (wall, moved.date_time_utc(), moved.date_time_utc().date());
    Some((moved, noon, next_day, offset_from_mon))
}

pub fn wire_text(at: Timestamp, raw_iso: &str, raw_day: &str, raw_time: &str) -> String {
    let _parsed_utc = Timestamp::parse_rfc3339(raw_iso).unwrap();
    let _parsed_zoned = Zoned::parse_rfc3339(raw_iso).unwrap();
    let _day = Date::parse("%d/%m/%Y", raw_day).unwrap();
    let _time = Time::parse("%H:%M", raw_time).unwrap();
    let _dt = DateTime::parse("%Y-%m-%d %H:%M", "2026-11-25 09:30").unwrap();
    let _wire = at.to_rfc3339();
    let _secs = at.to_rfc3339_with(Precision::Secs, true);
    at.format("%-d %b %Y").to_string()
}
