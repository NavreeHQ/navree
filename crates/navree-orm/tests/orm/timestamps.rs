//! Timestamps from the app's clock.

use chrono::Duration;
use navree_contracts::FixedClock;
use navree_orm::{Model, Record};

use crate::models::{Company, Deal, Setting};
use crate::support::{app, epoch};

fn advance(clock: &FixedClock, by: Duration) {
    clock.advance(by.to_std().expect("a forward step"));
}

#[tokio::test]
async fn timestamps_come_from_the_apps_clock() {
    let (_app, db, clock) = app(Vec::new()).await;
    let mut acme = Company::create(&*db, Company { name: "Acme".into(), ..Company::default() })
        .await
        .unwrap();
    assert_eq!(acme.created_at, epoch());
    assert_eq!(acme.updated_at, epoch());

    advance(&clock, Duration::minutes(5));
    acme.name = "Acme Corp".into();
    acme.save(&*db).await.unwrap();
    assert_eq!(acme.created_at, epoch());
    assert_eq!(acme.updated_at, epoch() + Duration::minutes(5));

    // An update timestamp the caller set is kept.
    let chosen = epoch() - Duration::days(1);
    acme.updated_at = chosen;
    acme.employees = 3;
    acme.save(&*db).await.unwrap();
    assert_eq!(Company::find_or_fail(&*db, acme.id).await.unwrap().updated_at, chosen);

    // Touch sets it to now.
    advance(&clock, Duration::minutes(5));
    assert!(acme.touch(&*db).await.unwrap());
    assert_eq!(acme.updated_at, epoch() + Duration::minutes(10));
    assert_eq!(Company::find_or_fail(&*db, acme.id).await.unwrap().updated_at, acme.updated_at);
}

#[tokio::test]
async fn caller_set_creation_timestamps_are_kept() {
    let (_app, db, _clock) = app(Vec::new()).await;
    let imported = epoch() - Duration::days(30);
    let deal = Deal::create(&*db, Deal { created_at: imported, ..Deal::new("Old", 1) })
        .await
        .unwrap();
    assert_eq!(deal.created_at, imported);
    assert_eq!(deal.updated_at, epoch());
}

#[tokio::test]
async fn models_with_one_timestamp_and_touching_other_columns() {
    let (_app, db, clock) = app(Vec::new()).await;
    let mut setting = Setting::create(&*db, Setting { key: "theme".into(), value: "dark".into(), ..Setting::default() })
        .await
        .unwrap();
    assert_eq!(setting.updated_at, epoch());

    advance(&clock, Duration::hours(1));
    assert!(setting.touch_column(&*db, Setting::UPDATED_AT).await.unwrap());
    assert_eq!(setting.updated_at, epoch() + Duration::hours(1));

    let mut deal = Deal::create(&*db, Deal::new("Closing", 1)).await.unwrap();
    advance(&clock, Duration::hours(1));
    deal.touch_column(&*db, Deal::CLOSED_AT).await.unwrap();
    assert_eq!(deal.closed_at, Some(epoch() + Duration::hours(2)));

    let mut event = Record::new(crate::models::Event::default());
    assert!(!event.touch(&*db).await.unwrap(), "no update timestamp to touch");
}

#[tokio::test]
async fn mass_writes_set_the_update_timestamp() {
    let (_app, db, clock) = app(Vec::new()).await;
    Company::create(&*db, Company { name: "A".into(), ..Company::default() })
        .await
        .unwrap();
    advance(&clock, Duration::minutes(1));
    Company::query().update(&*db, [Company::EMPLOYEES.set(7)]).await.unwrap();
    let stored = Company::find_or_fail(&*db, 1).await.unwrap();
    assert_eq!((stored.employees, stored.updated_at), (7, epoch() + Duration::minutes(1)));

    advance(&clock, Duration::minutes(1));
    Company::query().increment(&*db, Company::EMPLOYEES, 2).await.unwrap();
    let stored = Company::find_or_fail(&*db, 1).await.unwrap();
    assert_eq!((stored.employees, stored.updated_at), (9, epoch() + Duration::minutes(2)));

    advance(&clock, Duration::minutes(1));
    assert_eq!(Company::query().touch(&*db).await.unwrap(), 1);
    assert_eq!(Company::find_or_fail(&*db, 1).await.unwrap().updated_at, epoch() + Duration::minutes(3));
}
