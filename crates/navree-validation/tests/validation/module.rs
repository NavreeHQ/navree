//! `ValidationModule`: message and attribute overrides from options or
//! from the `validation` configuration section.

use std::collections::BTreeMap;
use std::sync::Arc;

use navree_testing::TestApp;
use navree_validation::{MessageTranslator, Messages, ValidationModule, ValidationOptions};
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
        ..ValidationOptions::default()
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
        .assert_error(400, None);
    assert_eq!(
        errors(&response.json_value()),
        &json!({
            "name": ["Name too short (2)."],
            "email": ["Bad email!"],
            "tags": ["labels must contain at least 1 elements"],
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
        .assert_error(400, None);
    assert_eq!(
        errors(&response.json_value()),
        &json!({
            "name": ["Name: 2+ chars."],
            "email": ["e-mail address must be a valid email address"],
            "tags": ["tags must contain at least 1 elements"],
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
        .assert_error(400, None);
    assert_eq!(
        errors(&response.json_value())["email"],
        json!(["email must be a valid email address"])
    );
    assert_eq!(*app.get::<Messages>().await, Messages::default());
}

#[tokio::test]
async fn message_lists_every_error_in_field_order() {
    let app = TestApp::builder()
        .module(ValidationTestModule)
        .build()
        .await;

    let response = app
        .client()
        .post("/v/users")
        .json(&bad_user())
        .send()
        .await
        .assert_validation_error("email")
        .assert_validation_error("name")
        .assert_validation_error("tags");
    assert_eq!(
        response.json_value(),
        json!({
            "statusCode": 400,
            "message": [
                "email must be a valid email address",
                "name must be longer than or equal to 2 characters",
                "tags must contain at least 1 elements",
            ],
            "error": "Bad Request",
            "code": "validation_failed",
            "errors": {
                "email": ["email must be a valid email address"],
                "name": ["name must be longer than or equal to 2 characters"],
                "tags": ["tags must contain at least 1 elements"],
            },
        })
    );
}

/// A translator that renders a small French table, with display names, and
/// falls back to the app's English templates.
struct French;

impl MessageTranslator for French {
    fn message(
        &self,
        messages: &Messages,
        field: &str,
        rule: &str,
        params: &[(&str, String)],
    ) -> String {
        let mut values = vec![("attribute", self.attribute(messages, field))];
        values.extend(params.iter().map(|(name, value)| (*name, value.clone())));
        let template = match rule {
            "email" => ":attribute doit être une adresse e-mail valide",
            "min.string" => ":attribute doit contenir au moins :min caractères",
            _ => messages
                .lookup(field, rule)
                .unwrap_or(":attribute is invalid"),
        };
        Messages::fill(template, &values)
    }

    fn attribute(&self, messages: &Messages, field: &str) -> String {
        match field {
            "name" => "nom".to_owned(),
            "odd" => "impair".to_owned(),
            _ => messages.attribute(field),
        }
    }
}

#[tokio::test]
async fn a_registered_translator_renders_every_message() {
    let app = TestApp::builder()
        .override_value::<dyn MessageTranslator>(Arc::new(French))
        .config("validation.messages.min.array", "Need :min :attribute.")
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
        .assert_error(400, None);
    assert_eq!(
        errors(&response.json_value()),
        &json!({
            "name": ["nom doit contenir au moins 2 caractères"],
            "email": ["email doit être une adresse e-mail valide"],
            "tags": ["Need 1 tags."],
        })
    );

    // A plain-text message (an app rule's) gets the translated name.
    let response = app
        .client()
        .post("/v/parity")
        .json(&json!({ "even": 2, "odd": 2 }))
        .send()
        .await
        .assert_error(400, None);
    assert_eq!(
        errors(&response.json_value()),
        &json!({ "odd": ["impair must be an odd number"] })
    );
}

#[test]
fn lookup_and_fill_expose_the_templates() {
    let messages = Messages::default().with_message("email.unique", "Taken: :attribute");
    assert_eq!(
        messages.lookup("email", "unique"),
        Some("Taken: :attribute")
    );
    assert_eq!(
        messages.lookup("name", "unique"),
        Some(":attribute has already been taken")
    );
    assert_eq!(messages.lookup("name", "no_such_rule"), None);
    let values = [("attribute", "name".to_owned()), ("min", "2".to_owned())];
    assert_eq!(
        Messages::fill(":attribute needs :min (:other)", &values),
        "name needs 2 (:other)"
    );
}

// MOD-005: `AppMessages::of` is what the extractors render with: the app's
// `validation.messages` and `validation.attributes`, and its translator when it has one.
#[tokio::test]
async fn app_messages_are_the_ones_the_extractors_use() {
    use navree_validation::{AppMessages, Violation};

    let plain = TestApp::builder()
        .config("validation.messages.unique", ":Attribute is in use")
        .config("validation.attributes.email", "e-mail")
        .module(ValidationModule)
        .build()
        .await;
    let messages = AppMessages::of(Some(plain.ctx())).await;
    assert!(messages.translator().is_none());
    assert_eq!(
        messages.render_violation("email", &Violation::rule("unique")),
        "E-mail is in use"
    );
    // A form names its fields its own way, over `validation.attributes`.
    let labelled = messages.clone().with_attributes([("email", "Work email")]);
    assert_eq!(
        labelled.render_violation("email", &Violation::rule("unique")),
        "Work email is in use"
    );
    // A key the app has no message for takes the default given; one it has keeps its own.
    let defaults = labelled.with_default_messages([
        ("form.number", ":attribute: enter a number."),
        ("unique", "never used"),
    ]);
    assert_eq!(
        defaults.render("age", "form.number", &[]),
        "age: enter a number."
    );
    assert_eq!(
        defaults.render("email", "unique", &[]),
        "Work email is in use"
    );

    // Without an app: the built-in English templates.
    let bare = AppMessages::of(None).await;
    assert_eq!(
        bare.render_violation("email", &Violation::rule("unique")),
        "email has already been taken"
    );

    // With a translator: through it, display names included.
    let translated = TestApp::builder()
        .override_value::<dyn MessageTranslator>(Arc::new(French))
        .module(ValidationModule)
        .build()
        .await;
    let messages = AppMessages::of(Some(translated.ctx())).await;
    assert!(messages.translator().is_some());
    assert_eq!(
        messages.render_violation("name", &Violation::rule("min.string").with("min", 2)),
        "nom doit contenir au moins 2 caractères"
    );
    let mut violations = navree_validation::Violations::new();
    violations.add("email", Violation::rule("email"));
    let errors = messages.render_violations(&violations, "body");
    assert_eq!(
        errors.get("email"),
        Some(&["email doit être une adresse e-mail valide".to_owned()][..])
    );
}
