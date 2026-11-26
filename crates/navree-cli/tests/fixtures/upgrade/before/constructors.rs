//! Constructors, sentinels and weekday names for Ludhiana dispatch schedules.

use chrono::{
    DateTime, Duration, FixedOffset, NaiveDate, NaiveTime, SecondsFormat, TimeDelta, TimeZone, Utc,
    Weekday,
};

pub fn instants_and_bounds(
    secs: i64,
    millis: i64,
) -> (Vec<DateTime<Utc>>, Option<DateTime<Utc>>) {
    let now = Utc::now();
    let fixed = Utc.with_ymd_and_hms(2026, 11, 25, 4, 0, 0).unwrap();
    let maybe = Utc.with_ymd_and_hms(2026, 11, 25, 4, 0, 0).single();
    let from_whole = DateTime::from_timestamp(secs, 0).unwrap_or(DateTime::UNIX_EPOCH);
    let from_nanos = DateTime::from_timestamp(secs, 250_000_000).unwrap();
    let from_ms = DateTime::from_timestamp_millis(millis).unwrap_or(DateTime::MIN_UTC);
    (
        vec![now, fixed, from_whole, from_nanos, from_ms, DateTime::MAX_UTC],
        maybe,
    )
}

pub fn civil_and_offsets() -> (NaiveDate, Option<NaiveDate>, NaiveTime, FixedOffset) {
    let harvest = NaiveDate::from_ymd_opt(2026, 4, 13).unwrap();
    let tentative = NaiveDate::from_ymd_opt(2026, 4, 14);
    let _bounds = (NaiveDate::MIN, NaiveDate::MAX, NaiveTime::MIN);
    let bell = NaiveTime::from_hms_opt(9, 15, 0).expect("valid IST opening time");
    let ist = FixedOffset::east_opt(5 * 3_600 + 30 * 60).unwrap();
    (harvest, tentative, bell, ist)
}

pub fn spans_and_formats(std_wait: std::time::Duration) -> (TimeDelta, SecondsFormat) {
    let base = TimeDelta::seconds(90)
        + Duration::minutes(15)
        + TimeDelta::milliseconds(250)
        + TimeDelta::microseconds(50)
        + TimeDelta::nanoseconds(10)
        + TimeDelta::zero();
    let converted = TimeDelta::from_std(std_wait).unwrap_or(TimeDelta::MAX);
    let _all = [
        SecondsFormat::Secs,
        SecondsFormat::Millis,
        SecondsFormat::Micros,
        SecondsFormat::Nanos,
        SecondsFormat::AutoSi,
    ];
    (base + converted, SecondsFormat::AutoSi)
}

pub fn is_mandi_open(day: Weekday) -> bool {
    match day {
        Weekday::Mon | Weekday::Tue | Weekday::Wed | Weekday::Thu | Weekday::Fri | Weekday::Sat => {
            true
        }
        Weekday::Sun => false,
    }
}
