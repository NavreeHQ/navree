//! keel's extra garde rules: `confirmed`, `rules::uuid`, `rules::date_ymd`
//! and `rules::one_of`.

use serde_json::json;

use crate::support::{app, errors};

#[tokio::test]
async fn confirmed_rule() {
    let app = app().await;

    let response = app
        .client()
        .post("/v/register")
        .json(&json!({ "password": "correct horse", "password_confirmation": "battery staple" }))
        .send()
        .await
        .assert_error(400, None);
    assert_eq!(
        errors(&response.json_value()),
        &json!({ "password": ["password confirmation does not match"] })
    );

    app.client()
        .post("/v/register")
        .json(&json!({ "password": "correct horse", "password_confirmation": "correct horse" }))
        .send()
        .await
        .assert_status(200);
}

#[tokio::test]
async fn uuid_date_and_one_of_rules() {
    let app = app().await;
    let owner = uuid::Uuid::now_v7().to_string();

    let response = app
        .client()
        .post("/v/schedule")
        .json(&json!({ "owner_id": "nope", "due_on": "2024-02-30", "stage": "closed" }))
        .send()
        .await
        .assert_error(400, None);
    assert_eq!(
        errors(&response.json_value()),
        &json!({
            "owner_id": ["owner_id must be a UUID"],
            "due_on": ["due_on must be a valid date in the format YYYY-MM-DD"],
            "stage": ["stage must be one of the following values: lead, won"],
        })
    );

    for due_on in [
        "2024-1-05",
        "05/01/2024",
        "2024-01-05T00:00:00Z",
        "+2024-01-05",
    ] {
        let response = app
            .client()
            .post("/v/schedule")
            .json(&json!({ "owner_id": owner, "due_on": due_on, "stage": "won" }))
            .send()
            .await
            .assert_error(400, None);
        assert_eq!(
            errors(&response.json_value()),
            &json!({ "due_on": ["due_on must be a valid date in the format YYYY-MM-DD"] }),
            "{due_on}"
        );
    }

    for due_on in [json!("2024-02-29"), json!(null)] {
        app.client()
            .post("/v/schedule")
            .json(&json!({ "owner_id": owner, "due_on": due_on, "stage": "lead" }))
            .send()
            .await
            .assert_status(200);
    }

    // Only the hyphenated form is a UUID, as in Laravel.
    let simple = uuid::Uuid::now_v7().simple().to_string();
    app.client()
        .post("/v/schedule")
        .json(&json!({ "owner_id": simple, "due_on": null, "stage": "lead" }))
        .send()
        .await
        .assert_error(400, None);
}

#[tokio::test]
async fn app_rules_use_the_message_table_or_their_own_text() {
    use keel_testing::TestApp;
    use keel_validation::{ValidationModule, ValidationOptions};

    let body = json!({ "even": 3, "odd": 4 });

    // Without a template, a keyed rule renders the `invalid` message.
    let response = app()
        .await
        .client()
        .post("/v/parity")
        .json(&body)
        .send()
        .await
        .assert_error(400, None);
    assert_eq!(
        errors(&response.json_value()),
        &json!({
            "even": ["even is invalid"],
            "odd": ["odd must be an odd number"],
        })
    );

    let options = ValidationOptions {
        messages: [("even".to_owned(), ":value is not even.".to_owned())].into(),
        ..ValidationOptions::default()
    };
    let app = TestApp::builder()
        .module(ValidationModule::with_options(options))
        .module(crate::support::ValidationTestModule)
        .build()
        .await;
    let response = app
        .client()
        .post("/v/parity")
        .json(&body)
        .send()
        .await
        .assert_error(400, None);
    assert_eq!(
        errors(&response.json_value())["even"],
        json!(["3 is not even."])
    );
}
