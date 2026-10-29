//! Creating, saving, deleting, refreshing, replicating, touching and
//! incrementing records.

use navree_db::Executor as _;
use navree_orm::{Model, OrmError, Record, Ulid};
use uuid::Uuid;

use crate::models::{Company, Deal, Event, Setting, Stage};
use crate::support::{count_rows, sqlite};

fn company(name: &str) -> Company {
    Company {
        name: name.to_owned(),
        active: true,
        ..Company::default()
    }
}

#[tokio::test]
async fn create_reads_back_a_database_assigned_key_and_fills_timestamps() {
    let t = sqlite().await;
    let first = Company::create(&t.db, company("Acme")).await.unwrap();
    let second = Company::create(&t.db, company("Globex")).await.unwrap();
    assert_eq!((first.id, second.id), (1, 2));
    assert!(first.exists());
    assert!(first.was_recently_created());
    assert_ne!(
        first.created_at,
        chrono::DateTime::<chrono::Utc>::UNIX_EPOCH
    );
    assert_eq!(first.created_at, first.updated_at);
    assert!(first.is_clean(), "a saved record is clean");
    assert!(!first.was_changed(), "an insert records no changes");

    let found = Company::find(&t.db, 1).await.unwrap().unwrap();
    assert_eq!(found.name, "Acme");
    assert_eq!(found.created_at, first.created_at);
    assert!(!found.was_recently_created());
}

#[tokio::test]
async fn uuid_and_ulid_keys_are_generated_when_empty_and_kept_when_set() {
    let t = sqlite().await;
    let deal = Deal::create(&t.db, Deal::new("Renewal", 1000))
        .await
        .unwrap();
    assert_eq!(deal.id.get_version_num(), 7);

    let chosen = Uuid::now_v7();
    let deal = Deal::create(
        &t.db,
        Deal {
            id: chosen,
            ..Deal::new("Upsell", 50)
        },
    )
    .await
    .unwrap();
    assert_eq!(deal.id, chosen);
    assert!(Deal::find(&t.db, chosen).await.unwrap().is_some());

    let event = Event::create(
        &t.db,
        Event {
            name: "signup".into(),
            ..Event::default()
        },
    )
    .await
    .unwrap();
    assert!(!event.id.is_nil());
    let found = Event::find(&t.db, event.id).await.unwrap().unwrap();
    assert_eq!(found.name, "signup");
    assert_eq!(found.id, event.id);
}

#[tokio::test]
async fn an_application_key_must_be_set() {
    let t = sqlite().await;
    let error = Setting::create(
        &t.db,
        Setting {
            value: "dark".into(),
            ..Setting::default()
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(error, OrmError::MissingKey { .. }), "{error:?}");
    assert_eq!(
        error.to_string(),
        "cannot insert a Setting without a key: set `key` first (its key type is manual, so no key is generated)"
    );
    let setting = Setting::create(
        &t.db,
        Setting {
            key: "theme".into(),
            value: "dark".into(),
            ..Setting::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        Setting::find_or_fail(&t.db, "theme".to_owned())
            .await
            .unwrap()
            .value,
        "dark"
    );
    assert_eq!(setting.key, "theme");
}

#[tokio::test]
async fn save_updates_only_the_dirty_columns() {
    let t = sqlite().await;
    let mut acme = Company::create(&t.db, company("Acme")).await.unwrap();
    // Someone else changes another column meanwhile.
    t.db.pool()
        .execute_script("UPDATE companies SET employees = 50 WHERE id = 1")
        .await
        .unwrap();
    acme.name = "Acme Corp".into();
    assert!(acme.save(&t.db).await.unwrap());
    let stored = Company::find_or_fail(&t.db, 1).await.unwrap();
    assert_eq!(stored.name, "Acme Corp");
    assert_eq!(stored.employees, 50, "untouched columns are not written");
    assert!(acme.was_changed_on(["name"]));
    assert!(acme.was_changed_on([Company::UPDATED_AT]));
    assert!(!acme.was_changed_on([Company::EMPLOYEES]));

    // A clean record writes nothing (and reports success).
    let before = stored.updated_at;
    let mut stored = stored;
    assert!(stored.save(&t.db).await.unwrap());
    assert_eq!(
        Company::find_or_fail(&t.db, 1).await.unwrap().updated_at,
        before
    );
}

#[tokio::test]
async fn update_changes_and_saves_a_stored_record_only() {
    let t = sqlite().await;
    let mut acme = Company::create(&t.db, company("Acme")).await.unwrap();
    assert!(acme.update(&t.db, |c| c.employees = 12).await.unwrap());
    assert_eq!(Company::find_or_fail(&t.db, 1).await.unwrap().employees, 12);

    let mut fresh = Record::new(company("Unsaved"));
    assert!(!fresh.update(&t.db, |c| c.employees = 3).await.unwrap());
    assert_eq!(fresh.employees, 0, "nothing changed");
    assert_eq!(count_rows(&t.db, "companies").await, 1);
}

#[tokio::test]
async fn delete_removes_the_row_and_destroy_deletes_by_key() {
    let t = sqlite().await;
    let mut acme = Company::create(&t.db, company("Acme")).await.unwrap();
    for name in ["B", "C", "D"] {
        Company::create(&t.db, company(name)).await.unwrap();
    }
    assert!(acme.delete(&t.db).await.unwrap());
    assert!(!acme.exists());
    assert!(!acme.delete(&t.db).await.unwrap(), "already gone");
    assert_eq!(count_rows(&t.db, "companies").await, 3);

    assert_eq!(Company::destroy(&t.db, [2_i64, 3, 99]).await.unwrap(), 2);
    assert_eq!(Company::destroy(&t.db, Vec::<i64>::new()).await.unwrap(), 0);
    assert_eq!(count_rows(&t.db, "companies").await, 1);
}

#[tokio::test]
async fn refresh_and_fresh_reload_from_the_database() {
    let t = sqlite().await;
    let mut acme = Company::create(&t.db, company("Acme")).await.unwrap();
    t.db.pool()
        .execute_script("UPDATE companies SET name = 'Renamed' WHERE id = 1")
        .await
        .unwrap();
    let fresh = acme.fresh(&t.db).await.unwrap().unwrap();
    assert_eq!(fresh.name, "Renamed");
    assert_eq!(acme.name, "Acme", "fresh leaves the record alone");

    acme.name = "Local".into();
    acme.refresh(&t.db).await.unwrap();
    assert_eq!(acme.name, "Renamed");
    assert!(acme.is_clean());

    t.db.pool()
        .execute_script("DELETE FROM companies")
        .await
        .unwrap();
    assert!(acme.fresh(&t.db).await.unwrap().is_none());
    let error = acme.refresh(&t.db).await.unwrap_err();
    assert!(error.is_not_found());
    assert_eq!(error.to_string(), "no Company found for key 1");

    let mut unsaved = Record::new(company("New"));
    unsaved.refresh(&t.db).await.unwrap();
    assert!(unsaved.fresh(&t.db).await.unwrap().is_none());
}

#[tokio::test]
async fn replicate_copies_the_columns_without_key_and_timestamps() {
    let t = sqlite().await;
    let original = Deal::create(
        &t.db,
        Deal {
            stage: Stage::Qualified,
            ..Deal::new("Template", 900)
        },
    )
    .await
    .unwrap();
    let mut copy = original.replicate();
    assert!(!copy.exists());
    assert!(copy.id.is_nil());
    assert_eq!(copy.stage, Stage::Qualified);
    assert_eq!(copy.title, "Template");
    copy.title = "Copy".into();
    copy.save(&t.db).await.unwrap();
    assert_ne!(copy.id, original.id);
    assert_eq!(count_rows(&t.db, "deals").await, 2);
}

#[tokio::test]
async fn increment_and_decrement_change_the_row_and_the_record() {
    let t = sqlite().await;
    let mut acme = Company::create(&t.db, company("Acme")).await.unwrap();
    t.db.pool()
        .execute_script("UPDATE companies SET employees = 10 WHERE id = 1")
        .await
        .unwrap();
    // The record holds 0, the row 10: the database does the arithmetic.
    assert!(acme.increment(&t.db, Company::EMPLOYEES, 5).await.unwrap());
    assert_eq!(acme.employees, 5);
    assert_eq!(Company::find_or_fail(&t.db, 1).await.unwrap().employees, 15);
    assert!(acme.was_changed_on([Company::EMPLOYEES]));
    assert!(acme.is_clean());

    acme.decrement_with(
        &t.db,
        Company::EMPLOYEES,
        2,
        [Company::NAME.set("Acme Ltd")],
    )
    .await
    .unwrap();
    assert_eq!(acme.name, "Acme Ltd");
    let stored = Company::find_or_fail(&t.db, 1).await.unwrap();
    assert_eq!((stored.employees, stored.name.as_str()), (13, "Acme Ltd"));

    let error = Record::new(company("New"))
        .increment(&t.db, Company::EMPLOYEES, 1)
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "cannot increment a Company that has not been saved"
    );

    let mut max = Company::find_or_fail(&t.db, 1).await.unwrap();
    max.employees = i32::MAX;
    let error = max
        .increment(&t.db, Company::EMPLOYEES, 1)
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "invalid query: incrementing `employees` would overflow i32"
    );
}

#[tokio::test]
async fn find_many_find_or_fail_and_all() {
    let t = sqlite().await;
    for name in ["A", "B", "C"] {
        Company::create(&t.db, company(name)).await.unwrap();
    }
    let some = Company::find_many(&t.db, [3_i64, 1, 42]).await.unwrap();
    let mut keys = some.model_keys();
    keys.sort_unstable();
    assert_eq!(keys, [1, 3]);
    assert_eq!(Company::all(&t.db).await.unwrap().len(), 3);
    assert!(Company::find(&t.db, 42).await.unwrap().is_none());
    let error = Company::find_or_fail(&t.db, 42).await.unwrap_err();
    assert!(error.is_not_found());
    assert_eq!(error.to_string(), "no Company found for key 42");
}

#[tokio::test]
async fn unique_violations_are_recognised() {
    let t = sqlite().await;
    let tag = crate::models::Tag {
        name: "vip".into(),
        color: "gold".into(),
        ..Default::default()
    };
    crate::models::Tag::create(&t.db, tag.clone())
        .await
        .unwrap();
    let error = crate::models::Tag::create(&t.db, tag).await.unwrap_err();
    assert!(error.is_unique_violation(), "{error:?}");
}

#[tokio::test]
async fn ulid_keys_order_by_creation() {
    let t = sqlite().await;
    let mut ids = Vec::new();
    for name in ["a", "b", "c"] {
        let event = Event::create(
            &t.db,
            Event {
                name: name.into(),
                ..Event::default()
            },
        )
        .await
        .unwrap();
        ids.push(event.id);
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    let ordered: Vec<Ulid> = Event::query()
        .order_by(Event::ID)
        .get(&t.db)
        .await
        .unwrap()
        .model_keys();
    assert_eq!(ordered, ids);
}
