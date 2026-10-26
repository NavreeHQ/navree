//! `Valid<Query<T>>` and `Valid<Form<T>>`: string values parsed by serde,
//! then garde rules, all as 400 field errors.

use serde_json::json;

use crate::support::{app, errors};

#[tokio::test]
async fn query_errors_are_400_with_field_errors() {
    let app = app().await;

    let response = app
        .client()
        .get("/v/search?per_page=abc")
        .send()
        .await
        .assert_error(400, None);
    assert_eq!(
        errors(&response.json_value()),
        &json!({ "per_page": ["per_page must be an integer number"] })
    );

    let response = app
        .client()
        .get("/v/search")
        .query(&[("q", "a"), ("per_page", "500")])
        .send()
        .await
        .assert_error(400, None);
    assert_eq!(
        errors(&response.json_value()),
        &json!({
            "q": ["q must be longer than or equal to 2 characters"],
            "per_page": ["per_page must not be greater than 100"],
        })
    );

    app.client()
        .get("/v/search?q=acme&per_page=20")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("ok.per_page", json!(20));
    app.client()
        .get("/v/search")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("ok.q", json!(null));
}

#[tokio::test]
async fn form_body_is_validated() {
    let app = app().await;

    let response = app
        .client()
        .post("/v/subscribe")
        .form(&[("email", "nope")])
        .send()
        .await
        .assert_error(400, None);
    assert_eq!(
        errors(&response.json_value()),
        &json!({ "email": ["email must be a valid email address"] })
    );

    let response = app
        .client()
        .post("/v/subscribe")
        .form(&[("email", "ada@example.com"), ("newsletter", "maybe")])
        .send()
        .await
        .assert_error(400, None);
    assert_eq!(
        errors(&response.json_value()),
        &json!({ "newsletter": ["newsletter must be a boolean value"] })
    );

    app.client()
        .post("/v/subscribe")
        .form(&[("email", "ada@example.com"), ("newsletter", "true")])
        .send()
        .await
        .assert_status(200)
        .assert_json_path("ok.newsletter", json!(true));

    app.client()
        .post("/v/subscribe")
        .json(&json!({ "email": "ada@example.com" }))
        .send()
        .await
        .assert_error(
            415,
            "Expected request with `Content-Type: application/x-www-form-urlencoded`",
        );
}
