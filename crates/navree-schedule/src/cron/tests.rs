use chrono_tz::{America::New_York, Asia::Kolkata, Europe::London, Tz};

use super::*;

fn cron(text: &str) -> CronExpression {
    text.parse().unwrap_or_else(|e| panic!("`{text}`: {e}"))
}

fn at(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text)
        .unwrap()
        .with_timezone(&Utc)
}

fn next(text: &str, after: &str, zone: Tz) -> Option<String> {
    cron(text)
        .next_after(at(after), &zone)
        .map(|t| t.to_rfc3339())
}

fn error(text: &str) -> CronError {
    text.parse::<CronExpression>().unwrap_err()
}

#[test]
fn accepted_forms_parse() {
    for text in [
        "* * * * *",
        "*/15 * * * *",
        "0 9-17 * * MON-FRI",
        "0,30 8,20 1,15 JAN,JUL *",
        "5-59/10 * * * *",
        "0 0 L * *",
        "0 0 LW * *",
        "0 0 15W * *",
        "0 0 1,15W,L * *",
        "0 0 ? * MON",
        "0 0 * * ?",
        "0 0 * * 5L",
        "0 0 * * FRIL",
        "0 0 * * 5#2",
        "0 0 * * FRI#L",
        "0 0 1-7 * +MON",
        "0 0 * * 7",
        "0 0 * * FRI-SUN",
        "0 0 * jan-mar/2 sun",
        "  0   0 *\t*  *  ",
        "@yearly",
        "@ANNUALLY",
        "@monthly",
        "@weekly",
        "@daily",
        "@hourly",
        "00 06 01 01 00",
    ] {
        cron(text);
    }
}

#[test]
fn rejected_forms_name_the_field_and_token() {
    let cases = [
        ("61 * * * *", "minute", "61"),
        ("0 24 * * *", "hour", "24"),
        ("0 0 0 * *", "day of month", "0"),
        ("0 0 32 * *", "day of month", "32"),
        ("0 0 * 13 *", "month", "13"),
        ("0 0 * * 8", "day of week", "8"),
        ("0 0 * FOO *", "month", "FOO"),
        ("0 0 * * MONDAY", "day of week", "MONDAY"),
        ("MON * * * *", "minute", "MON"),
        ("* * MON * *", "day of month", "MON"),
        ("*/0 * * * *", "minute", "*/0"),
        ("5/15 * * * *", "minute", "5/15"),
        ("/15 * * * *", "minute", "/15"),
        ("1,,2 * * * *", "minute", "1,,2"),
        ("1, * * * *", "minute", "1,"),
        ("10-5 * * * *", "minute", "10-5"),
        ("1-2-3 * * * *", "minute", "1-2-3"),
        ("0 0 15L * *", "day of month", "15L"),
        ("0 0 W * *", "day of month", "W"),
        ("0 0 * * L", "day of week", "L"),
        ("0 0 * * 5#6", "day of week", "5#6"),
        ("0 0 * * 5#0", "day of week", "5#0"),
        ("0 0 * * 9L", "day of week", "9L"),
        ("0 0 * * 1+", "day of week", "1+"),
        ("*/-1 * * * *", "minute", "*/-1"),
        ("1.5 * * * *", "minute", "1.5"),
    ];
    for (text, field, token) in cases {
        let e = error(text);
        assert_eq!((e.field(), e.token()), (field, token), "`{text}`: {e}");
    }
    // Rejected, with the token or the whole item named.
    for text in [
        "0 0 ?/5 * *",
        "0 0 * * 1-5#2",
        "0 0 * * *,1",
        "\u{e9} * * * *",
    ] {
        let e = error(text);
        assert!(!e.token().is_empty(), "`{text}`: {e}");
    }
    // Seconds, years and missing fields are whole-expression errors.
    for text in [
        "0 0 0 * * *",
        "0 0 0 * * * 2026",
        "* * * *",
        "",
        "   ",
        "@daily now",
    ] {
        assert_eq!(error(text).field(), "expression", "`{text}`");
    }
    assert!(error("@reboot").to_string().contains("@reboot"));
    assert!(error("0 0 0 * * *").to_string().contains("seconds"));
}

#[test]
fn error_keeps_the_token_as_written() {
    assert_eq!(error("0 0 * * mondaY").token(), "mondaY");
}

#[test]
fn long_expressions_are_rejected() {
    let long = format!("{} * * * *", vec!["1"; 200].join(","));
    assert!(long.len() > MAX_EXPRESSION_LEN);
    let e = error(&long);
    assert_eq!(e.field(), "expression");
    assert!(e.to_string().contains("limit"), "{e}");
    // All 60 minutes written out still fits.
    let all: Vec<String> = (0..60).map(|m| m.to_string()).collect();
    cron(&format!("{} * * * *", all.join(",")));
}

#[test]
fn next_runs_in_utc() {
    let utc = chrono_tz::UTC;
    assert_eq!(
        next("*/15 * * * *", "2026-10-26T09:07:30Z", utc).unwrap(),
        "2026-10-26T09:15:00+00:00"
    );
    // Strictly after: a start on a match gives the next one.
    assert_eq!(
        next("*/15 * * * *", "2026-10-26T09:15:00Z", utc).unwrap(),
        "2026-10-26T09:30:00+00:00"
    );
    assert_eq!(
        next("0 6 * * MON-FRI", "2026-10-30T07:00:00Z", utc).unwrap(),
        "2026-11-02T06:00:00+00:00"
    );
    assert_eq!(
        next("@yearly", "2026-10-30T07:00:00Z", utc).unwrap(),
        "2027-01-01T00:00:00+00:00"
    );
    assert_eq!(
        next("@weekly", "2026-10-25T00:00:00Z", utc).unwrap(),
        "2026-11-01T00:00:00+00:00"
    );
}

#[test]
fn day_of_month_and_week_combine_as_or_unless_plus() {
    let utc = chrono_tz::UTC;
    // 2026-11-01 is a Sunday. "1st or any Monday".
    assert_eq!(
        next("0 0 1 * MON", "2026-10-31T00:00:00Z", utc).unwrap(),
        "2026-11-01T00:00:00+00:00"
    );
    assert_eq!(
        next("0 0 1 * MON", "2026-11-01T00:00:00Z", utc).unwrap(),
        "2026-11-02T00:00:00+00:00"
    );
    // "The 1st that is also a Monday": Monday 1 February 2027 is the next.
    assert_eq!(
        next("0 0 1 * +MON", "2026-10-31T00:00:00Z", utc).unwrap(),
        "2027-02-01T00:00:00+00:00"
    );
}

#[test]
fn last_nth_and_nearest_weekday() {
    let utc = chrono_tz::UTC;
    assert_eq!(
        next("0 0 L * *", "2026-02-01T00:00:00Z", utc).unwrap(),
        "2026-02-28T00:00:00+00:00"
    );
    assert_eq!(
        next("0 0 L * *", "2028-02-01T00:00:00Z", utc).unwrap(),
        "2028-02-29T00:00:00+00:00"
    );
    // 2026-05-31 is a Sunday: the last weekday is Friday the 29th.
    assert_eq!(
        next("0 0 LW 5 *", "2026-05-01T00:00:00Z", utc).unwrap(),
        "2026-05-29T00:00:00+00:00"
    );
    // 2026-08-15 is a Saturday: 15W is Friday the 14th.
    assert_eq!(
        next("0 0 15W 8 *", "2026-08-01T00:00:00Z", utc).unwrap(),
        "2026-08-14T00:00:00+00:00"
    );
    // 2026-08-01 is a Saturday: 1W moves on to Monday the 3rd.
    assert_eq!(
        next("0 0 1W 8 *", "2026-07-31T00:00:00Z", utc).unwrap(),
        "2026-08-03T00:00:00+00:00"
    );
    // The last Friday of October 2026 is the 30th.
    assert_eq!(
        next("0 0 * 10 5L", "2026-10-01T00:00:00Z", utc).unwrap(),
        "2026-10-30T00:00:00+00:00"
    );
    // The second Tuesday of November 2026 is the 10th.
    assert_eq!(
        next("0 0 * 11 TUE#2", "2026-11-01T00:00:00Z", utc).unwrap(),
        "2026-11-10T00:00:00+00:00"
    );
    // 7 is Sunday, also as `7L`.
    assert_eq!(
        next("0 0 * 11 7L", "2026-11-01T00:00:00Z", utc).unwrap(),
        "2026-11-29T00:00:00+00:00"
    );
}

#[test]
fn never_matching_expressions_have_no_next_run() {
    let utc = chrono_tz::UTC;
    for text in ["0 0 30 2 *", "0 0 31 4,6,9,11 *", "0 0 30 FEB *"] {
        assert_eq!(
            cron(text).next_after(at("2026-01-01T00:00:00Z"), &utc),
            None,
            "{text}"
        );
    }
    // 29 February on a Monday (and only then) exists, but only some years.
    let leap_monday = cron("0 0 29 2 +MON");
    let next = leap_monday
        .next_after(at("2026-01-01T00:00:00Z"), &utc)
        .unwrap();
    assert_eq!(next.to_rfc3339(), "2044-02-29T00:00:00+00:00");
}

#[test]
fn spring_forward_runs_a_skipped_time_once_after_the_gap() {
    // New York, 2026-03-08: 02:00 EST jumps to 03:00 EDT (07:00Z).
    assert_eq!(
        next("30 2 * * *", "2026-03-07T12:00:00Z", New_York).unwrap(),
        "2026-03-08T07:00:00+00:00"
    );
    // The skipped 02:30 runs at 03:00 EDT; the next day's at 02:30 EDT.
    assert_eq!(
        next("30 2 * * *", "2026-03-08T07:00:00Z", New_York).unwrap(),
        "2026-03-09T06:30:00+00:00"
    );
    // Several skipped fixed times run once.
    let fixed = cron("0,15,30,45 2 * * *");
    let first = fixed
        .next_after(at("2026-03-08T06:59:00Z"), &New_York)
        .unwrap();
    assert_eq!(first.to_rfc3339(), "2026-03-08T07:00:00+00:00");
    let second = fixed.next_after(first, &New_York).unwrap();
    assert_eq!(second.to_rfc3339(), "2026-03-09T06:00:00+00:00");
    // A wildcard expression catches nothing up: `*/15` in the hour 2 does
    // not exist on that night.
    let every_15 = cron("*/15 2 * * *");
    assert_eq!(
        every_15
            .next_after(at("2026-03-08T06:59:00Z"), &New_York)
            .unwrap()
            .to_rfc3339(),
        "2026-03-09T06:00:00+00:00"
    );
    // A time at the end of the gap and one inside it still run once there.
    let both = cron("0,30 2,3 * * *");
    let first = both
        .next_after(at("2026-03-08T06:30:00Z"), &New_York)
        .unwrap();
    assert_eq!(first.to_rfc3339(), "2026-03-08T07:00:00+00:00");
    assert_eq!(
        both.next_after(first, &New_York).unwrap().to_rfc3339(),
        "2026-03-08T07:30:00+00:00"
    );
}

#[test]
fn fall_back_runs_a_repeated_time_once_the_first_time() {
    // New York, 2026-11-01: 02:00 EDT falls back to 01:00 EST (06:00Z).
    // 01:30 happens at 05:30Z (EDT) and 06:30Z (EST): only the first runs.
    assert_eq!(
        next("30 1 * * *", "2026-11-01T04:00:00Z", New_York).unwrap(),
        "2026-11-01T05:30:00+00:00"
    );
    assert_eq!(
        next("30 1 * * *", "2026-11-01T05:30:00Z", New_York).unwrap(),
        "2026-11-02T06:30:00+00:00"
    );
    // A start inside the second pass.
    assert_eq!(
        next("10 1 * * *", "2026-11-01T06:05:00Z", New_York).unwrap(),
        "2026-11-02T06:10:00+00:00"
    );
}

#[test]
fn zones_without_dst_and_london() {
    assert_eq!(
        next("0 9 * * *", "2026-10-26T02:00:00Z", Kolkata).unwrap(),
        "2026-10-26T03:30:00+00:00"
    );
    // London 2026-03-29: 01:00 GMT jumps to 02:00 BST (01:00Z); 01:30 is skipped.
    assert_eq!(
        next("30 1 * * *", "2026-03-28T12:00:00Z", London).unwrap(),
        "2026-03-29T01:00:00+00:00"
    );
}

#[test]
fn is_due_agrees_with_next_after() {
    for (text, zone, start) in [
        ("*/15 0-4 * * *", New_York, "2026-03-07T20:00:00Z"),
        ("30 2 * * *", New_York, "2026-03-07T20:00:00Z"),
        ("30 1 * * *", New_York, "2026-10-31T20:00:00Z"),
        ("*/20 0-3 * * *", London, "2026-10-24T20:00:00Z"),
        ("30 1 * * *", London, "2026-03-28T20:00:00Z"),
        ("0 9 * * *", Kolkata, "2026-10-24T20:00:00Z"),
    ] {
        let expr = cron(text);
        let start = at(start);
        let end = start + TimeDelta::days(3);
        let mut expected = Vec::new();
        let mut cursor = start;
        while let Some(run) = expr.next_after(cursor, &zone) {
            if run >= end {
                break;
            }
            expected.push(run);
            cursor = run;
        }
        let mut due = Vec::new();
        let mut tick = start + TimeDelta::minutes(1);
        while tick < end {
            if expr.is_due(tick, &zone) {
                due.push(tick);
            }
            tick += TimeDelta::minutes(1);
        }
        assert_eq!(due, expected, "{text} in {zone}");
    }
}

/// Every real minute in `[from, to)` at which `cron` is due, by asking
/// `is_due` of each one.
fn due_minutes(cron: &CronExpression, zone: Tz, from: &str, to: &str) -> Vec<DateTime<Utc>> {
    let (mut tick, to) = (at(from), at(to));
    let mut due = Vec::new();
    while tick < to {
        if cron.is_due(tick, &zone) {
            due.push(tick);
        }
        tick += TimeDelta::minutes(1);
    }
    due
}

/// The runs `next_after` finds in `[from, to)`.
fn runs(cron: &CronExpression, zone: Tz, from: &str, to: &str) -> Vec<DateTime<Utc>> {
    let (from, to) = (at(from), at(to));
    let mut runs = Vec::new();
    let mut cursor = from - TimeDelta::minutes(1);
    while let Some(run) = cron.next_after(cursor, &zone) {
        if run >= to {
            break;
        }
        runs.push(run);
        cursor = run;
    }
    runs
}

/// Clock changes: (zone, window around the spring-forward change, window
/// around the fall-back change), each six real hours.
fn changes() -> Vec<(Tz, [(&'static str, &'static str); 2])> {
    use chrono_tz::Australia::Lord_Howe;
    vec![
        (
            New_York,
            [
                ("2026-03-08T04:00:00Z", "2026-03-08T10:00:00Z"),
                ("2026-11-01T03:00:00Z", "2026-11-01T09:00:00Z"),
            ],
        ),
        (
            London,
            [
                ("2026-03-28T22:00:00Z", "2026-03-29T04:00:00Z"),
                ("2026-10-24T22:00:00Z", "2026-10-25T04:00:00Z"),
            ],
        ),
        // Lord Howe shifts by 30 minutes: forward 02:00 -> 02:30 (2026-10-04
        // 15:30Z), back 02:00 -> 01:30 (2026-04-04 15:00Z).
        (
            Lord_Howe,
            [
                ("2026-10-04T12:30:00Z", "2026-10-04T18:30:00Z"),
                ("2026-04-04T12:00:00Z", "2026-04-04T18:00:00Z"),
            ],
        ),
    ]
}

#[test]
fn wildcard_expressions_run_on_every_real_minute_whose_clock_matches() {
    for text in [
        "* * * * *",
        "*/15 * * * *",
        "0 * * * *",
        "*/20 1-3 * * *",
        "@hourly",
    ] {
        let expr = cron(text);
        for (zone, windows) in changes() {
            for (from, to) in windows {
                let runs = runs(&expr, zone, from, to);
                // The same instants as the wall clock says, no duplicate.
                let expected: Vec<_> = {
                    let (mut t, end) = (at(from), at(to));
                    let mut matching = Vec::new();
                    while t < end {
                        if expr.matches_local(t.with_timezone(&zone).naive_local()) {
                            matching.push(t);
                        }
                        t += TimeDelta::minutes(1);
                    }
                    matching
                };
                assert_eq!(runs, expected, "{text} in {zone}, {from}");
                assert_eq!(runs, due_minutes(&expr, zone, from, to), "{text} in {zone}");
                let mut sorted = runs.clone();
                sorted.dedup();
                assert_eq!(sorted, runs, "duplicate instant: {text} in {zone}");
            }
        }
    }
}

#[test]
fn every_minute_never_pauses_and_hourly_runs_every_real_hour() {
    for (zone, windows) in changes() {
        for (from, to) in windows {
            // Six real hours: 360 runs, one per real minute.
            let all = runs(&cron("* * * * *"), zone, from, to);
            assert_eq!(all.len(), 360, "{zone}, {from}");
            assert!(
                all.windows(2).all(|w| w[1] - w[0] == TimeDelta::minutes(1)),
                "{zone}, {from}: a pause"
            );
            // Every quarter of an hour on the wall clock: no gap longer
            // than the shift allows.
            let quarters = runs(&cron("*/15 * * * *"), zone, from, to);
            assert!(
                quarters
                    .windows(2)
                    .all(|w| w[1] - w[0] <= TimeDelta::minutes(45)),
                "{zone}, {from}: {quarters:?}"
            );
        }
    }
    // New York, fall back: 01:00 happens twice, and hourly runs both.
    let hourly = runs(
        &cron("0 * * * *"),
        New_York,
        "2026-11-01T03:00:00Z",
        "2026-11-01T09:00:00Z",
    );
    let hours: Vec<_> = hourly.iter().map(|t| t.format("%H").to_string()).collect();
    assert_eq!(hours, ["03", "04", "05", "06", "07", "08"]);
    // London, fall back: 01:00 BST (00:00Z) and 01:00 GMT (01:00Z).
    let hourly = runs(
        &cron("0 * * * *"),
        London,
        "2026-10-24T22:00:00Z",
        "2026-10-25T04:00:00Z",
    );
    let hours: Vec<_> = hourly.iter().map(|t| t.format("%H").to_string()).collect();
    assert_eq!(hours, ["22", "23", "00", "01", "02", "03"]);
}

#[test]
fn wildcard_expressions_do_not_catch_up_a_gap() {
    // New York, spring forward: 02:00-03:00 does not exist.
    let half = runs(
        &cron("30 * * * *"),
        New_York,
        "2026-03-08T05:00:00Z",
        "2026-03-08T09:00:00Z",
    );
    let at_local: Vec<_> = half
        .iter()
        .map(|t| t.with_timezone(&New_York).format("%H:%M").to_string())
        .collect();
    // 00:30 EST, 01:30 EST, then 03:30 EDT (not 03:00).
    assert_eq!(at_local, ["00:30", "01:30", "03:30", "04:30"]);
    let hourly = runs(
        &cron("0 * * * *"),
        New_York,
        "2026-03-08T05:00:00Z",
        "2026-03-08T09:00:00Z",
    );
    let at_local: Vec<_> = hourly
        .iter()
        .map(|t| t.with_timezone(&New_York).format("%H:%M").to_string())
        .collect();
    assert_eq!(at_local, ["00:00", "01:00", "03:00", "04:00"]);
}

#[test]
fn fixed_time_expressions_run_once_at_a_skipped_and_at_a_repeated_time() {
    for (zone, text, spring, fall) in [
        (
            New_York,
            "30 2 * * *",
            ("2026-03-08T04:00:00Z", "2026-03-08T12:00:00Z"),
            ("2026-11-01T03:00:00Z", "2026-11-01T12:00:00Z"),
        ),
        (
            New_York,
            "30 1 * * *",
            ("2026-03-08T04:00:00Z", "2026-03-08T12:00:00Z"),
            ("2026-11-01T03:00:00Z", "2026-11-01T12:00:00Z"),
        ),
        (
            London,
            "30 1 * * *",
            ("2026-03-28T22:00:00Z", "2026-03-29T04:00:00Z"),
            ("2026-10-24T22:00:00Z", "2026-10-25T04:00:00Z"),
        ),
        (
            chrono_tz::Australia::Lord_Howe,
            "15 2 * * *",
            ("2026-10-04T12:30:00Z", "2026-10-04T18:30:00Z"),
            ("2026-04-04T12:00:00Z", "2026-04-04T18:00:00Z"),
        ),
        (
            chrono_tz::Australia::Lord_Howe,
            "45 1 * * *",
            ("2026-10-04T12:30:00Z", "2026-10-04T18:30:00Z"),
            ("2026-04-04T12:00:00Z", "2026-04-04T18:00:00Z"),
        ),
    ] {
        let expr = cron(text);
        for (from, to) in [spring, fall] {
            let found = runs(&expr, zone, from, to);
            assert_eq!(found.len(), 1, "{text} in {zone}, {from}: {found:?}");
            assert_eq!(
                found,
                due_minutes(&expr, zone, from, to),
                "{text} in {zone}"
            );
        }
    }
}

#[test]
fn classification_follows_the_minute_and_hour_fields() {
    for (text, wildcard) in [
        ("* * * * *", true),
        ("*/15 * * * *", true),
        ("0 * * * *", true),
        ("0 */2 * * *", true),
        ("@hourly", true),
        ("30 2 * * *", false),
        ("0,30 1-3 * * *", false),
        ("0 0 * * *", false),
        ("@daily", false),
        ("@weekly", false),
        ("@monthly", false),
        ("@yearly", false),
        ("0 0 * * MON-FRI", false),
        ("0-59/5 8 * * *", false),
    ] {
        assert_eq!(cron(text).wildcard_time, wildcard, "{text}");
    }
}
