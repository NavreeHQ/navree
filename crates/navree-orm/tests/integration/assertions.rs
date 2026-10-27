//! The testing assertions: rows by values, typed assignments and filters,
//! counts, records, soft deletes, query counts, and what they report when
//! they fail.

use std::panic::AssertUnwindSafe;

use futures::FutureExt;
use navree_orm::testing::{
    assert_database_count, assert_database_empty, assert_database_has, assert_database_missing,
    assert_model_exists, assert_model_missing, assert_not_soft_deleted, assert_not_soft_deleted_in,
    assert_soft_deleted, assert_soft_deleted_in, count_queries, expect_query_count, json_value,
};
use navree_orm::{Model, Value};

use crate::models::{Company, Deal, DealFactory, Stage, Tag};
use crate::support::sqlite;

/// The panic message of `future`, which must panic.
async fn panic_message(future: impl std::future::Future<Output = ()>) -> String {
    let payload = AssertUnwindSafe(future)
        .catch_unwind()
        .await
        .expect_err("the assertion should fail");
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
        .unwrap_or_default()
}

#[tokio::test]
async fn rows_match_by_values_typed_assignments_and_filters() {
    let test = sqlite().await;
    let db = &test.db;
    Deal::factory()
        .set(Deal::TITLE, "Renewal")
        .state(DealFactory::won)
        .create(db)
        .await
        .unwrap();
    assert_database_has(db, "deals", [("title", "Renewal")]).await;
    assert_database_has(
        db,
        "deals",
        vec![
            ("title", Value::from("Renewal")),
            ("stage", Value::from("won")),
        ],
    )
    .await;
    // Typed: encoded as the model stores them (the stage's cast).
    assert_database_has(
        db,
        Deal::table_name(),
        [Deal::TITLE.set("Renewal"), Deal::STAGE.set(Stage::Won)],
    )
    .await;
    assert_database_has(db, "deals", Deal::AMOUNT_CENTS.gte(1_000)).await;
    assert_database_missing(db, "deals", [("title", "Gone")]).await;
    assert_database_missing(db, "deals", [Deal::STAGE.set(Stage::Lost)]).await;
    assert_database_missing(db, "deals", Deal::AMOUNT_CENTS.gt(1_000_000)).await;
}

#[tokio::test]
async fn values_compare_as_columns_store_them() {
    let test = sqlite().await;
    let db = &test.db;
    Company::factory()
        .set(Company::VERIFIED, true)
        .set(
            Company::PROFILE,
            Some(serde_json::json!({"tier": "gold", "seats": 12})),
        )
        .set(Company::EMPLOYEES, 42)
        .create(db)
        .await
        .unwrap();
    // A boolean stored as an integer, a number of another width, JSON
    // compared structurally (key order and spacing do not matter).
    assert_database_has(db, "companies", [("verified", Value::from(true))]).await;
    assert_database_has(db, "companies", [("employees", Value::from(42_i64))]).await;
    assert_database_has(
        db,
        "companies",
        [(
            "profile",
            json_value(serde_json::json!({"seats": 12, "tier": "gold"})),
        )],
    )
    .await;
    assert_database_missing(
        db,
        "companies",
        [("profile", json_value(serde_json::json!({"tier": "gold"})))],
    )
    .await;
    assert_database_has(db, "companies", [("country", Value::String(None))]).await;
    assert_database_missing(db, "companies", [("verified", Value::from(false))]).await;
}

#[tokio::test]
async fn failures_report_the_closest_rows() {
    let test = sqlite().await;
    let db = &test.db;
    Company::factory()
        .set(Company::NAME, "Acme")
        .set(Company::COUNTRY, "NL")
        .create(db)
        .await
        .unwrap();
    Company::factory()
        .set(Company::NAME, "Globex")
        .set(Company::COUNTRY, "DE")
        .create(db)
        .await
        .unwrap();
    let message = panic_message(assert_database_has(
        db,
        "companies",
        [("name", "Acme"), ("country", "DE")],
    ))
    .await;
    assert!(
        message.starts_with(
            r#"assert_database_has: no row of `companies` matches name = "Acme", country = "DE"."#
        ),
        "{message}"
    );
    assert!(
        message.contains("`companies` has 2 row(s); the closest:"),
        "{message}"
    );
    assert!(
        message.contains(r#"(matches 1 of 2; differs on country = "NL")"#),
        "{message}"
    );

    let missing = panic_message(assert_database_missing(db, "companies", [("name", "Acme")])).await;
    assert!(
        missing.starts_with(
            r#"assert_database_missing: 1 row(s) of `companies` match name = "Acme":"#
        ),
        "{missing}"
    );

    let filtered = panic_message(assert_database_has(
        db,
        "companies",
        Company::EMPLOYEES.gt(100_000),
    ))
    .await;
    assert!(
        filtered.contains(r#"matches "companies"."employees" > $1"#),
        "{filtered}"
    );
    assert!(filtered.contains("the first:"), "{filtered}");

    let typo = panic_message(assert_database_has(db, "companies", [("nmae", "Acme")])).await;
    assert!(
        typo.starts_with("`companies` has no column `nmae` (it has: id, name,"),
        "{typo}"
    );

    let empty = panic_message(assert_database_has(db, "tags", [("name", "vip")])).await;
    assert!(empty.ends_with("`tags` is empty."), "{empty}");
}

#[tokio::test]
async fn counts_and_empty_tables() {
    let test = sqlite().await;
    let db = &test.db;
    assert_database_empty(db, "tags").await;
    Tag::factory().count(3).create(db).await.unwrap();
    assert_database_count(db, "tags", 3).await;
    let message = panic_message(assert_database_count(db, "tags", 2)).await;
    assert_eq!(
        message,
        "assert_database_count: `tags` has 3 row(s), expected 2"
    );
    let message = panic_message(assert_database_empty(db, "tags")).await;
    assert_eq!(
        message,
        "assert_database_empty: `tags` has 3 row(s), expected none"
    );
}

#[tokio::test]
async fn records_exist_go_missing_and_soft_delete() {
    let test = sqlite().await;
    let db = &test.db;
    let mut acme = Company::factory().create_one(db).await.unwrap();
    assert_model_exists(db, &acme).await;
    assert_not_soft_deleted(db, &acme).await;
    acme.delete(db).await.unwrap();
    assert_soft_deleted(db, &acme).await;
    assert_model_exists(db, &acme).await;
    let message = panic_message(assert_model_missing(db, &acme)).await;
    assert!(message.contains("soft-deleted rows stay"), "{message}");
    let message = panic_message(assert_not_soft_deleted(db, &acme)).await;
    assert_eq!(
        message,
        format!(
            "assert_not_soft_deleted: Company {} is soft-deleted",
            acme.id
        )
    );

    let mut tag = Tag::factory().create_one(db).await.unwrap();
    tag.delete(db).await.unwrap();
    assert_model_missing(db, &tag).await;
    let message = panic_message(assert_model_exists(db, &tag)).await;
    assert_eq!(
        message,
        format!("assert_model_exists: Tag {} is not in `tags`", tag.id)
    );
    let message = panic_message(assert_soft_deleted(db, &tag)).await;
    assert_eq!(
        message,
        "assert_soft_deleted: Tag does not soft-delete (no #[model(soft_deletes)])"
    );
}

#[tokio::test]
async fn soft_deletes_by_table_and_values() {
    let test = sqlite().await;
    let db = &test.db;
    let mut gone = Company::factory()
        .set(Company::NAME, "Gone")
        .create_one(db)
        .await
        .unwrap();
    gone.delete(db).await.unwrap();
    Company::factory()
        .set(Company::NAME, "Live")
        .create(db)
        .await
        .unwrap();
    assert_soft_deleted_in(db, "companies", "deleted_at", [("name", "Gone")]).await;
    assert_not_soft_deleted_in(db, "companies", "deleted_at", [("name", "Live")]).await;
    let message = panic_message(assert_soft_deleted_in(
        db,
        "companies",
        "deleted_at",
        [("name", "Live")],
    ))
    .await;
    assert!(
        message.starts_with(r#"assert_soft_deleted_in: no row of `companies` matching name = "Live" has `deleted_at` set."#),
        "{message}"
    );
}

#[tokio::test]
async fn query_counts() {
    let test = sqlite().await;
    let db = &test.db;
    let deal = Deal::factory()
        .without_parents()
        .create_one(db)
        .await
        .unwrap();
    let found = expect_query_count(2, async {
        let found = Deal::find(db, deal.id).await.unwrap();
        Deal::query().count(db).await.unwrap();
        found
    })
    .await;
    assert!(found.is_some());
    let ((), creates) = count_queries(async {
        Deal::factory()
            .count(3)
            .without_parents()
            .create(db)
            .await
            .unwrap();
    })
    .await;
    assert_eq!(creates, 3, "one insert per record");
    let message = panic_message(async {
        expect_query_count(5, async {
            Deal::query().get(db).await.unwrap();
        })
        .await;
    })
    .await;
    assert_eq!(
        message,
        "expect_query_count: 1 statement(s) ran, expected 5"
    );
    assert_eq!(<Deal as Model>::table_name(), "deals");
}
