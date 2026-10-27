//! `LocalDay` and `DayRange`: calendar days counted on a zone's clock.

use chrono::{TimeDelta, Timelike, Weekday};
use garde::Validate;
use navree_time::{DayRange, LocalDay, Zone};

use crate::support::{date, day, utc, zone};

#[test]
fn days_parse_and_print_as_yyyy_mm_dd() {
    let d = day("2026-10-27");
    assert_eq!(d.to_string(), "2026-10-27");
    assert_eq!(d.date(), date("2026-10-27"));
    assert_eq!(LocalDay::new(2026, 10, 27), Some(d));
    assert_eq!(LocalDay::new(2026, 2, 30), None);
    assert_eq!(LocalDay::from(date("2026-10-27")), d);
    assert_eq!((d.year(), d.month(), d.day()), (2026, 10, 27));
    assert_eq!(d.weekday(), Weekday::Tue);
    for bad in ["2026-13-01", "2026-02-30", "27/10/2026", "2026-1-5", "", "2026-10-27T00:00"] {
        let error = bad.parse::<LocalDay>().unwrap_err();
        assert!(error.to_string().contains("yyyy-MM-dd"), "{bad}: {error}");
    }
}

#[test]
fn days_round_trip_through_json() {
    let d = day("2026-03-29");
    assert_eq!(serde_json::to_string(&d).unwrap(), r#""2026-03-29""#);
    assert_eq!(serde_json::from_str::<LocalDay>(r#""2026-03-29""#).unwrap(), d);
    assert!(serde_json::from_str::<LocalDay>(r#""2026-03-32""#).is_err());
    assert!(serde_json::from_str::<LocalDay>("20260329").is_err());
}

#[test]
fn days_step_forward_and_back() {
    let d = day("2026-12-31");
    assert_eq!(d.succ(), day("2027-01-01"));
    assert_eq!(d.pred(), day("2026-12-30"));
    assert_eq!(d.add_days(-365), day("2025-12-31"));
    assert_eq!(d.first_of_month(), day("2026-12-01"));
    assert!(day("2026-01-01") < day("2026-01-02"));
}

#[test]
fn an_instant_falls_on_the_local_day_of_its_zone() {
    let late_evening_utc = utc("2026-10-26T20:00:00Z");
    assert_eq!(LocalDay::of(late_evening_utc, &zone("Asia/Kolkata")), day("2026-10-27"));
    assert_eq!(LocalDay::of(late_evening_utc, &Zone::UTC), day("2026-10-26"));
    assert_eq!(zone("America/Los_Angeles").today(late_evening_utc), day("2026-10-26"));
    assert_eq!(zone("Pacific/Kiritimati").today(late_evening_utc), day("2026-10-27"));
}

#[test]
fn a_day_spans_from_its_local_midnight_to_the_next() {
    let kolkata = zone("Asia/Kolkata");
    let d = day("2026-10-27");
    assert_eq!(d.start_in(&kolkata), utc("2026-10-26T18:30:00Z"));
    assert_eq!(d.end_in(&kolkata), utc("2026-10-27T18:30:00Z"));
    assert_eq!(
        d.utc_bounds(&kolkata),
        (utc("2026-10-26T18:30:00Z"), utc("2026-10-27T18:30:00Z"))
    );
    assert_eq!(
        d.at(chrono::NaiveTime::from_hms_opt(9, 0, 0).unwrap(), &kolkata),
        utc("2026-10-27T03:30:00Z")
    );
}

#[test]
fn days_are_shorter_or_longer_when_clocks_change() {
    let london = zone("Europe/London");
    let length = |d: &str| {
        let (start, end) = day(d).utc_bounds(&london);
        end - start
    };
    assert_eq!(length("2026-03-29"), TimeDelta::hours(23));
    assert_eq!(length("2026-10-25"), TimeDelta::hours(25));
    assert_eq!(length("2026-06-01"), TimeDelta::hours(24));
}

#[test]
fn a_day_starts_at_its_first_instant_even_when_midnight_is_skipped() {
    // Wherever clocks jump forward at midnight, a day starts at 01:00.
    for name in ["America/Santiago", "America/Asuncion", "America/Havana", "Asia/Beirut"] {
        let z = zone(name);
        let mut found_gap = false;
        let mut d = day("2024-01-01");
        while d < day("2027-01-01") {
            let start = d.start_in(&z);
            assert_eq!(LocalDay::of(start, &z), d, "{name} {d}");
            assert_eq!(
                LocalDay::of(start - TimeDelta::seconds(1), &z),
                d.pred(),
                "{name} {d}: the second before the start is the day before"
            );
            if z.local(start).hour() != 0 {
                found_gap = true;
            }
            d = d.succ();
        }
        if name == "America/Santiago" {
            assert!(found_gap, "Santiago skips midnight when DST starts");
        }
    }
}

#[test]
fn ranges_cover_whole_local_days() {
    let range = DayRange::new(day("2026-10-01"), day("2026-10-31")).unwrap();
    assert_eq!(range.day_count(), 31);
    assert!(range.contains(day("2026-10-15")));
    assert!(!range.contains(day("2026-11-01")));
    let days: Vec<_> = range.days().collect();
    assert_eq!(days.len(), 31);
    assert_eq!(days[0], day("2026-10-01"));
    assert_eq!(days[30], day("2026-10-31"));

    let kolkata = zone("Asia/Kolkata");
    assert_eq!(
        range.utc_bounds(&kolkata),
        (utc("2026-09-30T18:30:00Z"), utc("2026-10-31T18:30:00Z"))
    );
    let london = zone("Europe/London");
    // October 2026 in London: BST until the 25th, then GMT.
    assert_eq!(
        range.utc_bounds(&london),
        (utc("2026-09-30T23:00:00Z"), utc("2026-11-01T00:00:00Z"))
    );
}

#[test]
fn ranges_have_helpers_for_common_windows() {
    let kolkata = zone("Asia/Kolkata");
    let now = utc("2026-10-26T20:00:00Z"); // 27 October in Kolkata
    let week = DayRange::last_days(7, &kolkata, now);
    assert_eq!((week.from, week.to), (day("2026-10-21"), day("2026-10-27")));
    assert_eq!(DayRange::last_days(0, &kolkata, now).day_count(), 1);
    let month = DayRange::month_of(day("2026-02-14"));
    assert_eq!((month.from, month.to), (day("2026-02-01"), day("2026-02-28")));
    let single = DayRange::single(day("2026-02-14"));
    assert_eq!(single.day_count(), 1);
}

#[test]
fn a_range_ending_before_it_starts_is_refused() {
    let error = DayRange::new(day("2026-10-31"), day("2026-10-01")).unwrap_err();
    assert!(error.to_string().contains("before"), "{error}");

    let backwards = DayRange {
        from: day("2026-10-31"),
        to: day("2026-10-01"),
    };
    let report = backwards.validate().unwrap_err();
    let text = report.to_string();
    assert!(text.contains("to"), "{text}");
    assert!(DayRange::single(day("2026-10-01")).validate().is_ok());
}

#[test]
fn ranges_deserialize_from_from_and_to() {
    let range: DayRange =
        serde_json::from_value(serde_json::json!({"from": "2026-10-01", "to": "2026-10-07"}))
            .unwrap();
    assert_eq!(range.day_count(), 7);
    assert_eq!(
        serde_json::to_value(&range).unwrap(),
        serde_json::json!({"from": "2026-10-01", "to": "2026-10-07"})
    );
}
