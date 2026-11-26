//! Text: `Display`, `Debug`, RFC 3339, RFC 2822, `strftime` patterns and
//! `str::parse`, held byte for byte against the golden files recorded from
//! chrono before `navree-datetime` took its place.

use std::fmt::{self, Write as _};

use navree_datetime::{
    Date, DateTime, Error, ErrorKind, Month, Offset, Precision, Span, Time, Timestamp, Weekday,
    Zone, Zoned, pattern,
};

fn recorded(file: &str) -> Vec<serde_json::Value> {
    let path = format!("{}/tests/golden/{file}", env!("CARGO_MANIFEST_DIR"));
    let content = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"));
    content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).unwrap_or_else(|error| panic!("{path}: {error}")))
        .collect()
}

fn number(line: &serde_json::Value, key: &str) -> i64 {
    line[key]
        .as_i64()
        .unwrap_or_else(|| panic!("missing integer `{key}` in {line}"))
}

fn text<'a>(line: &'a serde_json::Value, key: &str) -> &'a str {
    line[key]
        .as_str()
        .unwrap_or_else(|| panic!("missing string `{key}` in {line}"))
}

fn is_leap(line: &serde_json::Value) -> bool {
    line.get("leap")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

fn timestamp_of(line: &serde_json::Value) -> Timestamp {
    let secs = number(line, "secs");
    let nanos = u32::try_from(number(line, "nanos")).expect("nanos in u32");
    Timestamp::from_secs_nanos(secs, nanos)
        .unwrap_or_else(|error| panic!("timestamp {line}: {error}"))
}

fn fixed_zoned_of(line: &serde_json::Value) -> Zoned {
    let instant = timestamp_of(line);
    let offset = i32::try_from(number(line, "offset")).expect("offset in i32");
    let zone = Zone::fixed(Offset::east(offset).expect("offset in range"));
    instant.in_zone(zone)
}

fn named_zoned_of(line: &serde_json::Value) -> Zoned {
    let instant = timestamp_of(line);
    let zone = Zone::named(text(line, "zone")).expect("known IANA zone");
    instant.in_zone(zone)
}

fn date_of(line: &serde_json::Value) -> Date {
    let year = i32::try_from(number(line, "year")).expect("year in i32");
    let month = u8::try_from(number(line, "month")).expect("month in u8");
    let day = u8::try_from(number(line, "day")).expect("day in u8");
    Date::new(year, month, day).unwrap_or_else(|error| panic!("date {line}: {error}"))
}

fn time_of(line: &serde_json::Value) -> Time {
    let hour = u8::try_from(number(line, "hour")).expect("hour in u8");
    let minute = u8::try_from(number(line, "minute")).expect("minute in u8");
    let second = u8::try_from(number(line, "second")).expect("second in u8");
    let nanos = u32::try_from(number(line, "nanos")).expect("nanos in u32");
    Time::new(hour, minute, second)
        .and_then(|time| time.with_nanos(nanos))
        .unwrap_or_else(|error| panic!("time {line}: {error}"))
}

fn date_time_of(line: &serde_json::Value) -> DateTime {
    date_of(line).at(time_of(line))
}

fn span_of(line: &serde_json::Value) -> Span {
    let secs = number(line, "secs");
    let nanos = number(line, "nanos");
    if secs < 0 && nanos > 0 {
        Span::seconds(secs + 1) + Span::nanos(nanos - 1_000_000_000)
    } else {
        Span::seconds(secs) + Span::nanos(nanos)
    }
}

fn weekday_of(from_monday: i64) -> Weekday {
    match from_monday {
        0 => Weekday::Monday,
        1 => Weekday::Tuesday,
        2 => Weekday::Wednesday,
        3 => Weekday::Thursday,
        4 => Weekday::Friday,
        5 => Weekday::Saturday,
        6 => Weekday::Sunday,
        _ => panic!("invalid weekday index {from_monday}"),
    }
}

fn format_with_spec<T: fmt::Display + fmt::Debug>(value: &T, spec: &str) -> String {
    match spec {
        "{:>40}" => format!("{value:>40}"),
        "{:<40}" => format!("{value:<40}"),
        "{:^40}" => format!("{value:^40}"),
        "{:*^40}" => format!("{value:*^40}"),
        "{:040}" => format!("{value:040}"),
        "{:.3}" => format!("{value:.3}"),
        "{:.4}" => format!("{value:.4}"),
        "{:+}" => format!("{value:+}"),
        "{:>40?}" => format!("{value:>40?}"),
        "{:#?}" => format!("{value:#?}"),
        "{:>16}" => format!("{value:>16}"),
        "{:<16}" => format!("{value:<16}"),
        "{:^16}" => format!("{value:^16}"),
        "{:*^16}" => format!("{value:*^16}"),
        _ => panic!("unhandled format spec `{spec}`"),
    }
}

fn check_display_row<T: fmt::Display + fmt::Debug>(value: &T, line: &serde_json::Value) {
    if let Some(spec) = line.get("spec").and_then(serde_json::Value::as_str) {
        assert_eq!(format_with_spec(value, spec), text(line, "out"), "{line}");
    } else {
        assert_eq!(value.to_string(), text(line, "out"), "{line}");
        assert_eq!(format!("{value:?}"), text(line, "debug"), "{line}");
    }
}

#[test]
fn display_matches_chrono_for_every_value() {
    let mut checked = 0usize;
    for line in recorded("display.jsonl") {
        if is_leap(&line) {
            continue;
        }
        checked += 1;
        match text(&line, "type") {
            "DateTime<Utc>" => check_display_row(&timestamp_of(&line), &line),
            "DateTime<FixedOffset>" => check_display_row(&fixed_zoned_of(&line), &line),
            "DateTime<Tz>" => check_display_row(&named_zoned_of(&line), &line),
            "NaiveDate" => check_display_row(&date_of(&line), &line),
            "NaiveTime" => check_display_row(&time_of(&line), &line),
            "NaiveDateTime" => check_display_row(&date_time_of(&line), &line),
            "FixedOffset" => {
                let seconds = i32::try_from(number(&line, "offset")).expect("offset in i32");
                let offset = Offset::east(seconds).expect("offset in range");
                check_display_row(&offset, &line);
            }
            "Weekday" => {
                let day = weekday_of(number(&line, "from_monday"));
                check_display_row(&day, &line);
            }
            "Month" => {
                let month = Month::try_from(u8::try_from(number(&line, "number")).unwrap())
                    .expect("month 1..=12");
                assert_eq!(month.to_string(), text(&line, "name"), "{line}");
                assert_eq!(format!("{month:?}"), text(&line, "debug"), "{line}");
            }
            "TimeDelta" => {
                let span = span_of(&line);
                if let Some(spec) = line.get("spec").and_then(serde_json::Value::as_str) {
                    if !spec.contains('?') {
                        assert_eq!(format_with_spec(&span, spec), text(&line, "out"), "{line}");
                    }
                } else {
                    assert_eq!(span.to_string(), text(&line, "out"), "{line}");
                    assert_eq!(
                        format!("{span:?}"),
                        text(&line, "debug").replace("TimeDelta", "Span"),
                        "{line}"
                    );
                }
            }
            other => panic!("unexpected type `{other}` in {line}"),
        }
    }
    assert!(
        checked > 8_000,
        "expected full display corpus, checked {checked}"
    );

    let kolkata = Zone::named("Asia/Calcutta").expect("Asia/Kolkata");
    assert_eq!(kolkata.to_string(), "Asia/Kolkata");
    assert_eq!(format!("{kolkata:?}"), "Zone(\"Asia/Kolkata\")");
    assert_eq!(Zone::UTC.to_string(), "UTC");
    let ist = Zone::fixed(Offset::east(19_800).expect("+05:30"));
    assert_eq!(ist.to_string(), "+05:30");
    assert_eq!(format!("{ist:?}"), "Zone(+05:30)");
}

fn precision_of(name: &str) -> Precision {
    match name {
        "secs" => Precision::Secs,
        "millis" => Precision::Millis,
        "micros" => Precision::Micros,
        "nanos" => Precision::Nanos,
        "auto" => Precision::Auto,
        other => panic!("unexpected precision `{other}`"),
    }
}

#[test]
fn rfc3339_matches_chrono_for_every_option() {
    let mut checked = 0usize;
    for line in recorded("rfc3339.jsonl") {
        if is_leap(&line) {
            continue;
        }
        checked += 1;
        let call = text(&line, "call");
        let actual = match (text(&line, "type"), call) {
            ("DateTime<Utc>", "to_rfc3339") => timestamp_of(&line).to_rfc3339(),
            ("DateTime<Utc>", "to_rfc3339_opts") => timestamp_of(&line).to_rfc3339_with(
                precision_of(text(&line, "seconds")),
                line["z"].as_bool().expect("boolean z"),
            ),
            ("DateTime<FixedOffset>", "to_rfc3339") => fixed_zoned_of(&line).to_rfc3339(),
            ("DateTime<FixedOffset>", "to_rfc3339_opts") => fixed_zoned_of(&line).to_rfc3339_with(
                precision_of(text(&line, "seconds")),
                line["z"].as_bool().expect("boolean z"),
            ),
            ("DateTime<Tz>", "to_rfc3339") => named_zoned_of(&line).to_rfc3339(),
            ("DateTime<Tz>", "to_rfc3339_opts") => named_zoned_of(&line).to_rfc3339_with(
                precision_of(text(&line, "seconds")),
                line["z"].as_bool().expect("boolean z"),
            ),
            (ty, call) => panic!("unexpected ({ty}, {call}) in {line}"),
        };

        if let Some(expected) = line.get("out").and_then(serde_json::Value::as_str) {
            assert_eq!(actual, expected, "{line}");
        } else {
            // Chrono panicked inside `to_rfc3339_opts` when a non-UTC offset
            // pushed the local year to +262143 or -262144; `navree-datetime`
            // writes the signed year cleanly without panicking.
            assert_eq!(text(&line, "err"), "panic", "{line}");
            assert!(
                actual.starts_with("+262143-") || actual.starts_with("-262144-"),
                "unexpected edge output `{actual}` for {line}"
            );
        }
    }
    assert!(
        checked > 4_000,
        "expected full rfc3339 corpus, checked {checked}"
    );
}

#[test]
fn rfc2822_matches_chrono() {
    let mut checked = 0usize;
    for line in recorded("rfc2822.jsonl") {
        if is_leap(&line) {
            continue;
        }
        checked += 1;
        let result = match text(&line, "type") {
            "DateTime<Utc>" => timestamp_of(&line).to_rfc2822(),
            "DateTime<FixedOffset>" => fixed_zoned_of(&line).to_rfc2822(),
            "DateTime<Tz>" => named_zoned_of(&line).to_rfc2822(),
            other => panic!("unexpected type `{other}` in {line}"),
        };

        if let Some(expected) = line.get("out").and_then(serde_json::Value::as_str) {
            assert_eq!(result.expect("year in 0..=9999"), expected, "{line}");
        } else {
            assert_eq!(text(&line, "err"), "panic", "{line}");
            assert_eq!(
                result.expect_err("year outside 0..=9999").kind(),
                ErrorKind::OutOfRange,
                "{line}"
            );
        }
    }
    assert!(
        checked > 1_800,
        "expected full rfc2822 corpus, checked {checked}"
    );
}

fn check_formatted(formatted: navree_datetime::Formatted<'_>, line: &serde_json::Value) {
    if line.get("err").is_some() {
        assert!(formatted.to_text().is_err(), "expected error for {line}");
        let mut sink = String::new();
        assert!(
            write!(&mut sink, "{formatted}").is_err(),
            "expected Display error for {line}"
        );
    } else if let Some(spec) = line.get("spec").and_then(serde_json::Value::as_str) {
        assert_eq!(
            format_with_spec(&formatted, spec),
            text(line, "out"),
            "{line}"
        );
    } else {
        let expected = text(line, "out");
        assert_eq!(formatted.to_string(), expected, "{line}");
        assert_eq!(
            formatted.to_text().expect("valid pattern and fields"),
            expected,
            "{line}"
        );
    }
}

#[test]
fn every_strftime_specifier_formats_as_chrono_did() {
    let mut checked = 0usize;
    for line in recorded("strftime.jsonl") {
        if is_leap(&line) {
            continue;
        }
        checked += 1;
        let pat = text(&line, "pattern");
        match text(&line, "type") {
            "DateTime<Utc>" => check_formatted(timestamp_of(&line).format(pat), &line),
            "DateTime<FixedOffset>" => check_formatted(fixed_zoned_of(&line).format(pat), &line),
            "DateTime<Tz>" => check_formatted(named_zoned_of(&line).format(pat), &line),
            "NaiveDateTime" => check_formatted(date_time_of(&line).format(pat), &line),
            "NaiveDate" => check_formatted(date_of(&line).format(pat), &line),
            "NaiveTime" => check_formatted(time_of(&line).format(pat), &line),
            other => panic!("unexpected type `{other}` in {line}"),
        }
    }
    assert!(
        checked > 22_000,
        "expected full strftime corpus, checked {checked}"
    );
}

#[test]
fn everything_chrono_accepted_is_accepted_as_the_same_value() {
    let mut checked = 0usize;
    for line in recorded("parse_accept.jsonl") {
        let via = text(&line, "via");
        if via == "serde" {
            continue;
        }
        checked += 1;
        let input = text(&line, "value");
        let leap = is_leap(&line);

        match (text(&line, "type"), via) {
            ("DateTime<Utc>", "from_str") => {
                let parsed: Timestamp = input
                    .parse()
                    .unwrap_or_else(|error| panic!("{line}: {error}"));
                let expected_nanos = if leap {
                    999_999_999
                } else {
                    u32::try_from(number(&line, "nanos")).unwrap()
                };
                assert_eq!(parsed.as_secs(), number(&line, "secs"), "{line}");
                assert_eq!(parsed.subsec_nanos(), expected_nanos, "{line}");
            }
            ("DateTime<FixedOffset>", "from_str")
            | ("DateTime<FixedOffset>", "rfc3339")
            | ("DateTime<FixedOffset>", "rfc2822")
            | ("DateTime<FixedOffset>", "pattern") => {
                let (zoned, instant) = match via {
                    "from_str" => (
                        input
                            .parse::<Zoned>()
                            .unwrap_or_else(|error| panic!("{line}: {error}")),
                        input
                            .parse::<Timestamp>()
                            .unwrap_or_else(|error| panic!("{line}: {error}")),
                    ),
                    "rfc3339" => (
                        Zoned::parse_rfc3339(input)
                            .unwrap_or_else(|error| panic!("{line}: {error}")),
                        Timestamp::parse_rfc3339(input)
                            .unwrap_or_else(|error| panic!("{line}: {error}")),
                    ),
                    "rfc2822" => (
                        Zoned::parse_rfc2822(input)
                            .unwrap_or_else(|error| panic!("{line}: {error}")),
                        Timestamp::parse_rfc2822(input)
                            .unwrap_or_else(|error| panic!("{line}: {error}")),
                    ),
                    "pattern" => {
                        let pat = text(&line, "pattern");
                        (
                            Zoned::parse(pat, input)
                                .unwrap_or_else(|error| panic!("{line}: {error}")),
                            Timestamp::parse(pat, input)
                                .unwrap_or_else(|error| panic!("{line}: {error}")),
                        )
                    }
                    _ => unreachable!(),
                };
                let expected_nanos = if leap {
                    999_999_999
                } else {
                    u32::try_from(number(&line, "nanos")).unwrap()
                };
                let expected_offset = i32::try_from(number(&line, "offset")).unwrap();
                assert!(zoned.zone().is_fixed(), "{line}");
                assert_eq!(zoned.timestamp(), instant, "{line}");
                assert_eq!(zoned.timestamp().as_secs(), number(&line, "secs"), "{line}");
                assert_eq!(zoned.timestamp().subsec_nanos(), expected_nanos, "{line}");
                assert_eq!(zoned.offset().seconds(), expected_offset, "{line}");
            }
            ("NaiveDateTime", "from_str") | ("NaiveDateTime", "pattern") => {
                let parsed = if via == "from_str" {
                    input
                        .parse::<DateTime>()
                        .unwrap_or_else(|error| panic!("{line}: {error}"))
                } else {
                    DateTime::parse(text(&line, "pattern"), input)
                        .unwrap_or_else(|error| panic!("{line}: {error}"))
                };
                let expected_nanos = if leap {
                    999_999_999
                } else {
                    u32::try_from(number(&line, "nanos")).unwrap()
                };
                assert_eq!(parsed.date(), date_of(&line), "{line}");
                assert_eq!(
                    i64::from(parsed.time().hour()),
                    number(&line, "hour"),
                    "{line}"
                );
                assert_eq!(
                    i64::from(parsed.time().minute()),
                    number(&line, "minute"),
                    "{line}"
                );
                assert_eq!(
                    i64::from(parsed.time().second()),
                    number(&line, "second"),
                    "{line}"
                );
                assert_eq!(parsed.time().nanosecond(), expected_nanos, "{line}");
            }
            ("NaiveDate", "from_str") | ("NaiveDate", "pattern") => {
                let parsed = if via == "from_str" {
                    input
                        .parse::<Date>()
                        .unwrap_or_else(|error| panic!("{line}: {error}"))
                } else {
                    Date::parse(text(&line, "pattern"), input)
                        .unwrap_or_else(|error| panic!("{line}: {error}"))
                };
                assert_eq!(parsed, date_of(&line), "{line}");
            }
            ("NaiveTime", "from_str") | ("NaiveTime", "pattern") => {
                let parsed = if via == "from_str" {
                    input
                        .parse::<Time>()
                        .unwrap_or_else(|error| panic!("{line}: {error}"))
                } else {
                    Time::parse(text(&line, "pattern"), input)
                        .unwrap_or_else(|error| panic!("{line}: {error}"))
                };
                let expected_nanos = if leap {
                    999_999_999
                } else {
                    u32::try_from(number(&line, "nanos")).unwrap()
                };
                assert_eq!(i64::from(parsed.hour()), number(&line, "hour"), "{line}");
                assert_eq!(
                    i64::from(parsed.minute()),
                    number(&line, "minute"),
                    "{line}"
                );
                assert_eq!(
                    i64::from(parsed.second()),
                    number(&line, "second"),
                    "{line}"
                );
                assert_eq!(parsed.nanosecond(), expected_nanos, "{line}");
            }
            ("FixedOffset", "from_str") => {
                let parsed: Offset = input
                    .parse()
                    .unwrap_or_else(|error| panic!("{line}: {error}"));
                assert_eq!(
                    i64::from(parsed.seconds()),
                    number(&line, "offset"),
                    "{line}"
                );
            }
            ("Weekday", "from_str") => {
                let parsed: Weekday = input
                    .parse()
                    .unwrap_or_else(|error| panic!("{line}: {error}"));
                assert_eq!(
                    i64::from(parsed.days_from_monday()),
                    number(&line, "from_monday"),
                    "{line}"
                );
            }
            ("Month", "from_str") => {
                let parsed: Month = input
                    .parse()
                    .unwrap_or_else(|error| panic!("{line}: {error}"));
                assert_eq!(
                    i64::from(parsed.number()),
                    number(&line, "number"),
                    "{line}"
                );
            }
            (ty, via) => panic!("unexpected ({ty}, {via}) in {line}"),
        }
    }
    assert!(
        checked > 2_500,
        "expected full parse_accept corpus, checked {checked}"
    );

    assert_eq!(
        " asia/calcutta "
            .parse::<Zone>()
            .expect("legacy alias")
            .name(),
        "Asia/Kolkata"
    );
    assert_eq!("gmt".parse::<Zone>().expect("GMT alias"), Zone::UTC);
}

fn assert_parse_refused<T: fmt::Debug>(result: Result<T, Error>, line: &serde_json::Value) {
    let error = match result {
        Ok(value) => panic!("accepted {value:?} for {line}"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), ErrorKind::Parse, "{line}");
}

#[test]
fn everything_chrono_refused_is_refused() {
    let mut checked = 0usize;
    for line in recorded("parse_refuse.jsonl") {
        let via = text(&line, "via");
        if via == "serde" {
            continue;
        }
        checked += 1;
        let input = text(&line, "value");

        match (text(&line, "type"), via) {
            ("DateTime<Utc>", "from_str") => {
                assert_parse_refused(input.parse::<Timestamp>(), &line);
            }
            ("DateTime<FixedOffset>", "from_str") => {
                assert_parse_refused(input.parse::<Zoned>(), &line);
                assert_parse_refused(input.parse::<Timestamp>(), &line);
            }
            ("DateTime<FixedOffset>", "rfc3339") => {
                assert_parse_refused(Zoned::parse_rfc3339(input), &line);
                assert_parse_refused(Timestamp::parse_rfc3339(input), &line);
            }
            ("DateTime<FixedOffset>", "rfc2822") => {
                assert_parse_refused(Zoned::parse_rfc2822(input), &line);
                assert_parse_refused(Timestamp::parse_rfc2822(input), &line);
            }
            ("DateTime<FixedOffset>", "pattern") => {
                let pat = text(&line, "pattern");
                assert_parse_refused(Zoned::parse(pat, input), &line);
                assert_parse_refused(Timestamp::parse(pat, input), &line);
            }
            ("NaiveDateTime", "from_str") => {
                assert_parse_refused(input.parse::<DateTime>(), &line);
            }
            ("NaiveDateTime", "pattern") => {
                assert_parse_refused(DateTime::parse(text(&line, "pattern"), input), &line);
            }
            ("NaiveDate", "from_str") => {
                assert_parse_refused(input.parse::<Date>(), &line);
            }
            ("NaiveDate", "pattern") => {
                assert_parse_refused(Date::parse(text(&line, "pattern"), input), &line);
            }
            ("NaiveTime", "from_str") => {
                assert_parse_refused(input.parse::<Time>(), &line);
            }
            ("NaiveTime", "pattern") => {
                assert_parse_refused(Time::parse(text(&line, "pattern"), input), &line);
            }
            ("FixedOffset", "from_str") => {
                assert_parse_refused(input.parse::<Offset>(), &line);
            }
            ("Weekday", "from_str") => {
                assert_parse_refused(input.parse::<Weekday>(), &line);
            }
            ("Month", "from_str") => {
                assert_parse_refused(input.parse::<Month>(), &line);
            }
            (ty, via) => panic!("unexpected ({ty}, {via}) in {line}"),
        }
    }
    assert!(
        checked > 10_000,
        "expected full parse_refuse corpus, checked {checked}"
    );

    for bad_zone in ["", "   ", "+05:30", "Factory", "Mars/Olympus_Mons"] {
        assert_eq!(
            bad_zone.parse::<Zone>().unwrap_err().kind(),
            ErrorKind::Parse,
            "zone `{bad_zone}`"
        );
    }
}

#[test]
fn a_leap_second_is_read_as_the_last_nanosecond_of_its_minute() {
    let end_of_day = Time::new(23, 59, 59)
        .and_then(|time| time.with_nanos(999_999_999))
        .expect("last nanosecond of day");
    for sample in [
        "23:59:60",
        "23:59:60.0",
        "23:59:60.250",
        "23:59:60.123456789",
        "23:59:60.999999999",
    ] {
        assert_eq!(
            sample.parse::<Time>().expect(sample),
            end_of_day,
            "{sample}"
        );
        assert_eq!(
            Time::parse("%H:%M:%S%.f", sample).expect(sample),
            end_of_day,
            "{sample}"
        );
    }
    assert!("23:59:61".parse::<Time>().is_err());

    let day = Date::new(2016, 12, 31).expect("2016-12-31");
    let expected_wall = day.at(end_of_day);
    assert_eq!(
        "2016-12-31T23:59:60.500"
            .parse::<DateTime>()
            .expect("wall leap"),
        expected_wall
    );
    assert_eq!(
        DateTime::parse("%Y-%m-%d %H:%M:%S%.f", "2016-12-31 23:59:60.75").expect("pattern leap"),
        expected_wall
    );

    let expected_instant = expected_wall.in_utc();
    assert_eq!(
        Timestamp::parse_rfc3339("2016-12-31T23:59:60Z").expect("rfc3339 leap"),
        expected_instant
    );
    assert_eq!(
        Timestamp::parse_rfc3339("2017-01-01T05:29:60.450+05:30").expect("ist rfc3339 leap"),
        expected_instant
    );
    assert_eq!(
        Timestamp::parse_rfc2822("Sat, 31 Dec 2016 23:59:60 +0000").expect("rfc2822 leap"),
        expected_instant
    );

    let zoned = Zoned::parse_rfc3339("2017-01-01T05:29:60.800+05:30").expect("zoned leap");
    assert_eq!(zoned.timestamp(), expected_instant);
    assert_eq!(zoned.offset(), Offset::east(19_800).expect("+05:30"));
    assert_eq!(zoned.to_string(), "2017-01-01 05:29:59.999999999 +05:30");

    let rfc2822_zoned =
        Zoned::parse_rfc2822("Sun, 1 Jan 2017 05:29:60 +0530").expect("zoned rfc2822 leap");
    assert_eq!(rfc2822_zoned.timestamp(), expected_instant);
    assert_eq!(
        rfc2822_zoned.offset(),
        Offset::east(19_800).expect("+05:30")
    );

    let pattern_zoned = Zoned::parse("%d/%m/%Y %H:%M:%S %z", "01/01/2017 05:29:60 +0530")
        .expect("zoned pattern leap");
    assert_eq!(pattern_zoned.timestamp(), expected_instant);

    let leap_rows = recorded("parse_accept.jsonl")
        .into_iter()
        .filter(|line| text(line, "via") != "serde" && is_leap(line))
        .count();
    assert!(
        leap_rows >= 20,
        "expected leap rows in golden corpus, got {leap_rows}"
    );
}

#[test]
fn pattern_check_refuses_what_chrono_refused() {
    let rows = recorded("patterns.jsonl");
    assert_eq!(rows.len(), 693);
    for line in rows {
        let pat = text(&line, "value");
        match text(&line, "out") {
            "ok" => assert!(pattern::check(pat).is_ok(), "expected ok for `{pat}`"),
            "BadFormat" => assert!(pattern::check(pat).is_err(), "expected refusal for `{pat}`"),
            other => panic!("unexpected outcome `{other}` in {line}"),
        }
    }

    let unknown = pattern::check("%Y-%Q").expect_err("%Q is unknown");
    assert_eq!(unknown.position(), 3);
    assert_eq!(unknown.specifier(), "%Q");
    assert_eq!(Error::from(unknown).kind(), ErrorKind::Parse);

    let unfinished = pattern::check("100%").expect_err("trailing %");
    assert_eq!(unfinished.position(), 3);
    assert_eq!(unfinished.specifier(), "%");

    let bad_pad = pattern::check("%-B").expect_err("padding on month name");
    assert_eq!(bad_pad.position(), 0);
    assert_eq!(bad_pad.specifier(), "%-B");
}
