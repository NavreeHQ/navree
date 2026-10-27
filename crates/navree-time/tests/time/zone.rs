//! `Zone`: IANA names, aliases, countries, offsets and DST conversions.

use chrono::{Datelike, Timelike};
use navree_time::{Zone, tzdata_version};

use crate::support::{local, utc, zone};

#[test]
fn canonical_names_parse_as_they_are() {
    for name in ["Europe/Paris", "Asia/Kolkata", "America/New_York", "UTC", "Etc/GMT+5"] {
        assert_eq!(zone(name).name(), name);
    }
}

#[test]
fn names_are_trimmed_and_matched_without_case() {
    assert_eq!(zone(" europe/paris ").name(), "Europe/Paris");
    assert_eq!(zone("AMERICA/NEW_YORK").name(), "America/New_York");
    assert_eq!(zone("utc").name(), "UTC");
}

#[test]
fn legacy_aliases_become_names_every_postgres_accepts() {
    for (alias, name) in [
        ("Asia/Calcutta", "Asia/Kolkata"),
        ("asia/calcutta", "Asia/Kolkata"),
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
        assert_eq!(Zone::canonical_name(alias), Some(name), "{alias}");
    }
}

#[test]
fn country_zones_keep_their_own_names() {
    // zone.tab locations whose data is shared with another zone stay as
    // they are: every PostgreSQL knows them.
    for name in ["Europe/Oslo", "Africa/Accra", "Asia/Muscat", "Europe/Amsterdam"] {
        assert_eq!(zone(name).name(), name);
    }
}

#[test]
fn unknown_names_are_refused() {
    for bad in [
        "",
        "  ",
        "Mars/Olympus_Mons",
        "Europe/Paris'; DROP TABLE users; --",
        "+05:30",
        "Europe//Paris",
        "EST5EDT/x",
    ] {
        let error = Zone::parse(bad).unwrap_err();
        assert!(
            error.to_string().contains("unknown time zone"),
            "{bad}: {error}"
        );
        assert_eq!(Zone::canonical_name(bad), None);
    }
}

#[test]
fn every_alias_and_country_zone_is_a_canonical_zone() {
    for zone in Zone::all() {
        assert_eq!(Zone::canonical_name(zone.name()), Some(zone.name()));
        assert!(
            zone.name()
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"/_+-".contains(&b)),
            "{}",
            zone.name()
        );
    }
    let all: Vec<_> = Zone::all().map(|z| z.name()).collect();
    assert!(all.len() > 300, "{}", all.len());
    assert!(all.windows(2).all(|w| w[0] < w[1]), "sorted and unique");
    assert!(all.contains(&"Asia/Kolkata"));
    assert!(!all.contains(&"Asia/Calcutta"));
    assert!(!all.contains(&"US/Eastern"));
    assert!(all.contains(&"UTC"));
}

#[test]
fn countries_have_a_primary_zone() {
    assert_eq!(Zone::for_country("IN").unwrap().name(), "Asia/Kolkata");
    assert_eq!(Zone::for_country("in").unwrap().name(), "Asia/Kolkata");
    assert_eq!(Zone::for_country("US").unwrap().name(), "America/New_York");
    assert_eq!(Zone::for_country("AE").unwrap().name(), "Asia/Dubai");
    assert_eq!(Zone::for_country("FR").unwrap().name(), "Europe/Paris");
    assert_eq!(Zone::for_country("GB").unwrap().name(), "Europe/London");
    assert_eq!(Zone::for_country("SA").unwrap().name(), "Asia/Riyadh");
    assert_eq!(Zone::for_country("NO").unwrap().name(), "Europe/Oslo");
    assert_eq!(Zone::for_country("UA").unwrap().name(), "Europe/Kyiv");
    assert_eq!(Zone::for_country("AU").unwrap().name(), "Australia/Sydney");
    assert!(Zone::for_country("XX").is_none());
    assert!(Zone::for_country("USA").is_none());

    let us: Vec<_> = Zone::in_country("US").iter().map(|z| z.name()).collect();
    assert_eq!(us[0], "America/New_York");
    assert!(us.contains(&"America/Los_Angeles"));
    assert!(us.contains(&"Pacific/Honolulu"));
    assert!(us.contains(&"America/Indiana/Indianapolis"));
    assert!(Zone::in_country("ZZ").is_empty());

    assert_eq!(zone("Asia/Kolkata").countries(), ["IN"]);
    assert!(zone("Europe/Zurich").countries().contains(&"CH"));
    assert!(zone("UTC").countries().is_empty());
}

#[test]
fn every_country_zone_parses() {
    for country in Zone::countries_with_zones() {
        let zones = Zone::in_country(country);
        assert!(!zones.is_empty(), "{country}");
        assert_eq!(Zone::for_country(country), Some(zones[0]));
    }
    assert!(Zone::countries_with_zones().count() > 240);
}

#[test]
fn zones_serialize_as_their_name_and_deserialize_normalized() {
    let kolkata = zone("Asia/Kolkata");
    assert_eq!(
        serde_json::to_string(&kolkata).unwrap(),
        r#""Asia/Kolkata""#
    );
    let parsed: Zone = serde_json::from_str(r#""Asia/Calcutta""#).unwrap();
    assert_eq!(parsed, kolkata);
    let error = serde_json::from_str::<Zone>(r#""Mars/Base""#).unwrap_err();
    assert!(
        error.to_string().contains("unknown time zone `Mars/Base`"),
        "{error}"
    );
}

#[test]
fn display_parse_default_and_ordering() {
    assert_eq!(zone("Europe/Paris").to_string(), "Europe/Paris");
    assert_eq!("asia/calcutta".parse::<Zone>().unwrap().name(), "Asia/Kolkata");
    assert_eq!(Zone::default(), Zone::UTC);
    assert_eq!(Zone::UTC.name(), "UTC");
    assert!(zone("Africa/Abidjan") < zone("Europe/Paris"));
    assert_eq!(format!("{:?}", zone("Europe/Paris")), r#"Zone("Europe/Paris")"#);
}

#[test]
fn offsets_follow_daylight_saving_time() {
    let kolkata = zone("Asia/Kolkata");
    let at = utc("2026-01-15T12:00:00Z");
    assert_eq!(kolkata.offset_at(at).local_minus_utc(), 5 * 3600 + 1800);
    assert_eq!(kolkata.offset_label(at), "+05:30");
    let new_york = zone("America/New_York");
    assert_eq!(new_york.offset_label(utc("2026-01-15T12:00:00Z")), "-05:00");
    assert_eq!(new_york.offset_label(utc("2026-07-15T12:00:00Z")), "-04:00");
    assert_eq!(Zone::UTC.offset_label(at), "+00:00");
    assert_eq!(zone("Asia/Kathmandu").offset_label(at), "+05:45");
}

#[test]
fn instants_read_on_the_local_clock() {
    let local_time = zone("Asia/Kolkata").local(utc("2026-10-26T20:00:00Z"));
    assert_eq!(
        (local_time.day(), local_time.hour(), local_time.minute()),
        (27, 1, 30)
    );
}

#[test]
fn wall_clock_times_become_instants_across_dst_changes() {
    let new_york = zone("America/New_York");
    // An ordinary time.
    assert_eq!(
        new_york.to_utc(local("2026-06-01T09:00")),
        utc("2026-06-01T13:00:00Z")
    );
    // 02:30 on 8 March 2026 does not exist (02:00 jumps to 03:00): the time
    // moves forward by the gap, to 03:30 EDT.
    assert_eq!(
        new_york.to_utc(local("2026-03-08T02:30")),
        utc("2026-03-08T07:30:00Z")
    );
    // 01:30 on 1 November 2026 happens twice: the earlier one (EDT) is used.
    assert_eq!(
        new_york.to_utc(local("2026-11-01T01:30")),
        utc("2026-11-01T05:30:00Z")
    );
    // A whole skipped day: Samoa jumped from 29 to 31 December 2011.
    let apia = zone("Pacific/Apia");
    let skipped = apia.to_utc(local("2011-12-30T12:00"));
    assert_eq!(apia.local(skipped).day(), 31);
}

#[test]
fn the_tz_database_version_is_known() {
    let version = tzdata_version();
    assert!(version.starts_with("20"), "{version}");
}
