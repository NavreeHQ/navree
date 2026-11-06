//! Dirty tracking: what changed since a record was read or saved, and what
//! its last save changed.

use navree_orm::{Decimal, Model, Record, Value};

use crate::models::{Company, Deal, Stage};
use crate::support::sqlite;

#[tokio::test]
async fn a_new_record_is_dirty_in_every_column() {
    let record = Record::new(Company {
        name: "Agra".into(),
        ..Company::default()
    });
    assert!(record.is_dirty());
    assert_eq!(
        record.get_dirty().unwrap().len(),
        Company::meta().all_columns().len()
    );
    assert!(record.get_original().is_none());
}

#[tokio::test]
async fn changes_are_tracked_per_column_until_saved() {
    let t = sqlite().await;
    Deal::create(&t.db, Deal::new("Renewal", 1000))
        .await
        .unwrap();
    let mut deal = Deal::query().first_or_fail(&t.db).await.unwrap();
    assert!(deal.is_clean());

    deal.stage = Stage::ClosedWon;
    deal.amount_cents = 1500;
    assert!(deal.is_dirty());
    assert!(deal.is_dirty_on([Deal::STAGE]));
    assert!(deal.is_dirty_on(["amount_cents"]));
    assert!(deal.is_clean_on([Deal::TITLE]));
    assert_eq!(
        deal.get_dirty().unwrap(),
        [
            ("amount_cents", Value::I64(1500)),
            ("stage", Value::from("closed_won")),
        ]
    );
    assert_eq!(deal.get_original().unwrap().stage, Stage::Lead);
    assert_eq!(
        deal.get_original_value(Deal::AMOUNT_CENTS).unwrap(),
        Some(Value::I64(1000))
    );

    deal.save(&t.db).await.unwrap();
    assert!(deal.is_clean());
    assert!(deal.was_changed());
    assert!(deal.was_changed_on(["stage", "title"]));
    assert!(!deal.was_changed_on([Deal::TITLE]));
    let changed: Vec<&str> = deal
        .get_changes()
        .iter()
        .map(|(column, _)| *column)
        .collect();
    assert_eq!(changed, ["amount_cents", "stage", "updated_at"]);
    assert_eq!(deal.get_previous()[1], ("stage", Value::from("lead")));
    assert_eq!(deal.get_original().unwrap().stage, Stage::ClosedWon);
}

#[tokio::test]
async fn discarding_changes_restores_the_original() {
    let t = sqlite().await;
    let mut deal = Deal::create(&t.db, Deal::new("Renewal", 1000))
        .await
        .unwrap();
    deal.title = "Changed".into();
    deal.discard_changes();
    assert_eq!(deal.title, "Renewal");
    assert!(deal.is_clean());
}

#[tokio::test]
async fn equivalent_values_are_not_changes() {
    let t = sqlite().await;
    let mut deal = Deal::create(
        &t.db,
        Deal {
            probability: 0.3,
            value: Decimal::new(125, 1),
            ..Deal::new("Renewal", 1000)
        },
    )
    .await
    .unwrap();
    // Arithmetic noise in a float, a decimal with another scale.
    deal.probability = 0.1 + 0.2;
    deal.value = Decimal::new(1250, 2);
    assert!(deal.is_clean(), "{:?}", deal.get_dirty());

    // Read back from SQLite (which keeps NUMERIC as a float).
    let mut stored = Deal::find_or_fail(&t.db, deal.id).await.unwrap();
    assert_eq!(stored.value, Decimal::new(125, 1));
    stored.value = Decimal::new(1250, 2);
    assert!(stored.is_clean());
}

#[tokio::test]
async fn syncing_selected_columns() {
    let t = sqlite().await;
    let mut deal = Deal::create(&t.db, Deal::new("Renewal", 1000))
        .await
        .unwrap();
    deal.title = "A".into();
    deal.amount_cents = 1;
    deal.sync_original_on([Deal::TITLE]).unwrap();
    assert!(deal.is_clean_on([Deal::TITLE]));
    assert!(deal.is_dirty_on([Deal::AMOUNT_CENTS]));
    deal.sync_original();
    assert!(deal.is_clean());
}
