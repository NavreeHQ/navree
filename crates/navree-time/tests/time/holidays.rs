//! `Holidays`: public holidays by country, extra closing days and custom calendars.

use std::sync::Arc;

use navree_time::{
    BuiltInHolidays, DayRange, FixedHolidays, Holiday, HolidayCalendar, Holidays, easter_sunday,
};

use crate::support::day;

fn dates(holidays: &Holidays, year: i32) -> Vec<String> {
    holidays
        .in_year(year)
        .into_iter()
        .map(|h| h.date.to_string())
        .collect()
}

#[test]
fn easter_follows_the_gregorian_computus() {
    for (year, date) in [
        (2000, "2000-04-23"),
        (2019, "2019-04-21"),
        (2024, "2024-03-31"),
        (2025, "2025-04-20"),
        (2026, "2026-04-05"),
        (2027, "2027-03-28"),
        (2038, "2038-04-25"),
    ] {
        assert_eq!(easter_sunday(year), day(date), "{year}");
    }
}

#[test]
fn united_states_federal_holidays_with_observed_days() {
    let us = Holidays::for_country("US");
    assert_eq!(
        dates(&us, 2026),
        [
            "2026-01-01", // New Year's Day
            "2026-01-19", // Martin Luther King Jr. Day
            "2026-02-16", // Presidents' Day
            "2026-05-25", // Memorial Day
            "2026-06-19", // Juneteenth
            "2026-07-03", // Independence Day, observed (the 4th is a Saturday)
            "2026-09-07", // Labor Day
            "2026-10-12", // Columbus Day
            "2026-11-11", // Veterans Day
            "2026-11-26", // Thanksgiving
            "2026-12-25", // Christmas
        ]
    );
    assert_eq!(
        us.holiday(day("2026-11-26")).map(|h| h.name),
        Some("Thanksgiving Day".to_owned())
    );
}

#[test]
fn england_and_wales_bank_holidays_with_substitutes() {
    let gb = Holidays::for_country("GB");
    assert_eq!(
        dates(&gb, 2026),
        [
            "2026-01-01",
            "2026-04-03", // Good Friday
            "2026-04-06", // Easter Monday
            "2026-05-04",
            "2026-05-25",
            "2026-08-31",
            "2026-12-25",
            "2026-12-28", // Boxing Day, substitute (the 26th is a Saturday)
        ]
    );
    // 2027: Christmas on a Saturday, Boxing Day on a Sunday.
    let late = dates(&gb, 2027);
    assert!(late.contains(&"2027-12-27".to_owned()), "{late:?}");
    assert!(late.contains(&"2027-12-28".to_owned()), "{late:?}");
}

#[test]
fn france_and_germany_follow_easter() {
    let fr = dates(&Holidays::for_country("FR"), 2026);
    for d in [
        "2026-04-06",
        "2026-05-01",
        "2026-05-08",
        "2026-05-14",
        "2026-05-25",
        "2026-07-14",
        "2026-08-15",
        "2026-11-01",
        "2026-11-11",
        "2026-12-25",
    ] {
        assert!(fr.contains(&d.to_owned()), "FR {d}: {fr:?}");
    }
    let de = dates(&Holidays::for_country("DE"), 2026);
    for d in [
        "2026-04-03",
        "2026-04-06",
        "2026-05-14",
        "2026-05-25",
        "2026-10-03",
        "2026-12-26",
    ] {
        assert!(de.contains(&d.to_owned()), "DE {d}: {de:?}");
    }
}

#[test]
fn fixed_national_days_elsewhere() {
    assert_eq!(
        dates(&Holidays::for_country("IN"), 2026),
        ["2026-01-26", "2026-08-15", "2026-10-02"]
    );
    let ae = dates(&Holidays::for_country("AE"), 2026);
    assert!(ae.contains(&"2026-12-02".to_owned()) && ae.contains(&"2026-12-03".to_owned()));
    let sa = dates(&Holidays::for_country("SA"), 2026);
    assert_eq!(sa, ["2026-02-22", "2026-09-23"]);
    let ca = dates(&Holidays::for_country("CA"), 2026);
    for d in ["2026-05-18", "2026-07-01", "2026-09-30", "2026-10-12"] {
        assert!(ca.contains(&d.to_owned()), "CA {d}: {ca:?}");
    }
    let au = dates(&Holidays::for_country("AU"), 2026);
    for d in ["2026-01-26", "2026-04-25", "2026-04-03", "2026-04-06"] {
        assert!(au.contains(&d.to_owned()), "AU {d}: {au:?}");
    }
}

#[test]
fn lookups_by_day_and_range() {
    let gb = Holidays::for_country("gb");
    assert_eq!(gb.country(), Some("GB"));
    assert!(gb.is_holiday(day("2026-12-25")));
    assert!(!gb.is_holiday(day("2026-12-24")));
    let december = DayRange::month_of(day("2026-12-01"));
    let names: Vec<_> = gb.between(&december).into_iter().map(|h| h.name).collect();
    assert_eq!(names, ["Christmas Day", "Boxing Day (substitute day)"]);
    // A range across a new year.
    let range = DayRange::new(day("2026-12-20"), day("2027-01-05")).unwrap();
    assert_eq!(gb.between(&range).len(), 3);
}

#[test]
fn extra_days_close_too() {
    let holidays = Holidays::for_country("IN").with_days([day("2026-11-08")]);
    assert!(holidays.is_holiday(day("2026-11-08")));
    assert_eq!(
        holidays.holiday(day("2026-11-08")).map(|h| h.name),
        Some("Closed".to_owned())
    );
    let none = Holidays::none();
    assert!(!none.is_holiday(day("2026-12-25")));
    assert!(none.country().is_none());
    let extra_only = Holidays::none().with_days([day("2026-12-24")]);
    assert!(extra_only.is_holiday(day("2026-12-24")));
}

#[test]
fn countries_without_rules_have_only_their_extra_days() {
    let unknown = Holidays::for_country("NP");
    assert!(unknown.in_year(2026).is_empty());
    assert!(BuiltInHolidays::countries().contains(&"US"));
    assert!(!BuiltInHolidays::countries().contains(&"NP"));
}

#[test]
fn fixed_calendars_serve_app_data_and_tests() {
    let calendar = FixedHolidays::new()
        .add("NP", day("2026-10-21"), "Dashain")
        .add("NP", day("2026-11-08"), "Tihar")
        .add("GB", day("2026-07-01"), "Company day");
    let np = Holidays::with_calendar("NP", calendar.clone());
    assert_eq!(dates(&np, 2026), ["2026-10-21", "2026-11-08"]);
    assert_eq!(
        calendar.holidays("NP", 2027),
        Vec::<Holiday>::new(),
        "other years are empty"
    );
    let shared: Arc<dyn HolidayCalendar> = Arc::new(calendar);
    let gb = Holidays::with_shared_calendar("GB", shared);
    assert_eq!(dates(&gb, 2026), ["2026-07-01"]);
}
