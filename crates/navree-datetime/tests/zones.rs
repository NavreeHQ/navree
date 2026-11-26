//! Zones: the offset a zone keeps at an instant, the instant a time on its
//! clocks is, and the steps of its calendar, at the places where they are
//! easy to get wrong (the night the clocks go forward, the night they go
//! back, a zone that changes at midnight, a change of half an hour, the
//! ends of the range), and against the values recorded from Navree's zones
//! before the tables moved under this crate.

use std::collections::{BTreeSet, HashSet};

use navree_datetime::__private::Entry;
use navree_datetime::{
    Date, DateTime, Disambiguation, ErrorKind, Offset, Span, Time, Timestamp, Zone, Zoned,
};

const MINUTE: i32 = 60;
const HOUR: i32 = 3_600;

fn zone(name: &str) -> Zone {
    Zone::named(name).unwrap_or_else(|error| panic!("{name}: {error}"))
}

fn date(year: i32, month: u8, day: u8) -> Date {
    Date::new(year, month, day).expect("a day that exists")
}

fn time(hour: u8, minute: u8, second: u8) -> Time {
    Time::new(hour, minute, second).expect("a time that exists")
}

fn utc(year: i32, month: u8, day: u8, hour: u8, minute: u8, second: u8) -> Timestamp {
    Timestamp::utc(year, month, day, hour, minute, second).expect("an instant that exists")
}

/// A day and a time on a wall clock.
fn wall(year: i32, month: u8, day: u8, hour: u8, minute: u8, second: u8) -> DateTime {
    date(year, month, day).at(time(hour, minute, second))
}

fn instant(secs: i64, nanos: u32) -> Timestamp {
    Timestamp::from_secs_nanos(secs, nanos).expect("an instant in the range")
}

fn east(seconds: i32) -> Offset {
    Offset::east(seconds).expect("an offset in the range")
}

/// The instant a zone's clocks show `local`, read by `how`.
fn read(zone: Zone, local: DateTime, how: Disambiguation) -> Timestamp {
    zone.to_timestamp(local, how)
        .unwrap_or_else(|error| panic!("{local:?} in {zone:?} by {how:?}: {error}"))
}

/// The kind of error `Disambiguation::Reject` gives for `local`.
fn refused(zone: Zone, local: DateTime) -> ErrorKind {
    match zone.to_timestamp(local, Disambiguation::Reject) {
        Ok(at) => panic!("{local:?} in {zone:?} is {at:?}"),
        Err(error) => error.kind(),
    }
}

/// What a zone's calendar and clocks show at an instant.
fn shown(at: Timestamp, zone: Zone) -> DateTime {
    at.in_zone(zone).date_time()
}

/// The lines of one of the files recorded before chrono left, each a JSON
/// object.
fn recorded(file: &str) -> Vec<serde_json::Value> {
    let path = format!("{}/tests/golden/{file}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"));
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).unwrap_or_else(|error| panic!("{path}: {error}")))
        .collect()
}

fn number(line: &serde_json::Value, key: &str) -> i64 {
    line[key]
        .as_i64()
        .unwrap_or_else(|| panic!("no number `{key}` in {line}"))
}

fn text<'a>(line: &'a serde_json::Value, key: &str) -> &'a str {
    line[key]
        .as_str()
        .unwrap_or_else(|| panic!("no text `{key}` in {line}"))
}

/// The year, month, day, hour, minute, second and nanosecond that canonical
/// text starts with: `2026-01-15T17:30:00.000000000+05:30[Asia/Kolkata]`,
/// `+262143-01-01T05:29:59.999999999+05:30`, `-0001-12-31T00:00:00.000000000Z`.
fn canonical_wall(text: &str) -> Option<(i32, u8, u8, u8, u8, u8, u32)> {
    let (day, clock) = text.split_once('T')?;
    let (sign, day) = match day.strip_prefix('-') {
        Some(day) => (-1, day),
        None => (1, day.strip_prefix('+').unwrap_or(day)),
    };
    let mut day = day.split('-');
    let year: i32 = day.next()?.parse().ok()?;
    let month = day.next()?.parse().ok()?;
    let day_of_month = day.next()?.parse().ok()?;
    if day.next().is_some() {
        return None;
    }
    // `HH:MM:SS.fffffffff`.
    let clock = clock.get(..18)?;
    Some((
        sign * year,
        month,
        day_of_month,
        clock.get(..2)?.parse().ok()?,
        clock.get(3..5)?.parse().ok()?,
        clock.get(6..8)?.parse().ok()?,
        clock.get(9..)?.parse().ok()?,
    ))
}

/// The seven numbers of what a zoned value shows, in the order of
/// [`canonical_wall`].
fn wall_numbers(zoned: Zoned) -> (i32, u8, u8, u8, u8, u8, u32) {
    let (day, clock) = (zoned.date(), zoned.time());
    (
        day.year(),
        day.month_number(),
        day.day(),
        clock.hour(),
        clock.minute(),
        clock.second(),
        clock.nanosecond(),
    )
}

#[test]
fn kolkata_is_530_east_always() {
    let kolkata = zone("Asia/Kolkata");
    let ist = east(5 * HOUR + 30 * MINUTE);

    // Since 15 October 1945 India has kept one time: every month from then
    // to the year 2400, and the last instant there is.
    for year in 1946..=2400 {
        for month in 1..=12 {
            let at = utc(year, month, 1, 0, 0, 0);
            assert_eq!(kolkata.offset_at(at), ist, "{year}-{month}");
            assert_eq!(kolkata.abbreviation_at(at), "IST", "{year}-{month}");
        }
    }
    assert_eq!(kolkata.offset_at(Timestamp::MAX), ist);
    assert_eq!(kolkata.abbreviation_at(Timestamp::MAX), "IST");

    // So no time is skipped and none comes twice: every hour of a day is
    // one instant, five and a half hours before UTC shows it.
    for hour in 0..24 {
        let local = wall(2026, 11, 25, hour, 30, 0);
        let at = local.in_utc() - Span::seconds(19_800);
        for how in [
            Disambiguation::Compatible,
            Disambiguation::Earlier,
            Disambiguation::Later,
            Disambiguation::Reject,
        ] {
            assert_eq!(read(kolkata, local, how), at, "{hour}:30 by {how:?}");
        }
        assert_eq!(shown(at, kolkata), local);
    }

    // Before that the tables hold what the clocks of Kolkata kept: local
    // mean time, Madras time, and an hour of saving in the war.
    let before = [
        (Timestamp::MIN, 5 * HOUR + 53 * MINUTE + 28, "LMT"),
        (utc(1900, 6, 1, 0, 0, 0), 5 * HOUR + 21 * MINUTE + 10, "MMT"),
        (utc(1920, 6, 1, 0, 0, 0), 5 * HOUR + 30 * MINUTE, "IST"),
        (utc(1942, 1, 1, 0, 0, 0), 6 * HOUR + 30 * MINUTE, "+0630"),
        (utc(1942, 7, 1, 0, 0, 0), 5 * HOUR + 30 * MINUTE, "IST"),
        (utc(1944, 1, 1, 0, 0, 0), 6 * HOUR + 30 * MINUTE, "+0630"),
    ];
    for (at, seconds, abbreviation) in before {
        assert_eq!(kolkata.offset_at(at), east(seconds), "{at:?}");
        assert_eq!(kolkata.abbreviation_at(at), abbreviation, "{at:?}");
    }
}

#[test]
fn new_york_spring_gap_is_compatible_later() {
    let new_york = zone("America/New_York");
    assert_eq!(Disambiguation::default(), Disambiguation::Compatible);

    // 8 March 2026: at 02:00 the clocks go to 03:00, so 02:30 is never
    // shown. It is read an hour on: 03:30 of daylight time, 07:30 in UTC.
    let skipped = wall(2026, 3, 8, 2, 30, 0);
    let later = utc(2026, 3, 8, 7, 30, 0);
    assert_eq!(read(new_york, skipped, Disambiguation::Compatible), later);
    assert_eq!(read(new_york, skipped, Disambiguation::Later), later);
    assert_eq!(shown(later, new_york), wall(2026, 3, 8, 3, 30, 0));

    // `Earlier` reads it an hour back: 01:30 of standard time.
    let earlier = utc(2026, 3, 8, 6, 30, 0);
    assert_eq!(read(new_york, skipped, Disambiguation::Earlier), earlier);
    assert_eq!(shown(earlier, new_york), wall(2026, 3, 8, 1, 30, 0));

    // `Reject` says what is wrong, and with which time and zone.
    let error = new_york
        .to_timestamp(skipped, Disambiguation::Reject)
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Gap);
    assert!(error.to_string().contains("2026-03-08 02:30:00"), "{error}");
    assert!(error.to_string().contains("America/New_York"), "{error}");

    // The gap is the hour from 02:00:00 to 02:59:59, and nothing else.
    assert_eq!(
        read(new_york, wall(2026, 3, 8, 1, 59, 59), Disambiguation::Reject),
        utc(2026, 3, 8, 6, 59, 59)
    );
    assert_eq!(refused(new_york, wall(2026, 3, 8, 2, 0, 0)), ErrorKind::Gap);
    assert_eq!(
        refused(new_york, wall(2026, 3, 8, 2, 59, 59)),
        ErrorKind::Gap
    );
    assert_eq!(
        read(new_york, wall(2026, 3, 8, 3, 0, 0), Disambiguation::Reject),
        utc(2026, 3, 8, 7, 0, 0)
    );
    assert_eq!(
        read(
            new_york,
            wall(2026, 3, 8, 2, 0, 0),
            Disambiguation::Compatible
        ),
        utc(2026, 3, 8, 7, 0, 0)
    );
    assert_eq!(
        read(
            new_york,
            wall(2026, 3, 8, 2, 59, 59),
            Disambiguation::Compatible
        ),
        utc(2026, 3, 8, 7, 59, 59)
    );

    // What is below the second comes through as it is.
    let with_a_fraction = date(2026, 3, 8).at(time(2, 30, 0).with_nanos(250_000_000).unwrap());
    let read_on = read(new_york, with_a_fraction, Disambiguation::Compatible);
    assert_eq!(read_on.as_secs(), later.as_secs());
    assert_eq!(read_on.subsec_nanos(), 250_000_000);

    // The offsets either side of the change.
    assert_eq!(
        new_york.offset_at(utc(2026, 3, 8, 6, 59, 59)),
        east(-5 * HOUR)
    );
    assert_eq!(new_york.abbreviation_at(utc(2026, 3, 8, 6, 59, 59)), "EST");
    assert_eq!(new_york.offset_at(utc(2026, 3, 8, 7, 0, 0)), east(-4 * HOUR));
    assert_eq!(new_york.abbreviation_at(utc(2026, 3, 8, 7, 0, 0)), "EDT");
}

#[test]
fn london_fall_fold_compatible_is_earlier() {
    let london = zone("Europe/London");

    // 25 October 2026: at 02:00 of summer time the clocks go back to 01:00,
    // so 01:30 is shown twice: at 00:30 and at 01:30 in UTC.
    let repeated = wall(2026, 10, 25, 1, 30, 0);
    let first = utc(2026, 10, 25, 0, 30, 0);
    let second = utc(2026, 10, 25, 1, 30, 0);
    assert_eq!(read(london, repeated, Disambiguation::Compatible), first);
    assert_eq!(read(london, repeated, Disambiguation::Earlier), first);
    assert_eq!(read(london, repeated, Disambiguation::Later), second);

    let error = london
        .to_timestamp(repeated, Disambiguation::Reject)
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Ambiguous);
    assert!(error.to_string().contains("2026-10-25 01:30:00"), "{error}");
    assert!(error.to_string().contains("Europe/London"), "{error}");

    // Both instants show 01:30: the first in summer time, the second not.
    let in_summer_time = first.in_zone(london);
    assert_eq!(in_summer_time.date_time(), repeated);
    assert_eq!(in_summer_time.offset(), east(HOUR));
    assert_eq!(london.abbreviation_at(first), "BST");
    let in_winter_time = second.in_zone(london);
    assert_eq!(in_winter_time.date_time(), repeated);
    assert_eq!(in_winter_time.offset(), Offset::UTC);
    assert_eq!(london.abbreviation_at(second), "GMT");

    // The fold is the hour from 01:00:00 to 01:59:59, and nothing else.
    assert_eq!(
        read(london, wall(2026, 10, 25, 0, 59, 59), Disambiguation::Reject),
        utc(2026, 10, 24, 23, 59, 59)
    );
    assert_eq!(
        refused(london, wall(2026, 10, 25, 1, 0, 0)),
        ErrorKind::Ambiguous
    );
    assert_eq!(
        read(london, wall(2026, 10, 25, 1, 0, 0), Disambiguation::Earlier),
        utc(2026, 10, 25, 0, 0, 0)
    );
    assert_eq!(
        read(london, wall(2026, 10, 25, 1, 0, 0), Disambiguation::Later),
        utc(2026, 10, 25, 1, 0, 0)
    );
    assert_eq!(
        refused(london, wall(2026, 10, 25, 1, 59, 59)),
        ErrorKind::Ambiguous
    );
    assert_eq!(
        read(
            london,
            wall(2026, 10, 25, 1, 59, 59),
            Disambiguation::Compatible
        ),
        utc(2026, 10, 25, 0, 59, 59)
    );
    assert_eq!(
        read(london, wall(2026, 10, 25, 2, 0, 0), Disambiguation::Reject),
        utc(2026, 10, 25, 2, 0, 0)
    );
}

#[test]
fn lord_howe_half_hour_dst() {
    let lord_howe = zone("Australia/Lord_Howe");
    let standard = east(10 * HOUR + 30 * MINUTE);
    let summer = east(11 * HOUR);

    // The one zone whose clocks move by half an hour.
    let january = utc(2026, 1, 15, 12, 0, 0);
    let july = utc(2026, 7, 15, 12, 0, 0);
    assert_eq!(lord_howe.offset_at(january), summer);
    assert_eq!(lord_howe.abbreviation_at(january), "+11");
    assert_eq!(lord_howe.offset_at(july), standard);
    assert_eq!(lord_howe.abbreviation_at(july), "+1030");

    // 4 October 2026, 02:00: the clocks go to 02:30.
    assert_eq!(lord_howe.offset_at(utc(2026, 10, 3, 15, 29, 59)), standard);
    assert_eq!(lord_howe.offset_at(utc(2026, 10, 3, 15, 30, 0)), summer);
    let skipped = wall(2026, 10, 4, 2, 15, 0);
    assert_eq!(refused(lord_howe, skipped), ErrorKind::Gap);
    let on = read(lord_howe, skipped, Disambiguation::Compatible);
    assert_eq!(on, utc(2026, 10, 3, 15, 45, 0));
    assert_eq!(shown(on, lord_howe), wall(2026, 10, 4, 2, 45, 0));
    let back = read(lord_howe, skipped, Disambiguation::Earlier);
    assert_eq!(back, utc(2026, 10, 3, 15, 15, 0));
    assert_eq!(shown(back, lord_howe), wall(2026, 10, 4, 1, 45, 0));
    // The half hour either side of it is on the clock.
    assert_eq!(
        read(
            lord_howe,
            wall(2026, 10, 4, 1, 59, 59),
            Disambiguation::Reject
        ),
        utc(2026, 10, 3, 15, 29, 59)
    );
    assert_eq!(
        read(lord_howe, wall(2026, 10, 4, 2, 30, 0), Disambiguation::Reject),
        utc(2026, 10, 3, 15, 30, 0)
    );

    // 5 April 2026, 02:00: the clocks go back to 01:30.
    let repeated = wall(2026, 4, 5, 1, 45, 0);
    assert_eq!(refused(lord_howe, repeated), ErrorKind::Ambiguous);
    assert_eq!(
        read(lord_howe, repeated, Disambiguation::Compatible),
        utc(2026, 4, 4, 14, 45, 0)
    );
    assert_eq!(
        read(lord_howe, repeated, Disambiguation::Later),
        utc(2026, 4, 4, 15, 15, 0)
    );

    // So the day of the change is half an hour short, and half an hour
    // long in April.
    let length = |day: Date| {
        let start = day.start_of_day(lord_howe).unwrap().timestamp();
        let end = day
            .tomorrow()
            .unwrap()
            .start_of_day(lord_howe)
            .unwrap()
            .timestamp();
        end - start
    };
    assert_eq!(length(date(2026, 10, 4)), Span::minutes(23 * 60 + 30));
    assert_eq!(length(date(2026, 4, 5)), Span::minutes(24 * 60 + 30));
    assert_eq!(length(date(2026, 7, 15)), Span::hours(24));
}

#[test]
fn chatham_offset_1245() {
    let chatham = zone("Pacific/Chatham");
    let standard = east(12 * HOUR + 45 * MINUTE);
    let summer = east(13 * HOUR + 45 * MINUTE);

    let july = utc(2026, 7, 15, 12, 0, 0);
    let january = utc(2026, 1, 15, 12, 0, 0);
    assert_eq!(chatham.offset_at(july), standard);
    assert_eq!(chatham.abbreviation_at(july), "+1245");
    assert_eq!(chatham.offset_at(january), summer);
    assert_eq!(chatham.abbreviation_at(january), "+1345");
    assert_eq!(shown(july, chatham), wall(2026, 7, 16, 0, 45, 0));
    assert_eq!(shown(january, chatham), wall(2026, 1, 16, 1, 45, 0));

    // 27 September 2026, 02:45: the clocks go to 03:45.
    assert_eq!(chatham.offset_at(utc(2026, 9, 26, 13, 59, 59)), standard);
    assert_eq!(chatham.offset_at(utc(2026, 9, 26, 14, 0, 0)), summer);
    assert_eq!(
        read(chatham, wall(2026, 9, 27, 2, 44, 59), Disambiguation::Reject),
        utc(2026, 9, 26, 13, 59, 59)
    );
    let skipped = wall(2026, 9, 27, 3, 0, 0);
    assert_eq!(refused(chatham, skipped), ErrorKind::Gap);
    let on = read(chatham, skipped, Disambiguation::Compatible);
    assert_eq!(on, utc(2026, 9, 26, 14, 15, 0));
    assert_eq!(shown(on, chatham), wall(2026, 9, 27, 4, 0, 0));
    assert_eq!(
        read(chatham, wall(2026, 9, 27, 3, 45, 0), Disambiguation::Reject),
        utc(2026, 9, 26, 14, 0, 0)
    );

    // A wall clock in Chatham reads back to the instant it showed.
    for at in [july, january, utc(2026, 9, 26, 14, 0, 0)] {
        assert_eq!(
            read(chatham, shown(at, chatham), Disambiguation::Reject),
            at
        );
    }
}

#[test]
fn aliases_map_to_canonical() {
    for (alias, name) in [
        ("Asia/Calcutta", "Asia/Kolkata"),
        ("asia/calcutta", "Asia/Kolkata"),
        ("  ASIA/CALCUTTA ", "Asia/Kolkata"),
        ("US/Eastern", "America/New_York"),
        ("US/Pacific", "America/Los_Angeles"),
        ("Europe/Kiev", "Europe/Kyiv"),
        ("Asia/Saigon", "Asia/Ho_Chi_Minh"),
        ("Asia/Katmandu", "Asia/Kathmandu"),
        ("Asia/Rangoon", "Asia/Yangon"),
        ("America/Buenos_Aires", "America/Argentina/Buenos_Aires"),
        ("America/Godthab", "America/Nuuk"),
        ("Pacific/Ponape", "Pacific/Pohnpei"),
        ("Pacific/Truk", "Pacific/Chuuk"),
        ("Australia/ACT", "Australia/Sydney"),
        ("Asia/Chungking", "Asia/Shanghai"),
        ("Iceland", "Atlantic/Reykjavik"),
        ("Etc/UTC", "UTC"),
        ("Etc/GMT", "UTC"),
        ("GMT", "UTC"),
        ("Zulu", "UTC"),
        ("Universal", "UTC"),
    ] {
        assert_eq!(zone(alias).name(), name, "{alias}");
        assert_eq!(zone(alias), zone(name), "{alias}");
        assert_eq!(Zone::canonical_name(alias), Some(name), "{alias}");
    }
    assert_eq!(zone("utc"), Zone::UTC);
    assert_eq!(Zone::default(), Zone::UTC);
    assert_eq!(Zone::UTC.name(), "UTC");

    // A current name stays as it is, in whatever case it comes, and so does
    // a country's location that shares its clocks with another zone.
    for name in [
        "Europe/Paris",
        "Asia/Kolkata",
        "America/New_York",
        "UTC",
        "Etc/GMT+5",
        "Europe/Oslo",
        "Africa/Accra",
        "Asia/Muscat",
        "Europe/Amsterdam",
    ] {
        assert_eq!(zone(name).name(), name);
        assert_eq!(zone(&name.to_ascii_lowercase()).name(), name);
        assert_eq!(zone(&format!(" {} ", name.to_ascii_uppercase())).name(), name);
        assert_eq!(format!("{:?}", zone(name)), format!("Zone({name:?})"));
    }

    // What is no zone is refused, `Factory` among it.
    for bad in [
        "",
        "  ",
        "Mars/Olympus_Mons",
        "Europe/Paris'; DROP TABLE users; --",
        "+05:30",
        "Europe//Paris",
        "EST5EDT/x",
        "Factory",
        "factory",
    ] {
        let error = Zone::named(bad).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Invalid, "{bad}");
        assert!(
            error.to_string().contains("unknown time zone"),
            "{bad}: {error}"
        );
        assert_eq!(Zone::canonical_name(bad), None, "{bad}");
    }
    let error = Zone::named("Mars/Olympus_Mons").unwrap_err();
    assert_eq!(error.to_string(), "unknown time zone `Mars/Olympus_Mons`");

    // The list of zones has the current names only, in order.
    let all: Vec<&str> = Zone::all().map(Zone::name).collect();
    assert!(all.len() > 300, "{}", all.len());
    assert!(all.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(all.contains(&"Asia/Kolkata"));
    assert!(all.contains(&"UTC"));
    for legacy in ["Asia/Calcutta", "US/Eastern", "GMT", "Factory", ""] {
        assert!(!all.contains(&legacy), "{legacy}");
    }
    for name in all {
        assert_eq!(Zone::canonical_name(name), Some(name));
        assert!(
            name.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"/_+-".contains(&byte)),
            "{name}"
        );
    }
}

#[test]
fn every_zone_and_link_of_the_tables_parses() {
    let current: BTreeSet<&str> = Zone::all().map(Zone::name).collect();
    let entries: Vec<Entry> = Entry::all().collect();
    assert!(entries.len() > current.len());
    assert!(
        entries
            .windows(2)
            .all(|pair| pair[0].name() < pair[1].name())
    );

    let instants = [
        Timestamp::MIN,
        utc(1850, 1, 1, 0, 0, 0),
        utc(1900, 6, 15, 0, 0, 0),
        utc(1942, 7, 1, 0, 0, 0),
        Timestamp::UNIX_EPOCH,
        utc(2000, 2, 29, 12, 0, 0),
        utc(2026, 1, 15, 12, 0, 0),
        utc(2026, 7, 15, 12, 0, 0),
        utc(2400, 1, 15, 12, 0, 0),
        utc(2400, 7, 15, 12, 0, 0),
        Timestamp::MAX,
    ];
    let mut legacy = 0;
    for entry in entries {
        let name = entry.name();
        assert_eq!(Entry::find(name), Some(entry), "{name}");
        assert_eq!(
            Entry::find(&name.to_ascii_lowercase()),
            Some(entry),
            "{name}"
        );
        if name == "Factory" {
            assert!(Zone::named(name).is_err());
            continue;
        }

        // Every other name is a zone, in any case and with space round it.
        let zone = zone(name);
        assert!(!zone.is_fixed(), "{name}");
        assert!(current.contains(zone.name()), "{name} is {}", zone.name());
        assert_eq!(Zone::canonical_name(name), Some(zone.name()), "{name}");
        assert_eq!(
            Zone::named(&name.to_ascii_uppercase()).ok(),
            Some(zone),
            "{name}"
        );
        assert_eq!(
            Zone::named(&format!("\t{} ", name.to_ascii_lowercase())).ok(),
            Some(zone),
            "{name}"
        );
        assert_eq!(Entry::of(zone).map(Entry::name), Some(zone.name()), "{name}");

        // A current name is its own zone. A legacy name is the zone it
        // stands for, which keeps the offset the legacy name kept.
        assert_eq!(current.contains(name), zone.name() == name, "{name}");
        legacy += usize::from(zone.name() != name);
        for at in instants {
            assert_eq!(
                zone.offset_at(at).seconds(),
                entry.span_at(at.as_secs()).period.offset(),
                "{name} at {at:?}"
            );
        }
    }
    assert!(legacy > 100, "{legacy}");
}

#[test]
fn start_of_day_in_a_midnight_gap_is_the_first_instant_that_exists() {
    // Three zones that go forward at midnight: on the day of the change
    // 00:00 is never shown, and the day starts at 01:00.
    let changes = [
        ("America/Havana", date(2026, 3, 8), utc(2026, 3, 8, 5, 0, 0)),
        ("America/Santiago", date(2026, 9, 6), utc(2026, 9, 6, 4, 0, 0)),
        ("Asia/Beirut", date(2026, 3, 29), utc(2026, 3, 28, 22, 0, 0)),
    ];
    for (name, day, first_instant) in changes {
        let zone = zone(name);
        assert_eq!(
            refused(zone, day.at(Time::MIDNIGHT)),
            ErrorKind::Gap,
            "{name}"
        );

        let start = day.start_of_day(zone).unwrap();
        assert_eq!(start.timestamp(), first_instant, "{name}");
        assert_eq!(start.zone(), zone, "{name}");
        assert_eq!(start.date(), day, "{name}");
        assert_eq!(start.time(), time(1, 0, 0), "{name}");

        // Nothing of the day is before it: the nanosecond before is the
        // last of the day before.
        let before = start - Span::nanos(1);
        assert_eq!(before.date(), day.yesterday().unwrap(), "{name}");
        assert_eq!(
            before.time(),
            time(23, 59, 59).with_nanos(999_999_999).unwrap(),
            "{name}"
        );

        // Every instant of the day starts its day there.
        for hours in [0, 1, 11, 22] {
            let during = start + Span::hours(hours);
            assert_eq!(during.date(), day, "{name} + {hours}h");
            assert_eq!(
                during.start_of_day().timestamp(),
                first_instant,
                "{name} + {hours}h"
            );
            assert_eq!(during.start_of_day().zone(), zone, "{name} + {hours}h");
        }

        // The day is 23 hours long, and the day before it ends where it
        // starts.
        let next = day.tomorrow().unwrap().start_of_day(zone).unwrap();
        assert_eq!(
            next.timestamp() - start.timestamp(),
            Span::hours(23),
            "{name}"
        );
        assert_eq!(next.time(), Time::MIDNIGHT, "{name}");
        let previous = day.yesterday().unwrap().start_of_day(zone).unwrap();
        assert_eq!(
            start.timestamp() - previous.timestamp(),
            Span::hours(24),
            "{name}"
        );
    }

    // Where the clocks go back to midnight, midnight comes twice and the
    // day starts at the first: Havana, 1 November 2026, a day of 25 hours.
    let havana = zone("America/Havana");
    let day = date(2026, 11, 1);
    assert_eq!(
        refused(havana, day.at(Time::MIDNIGHT)),
        ErrorKind::Ambiguous
    );
    let start = day.start_of_day(havana).unwrap();
    assert_eq!(start.timestamp(), utc(2026, 11, 1, 4, 0, 0));
    assert_eq!(start.time(), Time::MIDNIGHT);
    assert_eq!(start.offset(), east(-4 * HOUR));
    let second_midnight = start + Span::hours(1);
    assert_eq!(second_midnight.time(), Time::MIDNIGHT);
    assert_eq!(second_midnight.offset(), east(-5 * HOUR));
    assert_eq!(second_midnight.start_of_day().timestamp(), start.timestamp());
    let next = date(2026, 11, 2).start_of_day(havana).unwrap();
    assert_eq!(next.timestamp() - start.timestamp(), Span::hours(25));

    // Where they go back from midnight to 23:00, as in Santiago on 5 April
    // 2026, midnight comes once, after the hour that comes twice.
    let santiago = zone("America/Santiago");
    let start = date(2026, 4, 5).start_of_day(santiago).unwrap();
    assert_eq!(start.timestamp(), utc(2026, 4, 5, 4, 0, 0));
    assert_eq!(start.time(), Time::MIDNIGHT);
    let previous = date(2026, 4, 4).start_of_day(santiago).unwrap();
    assert_eq!(previous.timestamp(), utc(2026, 4, 4, 3, 0, 0));
    assert_eq!(start.timestamp() - previous.timestamp(), Span::hours(25));

    // An ordinary day starts at midnight.
    let kolkata = zone("Asia/Kolkata");
    let start = date(2026, 11, 25).start_of_day(kolkata).unwrap();
    assert_eq!(start.timestamp(), utc(2026, 11, 24, 18, 30, 0));
    assert_eq!(start.date_time(), wall(2026, 11, 25, 0, 0, 0));
    assert_eq!(
        utc(2026, 11, 25, 4, 0, 0)
            .in_zone(kolkata)
            .start_of_day()
            .timestamp(),
        start.timestamp()
    );

    // The ends of the range: a day whose midnight lies outside it has no
    // start, and an instant on such a day starts its day at the first
    // instant there is.
    assert_eq!(
        Date::MIN.start_of_day(Zone::UTC).unwrap().timestamp(),
        Timestamp::MIN
    );
    assert_eq!(
        Date::MIN.start_of_day(kolkata).unwrap_err().kind(),
        ErrorKind::OutOfRange
    );
    assert_eq!(
        Timestamp::MIN.in_zone(kolkata).start_of_day().timestamp(),
        Timestamp::MIN
    );
    let last = Timestamp::MAX.in_zone(kolkata);
    assert_eq!(last.date().year(), 262_143);
    assert_eq!(
        last.start_of_day().timestamp(),
        instant(Timestamp::MAX.as_secs() - 19_799, 0)
    );
}

#[test]
fn add_days_keeps_the_wall_clock_across_dst() {
    let new_york = zone("America/New_York");

    // The evening before the clocks go forward: a day later on the clock is
    // 23 hours later.
    let evening = utc(2026, 3, 8, 1, 0, 0).in_zone(new_york);
    assert_eq!(evening.date_time(), wall(2026, 3, 7, 20, 0, 0));
    let next = evening.add_days(1).unwrap();
    assert_eq!(next.date_time(), wall(2026, 3, 8, 20, 0, 0));
    assert_eq!(next.timestamp(), utc(2026, 3, 9, 0, 0, 0));
    assert_eq!(next.timestamp() - evening.timestamp(), Span::hours(23));
    assert_eq!(next.zone(), new_york);
    assert_eq!(next.offset(), east(-4 * HOUR));
    assert_eq!(next.add_days(-1).unwrap().timestamp(), evening.timestamp());
    assert_eq!(evening.add_days(0).unwrap().timestamp(), evening.timestamp());
    assert_eq!(
        evening.add_days(7).unwrap().date_time(),
        wall(2026, 3, 14, 20, 0, 0)
    );
    assert_eq!(
        evening.add_days(7).unwrap().timestamp(),
        utc(2026, 3, 15, 0, 0, 0)
    );

    // The day before they go back: a day later on the clock is 25 hours.
    let noon = utc(2026, 10, 31, 16, 0, 0).in_zone(new_york);
    assert_eq!(noon.date_time(), wall(2026, 10, 31, 12, 0, 0));
    let next = noon.add_days(1).unwrap();
    assert_eq!(next.date_time(), wall(2026, 11, 1, 12, 0, 0));
    assert_eq!(next.timestamp() - noon.timestamp(), Span::hours(25));

    // A day later is a time the clocks skip: it is read an hour on.
    let night = utc(2026, 3, 7, 7, 30, 0).in_zone(new_york);
    assert_eq!(night.date_time(), wall(2026, 3, 7, 2, 30, 0));
    let next = night.add_days(1).unwrap();
    assert_eq!(next.timestamp(), utc(2026, 3, 8, 7, 30, 0));
    assert_eq!(next.date_time(), wall(2026, 3, 8, 3, 30, 0));

    // A day later is a time the clocks show twice: it is the first.
    let night = utc(2026, 10, 31, 5, 30, 0).in_zone(new_york);
    assert_eq!(night.date_time(), wall(2026, 10, 31, 1, 30, 0));
    let next = night.add_days(1).unwrap();
    assert_eq!(next.timestamp(), utc(2026, 11, 1, 5, 30, 0));
    assert_eq!(next.date_time(), wall(2026, 11, 1, 1, 30, 0));
    assert_eq!(next.offset(), east(-4 * HOUR));

    // What is below the second stays.
    let with_a_fraction = instant(evening.timestamp().as_secs(), 123_456_789).in_zone(new_york);
    let next = with_a_fraction.add_days(1).unwrap();
    assert_eq!(next.timestamp().subsec_nanos(), 123_456_789);
    assert_eq!(next.timestamp().as_secs(), utc(2026, 3, 9, 0, 0, 0).as_secs());

    // In a zone that never moves its clocks, a day is 24 hours.
    let kolkata = zone("Asia/Kolkata");
    let morning = utc(2026, 11, 25, 4, 0, 0).in_zone(kolkata);
    assert_eq!(
        morning.add_days(30).unwrap().timestamp() - morning.timestamp(),
        Span::days(30)
    );

    // Past the range it is an error, not a panic.
    for (zoned, days) in [
        (Timestamp::MAX.in_zone(kolkata), 1),
        (Timestamp::MIN.in_zone(Zone::UTC), -1),
        (evening, i32::MAX),
        (evening, i32::MIN),
        (Timestamp::MAX.in_zone(new_york), i32::MAX),
        (Timestamp::MIN.in_zone(new_york), i32::MIN),
    ] {
        let error = zoned.add_days(days).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::OutOfRange, "{zoned:?} + {days}");
    }
}

#[test]
fn adding_span_days_is_exact() {
    let new_york = zone("America/New_York");

    // The same evening: 24 hours later the clock shows an hour more.
    let evening = utc(2026, 3, 8, 1, 0, 0).in_zone(new_york);
    let later = evening + Span::days(1);
    assert_eq!(later.timestamp(), utc(2026, 3, 9, 1, 0, 0));
    assert_eq!(later.timestamp() - evening.timestamp(), Span::hours(24));
    assert_eq!(later.date_time(), wall(2026, 3, 8, 21, 0, 0));
    assert_eq!(later.zone(), new_york);
    assert_eq!(later.offset(), east(-4 * HOUR));
    assert_eq!(
        later.timestamp(),
        evening.add_days(1).unwrap().timestamp() + Span::hours(1)
    );

    // And back.
    let back = later - Span::days(1);
    assert_eq!(back.timestamp(), evening.timestamp());
    assert_eq!(back.date_time(), wall(2026, 3, 7, 20, 0, 0));
    assert_eq!(back.offset(), east(-5 * HOUR));

    // A span crosses a gap as time, never as a clock: a minute after
    // 01:59:30 is 03:00:30.
    let before_the_gap = utc(2026, 3, 8, 6, 59, 30).in_zone(new_york);
    assert_eq!(before_the_gap.time(), time(1, 59, 30));
    assert_eq!((before_the_gap + Span::minutes(1)).time(), time(3, 0, 30));

    // The forms that answer `None` past the range.
    assert_eq!(
        evening.checked_add(Span::days(1)).map(Zoned::timestamp),
        Some(later.timestamp())
    );
    assert_eq!(
        later.checked_sub(Span::days(1)).map(Zoned::timestamp),
        Some(evening.timestamp())
    );
    assert!(
        Timestamp::MAX
            .in_zone(new_york)
            .checked_add(Span::nanos(1))
            .is_none()
    );
    assert!(
        Timestamp::MIN
            .in_zone(new_york)
            .checked_sub(Span::nanos(1))
            .is_none()
    );
}

#[test]
fn add_months_keeps_the_day_and_the_wall_clock() {
    let kolkata = zone("Asia/Kolkata");
    let new_york = zone("America/New_York");

    // A month after the 31st is the month's last day, at the same time.
    let end_of_january = utc(2026, 1, 31, 4, 30, 0).in_zone(kolkata);
    assert_eq!(end_of_january.date_time(), wall(2026, 1, 31, 10, 0, 0));
    assert_eq!(
        end_of_january.add_months(1).unwrap().date_time(),
        wall(2026, 2, 28, 10, 0, 0)
    );
    assert_eq!(
        end_of_january.add_months(1).unwrap().timestamp(),
        utc(2026, 2, 28, 4, 30, 0)
    );
    assert_eq!(
        utc(2024, 1, 31, 4, 30, 0)
            .in_zone(kolkata)
            .add_months(1)
            .unwrap()
            .date_time(),
        wall(2024, 2, 29, 10, 0, 0)
    );
    assert_eq!(
        end_of_january.add_months(-2).unwrap().date_time(),
        wall(2025, 11, 30, 10, 0, 0)
    );

    // Across a change of the clocks the time shown stays.
    let october = utc(2026, 10, 15, 13, 0, 0).in_zone(new_york);
    assert_eq!(october.date_time(), wall(2026, 10, 15, 9, 0, 0));
    let november = october.add_months(1).unwrap();
    assert_eq!(november.date_time(), wall(2026, 11, 15, 9, 0, 0));
    assert_eq!(november.timestamp(), utc(2026, 11, 15, 14, 0, 0));
    let a_year_on = utc(2026, 3, 15, 16, 0, 0)
        .in_zone(new_york)
        .add_months(12)
        .unwrap();
    assert_eq!(a_year_on.date_time(), wall(2027, 3, 15, 12, 0, 0));
    assert_eq!(a_year_on.timestamp(), utc(2027, 3, 15, 16, 0, 0));

    // A month later is a time the clocks skip: it is read an hour on.
    let february = utc(2026, 2, 8, 7, 30, 0).in_zone(new_york);
    assert_eq!(february.date_time(), wall(2026, 2, 8, 2, 30, 0));
    let march = february.add_months(1).unwrap();
    assert_eq!(march.timestamp(), utc(2026, 3, 8, 7, 30, 0));
    assert_eq!(march.date_time(), wall(2026, 3, 8, 3, 30, 0));

    // Past the range it is an error, not a panic.
    for (zoned, months) in [
        (Timestamp::MAX.in_zone(Zone::UTC), 1),
        (Timestamp::MIN.in_zone(Zone::UTC), -1),
        (february, i32::MAX),
        (february, i32::MIN),
    ] {
        let error = zoned.add_months(months).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::OutOfRange, "{zoned:?} + {months}");
    }
}

#[test]
fn offsets_match_chrono_over_the_golden_sweep() {
    let mut in_a_zone = 0;
    let mut at_an_offset = 0;
    let mut zones = BTreeSet::new();
    for line in recorded("display.jsonl") {
        let kind = line["type"].as_str().unwrap_or_default();
        if kind != "DateTime<Tz>" && kind != "DateTime<FixedOffset>" {
            continue;
        }
        let nanos = number(&line, "nanos");
        if line["leap"].as_bool() == Some(true) || nanos >= 1_000_000_000 {
            // A leap second, which no value here holds.
            continue;
        }
        let at = instant(number(&line, "secs"), nanos as u32);
        let offset = number(&line, "offset");

        let zoned = if kind == "DateTime<Tz>" {
            let name = text(&line, "zone");
            let zone = zone(name);
            zones.insert(zone.name());
            in_a_zone += 1;
            assert_eq!(
                i64::from(zone.offset_at(at).seconds()),
                offset,
                "{name} at {at:?}"
            );
            // `Display` ends with the abbreviation: `2026-01-15 17:30:00 IST`.
            if line.get("spec").is_none()
                && let Some(out) = line["out"].as_str()
            {
                assert_eq!(
                    out.rsplit(' ').next(),
                    Some(zone.abbreviation_at(at)),
                    "{name} at {at:?}: {out}"
                );
            }
            at.in_zone(zone)
        } else {
            at_an_offset += 1;
            let fixed = Zone::fixed(east(offset as i32));
            assert_eq!(fixed.abbreviation_at(at), "");
            at.in_zone(fixed)
        };

        assert_eq!(zoned.timestamp(), at);
        assert_eq!(i64::from(zoned.offset().seconds()), offset, "{line}");
        // The canonical text starts with what the calendar and the clock
        // show, worked out from the seconds by the program that recorded it.
        let value = text(&line, "value");
        assert_eq!(
            canonical_wall(value),
            Some(wall_numbers(zoned)),
            "{value}: {zoned:?}"
        );
    }

    assert!(in_a_zone > 100, "{in_a_zone} values in a zone");
    assert!(at_an_offset > 20, "{at_an_offset} values at an offset");
    for name in [
        "Asia/Kolkata",
        "America/New_York",
        "Europe/London",
        "Australia/Lord_Howe",
        "Pacific/Chatham",
        "America/Havana",
    ] {
        assert!(zones.contains(name), "no value in {name}");
    }
}

#[test]
fn wall_clock_times_and_days_are_the_instants_navree_time_gave() {
    let (mut once, mut twice, mut never, mut days) = (0, 0, 0, 0);
    for line in recorded("local.jsonl") {
        let zone = zone(text(&line, "zone"));
        let day = date(
            number(&line, "year") as i32,
            number(&line, "month") as u8,
            number(&line, "day") as u8,
        );
        match text(&line, "type") {
            "LocalResult" => {
                let local = day.at(time(
                    number(&line, "hour") as u8,
                    number(&line, "minute") as u8,
                    number(&line, "second") as u8,
                ));
                // What navree-time made of it before the tables moved.
                let was = number(&line, "to_utc");
                let compatible = read(zone, local, Disambiguation::Compatible);
                assert_eq!(compatible.as_secs(), was, "{line}");

                match text(&line, "out") {
                    "single" => {
                        once += 1;
                        let at = instant(number(&line, "secs"), 0);
                        assert_eq!(compatible, at, "{line}");
                        for how in [
                            Disambiguation::Earlier,
                            Disambiguation::Later,
                            Disambiguation::Reject,
                        ] {
                            assert_eq!(read(zone, local, how), at, "{how:?}: {line}");
                        }
                        assert_eq!(
                            i64::from(zone.offset_at(at).seconds()),
                            number(&line, "offset"),
                            "{line}"
                        );
                        assert_eq!(shown(at, zone), local, "{line}");
                    }
                    "ambiguous" => {
                        twice += 1;
                        let earlier = instant(number(&line, "secs"), 0);
                        let later = instant(number(&line, "later_secs"), 0);
                        assert!(earlier < later, "{line}");
                        assert_eq!(compatible, earlier, "{line}");
                        assert_eq!(
                            read(zone, local, Disambiguation::Earlier),
                            earlier,
                            "{line}"
                        );
                        assert_eq!(read(zone, local, Disambiguation::Later), later, "{line}");
                        assert_eq!(refused(zone, local), ErrorKind::Ambiguous, "{line}");
                        assert_eq!(
                            i64::from(zone.offset_at(earlier).seconds()),
                            number(&line, "offset"),
                            "{line}"
                        );
                        assert_eq!(
                            i64::from(zone.offset_at(later).seconds()),
                            number(&line, "later_offset"),
                            "{line}"
                        );
                        assert_eq!(shown(earlier, zone), local, "{line}");
                        assert_eq!(shown(later, zone), local, "{line}");
                    }
                    "none" => {
                        never += 1;
                        assert_eq!(refused(zone, local), ErrorKind::Gap, "{line}");
                        let later = read(zone, local, Disambiguation::Later);
                        assert_eq!(later, compatible, "{line}");
                        let earlier = read(zone, local, Disambiguation::Earlier);
                        assert!(earlier < later, "{line}");
                        // One lands as far after the time as the other
                        // before it: by the length of the gap.
                        let on_the_clock = |at: Timestamp| shown(at, zone).in_utc().as_secs();
                        let here = local.in_utc().as_secs();
                        assert!(on_the_clock(earlier) < here, "{line}");
                        assert!(here < on_the_clock(later), "{line}");
                        assert_eq!(
                            on_the_clock(later) - here,
                            here - on_the_clock(earlier),
                            "{line}"
                        );
                    }
                    other => panic!("`out` is {other}: {line}"),
                }
            }
            "LocalDay" => {
                days += 1;
                let (start, end) = (number(&line, "start"), number(&line, "end"));
                let first = day.start_of_day(zone).unwrap();
                assert_eq!(first.timestamp(), instant(start, 0), "{line}");
                let next = day.tomorrow().unwrap().start_of_day(zone).unwrap();
                assert_eq!(next.timestamp(), instant(end, 0), "{line}");
                // Every instant of the day is on it, and starts its day
                // where the day starts.
                if end > start {
                    for secs in [start, start + (end - start) / 2, end - 1] {
                        let during = instant(secs, 0).in_zone(zone);
                        assert_eq!(during.date(), day, "{secs}: {line}");
                        assert_eq!(
                            during.start_of_day().timestamp(),
                            first.timestamp(),
                            "{secs}: {line}"
                        );
                    }
                }
            }
            other => panic!("`type` is {other}: {line}"),
        }
    }
    assert!(once > 100, "{once} times shown once");
    assert!(twice > 50, "{twice} times shown twice");
    assert!(never > 50, "{never} times never shown");
    assert!(days > 100, "{days} days");
}

#[test]
fn a_whole_day_the_clocks_skip_has_no_instant() {
    // Samoa went from 29 December 2011 straight to the 31st.
    let apia = zone("Pacific/Apia");
    let noon = wall(2011, 12, 30, 12, 0, 0);
    assert_eq!(refused(apia, noon), ErrorKind::Gap);

    let on = read(apia, noon, Disambiguation::Compatible);
    assert_eq!(on, utc(2011, 12, 30, 22, 0, 0));
    assert_eq!(shown(on, apia), wall(2011, 12, 31, 12, 0, 0));
    let back = read(apia, noon, Disambiguation::Earlier);
    assert_eq!(back, utc(2011, 12, 29, 22, 0, 0));
    assert_eq!(shown(back, apia), wall(2011, 12, 29, 12, 0, 0));

    // The day starts where the next one does: it has no length.
    let skipped = date(2011, 12, 30).start_of_day(apia).unwrap();
    let next = date(2011, 12, 31).start_of_day(apia).unwrap();
    assert_eq!(skipped.timestamp(), utc(2011, 12, 30, 10, 0, 0));
    assert_eq!(skipped.timestamp(), next.timestamp());
    assert_eq!(skipped.date(), date(2011, 12, 31));
    assert_eq!(
        (skipped - Span::seconds(1)).date_time(),
        wall(2011, 12, 29, 23, 59, 59)
    );
}

#[test]
fn a_zone_far_past_its_last_change_follows_its_standing_rule() {
    // The year 2400 is centuries past every change on record.
    let winter = utc(2400, 1, 15, 12, 0, 0);
    let summer = utc(2400, 7, 15, 12, 0, 0);
    for (name, in_january, in_july) in [
        ("America/New_York", -5 * HOUR, -4 * HOUR),
        ("Europe/London", 0, HOUR),
        ("Europe/Dublin", 0, HOUR),
        // Rules that are not "the nth Sunday": Friday on or after the 23rd,
        // Saturday on or before the 30th, 24:00, and a change of two hours.
        ("Asia/Jerusalem", 2 * HOUR, 3 * HOUR),
        ("Asia/Hebron", 2 * HOUR, 3 * HOUR),
        ("Africa/Cairo", 2 * HOUR, 3 * HOUR),
        ("Antarctica/Troll", 0, 2 * HOUR),
        // South of the equator the summer is in January.
        ("Australia/Lord_Howe", 11 * HOUR, 10 * HOUR + 30 * MINUTE),
        ("Pacific/Chatham", 13 * HOUR + 45 * MINUTE, 12 * HOUR + 45 * MINUTE),
        // And a zone with no rule keeps its last offset.
        ("Asia/Kolkata", 5 * HOUR + 30 * MINUTE, 5 * HOUR + 30 * MINUTE),
    ] {
        let zone = zone(name);
        assert_eq!(zone.offset_at(winter), east(in_january), "{name}");
        assert_eq!(zone.offset_at(summer), east(in_july), "{name}");
    }

    // Ireland's standard time is its summer time: the saving is in winter,
    // and below zero.
    let dublin = zone("Europe/Dublin");
    assert_eq!(dublin.abbreviation_at(winter), "GMT");
    assert_eq!(dublin.abbreviation_at(summer), "IST");

    // In 2400, 1 March is a Wednesday, so the clocks of New York go
    // forward on 12 March and back on 5 November.
    let new_york = zone("America/New_York");
    assert_eq!(
        refused(new_york, wall(2400, 3, 12, 2, 30, 0)),
        ErrorKind::Gap
    );
    assert_eq!(
        read(
            new_york,
            wall(2400, 3, 12, 2, 30, 0),
            Disambiguation::Compatible
        ),
        utc(2400, 3, 12, 7, 30, 0)
    );
    assert_eq!(
        refused(new_york, wall(2400, 11, 5, 1, 30, 0)),
        ErrorKind::Ambiguous
    );
    assert_eq!(
        read(new_york, wall(2400, 3, 12, 1, 59, 59), Disambiguation::Reject),
        utc(2400, 3, 12, 6, 59, 59)
    );

    // The rule holds to the last instant there is, and the first instant
    // is in the local mean time the zone started with.
    assert_eq!(new_york.offset_at(Timestamp::MAX), east(-5 * HOUR));
    assert_eq!(new_york.abbreviation_at(Timestamp::MAX), "EST");
    assert_eq!(
        new_york.offset_at(Timestamp::MIN),
        east(-(4 * HOUR + 56 * MINUTE + 2))
    );
    assert_eq!(new_york.abbreviation_at(Timestamp::MIN), "LMT");
}

#[test]
fn the_ends_of_the_range_have_a_wall_clock() {
    let kolkata = zone("Asia/Kolkata");
    let new_york = zone("America/New_York");

    // Ahead of UTC the last instant is on the day after the last day.
    let last = Timestamp::MAX.in_zone(kolkata);
    assert_eq!(
        wall_numbers(last),
        (262_143, 1, 1, 5, 29, 59, 999_999_999)
    );
    assert_eq!(
        read(kolkata, last.date_time(), Disambiguation::Reject),
        Timestamp::MAX
    );

    // Behind UTC the first instant is on the day before the first day.
    let first = Timestamp::MIN.in_zone(new_york);
    assert_eq!(wall_numbers(first), (-262_144, 12, 31, 19, 3, 58, 0));
    assert_eq!(
        read(new_york, first.date_time(), Disambiguation::Reject),
        Timestamp::MIN
    );

    // The furthest an offset goes: two days past the last day.
    let furthest = Zone::fixed(east(25 * HOUR + 59 * MINUTE + 59));
    assert_eq!(
        wall_numbers(Timestamp::MAX.in_zone(furthest)),
        (262_143, 1, 2, 1, 59, 58, 999_999_999)
    );
    let furthest_west = Zone::fixed(Offset::west(25 * HOUR + 59 * MINUTE + 59).unwrap());
    assert_eq!(
        wall_numbers(Timestamp::MIN.in_zone(furthest_west)),
        (-262_144, 12, 30, 22, 0, 1, 0)
    );

    // A time on a clock whose instant is past the range is refused.
    let error = new_york
        .to_timestamp(
            Date::MAX.at(time(23, 59, 59)),
            Disambiguation::Compatible,
        )
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::OutOfRange);
    let error = kolkata
        .to_timestamp(Date::MIN.at(Time::MIDNIGHT), Disambiguation::Compatible)
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::OutOfRange);
}

#[test]
fn a_fixed_zone_keeps_one_offset() {
    let offset = east(5 * HOUR + 30 * MINUTE);
    let fixed = Zone::fixed(offset);
    assert!(fixed.is_fixed());
    assert_eq!(fixed.name(), "");
    assert!(fixed.countries().is_empty());
    for at in [
        Timestamp::MIN,
        Timestamp::UNIX_EPOCH,
        utc(2026, 7, 15, 12, 0, 0),
        Timestamp::MAX,
    ] {
        assert_eq!(fixed.offset_at(at), offset);
        assert_eq!(fixed.abbreviation_at(at), "");
        assert_eq!(at.in_zone(fixed).offset(), offset);
    }

    // No time is skipped and none comes twice.
    let local = wall(2026, 3, 8, 2, 30, 0);
    for how in [
        Disambiguation::Compatible,
        Disambiguation::Earlier,
        Disambiguation::Later,
        Disambiguation::Reject,
    ] {
        assert_eq!(read(fixed, local, how), utc(2026, 3, 7, 21, 0, 0), "{how:?}");
    }
    let zoned = utc(2026, 3, 7, 21, 0, 0).in_zone(fixed);
    assert_eq!(zoned.date_time(), local);
    assert_eq!(
        zoned.add_days(1).unwrap().timestamp() - zoned.timestamp(),
        Span::hours(24)
    );
    assert_eq!(
        zoned.start_of_day().timestamp(),
        utc(2026, 3, 7, 18, 30, 0)
    );

    // Two fixed zones are the same zone when their offsets are, and a
    // fixed zone is never a named one: not Kolkata, whose offset this is,
    // and not UTC at no offset at all.
    assert_eq!(fixed, Zone::fixed(east(19_800)));
    assert_ne!(fixed, Zone::fixed(east(19_801)));
    assert_ne!(fixed, zone("Asia/Kolkata"));
    assert_ne!(Zone::fixed(Offset::UTC), Zone::UTC);
    assert!(!Zone::UTC.is_fixed());
    assert_eq!(Zone::UTC.name(), "UTC");
    assert_eq!(Zone::UTC.abbreviation_at(Timestamp::UNIX_EPOCH), "UTC");
    assert_eq!(Zone::UTC.offset_at(Timestamp::MAX), Offset::UTC);
    assert_eq!(Entry::of(fixed), None);
    assert_eq!(Entry::of(Zone::UTC), Some(Entry::UTC));
}

#[test]
fn a_zoned_value_is_its_instant_seen_in_a_zone() {
    let kolkata = zone("Asia/Kolkata");
    let new_york = zone("America/New_York");
    let at = utc(2026, 1, 15, 12, 0, 0);

    let in_kolkata = at.in_zone(kolkata);
    assert_eq!(in_kolkata.timestamp(), at);
    assert_eq!(in_kolkata.zone(), kolkata);
    assert_eq!(in_kolkata.offset(), east(19_800));
    assert_eq!(in_kolkata.date(), date(2026, 1, 15));
    assert_eq!(in_kolkata.time(), time(17, 30, 0));
    assert_eq!(in_kolkata.date_time(), wall(2026, 1, 15, 17, 30, 0));
    let made = Zoned::new(at, kolkata);
    assert_eq!(
        (made.timestamp(), made.zone(), made.offset()),
        (at, kolkata, east(19_800))
    );

    // The same instant in another zone: another day and another clock.
    let in_new_york = in_kolkata.with_zone(new_york);
    assert_eq!(in_new_york.timestamp(), at);
    assert_eq!(in_new_york.zone(), new_york);
    assert_eq!(in_new_york.date_time(), wall(2026, 1, 15, 7, 0, 0));
    let late = utc(2026, 1, 15, 20, 0, 0);
    assert_eq!(late.in_zone(kolkata).date(), date(2026, 1, 16));
    assert_eq!(late.in_zone(new_york).date(), date(2026, 1, 15));

    // Equal, ordered and hashed by the instant, whatever the zone.
    let in_utc = at.in_zone(Zone::UTC);
    assert_eq!(in_kolkata, in_utc);
    assert_eq!(in_kolkata, in_new_york);
    assert_ne!(in_kolkata.zone(), in_utc.zone());
    let seen: HashSet<Zoned> = [in_kolkata, in_utc, in_new_york].into_iter().collect();
    assert_eq!(seen.len(), 1);
    let a_second_later = (at + Span::seconds(1)).in_zone(new_york);
    assert!(in_kolkata < a_second_later);
    assert!(a_second_later > in_utc);
    let mut sorted = [a_second_later, in_kolkata, late.in_zone(Zone::UTC)];
    sorted.sort();
    assert_eq!(
        sorted.map(Zoned::timestamp),
        [at, at + Span::seconds(1), late]
    );

    // Before the epoch, and with a fraction of a second.
    let old = instant(-1, 750_000_000).in_zone(kolkata);
    assert_eq!(old.date(), date(1970, 1, 1));
    assert_eq!(
        old.time(),
        time(5, 29, 59).with_nanos(750_000_000).unwrap()
    );
}

#[test]
fn countries_have_their_zones() {
    for (country, name) in [
        ("IN", "Asia/Kolkata"),
        ("in", "Asia/Kolkata"),
        (" In ", "Asia/Kolkata"),
        ("US", "America/New_York"),
        ("AE", "Asia/Dubai"),
        ("FR", "Europe/Paris"),
        ("GB", "Europe/London"),
        ("SA", "Asia/Riyadh"),
        ("NO", "Europe/Oslo"),
        ("UA", "Europe/Kyiv"),
        ("AU", "Australia/Sydney"),
    ] {
        assert_eq!(Zone::for_country(country), Some(zone(name)), "{country}");
    }
    for unknown in ["XX", "USA", "", "I"] {
        assert_eq!(Zone::for_country(unknown), None, "{unknown}");
        assert!(Zone::in_country(unknown).is_empty(), "{unknown}");
    }

    let american: Vec<&str> = Zone::in_country("US")
        .into_iter()
        .map(Zone::name)
        .collect();
    assert_eq!(american[0], "America/New_York");
    assert!(american.contains(&"America/Los_Angeles"));
    assert!(american.contains(&"Pacific/Honolulu"));
    assert!(american.contains(&"America/Indiana/Indianapolis"));

    assert_eq!(zone("Asia/Kolkata").countries(), ["IN"]);
    assert!(zone("Europe/Zurich").countries().contains(&"CH"));
    assert!(Zone::UTC.countries().is_empty());

    let countries: Vec<&str> = Zone::countries_with_zones().collect();
    assert!(countries.len() > 240, "{}", countries.len());
    assert!(countries.windows(2).all(|pair| pair[0] < pair[1]));
    for country in countries {
        let zones = Zone::in_country(country);
        assert!(!zones.is_empty(), "{country}");
        assert_eq!(Zone::for_country(country), Some(zones[0]), "{country}");
        for zone in zones {
            assert!(zone.countries().contains(&country), "{country}: {zone:?}");
        }
    }
}

#[test]
fn the_tz_database_release_is_named_and_today_is_a_day() {
    let version = Zone::tzdata_version();
    assert!(version.starts_with("20"), "{version}");
    assert!(version.len() >= 5, "{version}");

    // Today in the zone furthest ahead is today in the one furthest behind,
    // or a day or two after it.
    let ahead = zone("Pacific/Kiritimati").today();
    let behind = zone("Etc/GMT+12").today();
    let days = behind.days_until(ahead);
    assert!((0..=2).contains(&days), "{behind:?} to {ahead:?}");
}
