//! Serde, GraphQL scalar names and BSON conversions for `navree-datetime`,
//! held byte for byte against the golden files recorded from chrono.

#![cfg(feature = "serde")]

use navree_datetime::{Date, DateTime, Month, Offset, Span, Time, Timestamp, Weekday, Zone, Zoned};

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

fn month_of(number: i64) -> Month {
    match number {
        1 => Month::January,
        2 => Month::February,
        3 => Month::March,
        4 => Month::April,
        5 => Month::May,
        6 => Month::June,
        7 => Month::July,
        8 => Month::August,
        9 => Month::September,
        10 => Month::October,
        11 => Month::November,
        12 => Month::December,
        _ => panic!("invalid month {number}"),
    }
}

#[test]
fn serde_writes_and_reads_what_chrono_wrote() {
    let mut written_checked = 0usize;
    for line in recorded("serde.jsonl") {
        if is_leap(&line) {
            continue;
        }
        written_checked += 1;

        match text(&line, "type") {
            "DateTime<Utc>" => {
                let value = timestamp_of(&line);
                let json = serde_json::to_string(&value).unwrap();
                assert_eq!(json, text(&line, "out"), "{line}");
                let decoded: Timestamp = serde_json::from_str(&json).unwrap();
                assert_eq!(decoded, value, "{line}");
            }
            "DateTime<FixedOffset>" => {
                let value = fixed_zoned_of(&line);
                let json = serde_json::to_string(&value).unwrap();
                if let Some(expected) = line.get("out").and_then(serde_json::Value::as_str) {
                    assert_eq!(json, expected, "{line}");
                } else {
                    assert_eq!(text(&line, "err"), "panic", "{line}");
                    assert!(
                        json.starts_with("\"+262143-") || json.starts_with("\"-262144-"),
                        "unexpected boundary JSON `{json}` for {line}"
                    );
                }
                if value.offset().seconds() % 60 == 0 {
                    let decoded: Zoned = serde_json::from_str(&json).unwrap();
                    assert_eq!(decoded.timestamp(), value.timestamp(), "{line}");
                    assert_eq!(decoded.offset(), value.offset(), "{line}");
                }
            }
            "DateTime<Tz>" => {
                let value = named_zoned_of(&line);
                let json = serde_json::to_string(&value).unwrap();
                if let Some(expected) = line.get("out").and_then(serde_json::Value::as_str) {
                    assert_eq!(json, expected, "{line}");
                } else {
                    assert_eq!(text(&line, "err"), "panic", "{line}");
                }
                if value.offset().seconds() % 60 == 0 {
                    let decoded: Zoned = serde_json::from_str(&json).unwrap();
                    assert_eq!(decoded.timestamp(), value.timestamp(), "{line}");
                    assert_eq!(decoded.offset(), value.offset(), "{line}");
                }
            }
            "NaiveDate" => {
                let value = date_of(&line);
                let json = serde_json::to_string(&value).unwrap();
                assert_eq!(json, text(&line, "out"), "{line}");
                let decoded: Date = serde_json::from_str(&json).unwrap();
                assert_eq!(decoded, value, "{line}");
            }
            "NaiveTime" => {
                let value = time_of(&line);
                let json = serde_json::to_string(&value).unwrap();
                assert_eq!(json, text(&line, "out"), "{line}");
                let decoded: Time = serde_json::from_str(&json).unwrap();
                assert_eq!(decoded, value, "{line}");
            }
            "NaiveDateTime" => {
                let value = date_time_of(&line);
                let json = serde_json::to_string(&value).unwrap();
                assert_eq!(json, text(&line, "out"), "{line}");
                let decoded: DateTime = serde_json::from_str(&json).unwrap();
                assert_eq!(decoded, value, "{line}");
            }
            "TimeDelta" => {
                let value = span_of(&line);
                let json = serde_json::to_string(&value).unwrap();
                assert_eq!(json, text(&line, "out"), "{line}");
                let decoded: Span = serde_json::from_str(&json).unwrap();
                assert_eq!(decoded, value, "{line}");
            }
            "Weekday" => {
                let value = weekday_of(number(&line, "from_monday"));
                let json = serde_json::to_string(&value).unwrap();
                assert_eq!(json, text(&line, "out"), "{line}");
                let decoded: Weekday = serde_json::from_str(&json).unwrap();
                assert_eq!(decoded, value, "{line}");
            }
            "Month" => {
                let value = month_of(number(&line, "number"));
                let json = serde_json::to_string(&value).unwrap();
                assert_eq!(json, text(&line, "out"), "{line}");
                let decoded: Month = serde_json::from_str(&json).unwrap();
                assert_eq!(decoded, value, "{line}");
            }
            other => panic!("unexpected type `{other}` in {line}"),
        }
    }
    assert!(
        written_checked > 500,
        "expected full serde.jsonl corpus, checked {written_checked}"
    );

    let mut accepted_checked = 0usize;
    for line in recorded("parse_accept.jsonl") {
        if text(&line, "via") != "serde" {
            continue;
        }
        accepted_checked += 1;
        let input = text(&line, "value");
        let leap = is_leap(&line);

        match text(&line, "type") {
            "DateTime<Utc>" => {
                let parsed: Timestamp =
                    serde_json::from_str(input).unwrap_or_else(|error| panic!("{line}: {error}"));
                let expected_nanos = if leap {
                    999_999_999
                } else {
                    u32::try_from(number(&line, "nanos")).unwrap()
                };
                assert_eq!(parsed.as_secs(), number(&line, "secs"), "{line}");
                assert_eq!(parsed.subsec_nanos(), expected_nanos, "{line}");
            }
            "DateTime<FixedOffset>" => {
                let parsed: Zoned =
                    serde_json::from_str(input).unwrap_or_else(|error| panic!("{line}: {error}"));
                let expected_nanos = if leap {
                    999_999_999
                } else {
                    u32::try_from(number(&line, "nanos")).unwrap()
                };
                let expected_offset = i32::try_from(number(&line, "offset")).unwrap();
                assert!(parsed.zone().is_fixed(), "{line}");
                assert_eq!(
                    parsed.timestamp().as_secs(),
                    number(&line, "secs"),
                    "{line}"
                );
                assert_eq!(parsed.timestamp().subsec_nanos(), expected_nanos, "{line}");
                assert_eq!(parsed.offset().seconds(), expected_offset, "{line}");
            }
            "NaiveDateTime" => {
                let parsed: DateTime =
                    serde_json::from_str(input).unwrap_or_else(|error| panic!("{line}: {error}"));
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
            "NaiveDate" => {
                let parsed: Date =
                    serde_json::from_str(input).unwrap_or_else(|error| panic!("{line}: {error}"));
                assert_eq!(parsed, date_of(&line), "{line}");
            }
            "NaiveTime" => {
                let parsed: Time =
                    serde_json::from_str(input).unwrap_or_else(|error| panic!("{line}: {error}"));
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
            "TimeDelta" => {
                let parsed: Span =
                    serde_json::from_str(input).unwrap_or_else(|error| panic!("{line}: {error}"));
                assert_eq!(parsed, span_of(&line), "{line}");
            }
            "FixedOffset" => {
                let parsed: Offset =
                    serde_json::from_str(input).unwrap_or_else(|error| panic!("{line}: {error}"));
                assert_eq!(
                    i64::from(parsed.seconds()),
                    number(&line, "offset"),
                    "{line}"
                );
            }
            "Weekday" => {
                let parsed: Weekday =
                    serde_json::from_str(input).unwrap_or_else(|error| panic!("{line}: {error}"));
                assert_eq!(
                    i64::from(parsed.days_from_monday()),
                    number(&line, "from_monday"),
                    "{line}"
                );
            }
            "Month" => {
                let parsed: Month =
                    serde_json::from_str(input).unwrap_or_else(|error| panic!("{line}: {error}"));
                assert_eq!(
                    i64::from(parsed.number()),
                    number(&line, "number"),
                    "{line}"
                );
            }
            other => panic!("unexpected serde parse_accept type `{other}` in {line}"),
        }
    }
    assert!(
        accepted_checked > 100,
        "expected serde parse_accept rows, checked {accepted_checked}"
    );

    let mut refused_checked = 0usize;
    for line in recorded("parse_refuse.jsonl") {
        if text(&line, "via") != "serde" {
            continue;
        }
        refused_checked += 1;
        let input = text(&line, "value");

        match text(&line, "type") {
            "DateTime<Utc>" => assert!(
                serde_json::from_str::<Timestamp>(input).is_err(),
                "accepted Timestamp for {line}"
            ),
            "DateTime<FixedOffset>" => assert!(
                serde_json::from_str::<Zoned>(input).is_err(),
                "accepted Zoned for {line}"
            ),
            "NaiveDateTime" => assert!(
                serde_json::from_str::<DateTime>(input).is_err(),
                "accepted DateTime for {line}"
            ),
            "NaiveDate" => assert!(
                serde_json::from_str::<Date>(input).is_err(),
                "accepted Date for {line}"
            ),
            "NaiveTime" => assert!(
                serde_json::from_str::<Time>(input).is_err(),
                "accepted Time for {line}"
            ),
            "TimeDelta" => assert!(
                serde_json::from_str::<Span>(input).is_err(),
                "accepted Span for {line}"
            ),
            "FixedOffset" => assert!(
                serde_json::from_str::<Offset>(input).is_err(),
                "accepted Offset for {line}"
            ),
            "Weekday" => assert!(
                serde_json::from_str::<Weekday>(input).is_err(),
                "accepted Weekday for {line}"
            ),
            "Month" => assert!(
                serde_json::from_str::<Month>(input).is_err(),
                "accepted Month for {line}"
            ),
            other => panic!("unexpected serde parse_refuse type `{other}` in {line}"),
        }
    }
    assert!(
        refused_checked > 100,
        "expected serde parse_refuse rows, checked {refused_checked}"
    );

    let kolkata = Zone::named("Asia/Calcutta").unwrap();
    assert_eq!(serde_json::to_string(&kolkata).unwrap(), "\"Asia/Kolkata\"");
    let decoded_zone: Zone = serde_json::from_str("\"asia/calcutta\"").unwrap();
    assert_eq!(decoded_zone, kolkata);
    assert!(serde_json::from_str::<Zone>("\"Mars/Olympus_Mons\"").is_err());

    let ist = Offset::east(19_800).unwrap();
    assert_eq!(serde_json::to_string(&ist).unwrap(), "\"+05:30\"");
    assert_eq!(serde_json::from_str::<Offset>("\"+05:30\"").unwrap(), ist);
}

#[cfg(feature = "async-graphql")]
#[test]
fn graphql_scalar_names_are_unchanged() {
    use async_graphql::{InputType, OutputType, ScalarType, Value};

    assert_eq!(<Timestamp as InputType>::type_name(), "DateTime");
    assert_eq!(<Timestamp as OutputType>::type_name(), "DateTime");
    assert_eq!(<Zoned as InputType>::type_name(), "DateTime");
    assert_eq!(<Zoned as OutputType>::type_name(), "DateTime");
    assert_eq!(<Date as InputType>::type_name(), "NaiveDate");
    assert_eq!(<Date as OutputType>::type_name(), "NaiveDate");
    assert_eq!(<Time as InputType>::type_name(), "NaiveTime");
    assert_eq!(<Time as OutputType>::type_name(), "NaiveTime");
    assert_eq!(<DateTime as InputType>::type_name(), "NaiveDateTime");
    assert_eq!(<DateTime as OutputType>::type_name(), "NaiveDateTime");
    assert_eq!(<Zone as InputType>::type_name(), "TimeZone");
    assert_eq!(<Zone as OutputType>::type_name(), "TimeZone");

    let at = Timestamp::utc(2026, 11, 25, 4, 0, 0)
        .unwrap()
        .checked_add(Span::millis(250))
        .unwrap();
    let ts_val = ScalarType::to_value(&at);
    assert_eq!(ts_val, Value::String("2026-11-25T04:00:00.250Z".to_owned()));
    assert_eq!(<Timestamp as ScalarType>::parse(ts_val).unwrap(), at);

    let kolkata = Zone::named("Asia/Kolkata").unwrap();
    let zoned = at.in_zone(kolkata);
    let zoned_val = ScalarType::to_value(&zoned);
    assert_eq!(
        zoned_val,
        Value::String("2026-11-25T09:30:00.250+05:30".to_owned())
    );
    let parsed_zoned = <Zoned as ScalarType>::parse(zoned_val).unwrap();
    assert_eq!(parsed_zoned.timestamp(), at);
    assert_eq!(parsed_zoned.offset(), Offset::east(19_800).unwrap());

    let day = Date::new(2026, 11, 25).unwrap();
    let day_val = ScalarType::to_value(&day);
    assert_eq!(day_val, Value::String("2026-11-25".to_owned()));
    assert_eq!(<Date as ScalarType>::parse(day_val).unwrap(), day);

    let time = Time::new(9, 30, 0)
        .unwrap()
        .with_nanos(123_000_000)
        .unwrap();
    let time_val = ScalarType::to_value(&time);
    assert_eq!(time_val, Value::String("09:30:00.123".to_owned()));
    assert_eq!(<Time as ScalarType>::parse(time_val).unwrap(), time);

    let wall = day.at(time);
    let wall_val = ScalarType::to_value(&wall);
    assert_eq!(
        wall_val,
        Value::String("2026-11-25T09:30:00.123".to_owned())
    );
    assert_eq!(<DateTime as ScalarType>::parse(wall_val).unwrap(), wall);

    let zone_val = ScalarType::to_value(&kolkata);
    assert_eq!(zone_val, Value::String("Asia/Kolkata".to_owned()));
    assert_eq!(<Zone as ScalarType>::parse(zone_val).unwrap(), kolkata);
}

#[cfg(feature = "bson")]
#[test]
fn bson_datetime_and_extended_json_round_trip() {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct Dispatch {
        city: String,
        #[serde(with = "navree_datetime::bson::extended::date_time")]
        dispatched_at: Timestamp,
        #[serde(with = "navree_datetime::bson::extended::option_date_time", default)]
        delivered_at: Option<Timestamp>,
    }

    let at = Timestamp::from_millis(1_793_093_400_250).unwrap();
    let bson_dt = ::bson::DateTime::from(at);
    assert_eq!(bson_dt.timestamp_millis(), 1_793_093_400_250);
    assert_eq!(Timestamp::try_from(bson_dt).unwrap(), at);

    let zoned = at.in_zone(Zone::named("Asia/Kolkata").unwrap());
    assert_eq!(::bson::DateTime::from(zoned), bson_dt);

    // `bson::DateTime::MAX` (`i64::MAX` ms, ~292 million years) is past
    // `Timestamp::MAX` and converts as an `OutOfRange` error rather than panicking.
    assert!(Timestamp::try_from(::bson::DateTime::MAX).is_err());

    let dispatch = Dispatch {
        city: "Amritsar".to_owned(),
        dispatched_at: at,
        delivered_at: None,
    };
    let written = serde_json::to_value(&dispatch).unwrap();
    assert_eq!(
        written,
        serde_json::json!({
            "city": "Amritsar",
            "dispatched_at": { "$date": "2026-10-27T09:30:00.250Z" },
            "delivered_at": null,
        })
    );

    for candidate in [
        serde_json::json!({
            "city": "Amritsar",
            "dispatched_at": { "$date": "2026-10-27T09:30:00.250Z" },
        }),
        serde_json::json!({
            "city": "Amritsar",
            "dispatched_at": { "$date": { "$numberLong": "1793093400250" } },
        }),
        serde_json::json!({
            "city": "Amritsar",
            "dispatched_at": { "$date": 1_793_093_400_250_i64 },
        }),
        serde_json::json!({
            "city": "Amritsar",
            "dispatched_at": "2026-10-27T09:30:00.250Z",
            "delivered_at": null,
        }),
    ] {
        let parsed: Dispatch = serde_json::from_value(candidate.clone()).unwrap();
        assert_eq!(parsed, dispatch, "{candidate}");
    }

    assert!(
        serde_json::from_value::<Dispatch>(serde_json::json!({
            "city": "Ludhiana",
            "dispatched_at": "soon",
        }))
        .is_err()
    );
}
