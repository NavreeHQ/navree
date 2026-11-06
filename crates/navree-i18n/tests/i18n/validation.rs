//! Validation messages in the request's language.

use serde_json::json;

use crate::support;

#[tokio::test]
async fn validation_errors_are_in_the_requested_language() {
    let app = support::builder().build().await;

    // The docs' example: a missing field, with its French display name.
    app.client()
        .post("/api/deals/contacts")
        .header("Accept-Language", "fr-FR,fr;q=0.9")
        .json(&json!({ "last_name": "Lakhotia" }))
        .send()
        .await
        .assert_validation_error("first_name")
        .assert_json_path("errors.first_name.0", json!("prénom ne doit pas être vide"));

    // Rule messages, with plurals.
    app.client()
        .post("/api/deals/contacts")
        .header("Accept-Language", "fr")
        .json(&json!({ "first_name": "", "last_name": "Lakhotia", "email": "nope" }))
        .send()
        .await
        .assert_json_path(
            "errors",
            json!({
                "first_name": ["prénom doit contenir au moins 1 caractère"],
                "email": ["adresse e-mail doit être une adresse e-mail valide"],
            }),
        );

    // German comes from the shipped messages (no app file defines them).
    app.client()
        .post("/api/deals/contacts")
        .header("Accept-Language", "de")
        .json(&json!({ "last_name": "Lakhotia" }))
        .send()
        .await
        .assert_json_path(
            "errors.first_name.0",
            json!("first_name darf nicht leer sein"),
        );

    // English is unchanged.
    app.client()
        .post("/api/deals/contacts")
        .json(&json!({ "last_name": "Lakhotia" }))
        .send()
        .await
        .assert_json_path(
            "errors.first_name.0",
            json!("first_name should not be empty"),
        );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn validation_overrides_still_apply_in_the_default_language() {
    let app = support::builder()
        .config("validation.messages.required", "Please fill in :attribute")
        .build()
        .await;
    app.client()
        .post("/api/deals/contacts")
        .json(&json!({ "last_name": "Lakhotia" }))
        .send()
        .await
        .assert_json_path("errors.first_name.0", json!("Please fill in first_name"));
    // Translated languages keep their translation.
    app.client()
        .post("/api/deals/contacts")
        .header("Accept-Language", "fr")
        .json(&json!({ "last_name": "Lakhotia" }))
        .send()
        .await
        .assert_json_path("errors.first_name.0", json!("prénom ne doit pas être vide"));
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn international_types_and_rules_are_translated() {
    let app = support::builder().build().await;
    let invalid = json!({ "first_name": "Anu", "last_name": "Lakhotia", "country": "XX" });

    // A `Country` that does not parse, in the shipped German and French.
    app.client()
        .post("/api/deals/contacts")
        .header("Accept-Language", "de")
        .json(&invalid)
        .send()
        .await
        .assert_json_path(
            "errors.country.0",
            json!("country muss ein gültiger Ländercode nach ISO 3166-1 alpha-2 sein"),
        );
    app.client()
        .post("/api/deals/contacts")
        .header("Accept-Language", "fr")
        .json(&invalid)
        .send()
        .await
        .assert_json_path(
            "errors.country.0",
            json!("country doit être un code pays ISO 3166-1 alpha-2 valide"),
        );

    // `rules::unicode_name`, in Brazilian Portuguese for `pt`.
    app.client()
        .post("/api/deals/contacts")
        .header("Accept-Language", "pt")
        .json(&json!({ "first_name": "Anu", "last_name": "Lakhotia", "company": "<b>Agra</b>" }))
        .send()
        .await
        .assert_json_path(
            "errors.company.0",
            json!(
                "company deve conter apenas letras, números, espaços, hifens, apóstrofos e pontos"
            ),
        );
    app.shutdown().await.unwrap();
}
