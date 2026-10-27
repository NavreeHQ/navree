//! Bulk writes on a real server: inserts that skip duplicates, upserts by
//! unique fields, and what a transaction allows of them.

use navree_mongo::MongoDb;
use navree_orm::{Expr, Field, OrmError, Value, col, table};

use crate::support::TestMongo;

fn row(code: &str, rate: f64) -> Vec<(&'static str, Value)> {
    vec![("code", Value::from(code)), ("rate", Value::from(rate))]
}

async fn rate(mongo: &MongoDb, code: &str) -> Option<f64> {
    table("rates")
        .where_(col("code").eq(code))
        .value(mongo, "rate")
        .await
        .unwrap()
        .map(|value| f64::from_value(value).unwrap())
}

async fn rates_collection(t: &TestMongo) {
    t.db.schema()
        .create("rates", |t| {
            t.object_id_primary();
            t.string("code").unique();
            t.double("rate");
            t.string("source").nullable();
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn inserts_skip_what_a_unique_index_refuses() {
    let Some(t) = TestMongo::connect("inserts_skip_what_a_unique_index_refuses").await else {
        return;
    };
    rates_collection(&t).await;
    let mongo = &t.db;
    let inserted = table("rates")
        .insert_or_ignore(mongo, [row("EUR", 1.08), row("NOK", 0.09)])
        .await
        .unwrap();
    assert_eq!(inserted, 2);
    let inserted = table("rates")
        .insert_or_ignore(mongo, [row("EUR", 9.9), row("SEK", 0.087), row("NOK", 9.9)])
        .await
        .unwrap();
    assert_eq!(inserted, 1, "the duplicates were skipped, the rest went in");
    assert_eq!(rate(mongo, "EUR").await, Some(1.08));
    assert_eq!(rate(mongo, "SEK").await, Some(0.087));
    assert_eq!(table("rates").count(mongo).await.unwrap(), 3);

    // Any other refusal is an error: here the validator's.
    let error = table("rates")
        .insert_or_ignore(
            mongo,
            [vec![
                ("code", Value::from("DKK")),
                ("rate", Value::from("high")),
            ]],
        )
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("Document failed validation"),
        "{error}"
    );

    // Inside a transaction MongoDB aborts on any write error.
    let uow = mongo.begin().await.unwrap();
    let error = table("rates")
        .insert_or_ignore(&uow, [row("EUR", 1.0)])
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("inside a transaction"),
        "{error}"
    );
    uow.rollback().await.unwrap();
    t.cleanup().await;
}

#[tokio::test]
async fn upserts_update_by_unique_fields_or_insert() {
    let Some(t) = TestMongo::connect("upserts_update_by_unique_fields_or_insert").await else {
        return;
    };
    rates_collection(&t).await;
    let mongo = &t.db;
    table("rates")
        .insert_or_ignore(mongo, [row("EUR", 1.08)])
        .await
        .unwrap();
    let with_source = |code: &str, rate: f64| {
        vec![
            ("code", Value::from(code)),
            ("rate", Value::from(rate)),
            ("source", Value::from("ecb")),
        ]
    };
    let written = table("rates")
        .upsert(
            mongo,
            [with_source("EUR", 1.09), with_source("DKK", 0.13)],
            ["code"],
            ["rate"],
        )
        .await
        .unwrap();
    assert_eq!(written, 2, "one matched, one inserted");
    assert_eq!(rate(mongo, "EUR").await, Some(1.09));
    assert_eq!(rate(mongo, "DKK").await, Some(0.13));
    // Selected fields a document lacks read as NULL.
    let sources: Vec<(Value, Value)> = table("rates")
        .select(["code", "source"])
        .order_by("code")
        .get(mongo)
        .await
        .unwrap()
        .iter()
        .map(|row| {
            (
                row.get::<Value>("code").unwrap(),
                row.get::<Value>("source").unwrap(),
            )
        })
        .collect();
    assert_eq!(
        sources,
        [
            (Value::from("DKK"), Value::from("ecb")),
            (Value::from("EUR"), Value::String(None)),
        ],
        "fields not updated are set on insert only"
    );

    // With nothing to update, inside a transaction: only what is missing
    // goes in, and nothing aborts it.
    let uow = mongo.begin().await.unwrap();
    let written = table("rates")
        .upsert(
            &uow,
            [row("EUR", 5.0), row("CHF", 1.1)],
            ["code"],
            Vec::<&str>::new(),
        )
        .await
        .unwrap();
    assert_eq!(written, 2);
    uow.commit().await.unwrap();
    assert_eq!(rate(mongo, "EUR").await, Some(1.09), "kept as it was");
    assert_eq!(rate(mongo, "CHF").await, Some(1.1));

    // An update computed from the stored document is SQL's alone.
    let error = table("rates")
        .upsert(
            mongo,
            [row("EUR", 0.01)],
            ["code"],
            [col("rate").set_expr(Expr::col("rates.rate").add(Expr::excluded("rate")))],
        )
        .await
        .unwrap_err();
    assert!(matches!(error, OrmError::Unsupported { .. }), "{error:?}");
    assert_eq!(rate(mongo, "EUR").await, Some(1.09));
    t.cleanup().await;
}
