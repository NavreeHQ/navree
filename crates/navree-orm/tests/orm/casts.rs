//! Casts through the database: JSON, string and integer enums, custom
//! casts, nullable casts, and conditions encoded through casts.

use navree_db::sea_orm::ConnectionTrait;
use navree_orm::{Model, OrmError};

use crate::models::{Deal, DealMeta, Invoice, Line, Priority, Stage};
use crate::support::sqlite;

#[tokio::test]
async fn cast_fields_round_trip() {
    let t = sqlite().await;
    let deal = Deal::create(
        &t.db,
        Deal {
            stage: Stage::ClosedWon,
            priority: Priority::High,
            meta: DealMeta { source: "partner".into(), score: 9 },
            tags: vec!["a".into(), "b".into()],
            ..Deal::new("Casts", 1)
        },
    )
    .await
    .unwrap();
    let stored = Deal::find_or_fail(&t.db, deal.id).await.unwrap();
    assert_eq!(stored.stage, Stage::ClosedWon);
    assert_eq!(stored.priority, Priority::High);
    assert_eq!(stored.meta, DealMeta { source: "partner".into(), score: 9 });
    assert_eq!(stored.tags, ["a", "b"]);

    let raw = navree_orm::table("deals")
        .select(["stage", "priority"])
        .first(&t.db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(raw.get::<String>("stage").unwrap(), "closed_won");
    assert_eq!(raw.get::<i64>("priority").unwrap(), 3);

    // Conditions take the field's type and encode it as stored.
    assert_eq!(Deal::query().where_(Deal::STAGE.eq(Stage::ClosedWon)).count(&t.db).await.unwrap(), 1);
    assert_eq!(Deal::query().where_(Deal::PRIORITY.gte(Priority::Normal)).count(&t.db).await.unwrap(), 1);
}

#[tokio::test]
async fn custom_and_nullable_casts() {
    let t = sqlite().await;
    let invoice = Invoice::create(
        &t.db,
        Invoice {
            total: 1250,
            lines: Some(vec![Line { item: "seat".into(), cents: 1250 }]),
            previous_stage: None,
            discount: Some(50),
            ..Invoice::default()
        },
    )
    .await
    .unwrap();
    let stored = Invoice::find_or_fail(&t.db, invoice.id).await.unwrap();
    assert_eq!(*stored, *invoice);
    let raw = navree_orm::table("invoices").first(&t.db).await.unwrap().unwrap();
    assert_eq!(raw.get::<String>("total").unwrap(), "1250");
    assert_eq!(raw.get::<Option<String>>("previous_stage").unwrap(), None);
    assert_eq!(raw.get::<String>("discount").unwrap(), "50");

    let mut stored = stored;
    stored.previous_stage = Some(Stage::Qualified);
    stored.lines = None;
    stored.save(&t.db).await.unwrap();
    let again = Invoice::find_or_fail(&t.db, invoice.id).await.unwrap();
    assert_eq!(again.previous_stage, Some(Stage::Qualified));
    assert!(again.lines.is_none());
    assert_eq!(Invoice::query().where_(Invoice::PREVIOUS_STAGE.eq(Stage::Qualified)).count(&t.db).await.unwrap(), 1);
    assert_eq!(Invoice::query().where_(Invoice::LINES.eq(None)).count(&t.db).await.unwrap(), 1);
}

#[tokio::test]
async fn bad_stored_values_fail_with_the_casts_message() {
    let t = sqlite().await;
    t.db.conn()
        .execute_unprepared("INSERT INTO invoices (total) VALUES ('12.5')")
        .await
        .unwrap();
    let error = Invoice::query().first(&t.db).await.unwrap_err();
    assert!(matches!(error, OrmError::Cast { .. }), "{error:?}");
    assert_eq!(
        error.to_string(),
        "cannot cast column `total` of Invoice: `12.5` is not a number of cents"
    );
}
