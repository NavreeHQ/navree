//! Properties of the parser and the next-run search.

use chrono::{DateTime, TimeDelta, Utc};
use chrono_tz::Tz;
use navree_schedule::{CronExpression, MAX_EXPRESSION_LEN};
use proptest::prelude::*;

const ZONES: &[&str] = &[
    "UTC",
    "Asia/Kolkata",
    "America/New_York",
    "Europe/London",
    "Australia/Lord_Howe",
    "America/St_Johns",
    "Pacific/Auckland",
];

fn zone() -> impl Strategy<Value = Tz> {
    prop::sample::select(ZONES).prop_map(|name| name.parse::<Tz>().unwrap())
}

/// Seconds since the epoch between 2020 and 2040.
fn start() -> impl Strategy<Value = DateTime<Utc>> {
    (1_577_836_800_i64..2_208_988_800).prop_map(|s| DateTime::from_timestamp(s, 0).unwrap())
}

fn numeric(min: u32, max: u32) -> impl Strategy<Value = String> {
    prop_oneof![
        Just("*".to_owned()),
        (1..=max).prop_map(|step| format!("*/{step}")),
        (min..=max).prop_map(|v| v.to_string()),
        (min..=max, min..=max).prop_map(|(a, b)| format!("{}-{}", a.min(b), a.max(b))),
        (min..=max, min..=max, 1_u32..10).prop_map(|(a, b, step)| format!(
            "{}-{}/{step}",
            a.min(b),
            a.max(b)
        )),
        prop::collection::vec(min..=max, 1..5).prop_map(|values| {
            values
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(",")
        }),
    ]
}

fn day_of_month() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => numeric(1, 31),
        1 => Just("L".to_owned()),
        1 => Just("LW".to_owned()),
        1 => (1_u32..=31).prop_map(|d| format!("{d}W")),
        1 => Just("?".to_owned()),
    ]
}

fn month() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => numeric(1, 12),
        1 => prop::sample::select(&["JAN", "feb-APR", "JUL,dec", "MAR-MAY/2"][..]).prop_map(str::to_owned),
    ]
}

fn day_of_week() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => numeric(0, 7),
        1 => (0_u32..=7).prop_map(|d| format!("{d}L")),
        1 => (0_u32..=7, 1_u32..=5).prop_map(|(d, n)| format!("{d}#{n}")),
        1 => prop::sample::select(&["MON-FRI", "sat,sun", "FRI#L", "SUN-TUE", "?", "FRI-SUN"][..]).prop_map(str::to_owned),
        1 => numeric(0, 7).prop_map(|f| format!("+{f}")),
    ]
}

fn expression() -> impl Strategy<Value = String> {
    prop_oneof![
        8 => (numeric(0, 59), numeric(0, 23), day_of_month(), month(), day_of_week())
            .prop_map(|(m, h, dom, mon, dow)| format!("{m} {h} {dom} {mon} {dow}")),
        1 => prop::sample::select(&["@hourly", "@daily", "@weekly", "@monthly", "@yearly", "@annually"][..])
            .prop_map(str::to_owned),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Parsing never panics, whatever the input.
    #[test]
    fn parsing_arbitrary_text_never_panics(text in any::<String>()) {
        let _ = CronExpression::parse(&text);
    }

    /// Text made of the characters cron uses gets further than random text.
    #[test]
    fn parsing_cron_like_text_never_panics(text in "[0-9*/,?#LWl@a-zA-Z+ \t-]{0,300}") {
        let _ = CronExpression::parse(&text);
    }

    /// Anything accepted is accepted again as printed, and the search
    /// terminates (with a run or with none) for every start.
    #[test]
    fn accepted_text_roundtrips_and_searches_terminate(
        text in "[0-9*/,?#LWa-zA-Z@+ -]{0,60}",
        start in start(),
        zone in zone(),
    ) {
        if let Ok(cron) = CronExpression::parse(&text) {
            prop_assert_eq!(CronExpression::parse(&cron.to_string()).unwrap(), cron.clone());
            let _ = cron.next_after(start, &zone);
        }
    }

    /// Every generated expression parses; every run is strictly after the
    /// start, on a whole minute, and due; and no due minute is skipped.
    #[test]
    fn runs_are_due_and_strictly_after_the_start(
        text in expression(),
        start in start(),
        zone in zone(),
    ) {
        let cron = CronExpression::parse(&text).unwrap();
        prop_assert_eq!(CronExpression::parse(&cron.to_string()).unwrap(), cron.clone());
        let mut cursor = start;
        for _ in 0..3 {
            let Some(run) = cron.next_after(cursor, &zone) else {
                break;
            };
            prop_assert!(run > cursor, "{} is not after {}", run, cursor);
            prop_assert_eq!(run.timestamp() % 60, 0);
            prop_assert!(cron.is_due(run, &zone), "{} in {} is not due at {}", text, zone, run);
            // Nothing due in between, when the gap is short enough to scan.
            if run - cursor <= TimeDelta::days(3) {
                let mut minute = DateTime::from_timestamp(cursor.timestamp().div_euclid(60) * 60 + 60, 0).unwrap();
                while minute < run {
                    prop_assert!(!cron.is_due(minute, &zone), "{} in {}: {} is due before {}", text, zone, minute, run);
                    minute += TimeDelta::minutes(1);
                }
            }
            cursor = run;
        }
    }
}

#[test]
fn the_search_is_bounded() {
    let utc: Tz = "UTC".parse().unwrap();
    let began = std::time::Instant::now();
    for text in [
        "0 0 30 2 *",
        "0 0 31 4,6,9,11 *",
        "59 23 31 2,4,6,9,11 +*",
        "0 0 29 2 +MON",
        "0 0 W * *",
    ] {
        if let Ok(cron) = CronExpression::parse(text) {
            let start = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
            let _ = cron.next_after(start, &utc);
        }
    }
    assert!(
        began.elapsed() < std::time::Duration::from_secs(5),
        "{:?}",
        began.elapsed()
    );
    assert!(CronExpression::parse(&"1,".repeat(MAX_EXPRESSION_LEN)).is_err());
}
