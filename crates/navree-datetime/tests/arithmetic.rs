//! The range and the arithmetic of the types: the ends of the range against
//! the values recorded from chrono, the calendar against one kept by hand a
//! day at a time, and the sums of instants, days and spans at the places
//! where they are easy to get wrong (a month's end, either side of the
//! epoch, a span below zero, the ends of the range).

use std::time::{Duration, SystemTime};

use navree_datetime::{
    Date, DateTime, ErrorKind, Month, Offset, Span, Time, Timestamp, Weekday, civil,
};

fn date(year: i32, month: u8, day: u8) -> Date {
    Date::new(year, month, day).expect("a day that exists")
}

fn time(hour: u8, minute: u8, second: u8) -> Time {
    Time::new(hour, minute, second).expect("a time that exists")
}

fn utc(year: i32, month: u8, day: u8, hour: u8, minute: u8, second: u8) -> Timestamp {
    Timestamp::utc(year, month, day, hour, minute, second).expect("an instant that exists")
}

fn instant(secs: i64, nanos: u32) -> Timestamp {
    Timestamp::from_secs_nanos(secs, nanos).expect("an instant in the range")
}

/// Whether a year has a 29 February, written out here on its own.
fn leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// The days of each month of a year.
fn month_lengths(year: i64) -> [i64; 12] {
    let february = if leap(year) { 29 } else { 28 };
    [31, february, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
}

/// Days from 1 January 1970 to a day, counted a year and a month at a time:
/// slow, and nothing like the arithmetic it checks.
fn days_by_counting(year: i64, month: usize, day: i64) -> i64 {
    let year_length = |year| if leap(year) { 366 } else { 365 };
    let to_the_year: i64 = if year >= 1970 {
        (1970..year).map(year_length).sum()
    } else {
        -(year..1970).map(year_length).sum::<i64>()
    };
    let to_the_month: i64 = month_lengths(year)[..month - 1].iter().sum();
    to_the_year + to_the_month + day - 1
}

/// The seconds from the epoch and the nanoseconds of an instant in UTC as
/// chrono's `Display` wrote it: `2026-01-15 12:00:00 UTC`,
/// `2026-01-15 12:00:00.250 UTC`, `+262142-12-31 23:59:59.999999999 UTC`.
/// `None` for text of another shape.
fn instant_shown_in_utc(text: &str) -> Option<(i64, u32)> {
    let (day, clock) = text.strip_suffix(" UTC")?.split_once(' ')?;
    let (sign, day) = match day.strip_prefix('-') {
        Some(day) => (-1, day),
        None => (1, day.strip_prefix('+').unwrap_or(day)),
    };
    let mut day = day.split('-');
    let year: i64 = day.next()?.parse().ok()?;
    let month: usize = day.next()?.parse().ok()?;
    let day_of_month: i64 = day.next()?.parse().ok()?;
    if day.next().is_some() || !(1..=12).contains(&month) {
        return None;
    }
    let (clock, fraction) = clock.split_once('.').unwrap_or((clock, ""));
    let mut clock = clock.split(':');
    let hour: i64 = clock.next()?.parse().ok()?;
    let minute: i64 = clock.next()?.parse().ok()?;
    let second: i64 = clock.next()?.parse().ok()?;
    if clock.next().is_some() || fraction.len() > 9 {
        return None;
    }
    // Three, six or nine digits of a second.
    let nanos: u32 = format!("{fraction:0<9}").parse().ok()?;
    let days = days_by_counting(sign * year, month, day_of_month);
    Some((days * 86_400 + hour * 3_600 + minute * 60 + second, nanos))
}

/// The instant of a line of the values recorded from chrono, where the line
/// is one: as the numbers the line gives (`"secs"` and `"nanos"` on a line
/// whose type is a `DateTime<..>`), or else read from the text chrono wrote
/// for it in UTC.
fn recorded_instant(line: &serde_json::Value) -> Option<(i64, u32)> {
    let in_numbers = || {
        if !line["type"].as_str()?.starts_with("DateTime<") {
            return None;
        }
        let nanos = u32::try_from(line["nanos"].as_u64()?).ok()?;
        // A second of nanoseconds or more is a leap second, which no value
        // here holds.
        (nanos < 1_000_000_000).then_some((line["secs"].as_i64()?, nanos))
    };
    in_numbers().or_else(|| line["out"].as_str().and_then(instant_shown_in_utc))
}

/// The earliest and the latest instant among the values recorded from
/// chrono, which hold the two ends of its range.
fn recorded_ends() -> ((i64, u32), (i64, u32)) {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/display.jsonl");
    let text = std::fs::read_to_string(path).unwrap_or_else(|error| panic!("{path}: {error}"));
    let instants: Vec<(i64, u32)> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let line: serde_json::Value =
                serde_json::from_str(line).unwrap_or_else(|error| panic!("{path}: {error}"));
            recorded_instant(&line)
        })
        .collect();
    let earliest = instants.iter().min().expect("instants among the lines");
    let latest = instants.iter().max().expect("instants among the lines");
    (*earliest, *latest)
}

#[test]
fn the_range_is_chronos() {
    // The latest instant: the last nanosecond of the year 262,142.
    assert_eq!(Timestamp::MAX.as_secs(), 8_210_266_876_799);
    assert_eq!(Timestamp::MAX.subsec_nanos(), 999_999_999);
    assert_eq!(
        Timestamp::MAX.date_time_utc(),
        Date::MAX.at(time(23, 59, 59).with_nanos(999_999_999).unwrap())
    );
    assert_eq!(Date::MAX, date(262_142, 12, 31));
    assert_eq!(Date::MAX.epoch_days(), 95_026_236);

    // The earliest: the first moment of the year -262,143.
    assert_eq!(Timestamp::MIN.as_secs(), -8_334_601_228_800);
    assert_eq!(Timestamp::MIN.subsec_nanos(), 0);
    assert_eq!(Timestamp::MIN.date_time_utc(), Date::MIN.at(Time::MIDNIGHT));
    assert_eq!(Date::MIN, date(-262_143, 1, 1));
    assert_eq!(Date::MIN.epoch_days(), -96_465_292);

    // One day past either end is no date.
    for (year, month, day) in [(262_143, 1, 1), (-262_144, 12, 31)] {
        let error = Date::new(year, month, day).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::OutOfRange, "{error}");
    }

    // A span holds `i64::MAX` milliseconds either way.
    assert_eq!(Span::MAX.as_millis(), i64::MAX);
    assert_eq!(Span::MIN.as_millis(), -i64::MAX);

    // And the two ends are the ones chrono had, by a count of the days that
    // shares nothing with the library's.
    assert_eq!(
        days_by_counting(262_142, 12, 31) * 86_400 + 86_399,
        Timestamp::MAX.as_secs()
    );
    assert_eq!(
        days_by_counting(-262_143, 1, 1) * 86_400,
        Timestamp::MIN.as_secs()
    );
    let (earliest, latest) = recorded_ends();
    assert_eq!(
        latest,
        (Timestamp::MAX.as_secs(), Timestamp::MAX.subsec_nanos())
    );
    assert_eq!(
        earliest,
        (Timestamp::MIN.as_secs(), Timestamp::MIN.subsec_nanos())
    );
}

#[test]
fn a_month_added_to_the_31st_lands_on_the_last_day() {
    assert_eq!(date(2026, 1, 31).add_months(1), Some(date(2026, 2, 28)));
    assert_eq!(date(2024, 1, 31).add_months(1), Some(date(2024, 2, 29)));

    // From the 31st, month by month and over the year's end.
    for (months, (year, month, day)) in [
        (2, (2026, 3, 31)),
        (3, (2026, 4, 30)),
        (11, (2026, 12, 31)),
        (12, (2027, 1, 31)),
        (13, (2027, 2, 28)),
        (25, (2028, 2, 29)),
    ] {
        assert_eq!(
            date(2026, 1, 31).add_months(months),
            Some(date(year, month, day)),
            "{months} months"
        );
    }

    // Backwards, the same way.
    assert_eq!(date(2026, 3, 31).add_months(-1), Some(date(2026, 2, 28)));
    assert_eq!(date(2026, 1, 31).add_months(-2), Some(date(2025, 11, 30)));
    assert_eq!(date(2026, 1, 15).add_months(-13), Some(date(2024, 12, 15)));
    assert_eq!(date(2026, 5, 20).add_months(0), Some(date(2026, 5, 20)));

    // A year keeps the day too, and 29 February gives way where there is
    // none.
    assert_eq!(date(2024, 2, 29).add_years(1), Some(date(2025, 2, 28)));
    assert_eq!(date(2024, 2, 29).add_years(4), Some(date(2028, 2, 29)));
    assert_eq!(date(2024, 2, 29).add_years(-24), Some(date(2000, 2, 29)));
    assert_eq!(date(2024, 2, 29).add_years(-124), Some(date(1900, 2, 28)));
    assert_eq!(date(2026, 11, 25).add_years(0), Some(date(2026, 11, 25)));

    // The ends of the range.
    assert_eq!(Date::MAX.add_months(-1), Some(date(262_142, 11, 30)));
    assert_eq!(
        date(262_142, 11, 30).add_months(1),
        Some(date(262_142, 12, 30))
    );
    assert_eq!(Date::MAX.add_months(1), None);
    assert_eq!(Date::MIN.add_months(-1), None);
    assert_eq!(date(2026, 1, 31).add_months(i32::MAX), None);
    assert_eq!(date(2026, 1, 31).add_months(i32::MIN), None);
    assert_eq!(Date::MAX.add_years(1), None);
    assert_eq!(Date::MIN.add_years(-1), None);
    assert_eq!(date(2026, 1, 31).add_years(i32::MAX), None);
    assert_eq!(date(-5, 1, 31).add_years(i32::MIN), None);

    // The ends of a month.
    assert_eq!(date(2026, 2, 14).first_of_month(), date(2026, 2, 1));
    assert_eq!(date(2026, 2, 14).last_of_month(), date(2026, 2, 28));
    assert_eq!(date(2024, 2, 14).last_of_month(), date(2024, 2, 29));
    assert_eq!(date(2026, 11, 25).last_of_month(), date(2026, 11, 30));
    assert_eq!(Month::February.days_in(1900), 28);
    assert_eq!(Month::February.days_in(2000), 29);
    assert_eq!(Month::December.days_in(2026), 31);
    assert_eq!(civil::days_in_month(2026, 13), 0);
    assert!(civil::is_leap_year(2024) && !civil::is_leap_year(2026));
    assert_eq!(
        (civil::days_in_year(2024), civil::days_in_year(2100)),
        (366, 365)
    );
}

#[test]
fn span_days_are_24_exact_hours() {
    assert_eq!(Span::days(1), Span::hours(24));
    assert_eq!(Span::days(1), Span::seconds(86_400));
    assert_eq!(Span::days(1).as_seconds(), 86_400);
    assert_eq!(Span::weeks(1), Span::days(7));
    assert_eq!(Span::hours(1), Span::minutes(60));
    assert_eq!(Span::minutes(1), Span::seconds(60));
    assert_eq!(Span::seconds(1), Span::millis(1_000));
    assert_eq!(Span::millis(1), Span::micros(1_000));
    assert_eq!(Span::micros(1), Span::nanos(1_000));

    // A day added to an instant is 86,400 seconds later, whatever a clock
    // on a wall does that night.
    let midnight = utc(2026, 3, 8, 0, 0, 0);
    assert_eq!(midnight + Span::days(1), utc(2026, 3, 9, 0, 0, 0));
    assert_eq!(
        (midnight + Span::days(1)).as_secs() - midnight.as_secs(),
        86_400
    );

    // The whole units in a span.
    let span =
        Span::days(2) + Span::hours(3) + Span::minutes(4) + Span::seconds(5) + Span::millis(6);
    assert_eq!(span.as_days(), 2);
    assert_eq!(span.as_hours(), 51);
    assert_eq!(span.as_minutes(), 3_064);
    assert_eq!(span.as_seconds(), 183_845);
    assert_eq!(span.as_millis(), 183_845_006);
    assert_eq!(span.as_micros(), Some(183_845_006_000));
    assert_eq!(span.subsec_nanos(), 6_000_000);
    assert_eq!(Span::millis(1_500).as_seconds_f64(), 1.5);
    assert_eq!(Span::MAX.as_micros(), None);

    // The forms that answer `None` past the range make the same spans.
    assert_eq!(Span::try_weeks(2), Some(Span::days(14)));
    assert_eq!(Span::try_days(7), Some(Span::days(7)));
    assert_eq!(Span::try_hours(-3), Some(Span::hours(-3)));
    assert_eq!(Span::try_minutes(90), Some(Span::minutes(90)));
    assert_eq!(Span::try_seconds(86_400), Some(Span::days(1)));
    assert_eq!(Span::try_millis(1_500), Some(Span::millis(1_500)));
    assert_eq!(Span::try_micros(-7), Some(Span::micros(-7)));
    assert_eq!(Span::try_nanos(9), Some(Span::nanos(9)));

    // A span is a value a constant can hold.
    const RETENTION: Span = Span::days(7);
    assert_eq!(RETENTION, Span::weeks(1));

    assert!(Span::ZERO.is_zero() && !Span::nanos(1).is_zero());
    assert_eq!(Span::default(), Span::ZERO);
    assert_eq!(Span::seconds(0), Span::ZERO);

    // To and from the standard library's length of time.
    assert_eq!(
        Span::try_from(Duration::from_millis(1_500)).unwrap(),
        Span::millis(1_500)
    );
    assert_eq!(
        Span::millis(1_500).to_std(),
        Some(Duration::from_millis(1_500))
    );
    assert_eq!(Span::ZERO.to_std(), Some(Duration::ZERO));
}

#[test]
fn civil_round_trips_every_day_from_1600_to_2400() {
    let first = civil::days_from_civil(1600, 1, 1);
    let last = civil::days_from_civil(2400, 12, 31);
    assert_eq!((first, last), (-135_140, 157_419));
    assert_eq!(civil::days_from_civil(1970, 1, 1), 0);

    // A calendar kept by hand beside the arithmetic, one day at a time.
    // 1 January 1600 was a Saturday.
    let (mut year, mut month, mut day) = (1600_i32, 1_u8, 1_u8);
    let mut ordinal = 1_u16;
    let mut weekday = Weekday::Saturday;
    for days in first..=last {
        assert_eq!(
            civil::civil_from_days(days),
            Some((year, month, day)),
            "day {days}"
        );
        assert_eq!(
            civil::days_from_civil(year, month, day),
            days,
            "{year}-{month}-{day}"
        );
        assert_eq!(civil::weekday_from_days(days), weekday, "day {days}");

        let found = Date::from_epoch_days(days).unwrap();
        assert_eq!(found, date(year, month, day));
        assert_eq!(
            (found.year(), found.month_number(), found.day()),
            (year, month, day)
        );
        assert_eq!(found.month().number(), month);
        assert_eq!(found.epoch_days(), days);
        assert_eq!(found.ordinal(), ordinal, "{year}-{month}-{day}");
        assert_eq!(Date::from_ordinal(year, ordinal).unwrap(), found);
        assert_eq!(found.weekday(), weekday, "{year}-{month}-{day}");

        // The day after.
        weekday = weekday.succ();
        if i64::from(day) < month_lengths(i64::from(year))[usize::from(month) - 1] {
            day += 1;
            ordinal += 1;
        } else if month < 12 {
            month += 1;
            day = 1;
            ordinal += 1;
        } else {
            year += 1;
            month = 1;
            day = 1;
            ordinal = 1;
        }
    }
    assert_eq!((year, month, day), (2401, 1, 1));

    // The count is no date past the years an `i32` holds, and one up to
    // them.
    assert_eq!(
        civil::civil_from_days(civil::days_from_civil(i32::MAX, 12, 31)),
        Some((i32::MAX, 12, 31))
    );
    assert_eq!(
        civil::civil_from_days(civil::days_from_civil(i32::MIN, 1, 1)),
        Some((i32::MIN, 1, 1))
    );
    assert_eq!(
        civil::civil_from_days(civil::days_from_civil(i32::MAX, 12, 31) + 1),
        None
    );
    assert_eq!(
        civil::civil_from_days(civil::days_from_civil(i32::MIN, 1, 1) - 1),
        None
    );
    assert_eq!(civil::civil_from_days(i64::MAX), None);
    assert_eq!(civil::civil_from_days(i64::MIN), None);
}

#[test]
fn a_timestamp_minus_a_timestamp_is_a_span() {
    let at = utc(2026, 11, 25, 9, 30, 0);
    assert_eq!((at + Span::millis(1_500)) - at, Span::millis(1_500));
    assert_eq!(at - (at + Span::millis(1_500)), Span::millis(-1_500));
    assert_eq!(at - at, Span::ZERO);
    assert_eq!(
        (at + Span::millis(1_500)).duration_since(at),
        Span::millis(1_500)
    );
    assert_eq!(at - Span::millis(1_500) + Span::millis(1_500), at);
    assert_eq!(at.checked_sub(Span::hours(-2)), Some(at + Span::hours(2)));

    // Either side of the epoch, where the nanoseconds have to borrow.
    let before = instant(-1, 900_000_000);
    let after = instant(0, 100_000_000);
    assert_eq!(after - before, Span::millis(200));
    assert_eq!(before - after, Span::millis(-200));
    assert_eq!(before + Span::millis(200), after);
    assert_eq!(after - Span::millis(200), before);
    assert!(before < Timestamp::UNIX_EPOCH && Timestamp::UNIX_EPOCH < after);

    // In place.
    let mut tick = at;
    tick += Span::minutes(1);
    assert_eq!(tick, utc(2026, 11, 25, 9, 31, 0));
    tick -= Span::seconds(60);
    assert_eq!(tick, at);
    let mut waited = Span::ZERO;
    waited += Span::minutes(1);
    waited -= Span::seconds(15);
    assert_eq!(waited, Span::seconds(45));

    // The whole range is a span that fits, both ways.
    let whole = Timestamp::MAX - Timestamp::MIN;
    assert_eq!(whole.as_seconds(), 16_544_868_105_599);
    assert_eq!(whole.subsec_nanos(), 999_999_999);
    assert_eq!(Timestamp::MIN - Timestamp::MAX, -whole);
    assert_eq!(Timestamp::MIN + whole, Timestamp::MAX);
    assert_eq!(Timestamp::MAX - whole, Timestamp::MIN);
}

#[test]
fn out_of_range_is_an_error_not_a_panic() {
    // Instants.
    assert_eq!(Timestamp::MAX.checked_add(Span::nanos(1)), None);
    assert_eq!(Timestamp::MIN.checked_sub(Span::nanos(1)), None);
    assert_eq!(Timestamp::MAX.checked_add(Span::ZERO), Some(Timestamp::MAX));
    assert_eq!(
        Timestamp::MAX.checked_sub(Span::nanos(1)),
        Some(instant(8_210_266_876_799, 999_999_998))
    );
    assert_eq!(
        Timestamp::MIN.checked_add(Span::nanos(1)),
        Some(instant(-8_334_601_228_800, 1))
    );
    assert_eq!(Timestamp::MIN.checked_add(Span::MAX), None);
    assert_eq!(Timestamp::MAX.checked_add(Span::MIN), None);
    assert_eq!(Timestamp::UNIX_EPOCH.checked_sub(Span::MIN), None);
    for secs in [i64::MIN, -8_334_601_228_801, 8_210_266_876_800, i64::MAX] {
        let error = Timestamp::from_secs(secs).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::OutOfRange, "{error}");
    }
    assert_eq!(
        Timestamp::from_secs(8_210_266_876_799).unwrap().as_secs(),
        8_210_266_876_799
    );
    assert_eq!(
        Timestamp::from_secs(-8_334_601_228_800).unwrap(),
        Timestamp::MIN
    );
    for count in [i64::MIN, i64::MAX] {
        assert_eq!(
            Timestamp::from_millis(count).unwrap_err().kind(),
            ErrorKind::OutOfRange
        );
        assert_eq!(
            Timestamp::from_micros(count).unwrap_err().kind(),
            ErrorKind::OutOfRange
        );
    }

    // Days, and days with a time.
    assert_eq!(Date::MAX.tomorrow(), None);
    assert_eq!(Date::MIN.yesterday(), None);
    assert_eq!(Date::MAX.yesterday(), Some(date(262_142, 12, 30)));
    assert_eq!(Date::MIN.tomorrow(), Some(date(-262_143, 1, 2)));
    assert_eq!(Date::MAX.add_days(1), None);
    assert_eq!(Date::MAX.add_days(i64::MAX), None);
    assert_eq!(Date::MIN.add_days(i64::MIN), None);
    assert_eq!(
        Date::MIN.add_days(Date::MIN.days_until(Date::MAX)),
        Some(Date::MAX)
    );
    for days in [i64::MIN, -96_465_293, 95_026_237, i64::MAX] {
        let error = Date::from_epoch_days(days).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::OutOfRange, "{error}");
    }
    assert_eq!(
        Date::from_ordinal(262_143, 1).unwrap_err().kind(),
        ErrorKind::OutOfRange
    );
    assert_eq!(Date::MAX.at(Time::MIDNIGHT).add(Span::days(1)), None);
    assert_eq!(Date::MIN.at(Time::MIDNIGHT).add(Span::nanos(-1)), None);
    assert_eq!(
        Date::MAX
            .at(Time::MIDNIGHT)
            .add(Span::days(1) - Span::nanos(1)),
        Some(Timestamp::MAX.date_time_utc())
    );

    // Spans.
    assert_eq!(Span::try_seconds(i64::MAX), None);
    assert_eq!(Span::try_seconds(i64::MIN), None);
    assert!(Span::try_seconds(i64::MAX / 1_000).is_some());
    assert_eq!(Span::try_seconds(i64::MAX / 1_000 + 1), None);
    assert!(Span::try_seconds(-(i64::MAX / 1_000)).is_some());
    assert_eq!(Span::try_seconds(-(i64::MAX / 1_000) - 1), None);
    assert_eq!(Span::try_millis(i64::MAX), Some(Span::MAX));
    assert_eq!(Span::try_millis(-i64::MAX), Some(Span::MIN));
    assert_eq!(Span::try_millis(i64::MIN), None);
    assert_eq!(Span::try_weeks(i64::MAX), None);
    assert_eq!(Span::try_days(i64::MIN), None);
    assert_eq!(Span::try_hours(i64::MAX), None);
    assert_eq!(Span::try_minutes(i64::MIN), None);
    assert_eq!(Span::MAX.checked_add(Span::nanos(1)), None);
    assert_eq!(Span::MIN.checked_sub(Span::nanos(1)), None);
    assert_eq!(Span::MAX.checked_add(Span::MIN), Some(Span::ZERO));
    assert_eq!(Span::MAX.checked_mul(2), None);
    assert_eq!(Span::MIN.checked_mul(i32::MIN), None);
    assert_eq!(Span::MAX.checked_mul(-1), Some(Span::MIN));
    assert_eq!(Span::MAX.checked_div(0), None);
    assert_eq!(
        Span::try_from(Duration::MAX).unwrap_err().kind(),
        ErrorKind::OutOfRange
    );
    assert_eq!(
        Span::try_from(Duration::from_secs(u64::MAX / 2))
            .unwrap_err()
            .kind(),
        ErrorKind::OutOfRange
    );
    assert_eq!(Span::nanos(-1).to_std(), None);

    // Offsets: up to 25:59:59 either way.
    assert_eq!(Offset::east(93_599).unwrap().seconds(), 93_599);
    assert_eq!(Offset::west(93_599).unwrap().seconds(), -93_599);
    for seconds in [93_600, -93_600, i32::MAX, i32::MIN] {
        assert_eq!(
            Offset::east(seconds).unwrap_err().kind(),
            ErrorKind::OutOfRange
        );
        assert_eq!(
            Offset::west(seconds).unwrap_err().kind(),
            ErrorKind::OutOfRange
        );
    }
}

#[test]
fn weekday_of_known_days() {
    for ((year, month, day), weekday) in [
        ((2026, 11, 25), Weekday::Wednesday),
        ((2026, 1, 26), Weekday::Monday),
        ((1970, 1, 1), Weekday::Thursday),
        ((1969, 12, 31), Weekday::Wednesday),
        ((2000, 2, 29), Weekday::Tuesday),
        ((2024, 2, 29), Weekday::Thursday),
        ((1900, 3, 1), Weekday::Thursday),
        ((1, 1, 1), Weekday::Monday),
        ((0, 1, 1), Weekday::Saturday),
        ((-262_143, 1, 1), Weekday::Thursday),
        ((262_142, 12, 31), Weekday::Monday),
    ] {
        assert_eq!(
            date(year, month, day).weekday(),
            weekday,
            "{year}-{month}-{day}"
        );
    }

    // The numbers of a weekday, from either start of the week.
    let week = [
        Weekday::Monday,
        Weekday::Tuesday,
        Weekday::Wednesday,
        Weekday::Thursday,
        Weekday::Friday,
        Weekday::Saturday,
        Weekday::Sunday,
    ];
    for (place, day) in (0_u8..).zip(week) {
        assert_eq!(day.days_from_monday(), place);
        assert_eq!(day.number_from_monday(), place + 1);
        assert_eq!(day.days_from_sunday(), (place + 1) % 7);
        assert_eq!(day.number_from_sunday(), (place + 1) % 7 + 1);
        assert_eq!(Weekday::try_from(place).unwrap(), day);
        assert_eq!(day.succ(), week[usize::from((place + 1) % 7)]);
        assert_eq!(day.pred(), week[usize::from((place + 6) % 7)]);
        assert_eq!(day.succ().pred(), day);
    }
    assert_eq!(Weekday::Sunday.number_from_sunday(), 1);
    assert_eq!(Weekday::Sunday.number_from_monday(), 7);
    assert_eq!(Weekday::try_from(7).unwrap_err().kind(), ErrorKind::Invalid);

    // The nth weekday of a month.
    let nth = Date::nth_weekday_of_month;
    assert_eq!(nth(2017, 3, Weekday::Friday, 2), Some(date(2017, 3, 10)));
    assert_eq!(
        nth(2026, 11, Weekday::Thursday, 4),
        Some(date(2026, 11, 26))
    );
    assert_eq!(nth(2026, 2, Weekday::Sunday, 1), Some(date(2026, 2, 1)));
    assert_eq!(nth(2023, 4, Weekday::Saturday, 5), Some(date(2023, 4, 29)));
    assert_eq!(nth(2023, 4, Weekday::Monday, 5), None);
    assert_eq!(nth(2026, 11, Weekday::Thursday, 0), None);
    assert_eq!(nth(2026, 11, Weekday::Thursday, 255), None);
    assert_eq!(nth(2026, 13, Weekday::Thursday, 1), None);
    assert_eq!(nth(2026, 0, Weekday::Thursday, 1), None);
    assert_eq!(nth(262_143, 1, Weekday::Thursday, 1), None);
}

#[test]
fn an_iso_week_belongs_to_the_year_of_its_thursday() {
    for ((year, month, day), week) in [
        ((2026, 11, 25), (2026, 48)),
        ((2015, 9, 5), (2015, 36)),
        ((2026, 1, 1), (2026, 1)),
        // The first days of January in the last week of the year before.
        ((2021, 1, 1), (2020, 53)),
        ((2027, 1, 1), (2026, 53)),
        ((2010, 1, 3), (2009, 53)),
        // The last days of December in week 1 of the year after.
        ((2024, 12, 30), (2025, 1)),
        ((2014, 12, 29), (2015, 1)),
        ((2020, 12, 31), (2020, 53)),
        // Year 0 and the years past four digits.
        ((0, 1, 2), (-1, 52)),
        ((0, 1, 3), (0, 1)),
        ((9999, 12, 31), (9999, 52)),
        ((10_000, 12, 31), (10_000, 52)),
        // The ends of the range: the last day's week is in a year no date
        // has.
        ((-262_143, 1, 1), (-262_143, 1)),
        ((262_142, 12, 31), (262_143, 1)),
    ] {
        assert_eq!(
            date(year, month, day).iso_week(),
            week,
            "{year}-{month}-{day}"
        );
    }
    assert_eq!(date(2026, 11, 25).ordinal(), 329);
    assert_eq!(date(2024, 12, 31).ordinal(), 366);
    assert_eq!(Date::from_ordinal(2026, 329).unwrap(), date(2026, 11, 25));
    assert_eq!(Date::from_ordinal(2024, 366).unwrap(), date(2024, 12, 31));
}

#[test]
fn a_day_and_a_time_in_utc_are_the_instant_they_name() {
    let at = utc(2026, 11, 25, 9, 30, 0);
    assert_eq!(at.as_secs(), 1_795_599_000);
    assert_eq!(utc(2001, 9, 9, 1, 46, 40).as_secs(), 1_000_000_000);
    assert_eq!(utc(1970, 1, 1, 0, 0, 0), Timestamp::UNIX_EPOCH);
    assert_eq!(utc(1969, 12, 31, 23, 59, 59), instant(-1, 0));
    assert_eq!(Timestamp::from_secs(1_795_599_000).unwrap(), at);

    // An instant and the wall clock of UTC, both ways.
    let wall = date(2026, 11, 25).at(time(9, 30, 0));
    assert_eq!(at.date_time_utc(), wall);
    assert_eq!(wall.in_utc(), at);
    assert_eq!(
        (wall.date(), wall.time()),
        (date(2026, 11, 25), time(9, 30, 0))
    );
    assert_eq!(wall, DateTime::new(date(2026, 11, 25), time(9, 30, 0)));
    assert_eq!(
        instant(-1, 750_000_000).date_time_utc(),
        date(1969, 12, 31).at(time(23, 59, 59).with_nanos(750_000_000).unwrap())
    );
    assert_eq!(date(2026, 11, 25).month(), Month::November);

    // The parts of a time of day.
    let precise = time(23, 59, 58).with_nanos(123_456_789).unwrap();
    assert_eq!(
        (
            precise.hour(),
            precise.minute(),
            precise.second(),
            precise.nanosecond()
        ),
        (23, 59, 58, 123_456_789)
    );
    assert_eq!(
        precise.with_hour(0).unwrap(),
        time(0, 59, 58).with_nanos(123_456_789).unwrap()
    );
    assert_eq!(precise.with_minute(1).unwrap().minute(), 1);
    assert_eq!(precise.with_second(0).unwrap().nanosecond(), 123_456_789);
    assert!(time(9, 30, 0) < time(9, 30, 1) && time(9, 59, 59) < time(10, 0, 0));
    assert!(time(9, 30, 0) < time(9, 30, 0).with_nanos(1).unwrap());

    // A wall clock that never jumps: 45 minutes over midnight, and back.
    let late = date(2026, 11, 25).at(time(23, 30, 0));
    assert_eq!(
        late.add(Span::minutes(45)),
        Some(date(2026, 11, 26).at(time(0, 15, 0)))
    );
    assert_eq!(
        late.add(Span::days(-1)),
        Some(date(2026, 11, 24).at(time(23, 30, 0)))
    );
    assert_eq!(
        date(2026, 1, 1).at(Time::MIDNIGHT).add(Span::nanos(-1)),
        Some(date(2025, 12, 31).at(time(23, 59, 59).with_nanos(999_999_999).unwrap()))
    );
    assert!(late < date(2026, 11, 26).at(Time::MIDNIGHT));
    assert!(date(2026, 11, 25) < date(2026, 12, 1) && date(2025, 12, 31) < date(2026, 1, 1));
    assert_eq!(date(2026, 11, 25).days_until(date(2027, 1, 1)), 37);
    assert_eq!(date(2027, 1, 1).days_until(date(2026, 11, 25)), -37);
    assert_eq!(date(2026, 11, 25).add_days(37), Some(date(2027, 1, 1)));
    assert_eq!(date(2026, 3, 1).add_days(-1), Some(date(2026, 2, 28)));
    assert_eq!(date(2024, 3, 1).yesterday(), Some(date(2024, 2, 29)));
    assert_eq!(date(2026, 12, 31).tomorrow(), Some(date(2027, 1, 1)));

    // What each type is when nothing is said: the epoch, and no time.
    assert_eq!(Timestamp::default(), Timestamp::UNIX_EPOCH);
    assert_eq!(Date::default(), date(1970, 1, 1));
    assert_eq!(Time::default(), Time::MIDNIGHT);
    assert_eq!(DateTime::default(), Timestamp::UNIX_EPOCH.date_time_utc());
    assert_eq!(Time::MIDNIGHT, time(0, 0, 0));

    // An offset east of UTC is ahead of it.
    assert_eq!(Offset::east(19_800).unwrap().seconds(), 19_800);
    assert_eq!(Offset::west(18_000).unwrap().seconds(), -18_000);
    assert_eq!(
        Offset::east(-18_000).unwrap(),
        Offset::west(18_000).unwrap()
    );
    assert_eq!(Offset::east(0).unwrap(), Offset::UTC);
}

#[test]
fn an_instant_before_the_epoch_counts_its_fraction_up() {
    // A millisecond before the epoch is in second -1, 999 milliseconds in.
    let just_before = Timestamp::from_millis(-1).unwrap();
    assert_eq!(just_before, instant(-1, 999_000_000));
    assert_eq!(just_before.as_secs(), -1);
    assert_eq!(just_before.as_millis(), -1);
    assert_eq!(just_before.as_micros(), -1_000);
    assert_eq!(
        Timestamp::from_micros(-1).unwrap(),
        instant(-1, 999_999_000)
    );
    assert_eq!(Timestamp::from_micros(-1).unwrap().as_micros(), -1);
    assert_eq!(Timestamp::from_micros(-1).unwrap().as_millis(), -1);

    // After it, the same counts the plain way.
    let at = Timestamp::from_millis(1_795_599_000_250).unwrap();
    assert_eq!(
        (at.as_secs(), at.subsec_nanos()),
        (1_795_599_000, 250_000_000)
    );
    assert_eq!(at.as_millis(), 1_795_599_000_250);
    assert_eq!(
        Timestamp::from_micros(1_795_599_000_000_123)
            .unwrap()
            .as_micros(),
        1_795_599_000_000_123
    );
    assert_eq!(instant(5, 123_456_789).as_millis(), 5_123);
    assert_eq!(instant(5, 123_456_789).as_micros(), 5_123_456);
    assert_eq!(Timestamp::MAX.as_millis(), 8_210_266_876_799_999);
    assert_eq!(Timestamp::MAX.as_micros(), 8_210_266_876_799_999_999);
    assert_eq!(Timestamp::MIN.as_micros(), -8_334_601_228_800_000_000);

    // Cut to the microseconds a database keeps.
    assert_eq!(
        instant(5, 123_456_789).trunc_micros(),
        instant(5, 123_456_000)
    );
    assert_eq!(
        instant(-1, 999_999_999).trunc_micros(),
        instant(-1, 999_999_000)
    );
    assert_eq!(
        instant(5, 123_456_000).trunc_micros(),
        instant(5, 123_456_000)
    );
    assert_eq!(Timestamp::MAX.trunc_micros().subsec_nanos(), 999_999_000);

    // The system's clock.
    let epoch = SystemTime::UNIX_EPOCH;
    assert_eq!(Timestamp::from(epoch), Timestamp::UNIX_EPOCH);
    assert_eq!(
        Timestamp::from(epoch + Duration::new(1_795_599_000, 250_000_000)),
        instant(1_795_599_000, 250_000_000)
    );
    assert_eq!(
        Timestamp::from(epoch - Duration::new(1, 250_000_000)),
        instant(-2, 750_000_000)
    );
    assert_eq!(Timestamp::from(epoch - Duration::new(3, 0)), instant(-3, 0));
    assert!(Timestamp::now() > utc(2026, 1, 1, 0, 0, 0));
}

#[test]
fn a_span_below_zero_counts_its_whole_units_towards_zero() {
    let short = Span::millis(-1_500);
    assert!(short.is_negative() && !Span::millis(1_500).is_negative());
    assert_eq!(short.as_seconds(), -1);
    assert_eq!(short.subsec_nanos(), -500_000_000);
    assert_eq!(short.as_millis(), -1_500);
    assert_eq!(short.as_micros(), Some(-1_500_000));
    assert_eq!(short.as_seconds_f64(), -1.5);
    assert_eq!(short.abs(), Span::millis(1_500));
    assert_eq!(-short, Span::millis(1_500));
    assert_eq!(Span::millis(1_500).abs(), Span::millis(1_500));

    // Less than a unit below zero is none of that unit.
    assert_eq!(Span::nanos(-1).as_seconds(), 0);
    assert_eq!(Span::nanos(-1).subsec_nanos(), -1);
    assert_eq!(Span::nanos(-1).as_millis(), 0);
    assert!(Span::nanos(-1).is_negative() && !Span::nanos(-1).is_zero());
    assert_eq!(Span::seconds(-90).as_minutes(), -1);
    assert_eq!(Span::hours(-36).as_days(), -1);
    assert_eq!(Span::hours(-36).as_hours(), -36);
    assert_eq!(Span::hours(-36).as_minutes(), -2_160);

    // Every span has its opposite, the two ends of the range too.
    assert_eq!(-Span::MAX, Span::MIN);
    assert_eq!(-Span::MIN, Span::MAX);
    assert_eq!(Span::MIN.abs(), Span::MAX);
    assert_eq!(-Span::ZERO, Span::ZERO);
    assert!(Span::MIN < Span::millis(-1) && Span::millis(-1) < Span::ZERO);
    assert!(Span::ZERO < Span::nanos(1) && Span::nanos(1) < Span::MAX);

    // Sums that carry and borrow a second.
    assert_eq!(Span::millis(700) + Span::millis(700), Span::millis(1_400));
    assert_eq!(Span::millis(200) - Span::millis(700), Span::millis(-500));
    assert_eq!(
        Span::millis(-700) + Span::millis(-700),
        Span::millis(-1_400)
    );

    // Times a whole number.
    assert_eq!(Span::millis(1_500) * 3, Span::millis(4_500));
    assert_eq!(Span::millis(-1_500) * 3, Span::millis(-4_500));
    assert_eq!(Span::millis(1_500) * -3, Span::millis(-4_500));
    assert_eq!(Span::nanos(999_999_999) * 2, Span::nanos(1_999_999_998));
    assert_eq!(Span::days(3).checked_mul(0), Some(Span::ZERO));
    assert_eq!(Span::hours(5) * -1, -Span::hours(5));

    // Into parts: the seconds and the nanoseconds are each divided, and
    // what is left of the seconds goes into the nanoseconds.
    assert_eq!(Span::seconds(7) / 2, Span::millis(3_500));
    assert_eq!(Span::seconds(-7) / 2, Span::millis(-3_500));
    assert_eq!(Span::millis(1_500) / -2, Span::millis(-750));
    assert_eq!(Span::millis(-1_500) / 2, Span::millis(-750));
    assert_eq!(Span::seconds(1) / 3, Span::nanos(333_333_333));
    assert_eq!(Span::seconds(-1) / 3, Span::nanos(-333_333_333));
    assert_eq!(Span::days(1) / 7, Span::nanos(12_342_857_142_857));
    assert_eq!(Span::days(-1) / 7, Span::nanos(-12_342_857_142_857));
    assert_eq!(Span::nanos(3) / 2, Span::nanos(1));
    assert_eq!(Span::nanos(3) / -2, Span::nanos(-1));
    assert_eq!(Span::nanos(-3) / 2, Span::nanos(-2));
    assert_eq!(Span::nanos(-1) / 2, Span::nanos(-1));
    assert_eq!(Span::MIN / -1, Span::MAX);
    assert_eq!(Span::MAX / 1, Span::MAX);
}

#[test]
fn a_value_that_does_not_exist_is_refused_with_its_kind() {
    for (year, month, day) in [
        (2026, 2, 29),
        (2026, 2, 30),
        (1900, 2, 29),
        (2026, 4, 31),
        (2026, 11, 0),
        (2026, 0, 1),
        (2026, 13, 1),
        (2026, 1, 32),
    ] {
        let error = Date::new(year, month, day).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Invalid, "{error}");
    }
    assert_eq!(
        Date::new(2026, 2, 30).unwrap_err().to_string(),
        "2026-02 has no day 30"
    );
    assert_eq!(date(2024, 2, 29).day(), 29);
    assert_eq!(date(2000, 2, 29).day(), 29);
    for ordinal in [0, 366, 400] {
        assert_eq!(
            Date::from_ordinal(2026, ordinal).unwrap_err().kind(),
            ErrorKind::Invalid
        );
    }

    // A minute has 60 seconds, always: there is no second 60.
    for (hour, minute, second) in [(24, 0, 0), (0, 60, 0), (23, 59, 60), (255, 255, 255)] {
        let error = Time::new(hour, minute, second).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Invalid, "{error}");
    }
    let noon = time(12, 0, 0);
    assert_eq!(
        noon.with_nanos(1_000_000_000).unwrap_err().kind(),
        ErrorKind::Invalid
    );
    assert_eq!(
        noon.with_nanos(999_999_999).unwrap().nanosecond(),
        999_999_999
    );
    assert_eq!(noon.with_hour(24).unwrap_err().kind(), ErrorKind::Invalid);
    assert_eq!(noon.with_minute(60).unwrap_err().kind(), ErrorKind::Invalid);
    assert_eq!(noon.with_second(60).unwrap_err().kind(), ErrorKind::Invalid);

    // An instant named by a day or a time that does not exist.
    assert_eq!(
        Timestamp::utc(2026, 2, 30, 0, 0, 0).unwrap_err().kind(),
        ErrorKind::Invalid
    );
    assert_eq!(
        Timestamp::utc(2026, 1, 1, 24, 0, 0).unwrap_err().kind(),
        ErrorKind::Invalid
    );
    assert_eq!(
        Timestamp::utc(262_143, 1, 1, 0, 0, 0).unwrap_err().kind(),
        ErrorKind::OutOfRange
    );
    assert_eq!(
        Timestamp::from_secs_nanos(0, 1_000_000_000)
            .unwrap_err()
            .kind(),
        ErrorKind::Invalid
    );

    // Months by number.
    assert_eq!(Month::try_from(1).unwrap(), Month::January);
    assert_eq!(Month::try_from(12).unwrap(), Month::December);
    assert_eq!(Month::December.number(), 12);
    for number in [0, 13, 255] {
        assert_eq!(
            Month::try_from(number).unwrap_err().kind(),
            ErrorKind::Invalid
        );
    }
    assert!(Month::January < Month::February && Month::November < Month::December);
}
