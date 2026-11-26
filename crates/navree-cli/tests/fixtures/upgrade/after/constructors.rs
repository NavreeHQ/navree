//! Constructors, sentinels and weekday names for Ludhiana dispatch schedules.

use navree::datetime::{Date, Offset, Precision, Span, Time, Timestamp, Weekday};

pub fn instants_and_bounds(
    secs: i64,
    millis: i64,
) -> (Vec<Timestamp>, Option<Timestamp>) {
    let now = Timestamp::now();
    let fixed = Timestamp::utc(2026, 11, 25, 4, 0, 0).unwrap();
    let maybe = Timestamp::utc(2026, 11, 25, 4, 0, 0).ok();
    let from_whole = Timestamp::from_secs(secs).ok().unwrap_or(Timestamp::UNIX_EPOCH);
    let from_nanos = Timestamp::from_secs_nanos(secs, 250_000_000).unwrap();
    let from_ms = Timestamp::from_millis(millis).ok().unwrap_or(Timestamp::MIN);
    (
        vec![now, fixed, from_whole, from_nanos, from_ms, Timestamp::MAX],
        maybe,
    )
}

pub fn civil_and_offsets() -> (Date, Option<Date>, Time, Offset) {
    let harvest = Date::new(2026, 4, 13).unwrap();
    let tentative = Date::new(2026, 4, 14).ok();
    let _bounds = (Date::MIN, Date::MAX, Time::MIDNIGHT);
    let bell = Time::new(9, 15, 0).expect("valid IST opening time");
    let ist = Offset::east(5 * 3_600 + 30 * 60).unwrap();
    (harvest, tentative, bell, ist)
}

pub fn spans_and_formats(std_wait: std::time::Duration) -> (Span, Precision) {
    let base = Span::seconds(90)
        + Span::minutes(15)
        + Span::millis(250)
        + Span::micros(50)
        + Span::nanos(10)
        + Span::ZERO;
    let converted = Span::try_from(std_wait).unwrap_or(Span::MAX);
    let _all = [
        Precision::Secs,
        Precision::Millis,
        Precision::Micros,
        Precision::Nanos,
        Precision::Auto,
    ];
    (base + converted, Precision::Auto)
}

pub fn is_mandi_open(day: Weekday) -> bool {
    match day {
        Weekday::Monday | Weekday::Tuesday | Weekday::Wednesday | Weekday::Thursday | Weekday::Friday | Weekday::Saturday => {
            true
        }
        Weekday::Sunday => false,
    }
}
