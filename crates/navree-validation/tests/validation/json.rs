//! `Valid<Json<T>>`: malformed bodies, serde errors and garde failures all
//! become 400 errors with field-level messages.

use serde_json::json;

use crate::support::{app, errors};

#[tokio::test]
async fn field_errors_use_field_messages() {
    let app = app().await;

    let response = app
        .client()
        .post("/v/users")
        .json(&json!({ "name": "A", "email": "nope", "age": 10, "tags": [] }))
        .send()
        .await
        .assert_error(400, None);

    assert_eq!(
        errors(&response.json_value()),
        &json!({
            "name": ["name must be longer than or equal to 2 characters"],
            "email": ["email must be a valid email address"],
            "age": ["age must not be less than 18"],
            "tags": ["tags must contain at least 1 elements"],
        })
    );
}

#[tokio::test]
async fn valid_input_reaches_the_handler() {
    let app = app().await;

    app.client()
        .post("/v/users")
        .json(&json!({ "name": "Ada", "email": "ada@example.com", "age": 36, "tags": ["x"] }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("ok.name", json!("Ada"));
}

#[tokio::test]
async fn invalid_json_is_400_with_body_error() {
    let app = app().await;

    let response = app
        .client()
        .post("/v/users")
        .header("content-type", "application/json")
        .body(r#"{"name": "#)
        .send()
        .await
        .assert_error(400, None);

    assert_eq!(
        errors(&response.json_value()),
        &json!({ "body": ["body must be a valid JSON string"] })
    );
}

#[tokio::test]
async fn nested_field_paths() {
    let app = app().await;

    let response = app
        .client()
        .post("/v/orders")
        .json(&json!({
            "customer": { "address": { "city": "" } },
            "items": [{ "sku": "A", "quantity": 1 }, { "sku": "", "quantity": 0 }],
        }))
        .send()
        .await
        .assert_error(400, None);
    assert_eq!(
        errors(&response.json_value()),
        &json!({
            "customer.address.city": ["customer.address.city must be longer than or equal to 1 characters"],
            "items.1.sku": ["items.1.sku must be longer than or equal to 1 characters"],
            "items.1.quantity": ["items.1.quantity must not be less than 1"],
        })
    );

    // serde reports the path of a nested type error too.
    let response = app
        .client()
        .post("/v/orders")
        .json(&json!({
            "customer": { "address": { "city": "Oslo" } },
            "items": [{ "sku": "A", "quantity": "two" }],
        }))
        .send()
        .await
        .assert_error(400, None);
    assert_eq!(
        errors(&response.json_value()),
        &json!({ "items.0.quantity": ["items.0.quantity must be an integer number"] })
    );
}

/// A valid `Typed` body, with `field` replaced (or removed when `None`).
fn typed(field: &str, value: Option<serde_json::Value>) -> serde_json::Value {
    let mut body = json!({
        "title": "Deal",
        "count": 3,
        "ratio": 0.5,
        "active": true,
        "labels": ["a"],
        "owner_id": null,
        "stage": "lead",
    });
    match value {
        Some(value) => body[field] = value,
        None => {
            body.as_object_mut().unwrap().remove(field);
        }
    }
    body
}

#[tokio::test]
async fn deserialization_errors_name_the_field() {
    let app = app().await;
    let cases = [
        ("title", None, "title should not be empty"),
        ("title", Some(json!(null)), "title should not be empty"),
        ("title", Some(json!(5)), "title must be a string"),
        ("count", Some(json!("x")), "count must be an integer number"),
        ("count", Some(json!(1.5)), "count must be an integer number"),
        ("ratio", Some(json!("x")), "ratio must be a number"),
        (
            "active",
            Some(json!("yes")),
            "active must be a boolean value",
        ),
        ("labels", Some(json!("a")), "labels must be an array"),
        ("owner_id", Some(json!("nope")), "owner_id must be a UUID"),
        (
            "stage",
            Some(json!("closed")),
            "stage must be one of the following values: lead, won",
        ),
        ("extra", Some(json!(1)), "property extra should not exist"),
    ];
    for (field, value, message) in cases {
        let response = app
            .client()
            .post("/v/typed")
            .json(&typed(field, value.clone()))
            .send()
            .await
            .assert_error(400, None);
        assert_eq!(
            errors(&response.json_value()),
            &json!({ field: [message] }),
            "{field} = {value:?}"
        );
        assert_eq!(response.json_value()["message"], json!([message]));
    }

    app.client()
        .post("/v/typed")
        .json(&typed("owner_id", Some(json!(uuid::Uuid::now_v7()))))
        .send()
        .await
        .assert_status(200);
}

#[tokio::test]
async fn non_json_content_type_is_415() {
    let app = app().await;

    app.client()
        .post("/v/users")
        .header("content-type", "text/plain")
        .body("name=x")
        .send()
        .await
        .assert_error(
            415,
            "Expected request with `Content-Type: application/json`",
        );
}
