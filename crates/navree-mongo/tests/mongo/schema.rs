//! The schema builder on a real server: validators that refuse what a
//! blueprint forbids, unique indexes over nullable fields, alterations of
//! documents, validators and indexes, time-series collections, views from
//! continuous aggregates, pretending, renames and drops.

use bson::{Bson, Document, doc};
use chrono::{DateTime, Duration, TimeZone, Utc};
use navree_mongo::MongoError;
use navree_orm::{Model, table};
use navree_schema::ContinuousAggregate;

use crate::models::Reading;
use crate::support::TestMongo;

fn index_named<'a>(indexes: &'a [Document], name: &str) -> Option<&'a Document> {
    indexes
        .iter()
        .find(|index| index.get_str("name").ok() == Some(name))
}

#[tokio::test]
async fn validators_refuse_what_the_blueprint_forbids() {
    let Some(t) = TestMongo::connect("validators_refuse_what_the_blueprint_forbids").await else {
        return;
    };
    let schema = t.db.schema();
    schema
        .create("deals", |t| {
            t.object_id_primary();
            t.string("title").length(20);
            t.integer("amount").unsigned();
            t.enumeration("stage", ["lead", "won"]);
            t.timestamp_tz("closed_at").nullable();
        })
        .await
        .unwrap();
    let deals = t.db.collection("deals");
    deals
        .insert_one(doc! { "title": "Renewal", "amount": 5, "stage": "won", "closed_at": Bson::Null })
        .await
        .unwrap();
    deals
        .insert_one(doc! { "title": "Pilot", "amount": 0, "stage": "lead" })
        .await
        .expect("a nullable field may be missing");
    for (broken, why) in [
        (doc! { "amount": 5, "stage": "won" }, "a required field is missing"),
        (doc! { "title": "x".repeat(21), "amount": 5, "stage": "won" }, "too long"),
        (doc! { "title": "A", "amount": -1, "stage": "won" }, "unsigned"),
        (doc! { "title": "A", "amount": 5, "stage": "lost" }, "not an allowed value"),
        (doc! { "title": "A", "amount": "5", "stage": "won" }, "the wrong type"),
        (doc! { "title": "A", "amount": 5, "stage": "won", "closed_at": "today" }, "not a date"),
    ] {
        let error = deals.insert_one(broken).await.expect_err(why);
        assert!(error.to_string().contains("Document failed validation"), "{why}: {error}");
    }
    let validator = schema.validator("deals").await.unwrap();
    assert_eq!(
        validator.get_array("required").unwrap(),
        &vec![Bson::from("_id"), Bson::from("title"), Bson::from("amount"), Bson::from("stage")]
    );
    assert!(schema.has_table("deals").await.unwrap());
    assert!(schema.has_column("deals", "title").await.unwrap());
    assert!(!schema.has_column("deals", "budget").await.unwrap());
    assert!(!schema.has_table("tickets").await.unwrap());

    // Creating it again is an error, unless asked to skip it.
    let error = schema
        .create("deals", |t| {
            t.object_id_primary();
        })
        .await
        .unwrap_err();
    assert!(matches!(error, MongoError::Schema { .. }), "{error:?}");
    assert!(error.to_string().contains("already exists"), "{error}");
    schema
        .create("deals", |t| {
            t.if_not_exists();
            t.object_id_primary();
        })
        .await
        .unwrap();
    t.cleanup().await;
}

#[tokio::test]
async fn unique_indexes_let_nulls_repeat_as_in_sql() {
    let Some(t) = TestMongo::connect("unique_indexes_let_nulls_repeat_as_in_sql").await else {
        return;
    };
    t.db.schema()
        .create("accounts", |t| {
            t.object_id_primary();
            t.string("email").unique();
            t.string("phone").nullable().unique();
        })
        .await
        .unwrap();
    let accounts = t.db.collection("accounts");
    for account in [
        doc! { "email": "a@example.com", "phone": Bson::Null },
        doc! { "email": "b@example.com", "phone": Bson::Null },
        doc! { "email": "c@example.com" },
        doc! { "email": "d@example.com", "phone": "+47 1" },
    ] {
        accounts.insert_one(account).await.unwrap();
    }
    for duplicate in [
        doc! { "email": "e@example.com", "phone": "+47 1" },
        doc! { "email": "a@example.com", "phone": "+47 2" },
    ] {
        let error = accounts.insert_one(duplicate).await.unwrap_err();
        assert!(error.to_string().contains("E11000"), "{error}");
    }
    t.cleanup().await;
}

#[tokio::test]
async fn alterations_change_documents_validators_and_indexes() {
    let Some(t) = TestMongo::connect("alterations_change_documents_validators_and_indexes").await
    else {
        return;
    };
    let schema = t.db.schema();
    schema
        .create("deals", |t| {
            t.object_id_primary();
            t.string("title");
            t.integer("amount").index();
        })
        .await
        .unwrap();
    let deals = t.db.collection("deals");
    deals
        .insert_many([doc! { "title": "A", "amount": 1 }, doc! { "title": "B", "amount": 2 }])
        .await
        .unwrap();

    // A new field with a default: existing documents get it.
    schema
        .table("deals", |t| {
            t.string("stage").default("lead");
        })
        .await
        .unwrap();
    assert_eq!(deals.count_documents(doc! { "stage": "lead" }).await.unwrap(), 2);
    assert!(schema.has_column("deals", "stage").await.unwrap());
    // A required field without one cannot be added to documents lacking it.
    let error = schema
        .table("deals", |t| {
            t.string("owner");
        })
        .await
        .unwrap_err();
    assert!(error.to_string().contains("lack it"), "{error}");

    // Renamed: in every document, the validator and the indexes.
    let before = schema.indexes("deals").await.unwrap();
    let amount_index = before
        .iter()
        .find(|index| index.get_document("key").is_ok_and(|key| key.contains_key("amount")))
        .and_then(|index| index.get_str("name").ok())
        .expect("the index over amount")
        .to_owned();
    schema
        .table("deals", |t| {
            t.rename_column("amount", "value");
        })
        .await
        .unwrap();
    assert_eq!(deals.count_documents(doc! { "value": { "$exists": true } }).await.unwrap(), 2);
    assert_eq!(deals.count_documents(doc! { "amount": { "$exists": true } }).await.unwrap(), 0);
    let properties = schema.validator("deals").await.unwrap();
    let properties = properties.get_document("properties").unwrap();
    assert!(properties.contains_key("value") && !properties.contains_key("amount"));
    let indexes = schema.indexes("deals").await.unwrap();
    let rebuilt = index_named(&indexes, &amount_index).expect("rebuilt under its name");
    assert_eq!(rebuilt.get_document("key").unwrap(), &doc! { "value": 1 });

    // Indexes renamed and dropped with their columns.
    schema
        .table("deals", |t| {
            t.rename_index(&amount_index, "deals_value_index");
        })
        .await
        .unwrap();
    let indexes = schema.indexes("deals").await.unwrap();
    assert!(index_named(&indexes, "deals_value_index").is_some());
    assert!(index_named(&indexes, &amount_index).is_none());
    schema
        .table("deals", |t| {
            t.drop_column("value");
        })
        .await
        .unwrap();
    assert_eq!(deals.count_documents(doc! { "value": { "$exists": true } }).await.unwrap(), 0);
    assert!(index_named(&schema.indexes("deals").await.unwrap(), "deals_value_index").is_none());
    assert!(!schema.has_column("deals", "value").await.unwrap());
    // The validator follows: documents without `value` are valid again.
    deals
        .insert_one(doc! { "title": "C", "stage": "won" })
        .await
        .unwrap();
    t.cleanup().await;
}

#[tokio::test]
async fn hypertables_are_time_series_collections() {
    let Some(t) = TestMongo::connect("hypertables_are_time_series_collections").await else {
        return;
    };
    let schema = t.db.schema();
    schema
        .create_hypertable("readings", "taken_at", |t| {
            t.object_id_primary();
            t.string("sensor");
            t.double("celsius");
            t.timestamp_tz("taken_at");
            t.hypertable("taken_at")
                .partition_by("sensor", 4)
                .chunk_interval("1 hour")
                .retain_for("30 days");
        })
        .await
        .unwrap();
    let options = schema.collection_options("readings").await.unwrap().unwrap();
    let series = options.get_document("timeseries").unwrap();
    assert_eq!(series.get_str("timeField").unwrap(), "taken_at");
    assert_eq!(series.get_str("metaField").unwrap(), "sensor");
    assert_eq!(series.get_str("granularity").unwrap(), "seconds");
    assert_eq!(options.get_i64("expireAfterSeconds").unwrap(), 30 * 24 * 3600);
    assert!(!options.contains_key("validator"));
    assert!(
        schema.notices().iter().any(|notice| notice.contains("take no validator")),
        "{:?}",
        schema.notices()
    );
    // Altered, it still takes none.
    schema
        .table("readings", |t| {
            t.string("unit").nullable();
        })
        .await
        .unwrap();

    // Models read and write it like any collection.
    let start: DateTime<Utc> = Utc.with_ymd_and_hms(2026, 10, 27, 8, 0, 0).unwrap();
    for (sensor, minutes, celsius) in [("roof", 5, 10.0), ("roof", 65, 20.0), ("cellar", 10, 4.0)] {
        Reading::create(
            &t.db,
            Reading {
                sensor: sensor.into(),
                celsius,
                taken_at: start + Duration::minutes(minutes),
                ..Reading::default()
            },
        )
        .await
        .unwrap();
    }
    assert_eq!(Reading::query().where_(Reading::SENSOR.eq("roof")).count(&t.db).await.unwrap(), 2);

    // A continuous aggregate is a view, computed when read.
    schema
        .create_continuous_aggregate(
            &ContinuousAggregate::over("readings_hourly", "readings")
                .time_bucket("1 hour", "taken_at", "bucket")
                .group_by("sensor")
                .aggregate("avg(celsius)", "average")
                .aggregate("count(*)", "readings"),
        )
        .await
        .unwrap();
    let hourly = table("readings_hourly")
        .order_by("bucket")
        .order_by("sensor")
        .get(&t.db)
        .await
        .unwrap();
    let rows: Vec<(DateTime<Utc>, String, f64, i64)> = hourly
        .iter()
        .map(|row| {
            (
                row.get("bucket").unwrap(),
                row.get("sensor").unwrap(),
                row.get("average").unwrap(),
                row.get("readings").unwrap(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            (start, "cellar".to_owned(), 4.0, 1),
            (start, "roof".to_owned(), 10.0, 1),
            (start + Duration::hours(1), "roof".to_owned(), 20.0, 1),
        ]
    );
    assert!(
        schema.collection_options("readings_hourly").await.unwrap().unwrap().contains_key("viewOn")
    );

    // An existing collection cannot become one.
    let error = schema.convert_to_hypertable("deals", "created_at").await.unwrap_err();
    assert!(error.to_string().contains("time-series"), "{error}");
    t.cleanup().await;
}

#[tokio::test]
async fn pretending_records_the_commands_and_changes_nothing() {
    let Some(t) = TestMongo::connect("pretending_records_the_commands_and_changes_nothing").await
    else {
        return;
    };
    let schema = t.db.schema().pretending();
    assert!(schema.is_pretend());
    schema
        .create("tickets", |t| {
            t.id();
            t.string("subject").unique();
            t.foreign_id("account_id").constrained();
        })
        .await
        .unwrap();
    assert!(!t.db.schema().has_table("tickets").await.unwrap());
    insta::assert_snapshot!(schema.statements().join("\n"));
    assert!(
        schema.notices().iter().any(|notice| notice.contains("no foreign keys")),
        "{:?}",
        schema.notices()
    );
    t.cleanup().await;
}

#[tokio::test]
async fn collections_are_renamed_listed_and_dropped() {
    let Some(t) = TestMongo::connect("collections_are_renamed_listed_and_dropped").await else {
        return;
    };
    let schema = t.db.schema();
    for name in ["deals", "notes"] {
        schema
            .create(name, |t| {
                t.object_id_primary();
            })
            .await
            .unwrap();
    }
    schema.rename("notes", "memos").await.unwrap();
    assert_eq!(schema.collections().await.unwrap(), ["deals", "memos"]);
    schema.drop("memos").await.unwrap();
    let error = schema.drop("memos").await.unwrap_err();
    assert!(error.to_string().contains("does not exist"), "{error}");
    schema.drop_if_exists("memos").await.unwrap();
    assert_eq!(schema.drop_all().await.unwrap(), ["deals"]);
    assert!(schema.collections().await.unwrap().is_empty());
    t.cleanup().await;
}
