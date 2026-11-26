//! Method rewrites, RFC 3339 and pattern argument order for Jaipur invoices.

use chrono::{
    DateTime, Datelike, NaiveDate, NaiveDateTime, NaiveTime, SecondsFormat, SubsecRound, TimeDelta,
    Utc,
};

pub fn inspect_instant(at: DateTime<Utc>, earlier: DateTime<Utc>) -> (i64, i64, i64, u32) {
    let rounded = at.trunc_subsecs(6);
    let delta = rounded.signed_duration_since(earlier);
    assert_eq!(
        (
            delta.num_days(),
            delta.num_hours(),
            delta.num_minutes(),
            delta.num_seconds(),
            delta.num_milliseconds(),
            delta.num_microseconds(),
        ),
        (0, 0, 0, 0, 0, Some(0))
    );
    (
        rounded.timestamp(),
        rounded.timestamp_millis(),
        rounded.timestamp_micros(),
        rounded.timestamp_subsec_nanos(),
    )
}

pub fn shift_and_convert(
    at: DateTime<Utc>,
    step: TimeDelta,
    day: NaiveDate,
    opens: NaiveTime,
) -> Option<(DateTime<Utc>, NaiveDateTime, NaiveDate, i64)> {
    let moved = at.checked_add_signed(step)?.checked_sub_signed(step)?;
    let wall = day.and_time(opens).and_utc();
    let noon = day.and_hms_opt(12, 0, 0).unwrap();
    let next_day = day.succ_opt()?.pred_opt()?;
    let offset_from_mon = i64::from(next_day.weekday().num_days_from_monday());
    let _ = (wall, moved.naive_utc(), moved.date_naive());
    Some((moved, noon, next_day, offset_from_mon))
}

pub fn wire_text(at: DateTime<Utc>, raw_iso: &str, raw_day: &str, raw_time: &str) -> String {
    let _parsed_utc = DateTime::parse_from_rfc3339(raw_iso).unwrap().with_timezone(&Utc);
    let _parsed_zoned = DateTime::parse_from_rfc3339(raw_iso).unwrap();
    let _day = NaiveDate::parse_from_str(raw_day, "%d/%m/%Y").unwrap();
    let _time = NaiveTime::parse_from_str(raw_time, "%H:%M").unwrap();
    let _dt = NaiveDateTime::parse_from_str("2026-11-25 09:30", "%Y-%m-%d %H:%M").unwrap();
    let _wire = at.to_rfc3339();
    let _secs = at.to_rfc3339_opts(SecondsFormat::Secs, true);
    at.format("%-d %b %Y").to_string()
}
