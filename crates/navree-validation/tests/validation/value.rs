//! `validate_value`: the `Valid<Json<T>>` checks for JSON that does not
//! arrive as a request body (MCP tool arguments, agent tool calls).

use std::collections::BTreeMap;

use navree_http::AppError;
use navree_kernel::AppContext;
use navree_testing::TestApp;
use navree_validation::{ValidationModule, ValidationOptions, validate_value};
use serde_json::{Value, json};

use crate::support::{CreateUser, errors};

fn body(error: &AppError) -> Value {
    error.body()
}

#[tokio::test]
async fn valid_input_deserializes() {
    let user: CreateUser = validate_value(
        json!({ "name": "Ada", "email": "ada@example.com", "age": 36, "tags": ["math"] }),
        None,
    )
    .await
    .expect("valid");
    assert_eq!(user.name, "Ada");
}

#[tokio::test]
async fn rule_failures_are_400s_with_field_errors() {
    let error = validate_value::<CreateUser>(
        json!({ "name": "A", "email": "nope", "age": 30, "tags": [] }),
        None,
    )
    .await
    .map(drop)
    .expect_err("invalid");
    assert_eq!(error.status, 400);
    assert_eq!(
        errors(&body(&error)),
        &json!({
            "name": ["name must be longer than or equal to 2 characters"],
            "email": ["email must be a valid email address"],
            "tags": ["tags must contain at least 1 elements"],
        })
    );
}

#[tokio::test]
async fn serde_failures_name_the_field() {
    let error = validate_value::<CreateUser>(json!({ "email": "ada@example.com" }), None)
        .await
        .map(drop)
        .expect_err("missing name");
    assert_eq!(error.status, 400);
    assert_eq!(
        errors(&body(&error)),
        &json!({ "name": ["name should not be empty"] })
    );
}

#[tokio::test]
async fn the_apps_messages_apply() {
    let options = ValidationOptions {
        messages: BTreeMap::from([("email".to_owned(), "Bad :attribute!".to_owned())]),
        ..ValidationOptions::default()
    };
    let app = TestApp::builder()
        .module(ValidationModule::with_options(options))
        .build()
        .await;
    let ctx: AppContext = app.ctx().clone();
    let error = validate_value::<CreateUser>(
        json!({ "name": "Ada", "email": "nope", "age": 30, "tags": ["x"] }),
        Some(&ctx),
    )
    .await
    .map(drop)
    .expect_err("invalid email");
    assert_eq!(errors(&body(&error)), &json!({ "email": ["Bad email!"] }));
}
