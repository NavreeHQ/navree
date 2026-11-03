//! The calendar family at the edges: the years chrono can hold and the calendar can't, zones
//! with odd offsets, times that are not a time of day, rules that are costly to ask, and ids
//! made from names. Nothing here may panic, and nothing a page or a request supplies may
//! be taken for more than it is.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Instant;

use chrono::{DateTime, Datelike, FixedOffset, NaiveDate, NaiveTime, Utc, Weekday};
use chrono_tz::Tz;
use navree_ui::dates::grid::month_grid;
use navree_ui::dates::zoned::{Resolved, resolve};
use navree_ui::dates::{
    Calendar, CalendarMode, DatePicker, DatePreset, DateRange, DateRangePicker, DateRangeQuery,
    DateRules, TimePicker, TimeRules, parse_list,
};
use navree_ui::{Node, render};

fn day(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

fn html(node: impl Into<Node>) -> String {
    render(&node.into())
}

/// Days at and past the edges of what chrono holds and what the calendar draws.
fn extremes() -> Vec<NaiveDate> {
    vec![
        NaiveDate::MIN,
        NaiveDate::MIN.succ_opt().unwrap(),
        day(-1, 12, 31),
        day(0, 1, 1),
        day(0, 12, 31),
        day(1, 1, 1),
        day(9999, 12, 31),
        day(10000, 1, 1),
        NaiveDate::MAX.pred_opt().unwrap(),
        NaiveDate::MAX,
    ]
}

/// Runs `body` and says which case panicked, with all the cases collected.
fn collect_panics(cases: impl IntoIterator<Item = (String, Box<dyn Fn()>)>) -> Vec<String> {
    let mut failed = Vec::new();
    for (name, body) in cases {
        if catch_unwind(AssertUnwindSafe(body)).is_err() {
            failed.push(name);
        }
    }
    failed
}

#[test]
fn a_zone_never_makes_a_range_panic_at_the_edges() {
    let named: Vec<(String, Tz)> = [
        "Asia/Kolkata",
        "Asia/Kathmandu",
        "Pacific/Apia",
        "Australia/Lord_Howe",
        "Etc/GMT-14",
        "Etc/GMT+12",
        "America/New_York",
    ]
    .iter()
    .map(|name| ((*name).to_owned(), name.parse::<Tz>().unwrap()))
    .collect();
    let mut cases: Vec<(String, Box<dyn Fn()>)> = Vec::new();
    for a in extremes() {
        for b in extremes() {
            for (name, zone) in &named {
                let zone = *zone;
                cases.push((
                    format!("{a} {b} {name}"),
                    Box::new(move || {
                        let _ = DateRange::new(a, b).utc_bounds(&zone);
                    }),
                ));
            }
            for hours in [14, -12] {
                let zone = FixedOffset::east_opt(hours * 3600).unwrap();
                cases.push((
                    format!("{a} {b} fixed {hours}"),
                    Box::new(move || {
                        let _ = DateRange::new(a, b).utc_bounds(&zone);
                    }),
                ));
            }
            cases.push((
                format!("{a} {b} utc"),
                Box::new(move || {
                    let _ = DateRange::new(a, b).utc_bounds(&Utc);
                }),
            ));
        }
    }
    let failed = collect_panics(cases);
    assert!(
        failed.is_empty(),
        "{} panicked, such as {:?}",
        failed.len(),
        &failed[..failed.len().min(5)]
    );
}

#[test]
fn a_wall_time_at_the_edge_of_chrono_resolves_without_panicking() {
    let noon = NaiveTime::from_hms_opt(12, 0, 0).unwrap();
    let midnight = NaiveTime::MIN;
    let east = FixedOffset::east_opt(14 * 3600).unwrap();
    for date in extremes() {
        for time in [noon, midnight] {
            let _ = resolve(date, time, &east);
            let _ = resolve(date, time, &Utc);
            let _ = resolve(date, time, &"Pacific/Apia".parse::<Tz>().unwrap());
        }
    }
    // An ordinary day still resolves as it did.
    assert!(matches!(
        resolve(day(2026, 11, 3), noon, &Utc),
        Resolved::Single(_)
    ));
}

#[test]
fn the_last_day_the_zone_holds_is_not_dropped_at_the_upper_limit() {
    let (start, end) = DateRange::single(NaiveDate::MAX).utc_bounds(&Utc);
    assert!(start < end, "{start} to {end}");
    assert_eq!(end, DateTime::<Utc>::MAX_UTC);
    // A day with a successor still spans a whole day.
    let (start, end) = DateRange::single(day(9999, 12, 31)).utc_bounds(&Utc);
    assert_eq!((end - start).num_hours(), 24);
}

#[test]
fn a_range_or_a_list_from_a_request_stays_inside_the_years_the_calendar_draws() {
    for bad in [
        "-262143-01-01",
        "0000-12-31",
        "10000-01-01",
        "+262142-12-31",
    ] {
        assert_eq!(DateRange::parse(bad, "2026-11-03"), None, "from {bad}");
        assert_eq!(DateRange::parse("2026-11-03", bad), None, "to {bad}");
        assert_eq!(parse_list(bad, 10), None, "list {bad}");
        assert_eq!(
            parse_list(&format!("2026-11-03,{bad}"), 10),
            None,
            "list with {bad}"
        );
        let query = DateRangeQuery {
            from: Some(bad.to_owned()),
            to: Some("2026-11-03".to_owned()),
        };
        assert_eq!(query.range(), None, "query {bad}");
    }
    assert!(DateRange::parse("0001-01-01", "9999-12-31").is_some());
    assert_eq!(
        parse_list("9999-12-31,0001-01-01", 10),
        Some(vec![day(1, 1, 1), day(9999, 12, 31)])
    );
    // The reviewer's case: it panicked in a zone with an offset.
    let query = DateRangeQuery {
        from: Some("-262143-01-01".to_owned()),
        to: Some("2026-11-03".to_owned()),
    };
    if let Some(range) = query.range() {
        let _ = range.utc_bounds(&"Asia/Kolkata".parse::<Tz>().unwrap());
    }
}

#[test]
fn a_query_with_a_key_twice_takes_the_first_and_is_never_an_error() {
    let query: DateRangeQuery =
        serde_urlencoded::from_str("from=2026-11-01&from=soon&to=2026-11-03&to=x").unwrap();
    assert_eq!(query.from.as_deref(), Some("2026-11-01"));
    assert_eq!(query.to.as_deref(), Some("2026-11-03"));
    assert_eq!(query.range(), DateRange::parse("2026-11-01", "2026-11-03"));
    // Another key is ignored, a missing one is none.
    let other: DateRangeQuery = serde_urlencoded::from_str("page=2&from=2026-11-01").unwrap();
    assert_eq!(other.range(), None);
    let json: DateRangeQuery =
        serde_json::from_str(r#"{"from":"2026-11-01","from":"2026-11-09","to":null}"#).unwrap();
    assert_eq!(json.from.as_deref(), Some("2026-11-01"));
    assert_eq!(json.to, None);
}

#[test]
fn extreme_days_given_by_a_page_never_panic_anything_that_draws() {
    let mut cases: Vec<(String, Box<dyn Fn()>)> = Vec::new();
    for a in extremes() {
        cases.push((
            format!("calendar today {a}"),
            Box::new(move || {
                html(Calendar::new("c").today(a));
            }),
        ));
        cases.push((
            format!("calendar month {a}"),
            Box::new(move || {
                html(
                    Calendar::new("c")
                        .today(day(2026, 11, 3))
                        .month(a)
                        .months(3),
                );
            }),
        ));
        cases.push((
            format!("calendar selected {a}"),
            Box::new(move || {
                html(Calendar::new("c").today(day(2026, 11, 3)).selected(a));
            }),
        ));
        cases.push((
            format!("calendar range {a}"),
            Box::new(move || {
                html(
                    Calendar::new("c")
                        .today(day(2026, 11, 3))
                        .mode(CalendarMode::Range)
                        .selected_range(DateRange::new(a, day(2026, 11, 3))),
                );
            }),
        ));
        cases.push((
            format!("calendar min max {a}"),
            Box::new(move || {
                html(Calendar::new("c").today(day(2026, 11, 3)).min(a).max(a));
                html(
                    Calendar::new("c").today(day(2026, 11, 3)).rules(
                        DateRules::new()
                            .min(a)
                            .disable_dates([a])
                            .disable_range(a, day(2026, 1, 1))
                            .disable_if(|d| d.day() == 4),
                    ),
                );
            }),
        ));
        cases.push((
            format!("date picker {a}"),
            Box::new(move || {
                html(
                    DatePicker::new("d")
                        .today(a)
                        .value(a)
                        .min(a)
                        .max(a)
                        .months(2),
                );
            }),
        ));
        cases.push((
            format!("range picker today {a}"),
            Box::new(move || {
                html(DateRangePicker::new("a", "b").today(a));
                html(
                    DateRangePicker::new("a", "b")
                        .today(a)
                        .presets(DatePreset::ALL),
                );
            }),
        ));
        cases.push((
            format!("range picker value {a}"),
            Box::new(move || {
                html(
                    DateRangePicker::new("a", "b")
                        .today(day(2026, 11, 3))
                        .min(a)
                        .max(a)
                        .value(DateRange::new(a, day(2026, 11, 3))),
                );
            }),
        ));
        for preset in DatePreset::ALL {
            for week_start in [Weekday::Sun, Weekday::Mon, Weekday::Sat] {
                cases.push((
                    format!("preset {preset:?} {a} {week_start}"),
                    Box::new(move || {
                        let _ = preset.range(a, week_start, Some(a));
                        let _ = preset.range(a, week_start, None);
                    }),
                ));
            }
        }
    }
    for (year, month) in [
        (262_142, 12),
        (-262_143, 1),
        (i32::MAX, 13),
        (i32::MIN, 0),
        (0, 0),
        (10_000, 1),
        (9999, 12),
        (1, 1),
    ] {
        cases.push((
            format!("grid {year}-{month}"),
            Box::new(move || {
                let grid = month_grid(year, month, Weekday::Mon, true);
                assert!(!grid.rows.is_empty());
            }),
        ));
    }
    let failed = collect_panics(cases);
    assert!(
        failed.is_empty(),
        "{} panicked, such as {:?}",
        failed.len(),
        &failed[..failed.len().min(8)]
    );
}

#[test]
fn a_time_that_is_not_a_time_of_day_is_refused() {
    let rules = TimeRules::new()
        .min(NaiveTime::from_hms_opt(9, 0, 0).unwrap())
        .max(NaiveTime::from_hms_opt(17, 0, 0).unwrap());
    let strict = rules.clone().strict_step();
    let leap = NaiveTime::from_hms_nano_opt(16, 59, 59, 1_500_000_000).unwrap();
    let fraction = NaiveTime::from_hms_nano_opt(12, 0, 0, 500_000_000).unwrap();
    for time in [leap, fraction] {
        assert!(rules.check(time).is_err(), "{time:?}");
        assert!(strict.check(time).is_err(), "{time:?} strict");
        assert!(
            TimeRules::new().check(time).is_err(),
            "{time:?} with no limits"
        );
    }
    // Whole seconds are fine.
    assert!(
        rules
            .check(NaiveTime::from_hms_opt(16, 59, 59).unwrap())
            .is_ok()
    );
    // A picker drawn with a leap second doesn't print one.
    let page = html(TimePicker::new("t").value(leap));
    assert!(!page.contains(":60"), "{page}");
}

#[test]
fn asking_whether_a_long_span_is_blocked_costs_what_the_rules_cost_not_the_days() {
    let start = day(2026, 1, 1);
    // 30,000 listed ranges, one a day for 80 years: a day-by-day walk over each range was the cost.
    let mut rules = DateRules::new();
    for n in 0..30_000 {
        let from = start + chrono::Duration::days(n);
        rules = rules.disable_range(
            from + chrono::Duration::days(1),
            from + chrono::Duration::days(1),
        );
    }
    let began = Instant::now();
    let blocked = rules.blocks_span(
        day(2026, 1, 1),
        day(2026, 1, 1) + chrono::Duration::days(39_000),
    );
    let took = began.elapsed();
    assert!(blocked);
    assert!(took.as_millis() < 250, "{took:?}");
    // A long span past the last of them is clear, and says so quickly.
    let began = Instant::now();
    let after = start + chrono::Duration::days(30_005);
    assert!(!rules.blocks_span(after, after + chrono::Duration::days(1000)));
    assert!(began.elapsed().as_millis() < 250);
}

#[test]
fn a_span_is_blocked_by_what_blocks_it_and_nothing_else() {
    // No rules: no span is blocked, however long, and the calendar's own years are the only limit.
    let none = DateRules::new();
    assert!(!none.blocks_span(day(1, 1, 1), day(9999, 12, 31)));
    assert!(!none.blocks_span(day(2026, 1, 1), day(2026, 12, 31)));
    // Limits: a span reaching past them is blocked.
    let limited = DateRules::new().min(day(2026, 3, 1)).max(day(2026, 3, 31));
    assert!(limited.blocks_span(day(2026, 2, 28), day(2026, 3, 5)));
    assert!(limited.blocks_span(day(2026, 3, 28), day(2026, 4, 2)));
    assert!(!limited.blocks_span(day(2026, 3, 1), day(2026, 3, 31)));
    // A weekday: any span of a week or more holds one, a shorter span only if it reaches it.
    let weekends = DateRules::new().disable_weekdays([Weekday::Sat, Weekday::Sun]);
    assert!(weekends.blocks_span(day(2026, 11, 2), day(2026, 11, 9)));
    assert!(!weekends.blocks_span(day(2026, 11, 2), day(2026, 11, 6)));
    assert!(weekends.blocks_span(day(2026, 11, 6), day(2026, 11, 7)));
    // Listed days and ranges, at the ends and in the middle, in either order of ends.
    let listed = DateRules::new()
        .disable_dates([day(2026, 11, 10)])
        .disable_range(day(2026, 12, 5), day(2026, 12, 8));
    assert!(listed.blocks_span(day(2026, 11, 10), day(2026, 11, 10)));
    assert!(listed.blocks_span(day(2026, 11, 12), day(2026, 11, 1)));
    assert!(!listed.blocks_span(day(2026, 11, 11), day(2026, 12, 4)));
    assert!(listed.blocks_span(day(2026, 12, 1), day(2026, 12, 5)));
    assert!(listed.blocks_span(day(2026, 12, 8), day(2026, 12, 20)));
    // A function can only be asked a day at a time. It is asked for a span up to a cap; a
    // longer one counts as blocked, as the browser can't reach one that long.
    let function = DateRules::new().disable_if(|d| d.day() == 31);
    assert!(function.blocks_span(day(2026, 1, 1), day(2026, 1, 31)));
    assert!(!function.blocks_span(day(2026, 2, 1), day(2026, 2, 28)));
    assert!(function.blocks_span(day(2000, 1, 1), day(2200, 1, 1)));
}

#[test]
fn two_names_never_make_the_same_id() {
    let ids: Vec<String> = [
        "start date",
        "start.date",
        "start-date",
        "start_date",
        "start date ",
        "",
        "field",
        "-",
        "a[0]",
        "a-5b0-5d",
    ]
    .iter()
    .map(|name| root_id(&html(DatePicker::new(*name).today(day(2026, 11, 3))), "dp-"))
    .collect();
    let mut sorted = ids.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), ids.len(), "{ids:?}");
    let times: Vec<String> = ["a b", "a.b", "a-b"]
        .iter()
        .map(|name| root_id(&html(TimePicker::new(*name)), "tp-"))
        .collect();
    assert_eq!(
        times
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        3,
        "{times:?}"
    );
    let ranges: Vec<String> = ["a b", "a.b"]
        .iter()
        .map(|name| {
            root_id(
                &html(DateRangePicker::new(*name, "to").today(day(2026, 11, 3))),
                "dr-",
            )
        })
        .collect();
    assert_ne!(ranges[0], ranges[1]);
    // A usual name is its own id, as it was.
    assert_eq!(
        root_id(
            &html(DatePicker::new("due_date").today(day(2026, 11, 3))),
            "dp-"
        ),
        "dp-due_date"
    );
    // Every character an id takes is one the page can use in a selector.
    for id in ids.iter().chain(&times).chain(&ranges) {
        assert!(
            id.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'),
            "{id}"
        );
    }
}

/// The id of the first element whose id starts with `prefix`.
fn root_id(html: &str, prefix: &str) -> String {
    let key = format!(" id=\"{prefix}");
    let from = html
        .find(&key)
        .unwrap_or_else(|| panic!("{key} is not in {html}"))
        + 5;
    html[from..from + html[from..].find('"').unwrap()].to_owned()
}

#[test]
fn a_zone_is_still_resolved_the_same_for_ordinary_days() {
    let ny: Tz = "America/New_York".parse().unwrap();
    let (start, end) = DateRange::new(day(2026, 3, 8), day(2026, 3, 8)).utc_bounds(&ny);
    assert_eq!((end - start).num_hours(), 23);
    let (start, end) = DateRange::new(day(2026, 11, 1), day(2026, 11, 1)).utc_bounds(&ny);
    assert_eq!((end - start).num_hours(), 25);
}
