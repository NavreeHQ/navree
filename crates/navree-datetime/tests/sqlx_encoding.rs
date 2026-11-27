//! SQLx type names, compatibility sets and binary/text encodings for
//! PostgreSQL, MySQL and SQLite, held byte for byte against the golden files
//! recorded from chrono.

#![cfg(any(
    feature = "sqlx-postgres",
    feature = "sqlx-mysql",
    feature = "sqlx-sqlite"
))]

use navree_datetime::{Date, DateTime, Time, Timestamp, Zoned};
#[cfg(any(feature = "sqlx-postgres", feature = "sqlx-sqlite"))]
use navree_datetime::{Offset, Zone};
use sqlx::{Encode, Type, TypeInfo as _};

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

#[cfg(any(feature = "sqlx-postgres", feature = "sqlx-mysql"))]
fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

fn timestamp_of(line: &serde_json::Value) -> Timestamp {
    let secs = number(line, "secs");
    let nanos = u32::try_from(number(line, "nanos")).expect("nanos in u32");
    Timestamp::from_secs_nanos(secs, nanos)
        .unwrap_or_else(|error| panic!("timestamp {line}: {error}"))
}

#[cfg(any(feature = "sqlx-postgres", feature = "sqlx-sqlite"))]
fn fixed_zoned_of(line: &serde_json::Value) -> Zoned {
    let instant = timestamp_of(line);
    let offset = i32::try_from(number(line, "offset")).expect("offset in i32");
    let zone = Zone::fixed(Offset::east(offset).expect("offset in range"));
    instant.in_zone(zone)
}

#[cfg(any(feature = "sqlx-postgres", feature = "sqlx-sqlite"))]
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

fn compatible_names<DB: sqlx::Database, V: Type<DB>>(infos: &[DB::TypeInfo]) -> Vec<String> {
    let mut names = Vec::new();
    for info in infos {
        let name = info.name().to_owned();
        if V::compatible(info) && !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

fn expected_compatible(line: &serde_json::Value) -> Vec<String> {
    line["compatible"]
        .as_array()
        .expect("compatible array")
        .iter()
        .map(|entry| entry.as_str().expect("type name").to_owned())
        .collect()
}

#[cfg(feature = "sqlx-sqlite")]
fn sqlite_text<V: for<'q> Encode<'q, sqlx::Sqlite>>(value: &V) -> Result<String, String> {
    let mut buffer = sqlx::sqlite::SqliteArgumentsBuffer::default();
    let _is_null = Encode::<sqlx::Sqlite>::encode_by_ref(value, &mut buffer)
        .map_err(|error| error.to_string())?;
    let debug = format!("{buffer:?}");
    let text = debug
        .strip_prefix("SqliteArgumentsBuffer([Text(\"")
        .and_then(|rest| rest.strip_suffix("\")])"))
        .filter(|text| !text.contains(['"', '\\']))
        .unwrap_or_else(|| panic!("SQLx bound {debug} to SQLite, not one plain text"));
    Ok(text.to_owned())
}

#[cfg(all(feature = "sqlx-postgres", feature = "sqlx-sqlite"))]
#[test]
fn the_never_expires_value_round_trips() {
    use navree_datetime::__private::{
        decode_postgres_timestamp_binary, decode_sqlite_timestamp_text,
    };
    use sqlx::postgres::{PgArgumentBuffer, Postgres};

    let never_expires = Timestamp::MAX;

    // SQLite text: full nanosecond precision with `+00:00`, byte for byte.
    let bound = sqlite_text(&never_expires).expect("SQLite encode Timestamp::MAX");
    assert_eq!(bound, "+262142-12-31T23:59:59.999999999+00:00");
    let decoded_sqlite =
        decode_sqlite_timestamp_text(&bound).expect("SQLite decode Timestamp::MAX");
    assert_eq!(decoded_sqlite, never_expires);

    // PostgreSQL binary: 8-byte big-endian microseconds since 2000-01-01T00:00:00Z.
    let mut pg_buf = PgArgumentBuffer::default();
    let _ = Encode::<Postgres>::encode_by_ref(&never_expires, &mut pg_buf)
        .expect("Postgres encode Timestamp::MAX");
    assert_eq!(hex(&pg_buf), "71ed5d384399ffff");

    let decoded_pg =
        decode_postgres_timestamp_binary(&pg_buf).expect("Postgres decode Timestamp::MAX");
    let expected_pg =
        Timestamp::from_secs_nanos(never_expires.as_secs(), 999_999_000).expect("micros max");
    assert_eq!(decoded_pg, expected_pg);

    let mut reencoded_buf = PgArgumentBuffer::default();
    let _ = Encode::<Postgres>::encode_by_ref(&decoded_pg, &mut reencoded_buf)
        .expect("Postgres re-encode decoded Timestamp::MAX");
    assert_eq!(hex(&reencoded_buf), "71ed5d384399ffff");
}

#[test]
fn sqlx_type_names_are_chronos() {
    #[cfg(feature = "sqlx-postgres")]
    {
        use sqlx::postgres::{PgHasArrayType, Postgres};

        let infos = vec![
            <bool as Type<Postgres>>::type_info(),
            <i16 as Type<Postgres>>::type_info(),
            <i32 as Type<Postgres>>::type_info(),
            <i64 as Type<Postgres>>::type_info(),
            <f32 as Type<Postgres>>::type_info(),
            <f64 as Type<Postgres>>::type_info(),
            <String as Type<Postgres>>::type_info(),
            <Vec<u8> as Type<Postgres>>::type_info(),
            <Date as Type<Postgres>>::type_info(),
            <Time as Type<Postgres>>::type_info(),
            <DateTime as Type<Postgres>>::type_info(),
            <Timestamp as Type<Postgres>>::type_info(),
        ];

        let mut headers = 0usize;
        for line in recorded("postgres.jsonl") {
            let Some(expected_info) = line.get("type_info").and_then(serde_json::Value::as_str)
            else {
                continue;
            };
            headers += 1;
            let expected_array = text(&line, "array_type_info");
            let expected_compat = expected_compatible(&line);

            match text(&line, "type") {
                "DateTime<Utc>" => {
                    assert_eq!(
                        <Timestamp as Type<Postgres>>::type_info().name(),
                        expected_info
                    );
                    assert_eq!(
                        <Timestamp as PgHasArrayType>::array_type_info().name(),
                        expected_array
                    );
                    assert_eq!(
                        compatible_names::<Postgres, Timestamp>(&infos),
                        expected_compat
                    );
                }
                "DateTime<FixedOffset>" | "DateTime<Tz>" => {
                    assert_eq!(<Zoned as Type<Postgres>>::type_info().name(), expected_info);
                    assert_eq!(
                        <Zoned as PgHasArrayType>::array_type_info().name(),
                        expected_array
                    );
                    assert_eq!(compatible_names::<Postgres, Zoned>(&infos), expected_compat);
                }
                "NaiveDateTime" => {
                    assert_eq!(
                        <DateTime as Type<Postgres>>::type_info().name(),
                        expected_info
                    );
                    assert_eq!(
                        <DateTime as PgHasArrayType>::array_type_info().name(),
                        expected_array
                    );
                    assert_eq!(
                        compatible_names::<Postgres, DateTime>(&infos),
                        expected_compat
                    );
                }
                "NaiveDate" => {
                    assert_eq!(<Date as Type<Postgres>>::type_info().name(), expected_info);
                    assert_eq!(
                        <Date as PgHasArrayType>::array_type_info().name(),
                        expected_array
                    );
                    assert_eq!(compatible_names::<Postgres, Date>(&infos), expected_compat);
                }
                "NaiveTime" => {
                    assert_eq!(<Time as Type<Postgres>>::type_info().name(), expected_info);
                    assert_eq!(
                        <Time as PgHasArrayType>::array_type_info().name(),
                        expected_array
                    );
                    assert_eq!(compatible_names::<Postgres, Time>(&infos), expected_compat);
                }
                other => panic!("unexpected Postgres header type `{other}`"),
            }
        }
        assert_eq!(headers, 6);
    }

    #[cfg(feature = "sqlx-mysql")]
    {
        use sqlx::MySql;

        let infos = vec![
            <bool as Type<MySql>>::type_info(),
            <i8 as Type<MySql>>::type_info(),
            <i16 as Type<MySql>>::type_info(),
            <i32 as Type<MySql>>::type_info(),
            <i64 as Type<MySql>>::type_info(),
            <u64 as Type<MySql>>::type_info(),
            <f32 as Type<MySql>>::type_info(),
            <f64 as Type<MySql>>::type_info(),
            <String as Type<MySql>>::type_info(),
            <Vec<u8> as Type<MySql>>::type_info(),
            <Date as Type<MySql>>::type_info(),
            <Time as Type<MySql>>::type_info(),
            <DateTime as Type<MySql>>::type_info(),
            <Timestamp as Type<MySql>>::type_info(),
        ];

        let mut headers = 0usize;
        for line in recorded("mysql.jsonl") {
            let Some(expected_info) = line.get("type_info").and_then(serde_json::Value::as_str)
            else {
                continue;
            };
            headers += 1;
            let expected_compat = expected_compatible(&line);

            match text(&line, "type") {
                "DateTime<Utc>" => {
                    assert_eq!(
                        <Timestamp as Type<MySql>>::type_info().name(),
                        expected_info
                    );
                    assert_eq!(<Zoned as Type<MySql>>::type_info().name(), expected_info);
                    assert_eq!(
                        compatible_names::<MySql, Timestamp>(&infos),
                        expected_compat
                    );
                    assert_eq!(compatible_names::<MySql, Zoned>(&infos), expected_compat);
                }
                "NaiveDateTime" => {
                    assert_eq!(<DateTime as Type<MySql>>::type_info().name(), expected_info);
                    assert_eq!(compatible_names::<MySql, DateTime>(&infos), expected_compat);
                }
                "NaiveDate" => {
                    assert_eq!(<Date as Type<MySql>>::type_info().name(), expected_info);
                    assert_eq!(compatible_names::<MySql, Date>(&infos), expected_compat);
                }
                "NaiveTime" => {
                    assert_eq!(<Time as Type<MySql>>::type_info().name(), expected_info);
                    assert_eq!(compatible_names::<MySql, Time>(&infos), expected_compat);
                }
                other => panic!("unexpected MySQL header type `{other}`"),
            }
        }
        assert_eq!(headers, 4);
    }

    #[cfg(feature = "sqlx-sqlite")]
    {
        use sqlx::Sqlite;

        let infos = vec![
            <bool as Type<Sqlite>>::type_info(),
            <i32 as Type<Sqlite>>::type_info(),
            <i64 as Type<Sqlite>>::type_info(),
            <f64 as Type<Sqlite>>::type_info(),
            <String as Type<Sqlite>>::type_info(),
            <Vec<u8> as Type<Sqlite>>::type_info(),
            <Date as Type<Sqlite>>::type_info(),
            <Time as Type<Sqlite>>::type_info(),
            <DateTime as Type<Sqlite>>::type_info(),
        ];

        let mut headers = 0usize;
        for line in recorded("sqlite.jsonl") {
            let Some(expected_info) = line.get("type_info").and_then(serde_json::Value::as_str)
            else {
                continue;
            };
            headers += 1;
            let expected_compat = expected_compatible(&line);

            match text(&line, "type") {
                "DateTime<Utc>" => {
                    assert_eq!(
                        <Timestamp as Type<Sqlite>>::type_info().name(),
                        expected_info
                    );
                    assert_eq!(
                        compatible_names::<Sqlite, Timestamp>(&infos),
                        expected_compat
                    );
                }
                "DateTime<FixedOffset>" | "DateTime<Tz>" => {
                    assert_eq!(<Zoned as Type<Sqlite>>::type_info().name(), expected_info);
                    assert_eq!(compatible_names::<Sqlite, Zoned>(&infos), expected_compat);
                }
                "NaiveDateTime" => {
                    assert_eq!(
                        <DateTime as Type<Sqlite>>::type_info().name(),
                        expected_info
                    );
                    assert_eq!(
                        compatible_names::<Sqlite, DateTime>(&infos),
                        expected_compat
                    );
                }
                "NaiveDate" => {
                    assert_eq!(<Date as Type<Sqlite>>::type_info().name(), expected_info);
                    assert_eq!(compatible_names::<Sqlite, Date>(&infos), expected_compat);
                }
                "NaiveTime" => {
                    assert_eq!(<Time as Type<Sqlite>>::type_info().name(), expected_info);
                    assert_eq!(compatible_names::<Sqlite, Time>(&infos), expected_compat);
                }
                other => panic!("unexpected SQLite header type `{other}`"),
            }
        }
        assert_eq!(headers, 6);
    }
}

#[test]
fn sqlx_encodings_are_chronos_bytes() {
    #[cfg(feature = "sqlx-postgres")]
    {
        use navree_datetime::__private::{
            decode_postgres_date_binary, decode_postgres_date_time_binary,
            decode_postgres_time_binary, decode_postgres_timestamp_binary,
        };
        use sqlx::postgres::{PgArgumentBuffer, Postgres};

        let mut checked = 0usize;
        for line in recorded("postgres.jsonl") {
            if line.get("type_info").is_some() || is_leap(&line) {
                continue;
            }
            checked += 1;
            let expected = text(&line, "bytes");
            let mut buf = PgArgumentBuffer::default();

            match text(&line, "type") {
                "DateTime<Utc>" => {
                    let value = timestamp_of(&line);
                    let _ = Encode::<Postgres>::encode_by_ref(&value, &mut buf).unwrap();
                    assert_eq!(hex(&buf), expected, "{line}");
                    let decoded = decode_postgres_timestamp_binary(&buf).unwrap();
                    assert_eq!(decoded.subsec_nanos() % 1_000, 0, "{line}");
                    assert!(
                        (decoded - value).abs() < navree_datetime::Span::micros(1),
                        "{line}: {decoded:?} vs {value:?}"
                    );
                    let mut reencoded = PgArgumentBuffer::default();
                    let _ = Encode::<Postgres>::encode_by_ref(&decoded, &mut reencoded).unwrap();
                    assert_eq!(hex(&reencoded), expected, "{line}");
                }
                "DateTime<FixedOffset>" => {
                    let value = fixed_zoned_of(&line);
                    let _ = Encode::<Postgres>::encode_by_ref(&value, &mut buf).unwrap();
                    assert_eq!(hex(&buf), expected, "{line}");
                }
                "DateTime<Tz>" => {
                    let value = named_zoned_of(&line);
                    let _ = Encode::<Postgres>::encode_by_ref(&value, &mut buf).unwrap();
                    assert_eq!(hex(&buf), expected, "{line}");
                }
                "NaiveDateTime" => {
                    let value = date_time_of(&line);
                    let _ = Encode::<Postgres>::encode_by_ref(&value, &mut buf).unwrap();
                    assert_eq!(hex(&buf), expected, "{line}");
                    let decoded = decode_postgres_date_time_binary(&buf).unwrap();
                    assert_eq!(decoded.time().nanosecond() % 1_000, 0, "{line}");
                    assert!(
                        (decoded.in_utc() - value.in_utc()).abs()
                            < navree_datetime::Span::micros(1),
                        "{line}: {decoded:?} vs {value:?}"
                    );
                    let mut reencoded = PgArgumentBuffer::default();
                    let _ = Encode::<Postgres>::encode_by_ref(&decoded, &mut reencoded).unwrap();
                    assert_eq!(hex(&reencoded), expected, "{line}");
                }
                "NaiveDate" => {
                    let value = date_of(&line);
                    let _ = Encode::<Postgres>::encode_by_ref(&value, &mut buf).unwrap();
                    assert_eq!(hex(&buf), expected, "{line}");
                    assert_eq!(decode_postgres_date_binary(&buf).unwrap(), value, "{line}");
                }
                "NaiveTime" => {
                    let value = time_of(&line);
                    let _ = Encode::<Postgres>::encode_by_ref(&value, &mut buf).unwrap();
                    assert_eq!(hex(&buf), expected, "{line}");
                    assert_eq!(
                        decode_postgres_time_binary(&buf).unwrap(),
                        value
                            .with_nanos(value.nanosecond() / 1_000 * 1_000)
                            .unwrap(),
                        "{line}"
                    );
                }
                other => panic!("unexpected Postgres type `{other}` in {line}"),
            }
        }
        assert!(
            checked > 5_000,
            "expected full postgres.jsonl, checked {checked}"
        );
    }

    #[cfg(feature = "sqlx-mysql")]
    {
        use navree_datetime::__private::{
            decode_mysql_date_bytes, decode_mysql_date_time_bytes, decode_mysql_timestamp_bytes,
        };
        use sqlx::MySql;

        let mut checked = 0usize;
        for line in recorded("mysql.jsonl") {
            if line.get("type_info").is_some() || is_leap(&line) {
                continue;
            }
            checked += 1;
            let mut buf = Vec::new();

            match text(&line, "type") {
                "DateTime<Utc>" => {
                    let value = timestamp_of(&line);
                    let result = Encode::<MySql>::encode_by_ref(&value, &mut buf);
                    if let Some(expected) = line.get("bytes").and_then(serde_json::Value::as_str) {
                        let _ = result.unwrap_or_else(|error| panic!("{line}: {error}"));
                        assert_eq!(hex(&buf), expected, "{line}");
                        let decoded = decode_mysql_timestamp_bytes(&buf).unwrap();
                        assert_eq!(decoded.as_secs(), value.as_secs(), "{line}");
                        assert_eq!(
                            decoded.subsec_nanos(),
                            value.subsec_nanos() / 1_000 * 1_000,
                            "{line}"
                        );
                    } else {
                        assert!(result.is_err(), "expected MySQL error for {line}");
                    }
                }
                "NaiveDateTime" => {
                    let value = date_time_of(&line);
                    let result = Encode::<MySql>::encode_by_ref(&value, &mut buf);
                    if let Some(expected) = line.get("bytes").and_then(serde_json::Value::as_str) {
                        let _ = result.unwrap_or_else(|error| panic!("{line}: {error}"));
                        assert_eq!(hex(&buf), expected, "{line}");
                        let decoded = decode_mysql_date_time_bytes(&buf).unwrap();
                        assert_eq!(decoded.date(), value.date(), "{line}");
                        assert_eq!(
                            decoded.time(),
                            value
                                .time()
                                .with_nanos(value.time().nanosecond() / 1_000 * 1_000)
                                .unwrap(),
                            "{line}"
                        );
                    } else {
                        assert!(result.is_err(), "expected MySQL error for {line}");
                    }
                }
                "NaiveDate" => {
                    let value = date_of(&line);
                    let result = Encode::<MySql>::encode_by_ref(&value, &mut buf);
                    if let Some(expected) = line.get("bytes").and_then(serde_json::Value::as_str) {
                        let _ = result.unwrap_or_else(|error| panic!("{line}: {error}"));
                        assert_eq!(hex(&buf), expected, "{line}");
                        assert_eq!(decode_mysql_date_bytes(&buf).unwrap(), value, "{line}");
                    } else {
                        assert!(result.is_err(), "expected MySQL error for {line}");
                    }
                }
                "NaiveTime" => {
                    let value = time_of(&line);
                    let _ = Encode::<MySql>::encode_by_ref(&value, &mut buf)
                        .unwrap_or_else(|error| panic!("{line}: {error}"));
                    assert_eq!(hex(&buf), text(&line, "bytes"), "{line}");
                }
                other => panic!("unexpected MySQL type `{other}` in {line}"),
            }
        }
        assert!(
            checked > 4_000,
            "expected full mysql.jsonl, checked {checked}"
        );
    }

    #[cfg(feature = "sqlx-sqlite")]
    {
        use navree_datetime::__private::{
            decode_sqlite_date_text, decode_sqlite_date_time_text, decode_sqlite_time_text,
            decode_sqlite_timestamp_text, decode_sqlite_zoned_text,
        };

        let mut checked = 0usize;
        for line in recorded("sqlite.jsonl") {
            if line.get("type_info").is_some() || is_leap(&line) {
                continue;
            }
            checked += 1;

            match text(&line, "type") {
                "DateTime<Utc>" => {
                    let value = timestamp_of(&line);
                    let written = sqlite_text(&value).unwrap();
                    assert_eq!(written, text(&line, "out"), "{line}");
                    assert_eq!(
                        decode_sqlite_timestamp_text(&written).unwrap(),
                        value,
                        "{line}"
                    );
                }
                "DateTime<FixedOffset>" => {
                    let value = fixed_zoned_of(&line);
                    let written = sqlite_text(&value).unwrap();
                    if let Some(expected) = line.get("out").and_then(serde_json::Value::as_str) {
                        assert_eq!(written, expected, "{line}");
                        if value.offset().seconds() % 60 == 0 {
                            let decoded = decode_sqlite_zoned_text(&written).unwrap();
                            assert_eq!(decoded.timestamp(), value.timestamp(), "{line}");
                            assert_eq!(decoded.offset(), value.offset(), "{line}");
                        }
                    } else {
                        assert_eq!(text(&line, "err"), "panic", "{line}");
                    }
                }
                "DateTime<Tz>" => {
                    let value = named_zoned_of(&line);
                    let written = sqlite_text(&value).unwrap();
                    if let Some(expected) = line.get("out").and_then(serde_json::Value::as_str) {
                        assert_eq!(written, expected, "{line}");
                        if value.offset().seconds() % 60 == 0 {
                            let decoded = decode_sqlite_zoned_text(&written).unwrap();
                            assert_eq!(decoded.timestamp(), value.timestamp(), "{line}");
                            assert_eq!(decoded.offset(), value.offset(), "{line}");
                        }
                    } else {
                        assert_eq!(text(&line, "err"), "panic", "{line}");
                    }
                }
                "NaiveDateTime" => {
                    let value = date_time_of(&line);
                    let written = sqlite_text(&value).unwrap();
                    assert_eq!(written, text(&line, "out"), "{line}");
                    assert_eq!(
                        decode_sqlite_date_time_text(&written).unwrap(),
                        value,
                        "{line}"
                    );
                }
                "NaiveDate" => {
                    let value = date_of(&line);
                    let written = sqlite_text(&value).unwrap();
                    assert_eq!(written, text(&line, "out"), "{line}");
                    assert_eq!(decode_sqlite_date_text(&written).unwrap(), value, "{line}");
                }
                "NaiveTime" => {
                    let value = time_of(&line);
                    let written = sqlite_text(&value).unwrap();
                    assert_eq!(written, text(&line, "out"), "{line}");
                    assert_eq!(decode_sqlite_time_text(&written).unwrap(), value, "{line}");
                }
                other => panic!("unexpected SQLite type `{other}` in {line}"),
            }
        }
        assert!(
            checked > 6_000,
            "expected full sqlite.jsonl, checked {checked}"
        );
    }
}
