//! Models on MongoDB: create, read, save, refresh, delete, soft delete,
//! keys of every kind, embedded documents and duplicate keys.

use bson::doc;
use chrono::{NaiveDate, Timelike};
use navree_mongo::ObjectId;
use navree_orm::{Decimal, Model, OrmError, Record};

use crate::models::{Account, Company, Deal, DealMeta, Stage, Ticket};
use crate::support::TestMongo;

#[tokio::test]
async fn models_are_created_found_saved_and_deleted() {
    let Some(t) = TestMongo::connect("models_are_created_found_saved_and_deleted").await else {
        return;
    };
    let mongo = &t.db;
    let acme = Company::create(
        mongo,
        Company {
            name: "Acme".into(),
            country: Some("NO".into()),
            employees: 12,
            ..Company::default()
        },
    )
    .await
    .unwrap();
    assert!(!acme.id.is_nil(), "the ObjectId was made before the insert");
    assert!(acme.exists() && acme.was_recently_created());
    assert_eq!(
        acme.created_at.nanosecond() % 1_000_000,
        0,
        "timestamps keep MongoDB's milliseconds"
    );

    let mut deal = Deal::create(
        mongo,
        Deal {
            company_id: Some(acme.id),
            price: Decimal::new(129_950, 2),
            stage: Stage::Qualified,
            meta: Some(DealMeta {
                source: "web".into(),
                score: 7,
                labels: vec!["vip".into()],
            }),
            tags: vec!["renewal".into(), "q4".into()],
            closes_on: NaiveDate::from_ymd_opt(2026, 12, 31),
            ..Deal::new("Renewal", 120_000)
        },
    )
    .await
    .unwrap();

    // Stored natively: an ObjectId, a Decimal128, a date, an embedded
    // document and an array.
    let raw = t
        .db
        .collection("deals")
        .find_one(doc! { "_id": bson::oid::ObjectId::from(deal.id) })
        .await
        .unwrap()
        .expect("the document");
    assert_eq!(raw.get_object_id("company_id").unwrap(), acme.id.into());
    assert!(matches!(raw.get("price"), Some(bson::Bson::Decimal128(_))));
    assert_eq!(raw.get_str("stage").unwrap(), "qualified");
    assert_eq!(raw.get_document("meta").unwrap().get_str("source").unwrap(), "web");
    assert_eq!(raw.get_array("tags").unwrap().len(), 2);
    assert!(raw.get_datetime("closes_on").is_ok());
    assert!(raw.get("deleted_at").is_some_and(|value| *value == bson::Bson::Null));

    let found = Deal::find_or_fail(mongo, deal.id).await.unwrap();
    assert_eq!(found.model(), deal.model(), "every field reads back as written");

    deal.amount = 150_000;
    deal.stage = Stage::Won;
    assert!(deal.is_dirty_on(["amount", "stage"]));
    assert!(deal.save(mongo).await.unwrap());
    assert!(deal.was_changed_on([Deal::AMOUNT]));
    let mut stale = found;
    stale.refresh(mongo).await.unwrap();
    assert_eq!((stale.amount, stale.stage), (150_000, Stage::Won));

    // Soft deletes: hidden from queries, kept in the collection.
    assert!(deal.delete(mongo).await.unwrap());
    assert!(deal.trashed());
    assert!(Deal::find(mongo, deal.id).await.unwrap().is_none());
    let trashed = Deal::query()
        .with_trashed()
        .find(mongo, deal.id)
        .await
        .unwrap()
        .expect("still stored");
    assert!(trashed.deleted_at.is_some());

    let error = Deal::find_or_fail(mongo, ObjectId::new()).await.unwrap_err();
    assert!(error.is_not_found(), "{error}");

    let mut acme = acme;
    assert!(acme.delete(mongo).await.unwrap());
    assert_eq!(Company::query().count(mongo).await.unwrap(), 0);
    t.cleanup().await;
}

#[tokio::test]
async fn integer_keys_are_counted_per_collection() {
    let Some(t) = TestMongo::connect("integer_keys_are_counted_per_collection").await else {
        return;
    };
    let mongo = &t.db;
    let mut ids = Vec::new();
    for subject in ["a", "b", "c"] {
        let ticket = Ticket::create(
            mongo,
            Ticket {
                subject: subject.into(),
                ..Ticket::default()
            },
        )
        .await
        .unwrap();
        ids.push(ticket.id);
    }
    assert_eq!(ids, [1, 2, 3]);
    let counter = t
        .db
        .collection("navree_sequences")
        .find_one(doc! { "_id": "tickets._id" })
        .await
        .unwrap()
        .expect("the counter");
    assert_eq!(counter.get_i64("value").unwrap(), 3);
    assert_eq!(Ticket::find_or_fail(mongo, 2).await.unwrap().subject, "b");
    t.cleanup().await;
}

#[tokio::test]
async fn manual_keys_and_duplicates() {
    let Some(t) = TestMongo::connect("manual_keys_and_duplicates").await else {
        return;
    };
    let mongo = &t.db;
    t.db.schema()
        .create("accounts", |t| {
            t.string("email").unique();
            t.string("name");
            t.uuid("external");
        })
        .await
        .unwrap();
    let error = Account::create(mongo, Account::default()).await.unwrap_err();
    assert!(matches!(error, OrmError::MissingKey { .. }), "{error:?}");
    let ada = Account {
        email: "ada@example.com".into(),
        name: "Ada".into(),
        external: uuid::Uuid::now_v7(),
    };
    Account::create(mongo, ada.clone()).await.unwrap();
    let found = Account::find_or_fail(mongo, "ada@example.com".to_owned())
        .await
        .unwrap();
    assert_eq!(found.external, ada.external, "UUIDs round-trip (binary subtype 4)");

    // A duplicate key is a unique violation, as on the SQL databases.
    let error = Account::create(mongo, ada.clone()).await.unwrap_err();
    assert!(error.is_unique_violation(), "{error:?}");
    // ...which create_or_first recovers from.
    let existing = Account::query()
        .create_or_first(mongo, [Account::EMAIL.set("ada@example.com")], |a| {
            a.name = "Someone else".into()
        })
        .await
        .unwrap();
    assert_eq!(existing.name, "Ada");
    let (first, created) = {
        let record: Record<Account> = Account::first_or_create(
            mongo,
            [Account::EMAIL.set("grace@example.com")],
            |a| a.name = "Grace".into(),
        )
        .await
        .unwrap();
        let created = record.was_recently_created();
        (record, created)
    };
    assert!(created && first.name == "Grace");
    t.cleanup().await;
}
