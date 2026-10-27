//! `TimeModule`: configuration, the `Zones` service, zone sources and the command.

use std::sync::Arc;

use navree_config::ConfigSources;
use navree_contracts::{ZoneLevel, ZoneOwner};
use navree_kernel::App;
use navree_time::{TimeModule, TimeOptions, Zones, ZonesFake};
use serde_json::json;
use uuid::Uuid;

use crate::support::{app, app_with, boot_error, day, utc, zone};

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

async fn zones(app: &App) -> Arc<Zones> {
    app.ctx().get::<Zones>().await.expect("Zones")
}

#[tokio::test]
async fn the_default_zone_comes_from_the_time_section() {
    let app = app(&[("time.default_zone", json!("Asia/Calcutta"))])
        .module(TimeModule::from_config())
        .boot()
        .await
        .unwrap();
    assert_eq!(zones(&app).await.default_zone(), zone("Asia/Kolkata"));
    let options = app.ctx().get::<TimeOptions>().await.unwrap();
    assert_eq!(options.default_zone, "Asia/Calcutta");
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
    let app = app_with(sources)
        .module(TimeModule::from_config())
        .boot()
        .await
        .unwrap();
    let zones = zones(&app).await;
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
    let app = app(&[("time.default_zone", json!("Asia/Tokyo"))])
        .module(TimeModule)
        .boot()
        .await
        .unwrap();
    assert_eq!(zones(&app).await.default_zone(), zone("Asia/Tokyo"));
}

#[tokio::test]
async fn for_root_takes_the_options() {
    let app = app(&[])
        .module(TimeModule::for_root(TimeOptions {
            default_zone: "Asia/Dubai".into(),
            ..TimeOptions::default()
        }))
        .boot()
        .await
        .unwrap();
    assert_eq!(zones(&app).await.default_zone(), zone("Asia/Dubai"));
}

#[tokio::test]
async fn an_unknown_default_zone_fails_the_boot() {
    let error = boot_error(
        app(&[("time.default_zone", json!("Mars/Base"))]).module(TimeModule::from_config()),
    )
    .await;
    assert!(error.contains("time.default_zone"), "{error}");
    assert!(error.contains("Mars/Base"), "{error}");
}

#[tokio::test]
async fn an_unknown_holiday_country_fails_the_boot() {
    let error = boot_error(
        app(&[("time.holidays.country", json!("Atlantis"))]).module(TimeModule::from_config()),
    )
    .await;
    assert!(error.contains("time.holidays.country"), "{error}");
}

#[tokio::test]
async fn invalid_for_root_options_fail_the_boot() {
    let error = boot_error(app(&[]).module(TimeModule::for_root(TimeOptions {
        default_zone: "Nowhere".into(),
        ..TimeOptions::default()
    })))
    .await;
    assert!(error.contains("time.default_zone"), "{error}");
}

struct Owners {
    fake: ZonesFake,
    kolkata: Uuid,
    country_only: Uuid,
    broken: Uuid,
    bare: Uuid,
}

fn workspaces() -> Owners {
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
    Owners {
        fake,
        kolkata,
        country_only,
        broken,
        bare,
    }
}

#[tokio::test]
async fn owners_resolve_to_their_zone_their_country_or_the_default() {
    let w = workspaces();
    let app = app(&[("time.default_zone", json!("Europe/London"))])
        .module(TimeModule::from_config())
        .module(w.fake.module())
        .boot()
        .await
        .unwrap();
    let zones = zones(&app).await;
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
    assert_eq!(of(w.kolkata).await, "Asia/Kolkata");
    assert_eq!(of(w.country_only).await, "Asia/Dubai");
    assert_eq!(of(w.broken).await, "Europe/London", "a bad stored zone falls back");
    assert_eq!(of(w.bare).await, "Europe/London");
    assert_eq!(of(Uuid::now_v7()).await, "Europe/London");
    // No user-level source: the default.
    assert_eq!(
        zones
            .zone_of(ZoneLevel::User, w.kolkata)
            .await
            .unwrap()
            .name(),
        "Europe/London"
    );

    let mut owners = zones.owners(ZoneLevel::Workspace).await.unwrap();
    owners.sort_by_key(|(id, _)| *id);
    let mut expected = vec![
        (w.kolkata, zone("Asia/Kolkata")),
        (w.country_only, zone("Asia/Dubai")),
        (w.broken, zone("Europe/London")),
        (w.bare, zone("Europe/London")),
    ];
    expected.sort_by_key(|(id, _)| *id);
    assert_eq!(owners, expected);
    assert!(zones.owners(ZoneLevel::User).await.unwrap().is_empty());
    assert_eq!(zones.levels(), [ZoneLevel::Workspace]);
}

#[tokio::test]
async fn the_fake_can_change_after_boot() {
    let w = workspaces();
    let app = app(&[])
        .module(TimeModule::from_config())
        .module(w.fake.module())
        .boot()
        .await
        .unwrap();
    w.fake.set(ZoneOwner::new(w.kolkata).zone("Europe/Paris"));
    let zones = zones(&app).await;
    assert_eq!(
        zones
            .zone_of(ZoneLevel::Workspace, w.kolkata)
            .await
            .unwrap()
            .name(),
        "Europe/Paris"
    );
    w.fake.remove(w.kolkata);
    assert_eq!(
        zones
            .zone_of(ZoneLevel::Workspace, w.kolkata)
            .await
            .unwrap()
            .name(),
        "UTC"
    );
}

#[tokio::test]
async fn two_fakes_at_one_level_fail_the_boot() {
    let error = boot_error(
        app(&[])
            .module(TimeModule::from_config())
            .module(ZonesFake::new(ZoneLevel::User).module())
            .module(ZonesFake::new(ZoneLevel::User).module()),
    )
    .await;
    assert!(!error.is_empty());
}

#[tokio::test]
async fn the_zones_command_lists_and_filters() {
    let app = app(&[])
        .module(TimeModule::from_config())
        .boot()
        .await
        .unwrap();
    let run = |args: &[&str]| {
        let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
        app.run_command("time:zones", args)
    };
    run(&[]).await.unwrap();
    run(&["--country", "IN"]).await.unwrap();
    run(&["kolkata"]).await.unwrap();
    assert!(run(&["--country", "XX"]).await.is_err());
    assert!(run(&["--bogus"]).await.is_err());
    assert!(run(&["--country"]).await.is_err());
    let listing =
        navree_time::zones_listing(None, Some("IN"), utc("2026-01-01T00:00:00Z")).unwrap();
    assert!(listing.contains("Asia/Kolkata"), "{listing}");
    assert!(listing.contains("+05:30"), "{listing}");
    assert!(!listing.contains("Europe/Paris"), "{listing}");
    let none = navree_time::zones_listing(Some("nowhere"), None, utc("2026-01-01T00:00:00Z"))
        .unwrap();
    assert_eq!(none, "No time zones match.");
}
