//! `ValidAsync` and `AsyncRule`: `unique` and `exists` run inside the
//! request's unit of work, after the validation rules pass.

use navree_db::{Db, Executor, Statement};
use navree_validation::{AsyncRule, RuleError};
use serde_json::json;
use uuid::Uuid;

use crate::databases::Scratch;
use crate::on_every_server;
use crate::support::{NotReserved, database_app, errors, insert};

async fn unique_rule_rejects_existing_and_ignores_self(scratch: Scratch) {
    let app = database_app(&scratch).await;
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
    assert!(!taken(None).passes(&*db).await.unwrap());
    assert!(taken(Some(alice)).passes(&*db).await.unwrap());
    assert!(!taken(Some(bob)).passes(&*db).await.unwrap());
}

async fn exists_rule(scratch: Scratch) {
    let app = database_app(&scratch).await;
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

/// A company deleted by marking `deleted_at` is not there for `exists`, and
/// holds no name for `unique`, once the rule says so.
async fn table_rules_can_leave_soft_deleted_rows_out(scratch: Scratch) {
    let app = database_app(&scratch).await;
    let db = app.get::<Db>().await;
    let (live, gone) = (Uuid::now_v7(), Uuid::now_v7());
    insert(&app, "companies", live, "Acme").await;
    insert(&app, "companies", gone, "Bharat Traders").await;
    db.execute(&Statement::portable(
        "UPDATE companies SET deleted_at = ? WHERE id = ?",
        ["2026-11-06T10:00:00Z".into(), gone.into()],
    ))
    .await
    .expect("soft-delete the company");

    let exists = |id: Uuid| AsyncRule::exists("company_id", "companies", "id", id);
    assert!(
        exists(gone).passes(&*db).await.unwrap(),
        "a plain rule still finds the row"
    );
    assert!(
        !exists(gone)
            .without_deleted("deleted_at")
            .passes(&*db)
            .await
            .unwrap()
    );
    assert!(
        exists(live)
            .without_deleted("deleted_at")
            .passes(&*db)
            .await
            .unwrap()
    );

    let free = |name: &str| AsyncRule::unique("name", "companies", "name", name.to_owned(), None);
    assert!(!free("Bharat Traders").passes(&*db).await.unwrap());
    assert!(
        free("Bharat Traders")
            .without_deleted("deleted_at")
            .passes(&*db)
            .await
            .unwrap(),
        "a deleted company holds no name"
    );
    assert!(
        !free("Acme")
            .without_deleted("deleted_at")
            .passes(&*db)
            .await
            .unwrap()
    );

    let error = exists(live)
        .without_deleted("deleted_at IS NULL OR 1")
        .passes(&*db)
        .await
        .unwrap_err();
    assert!(
        matches!(error, RuleError::InvalidIdentifier { .. }),
        "{error}"
    );
}

async fn validation_rules_run_before_async_rules(scratch: Scratch) {
    let app = database_app(&scratch).await;

    // `title` fails its rules, so the `exists` query never runs.
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

async fn rejects_bad_identifier_in_rule(scratch: Scratch) {
    let app = database_app(&scratch).await;
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
            .passes(&*db)
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
    db.fetch_all(&Statement::text("SELECT count(*) FROM companies"))
        .await
        .expect("the companies table still exists");
}

async fn async_rules_on_query_and_form(scratch: Scratch) {
    let app = database_app(&scratch).await;
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

async fn check_rules_run_in_the_unit_of_work_with_the_path_parameter(scratch: Scratch) {
    let app = database_app(&scratch).await;

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
    let error = rule().passes(&*db).await.unwrap_err();
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

// MOD-005: `check_async_rules` runs what `ValidAsync` runs, in the request's unit of
// work, and hands the failures back instead of answering: a taken address, a missing
// company, the route's `{id}` ignored, a null skipped.
async fn check_async_rules_returns_what_valid_async_refuses(scratch: Scratch) {
    let app = database_app(&scratch).await;
    let (alice, bob) = (Uuid::now_v7(), Uuid::now_v7());
    insert(&app, "users", alice, "alice@example.com").await;
    insert(&app, "users", bob, "bob@example.com").await;
    let acme = Uuid::now_v7();
    insert(&app, "companies", acme, "Acme").await;

    let failed = |response: navree_testing::TestResponse| response.assert_status(200).json_value();
    let taken = app
        .client()
        .post("/db/checked/users")
        .json(&json!({ "email": "alice@example.com" }))
        .send()
        .await;
    assert_eq!(failed(taken), json!({ "failed": [["email", "unique"]] }));
    let free = app
        .client()
        .post("/db/checked/users")
        .json(&json!({ "email": "carol@example.com" }))
        .send()
        .await;
    assert_eq!(failed(free), json!({ "failed": [] }));

    // The route's `{id}` is the record itself: alice keeps her address, bob may not take it.
    let own = app
        .client()
        .patch(&format!("/db/checked/users/{alice}"))
        .json(&json!({ "email": "alice@example.com" }))
        .send()
        .await;
    assert_eq!(failed(own), json!({ "failed": [] }));
    let other = app
        .client()
        .patch(&format!("/db/checked/users/{bob}"))
        .json(&json!({ "email": "alice@example.com" }))
        .send()
        .await;
    assert_eq!(failed(other), json!({ "failed": [["email", "unique"]] }));

    let missing = app
        .client()
        .post("/db/checked/deals")
        .json(&json!({ "title": "Big deal", "company_id": Uuid::now_v7() }))
        .send()
        .await;
    assert_eq!(
        failed(missing),
        json!({ "failed": [["company_id", "exists"]] })
    );
    for company in [json!(acme), json!(null)] {
        let found = app
            .client()
            .post("/db/checked/deals")
            .json(&json!({ "title": "Big deal", "company_id": company }))
            .send()
            .await;
        assert_eq!(failed(found), json!({ "failed": [] }));
    }

    // And `ValidAsync` still answers as it did, on the same rules.
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
}

// MOD-005: a rule that can't run is an error, not a failed field: no unit of work is a
// 500, and a rule with nothing to check needs none.
#[tokio::test]
async fn check_async_rules_without_a_unit_of_work_is_a_server_error() {
    let app = crate::support::app_with(crate::support::DatabaseTestModule).await;
    app.client()
        .post("/db/checked/deals")
        .json(&json!({ "title": "x", "company_id": Uuid::now_v7() }))
        .send()
        .await
        .assert_error(500, "Internal server error");
    let nothing = app
        .client()
        .post("/db/checked/deals")
        .json(&json!({ "title": "x", "company_id": null }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(nothing, json!({ "failed": [] }));
}

on_every_server!(
    "navree_validation";
    unique_rule_rejects_existing_and_ignores_self,
    exists_rule,
    table_rules_can_leave_soft_deleted_rows_out,
    validation_rules_run_before_async_rules,
    rejects_bad_identifier_in_rule,
    async_rules_on_query_and_form,
    check_rules_run_in_the_unit_of_work_with_the_path_parameter,
    check_async_rules_returns_what_valid_async_refuses,
);
