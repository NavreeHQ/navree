//! `ValidationModule`: message and attribute overrides from options or
//! from the `validation` configuration section.

use std::collections::BTreeMap;

use keel_testing::TestApp;
use keel_validation::{Messages, ValidationModule, ValidationOptions};
use serde_json::{Value, json};

use crate::support::{ValidationTestModule, errors};

fn bad_user() -> Value {
    json!({ "name": "A", "email": "nope", "age": 30, "tags": [] })
}

#[tokio::test]
async fn messages_are_overridable_via_options() {
    let options = ValidationOptions {
        messages: BTreeMap::from([
            ("email".to_owned(), "Bad :attribute!".to_owned()),
            (
                "name.min.string".to_owned(),
                "Name too short (:min).".to_owned(),
            ),
        ]),
        attributes: BTreeMap::from([("tags".to_owned(), "labels".to_owned())]),
    };
    let app = TestApp::builder()
        .module(ValidationModule::with_options(options))
        .module(ValidationTestModule)
        .build()
        .await;

    let response = app
        .client()
        .post("/v/users")
        .json(&bad_user())
        .send()
        .await
        .assert_problem(422, "validation_failed");
    assert_eq!(
        errors(&response.json_value()),
        &json!({
            "name": ["Name too short (2)."],
            "email": ["Bad email!"],
            "tags": ["The labels field must have at least 1 items."],
        })
    );
}

#[tokio::test]
async fn messages_come_from_the_validation_config_section() {
    let app = TestApp::builder()
        .config(
            "validation",
            json!({
                // Nested tables flatten to dotted keys (`min.string`).
                "messages": { "min": { "string": ":Attribute: :min+ chars." } },
                "attributes": { "email": "e-mail address" },
            }),
        )
        .module(ValidationModule)
        .module(ValidationTestModule)
        .build()
        .await;

    let response = app
        .client()
        .post("/v/users")
        .json(&bad_user())
        .send()
        .await
        .assert_problem(422, "validation_failed");
    assert_eq!(
        errors(&response.json_value()),
        &json!({
            "name": ["Name: 2+ chars."],
            "email": ["The e-mail address field must be a valid email address."],
            "tags": ["The tags field must have at least 1 items."],
        })
    );
    assert_eq!(
        app.get::<Messages>().await.attribute("email"),
        "e-mail address"
    );
}

#[tokio::test]
async fn for_root_uses_the_built_in_messages() {
    let app = TestApp::builder()
        .config("validation.messages.email", "ignored by for_root")
        .module(ValidationModule::for_root())
        .module(ValidationTestModule)
        .build()
        .await;

    let response = app
        .client()
        .post("/v/users")
        .json(&bad_user())
        .send()
        .await
        .assert_problem(422, "validation_failed");
    assert_eq!(
        errors(&response.json_value())["email"],
        json!(["The email field must be a valid email address."])
    );
    assert_eq!(*app.get::<Messages>().await, Messages::default());
}

#[tokio::test]
async fn the_problem_detail_summarises_like_laravel() {
    let app = TestApp::builder()
        .module(ValidationTestModule)
        .build()
        .await;

    app.client()
        .post("/v/users")
        .json(&bad_user())
        .send()
        .await
        .assert_problem(422, "validation_failed")
        .assert_json_path(
            "detail",
            json!("The email field must be a valid email address. (and 2 more errors)"),
        );
}
