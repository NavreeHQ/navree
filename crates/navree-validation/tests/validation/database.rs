//! `ValidAsync` and `AsyncRule`: `unique` and `exists` run inside the
//! request's unit of work, after the garde rules pass.

use navree_db::Db;
use navree_validation::{AsyncRule, RuleError};
use sea_orm::ConnectionTrait;
use serde_json::json;
use uuid::Uuid;

use crate::support::{NotReserved, database_app, errors, insert};

#[tokio::test]
async fn unique_rule_rejects_existing_and_ignores_self() {
    let app = database_app().await;
    let (alice, bob) = (Uuid::now_v7(), Uuid::now_v7());
    insert(&app, "users", alice, "alice@example.com").await;
    insert(&app, "users", bob, "bob@example.com").await;

    let response = app
        .client()
        .post("/db/users")
        .json(&json!({ "email": "alice@example.com" }))
        .send()
        .await
        .assert_error(400, None);
    assert_eq!(
        errors(&response.json_value()),
        &json!({ "email": ["email has already been taken"] })
    );
    app.client()
        .post("/db/users")
        .json(&json!({ "email": "carol@example.com" }))
        .send()
        .await
        .assert_status(200);

    // Updating alice keeps her own address; bob may not take it.
    app.client()
        .patch(&format!("/db/users/{alice}"))
        .json(&json!({ "email": "alice@example.com" }))
        .send()
        .await
        .assert_status(200);
    app.client()
        .patch(&format!("/db/users/{bob}"))
        .json(&json!({ "email": "alice@example.com" }))
        .send()
        .await
        .assert_error(400, None);

    // The same through the API, with an explicit id to ignore.
    let db = app.get::<Db>().await;
    let taken = |ignore| AsyncRule::unique("email", "users", "email", "alice@example.com", ignore);
    assert!(!taken(None).passes(db.conn()).await.unwrap());
    assert!(taken(Some(alice)).passes(db.conn()).await.unwrap());
    assert!(!taken(Some(bob)).passes(db.conn()).await.unwrap());
}

#[tokio::test]
async fn exists_rule() {
    let app = database_app().await;
    let acme = Uuid::now_v7();
    insert(&app, "companies", acme, "Acme").await;

    app.client()
        .post("/db/deals")
        .json(&json!({ "title": "Big deal", "company_id": acme }))
        .send()
        .await
        .assert_status(200);
    // A null value skips the rule (make the field required to demand one).
    app.client()
        .post("/db/deals")
        .json(&json!({ "title": "Big deal", "company_id": null }))
        .send()
        .await
        .assert_status(200);

    let response = app
        .client()
        .post("/db/deals")
        .json(&json!({ "title": "Big deal", "company_id": Uuid::now_v7() }))
        .send()
        .await
        .assert_error(400, None);
    assert_eq!(
        errors(&response.json_value()),
        &json!({ "company_id": ["company_id does not exist"] })
    );
}

#[tokio::test]
async fn garde_rules_run_before_async_rules() {
    let app = database_app().await;

    // `title` fails garde, so the `exists` query never runs.
    let response = app
        .client()
        .post("/db/deals")
        .json(&json!({ "title": "", "company_id": Uuid::now_v7() }))
        .send()
        .await
        .assert_error(400, None);
    assert_eq!(
        errors(&response.json_value()),
        &json!({ "title": ["title must be longer than or equal to 1 characters"] })
    );
}

#[tokio::test]
async fn rejects_bad_identifier_in_rule() {
    let app = database_app().await;
    let db = app.get::<Db>().await;

    for (table, column) in [
        ("companies; DROP TABLE companies", "name"),
        ("companies", "name = name OR 1"),
        ("Companies", "name"),
        ("companies", ""),
        ("1companies", "name"),
        ("public.companies", "name"),
    ] {
        let error = AsyncRule::exists("name", table, column, "Acme")
            .passes(db.conn())
            .await
            .unwrap_err();
        assert!(
            matches!(error, RuleError::InvalidIdentifier { .. }),
            "{table}.{column}: {error}"
        );
    }

    // Through HTTP it is a server error, and no SQL ran.
    app.client()
        .post("/db/sneaky")
        .json(&json!({ "name": "Acme" }))
        .send()
        .await
        .assert_error(500, "Internal server error");
    db.conn()
        .execute_unprepared("SELECT count(*) FROM companies")
        .await
        .expect("the companies table still exists");
}

#[tokio::test]
async fn async_rules_on_query_and_form() {
    let app = database_app().await;
    let acme = Uuid::now_v7();
    insert(&app, "companies", acme, "Acme").await;
    let missing = Uuid::now_v7().to_string();

    app.client()
        .get("/db/deals")
        .query(&[("title", "x"), ("company_id", &acme.to_string())])
        .send()
        .await
        .assert_status(200);
    let response = app
        .client()
        .get("/db/deals")
        .query(&[("title", "x"), ("company_id", &missing)])
        .send()
        .await
        .assert_error(400, None);
    assert_eq!(
        errors(&response.json_value()),
        &json!({ "company_id": ["company_id does not exist"] })
    );

    app.client()
        .post("/db/deals-form")
        .form(&[("title", "x"), ("company_id", &acme.to_string())])
        .send()
        .await
        .assert_status(200);
    app.client()
        .post("/db/deals-form")
        .form(&[("title", "x"), ("company_id", &missing)])
        .send()
        .await
        .assert_error(400, None);
}

#[tokio::test]
async fn async_rules_without_database_module_are_a_server_error() {
    let app = crate::support::app_with(crate::support::DatabaseTestModule).await;

    app.client()
        .post("/db/deals")
        .json(&json!({ "title": "x", "company_id": Uuid::now_v7() }))
        .send()
        .await
        .assert_error(500, "Internal server error");
    // No rule to run, no unit of work needed.
    app.client()
        .post("/db/deals")
        .json(&json!({ "title": "x", "company_id": null }))
        .send()
        .await
        .assert_status(200);
}

#[tokio::test]
async fn check_rules_run_in_the_unit_of_work_with_the_path_parameter() {
    let app = database_app().await;

    let response = app
        .client()
        .post("/db/handles")
        .json(&json!({ "code": "admin" }))
        .send()
        .await
        .assert_error(400, None);
    assert_eq!(
        errors(&response.json_value()),
        &json!({ "code": ["code has already been taken"] })
    );
    for code in [json!("bob"), json!(null)] {
        app.client()
            .post("/db/handles")
            .json(&json!({ "code": code }))
            .send()
            .await
            .assert_status(200);
    }
    // The route's `{code}` reaches the check: the record keeps its own code.
    app.client()
        .patch("/db/handles/admin")
        .json(&json!({ "code": "admin" }))
        .send()
        .await
        .assert_status(200);
    app.client()
        .patch("/db/handles/bob")
        .json(&json!({ "code": "admin" }))
        .send()
        .await
        .assert_error(400, None);

    // Outside a request: in a unit of work, not on a bare connection.
    let db = app.get::<Db>().await;
    let rule = || AsyncRule::check("code", "unique", NotReserved("admin".into()));
    let error = rule().passes(db.conn()).await.unwrap_err();
    assert!(
        matches!(error, RuleError::NeedsUnitOfWork { rule: "unique", .. }),
        "{error}"
    );
    let uow = db.uow(None, None).await;
    assert!(!rule().passes_in(&uow).await.unwrap());
    assert!(
        rule().skipped_when(true).passes_in(&uow).await.unwrap(),
        "a null value passes"
    );
    let plain = AsyncRule::exists("name", "companies", "name", "Acme");
    assert!(!plain.passes_in(&uow).await.unwrap());
}
