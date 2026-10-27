//! `TimeModule`: configuration, the `Zones` service, zone sources and the command.

use navree_config::ConfigSources;
use navree_contracts::{ZoneLevel, ZoneOwner};
use navree_testing::TestApp;
use navree_time::{TimeModule, TimeOptions, Zones, ZonesFake};
use serde_json::json;
use uuid::Uuid;

use crate::support::{day, zone};

#[test]
fn defaults_match_the_documentation() {
    let options = TimeOptions::default();
    assert_eq!(options.default_zone, "UTC");
    assert!(options.routes);
    assert_eq!(options.holidays.country, None);
    assert!(options.holidays.days.is_empty());
    let parsed: TimeOptions =
        serde_json::from_value(json!({"default_zone": "Asia/Dubai", "holidays": {"country": "AE"}}))
            .unwrap();
    assert_eq!(parsed.default_zone, "Asia/Dubai");
    assert!(parsed.routes);
    assert_eq!(parsed.holidays.country.as_deref(), Some("AE"));
    let csv: TimeOptions =
        serde_json::from_value(json!({"holidays": {"days": "2026-12-24, 2026-12-31"}})).unwrap();
    assert_eq!(csv.holidays.days, [day("2026-12-24"), day("2026-12-31")]);
}

#[tokio::test]
async fn the_default_zone_comes_from_the_time_section() {
    let app = TestApp::builder()
        .config("time.default_zone", "Asia/Calcutta")
        .module(TimeModule::from_config())
        .build()
        .await;
    let zones = app.get::<Zones>().await;
    assert_eq!(zones.default_zone(), zone("Asia/Kolkata"));
    assert_eq!(app.get::<TimeOptions>().await.default_zone, "Asia/Calcutta");
}

#[tokio::test]
async fn environment_variables_set_the_section() {
    let sources = ConfigSources::new().env_vars(
        "APP_",
        [
            ("APP_TIME__DEFAULT_ZONE", "Europe/Paris"),
            ("APP_TIME__HOLIDAYS__COUNTRY", "fr"),
            ("APP_TIME__HOLIDAYS__DAYS", "2026-12-24,2026-12-31"),
        ],
    );
    let app = TestApp::builder()
        .sources(sources)
        .module(TimeModule::from_config())
        .build()
        .await;
    let zones = app.get::<Zones>().await;
    assert_eq!(zones.default_zone(), zone("Europe/Paris"));
    let holidays = zones.holidays();
    assert_eq!(holidays.country(), Some("FR"));
    assert!(holidays.is_holiday(day("2026-07-14")));
    assert!(holidays.is_holiday(day("2026-12-24")));
    assert!(!holidays.is_holiday(day("2026-12-23")));
    assert!(zones.holidays_for("US").is_holiday(day("2026-07-03")));
}

#[tokio::test]
async fn a_plain_module_reads_the_section_too() {
    let app = TestApp::builder()
        .config("time.default_zone", "Asia/Tokyo")
        .module(TimeModule)
        .build()
        .await;
    assert_eq!(app.get::<Zones>().await.default_zone(), zone("Asia/Tokyo"));
}

#[tokio::test]
async fn for_root_takes_the_options() {
    let app = TestApp::builder()
        .module(TimeModule::for_root(TimeOptions {
            default_zone: "Asia/Dubai".into(),
            ..TimeOptions::default()
        }))
        .build()
        .await;
    assert_eq!(app.get::<Zones>().await.default_zone(), zone("Asia/Dubai"));
}

#[tokio::test]
async fn an_unknown_default_zone_fails_the_boot() {
    let error = TestApp::builder()
        .config("time.default_zone", "Mars/Base")
        .module(TimeModule::from_config())
        .try_build()
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("time.default_zone"), "{error}");
    assert!(error.contains("Mars/Base"), "{error}");
}

#[tokio::test]
async fn an_unknown_holiday_country_fails_the_boot() {
    let error = TestApp::builder()
        .config("time.holidays.country", "Atlantis")
        .module(TimeModule::from_config())
        .try_build()
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("time.holidays.country"), "{error}");
}

#[tokio::test]
async fn invalid_for_root_options_fail_the_boot() {
    let error = TestApp::builder()
        .module(TimeModule::for_root(TimeOptions {
            default_zone: "Nowhere".into(),
            ..TimeOptions::default()
        }))
        .try_build()
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("time.default_zone"), "{error}");
}

fn workspaces() -> (ZonesFake, Uuid, Uuid, Uuid, Uuid) {
    let (kolkata, country_only, broken, bare) = (
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
    );
    let fake = ZonesFake::new(ZoneLevel::Workspace)
        .owner(ZoneOwner::new(kolkata).zone("Asia/Calcutta"))
        .owner(ZoneOwner::new(country_only).country("AE"))
        .owner(ZoneOwner::new(broken).zone("Mars/Base"))
        .owner(ZoneOwner::new(bare));
    (fake, kolkata, country_only, broken, bare)
}

#[tokio::test]
async fn owners_resolve_to_their_zone_their_country_or_the_default() {
    let (fake, kolkata, country_only, broken, bare) = workspaces();
    let app = TestApp::builder()
        .config("time.default_zone", "Europe/London")
        .module(TimeModule::from_config())
        .module(fake.module())
        .build()
        .await;
    let zones = app.get::<Zones>().await;
    let of = |owner| {
        let zones = zones.clone();
        async move {
            zones
                .zone_of(ZoneLevel::Workspace, owner)
                .await
                .unwrap()
                .name()
        }
    };
    assert_eq!(of(kolkata).await, "Asia/Kolkata");
    assert_eq!(of(country_only).await, "Asia/Dubai");
    assert_eq!(of(broken).await, "Europe/London", "a bad stored zone falls back");
    assert_eq!(of(bare).await, "Europe/London");
    assert_eq!(of(Uuid::now_v7()).await, "Europe/London");
    // No user-level source: the default.
    assert_eq!(
        zones
            .zone_of(ZoneLevel::User, kolkata)
            .await
            .unwrap()
            .name(),
        "Europe/London"
    );

    let mut owners = zones.owners(ZoneLevel::Workspace).await.unwrap();
    owners.sort_by_key(|(id, _)| *id);
    let mut expected = vec![
        (kolkata, zone("Asia/Kolkata")),
        (country_only, zone("Asia/Dubai")),
        (broken, zone("Europe/London")),
        (bare, zone("Europe/London")),
    ];
    expected.sort_by_key(|(id, _)| *id);
    assert_eq!(owners, expected);
    assert!(zones.owners(ZoneLevel::User).await.unwrap().is_empty());
    assert_eq!(zones.levels(), [ZoneLevel::Workspace]);
}

#[tokio::test]
async fn the_fake_can_change_after_boot() {
    let (fake, kolkata, ..) = workspaces();
    let app = TestApp::builder()
        .module(TimeModule::from_config())
        .module(fake.module())
        .build()
        .await;
    fake.set(ZoneOwner::new(kolkata).zone("Europe/Paris"));
    let zones = app.get::<Zones>().await;
    assert_eq!(
        zones
            .zone_of(ZoneLevel::Workspace, kolkata)
            .await
            .unwrap()
            .name(),
        "Europe/Paris"
    );
    fake.remove(kolkata);
    assert_eq!(
        zones
            .zone_of(ZoneLevel::Workspace, kolkata)
            .await
            .unwrap()
            .name(),
        "UTC"
    );
}

#[tokio::test]
async fn the_zones_command_lists_and_filters() {
    let app = TestApp::builder()
        .module(TimeModule::from_config())
        .build()
        .await;
    app.run_command("time:zones", &[]).await.unwrap();
    app.run_command("time:zones", &["--country", "IN"]).await.unwrap();
    app.run_command("time:zones", &["kolkata"]).await.unwrap();
    assert!(app.run_command("time:zones", &["--country", "XX"]).await.is_err());
    assert!(app.run_command("time:zones", &["--bogus"]).await.is_err());
    let listing = navree_time::zones_listing(None, Some("IN"), crate::support::utc("2026-01-01T00:00:00Z"))
        .unwrap();
    assert!(listing.contains("Asia/Kolkata"), "{listing}");
    assert!(listing.contains("+05:30"), "{listing}");
    assert!(!listing.contains("Europe/Paris"), "{listing}");
}
