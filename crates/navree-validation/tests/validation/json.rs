//! `Valid<Json<T>>`: malformed bodies, serde errors and garde failures all
//! become 422 problems with Laravel-style field errors.

use serde_json::json;

use crate::support::{app, errors};

#[tokio::test]
async fn field_errors_use_laravel_messages() {
    let app = app().await;

    let response = app
        .client()
        .post("/v/users")
        .json(&json!({ "name": "A", "email": "nope", "age": 10, "tags": [] }))
        .send()
        .await
        .assert_problem(422, "validation_failed");

    assert_eq!(
        errors(&response.json_value()),
        &json!({
            "name": ["The name field must be at least 2 characters."],
            "email": ["The email field must be a valid email address."],
            "age": ["The age field must be at least 18."],
            "tags": ["The tags field must have at least 1 items."],
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
async fn invalid_json_is_422_with_body_error() {
    let app = app().await;

    let response = app
        .client()
        .post("/v/users")
        .header("content-type", "application/json")
        .body(r#"{"name": "#)
        .send()
        .await
        .assert_problem(422, "validation_failed");

    assert_eq!(
        errors(&response.json_value()),
        &json!({ "body": ["The body field must be a valid JSON string."] })
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
        .assert_problem(422, "validation_failed");
    assert_eq!(
        errors(&response.json_value()),
        &json!({
            "customer.address.city": ["The customer.address.city field must be at least 1 characters."],
            "items.1.sku": ["The items.1.sku field must be at least 1 characters."],
            "items.1.quantity": ["The items.1.quantity field must be at least 1."],
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
        .assert_problem(422, "validation_failed");
    assert_eq!(
        errors(&response.json_value()),
        &json!({ "items.0.quantity": ["The items.0.quantity field must be an integer."] })
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
        ("title", None, "The title field is required."),
        ("title", Some(json!(null)), "The title field is required."),
        ("title", Some(json!(5)), "The title field must be a string."),
        (
            "count",
            Some(json!("x")),
            "The count field must be an integer.",
        ),
        (
            "count",
            Some(json!(1.5)),
            "The count field must be an integer.",
        ),
        (
            "ratio",
            Some(json!("x")),
            "The ratio field must be a number.",
        ),
        (
            "active",
            Some(json!("yes")),
            "The active field must be true or false.",
        ),
        (
            "labels",
            Some(json!("a")),
            "The labels field must be an array.",
        ),
        (
            "owner_id",
            Some(json!("nope")),
            "The owner id field must be a valid UUID.",
        ),
        (
            "stage",
            Some(json!("closed")),
            "The selected stage is invalid.",
        ),
        ("extra", Some(json!(1)), "The extra field is prohibited."),
    ];
    for (field, value, message) in cases {
        let response = app
            .client()
            .post("/v/typed")
            .json(&typed(field, value.clone()))
            .send()
            .await
            .assert_problem(422, "validation_failed");
        assert_eq!(
            errors(&response.json_value()),
            &json!({ field: [message] }),
            "{field} = {value:?}"
        );
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
        .assert_problem(415, "unsupported_media_type");
}
