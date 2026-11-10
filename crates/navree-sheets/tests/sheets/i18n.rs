//! With the `i18n` feature: export headers from the app's Fluent messages
//! in the request's locale, and imports checked in the uploader's locale.

use navree_i18n::{I18nModule, I18nOptions, Source};
use navree_queue::OperationStatus;
use navree_testing::TestApp;
use serde_json::{Value, json};

use crate::support::{self, ADMIN, csv_lines, id_of, operation, principal, run_queue, seed};

async fn app() -> TestApp {
    let app = support::builder()
        .module(I18nModule::for_root(I18nOptions {
            locales: vec!["en".into(), "fr".into()],
            dir: "tests/fixtures/locales".into(),
            negotiation: vec![Source::Header],
            ..I18nOptions::default()
        }))
        .build()
        .await;
    seed(&app, &[("anu@example.com", "Anu", Some(36), true)]).await;
    app
}

#[tokio::test]
async fn export_headers_follow_the_request_locale() {
    let app = app().await;
    let admin = app.acting_as(principal(ADMIN));
    let english = admin
        .get("/api/exports/contacts")
        .send()
        .await
        .assert_status(200);
    assert_eq!(csv_lines(english.bytes())[0], "Email,Name,Age,Vip");
    let french = admin
        .get("/api/exports/contacts")
        .header("accept-language", "fr-CA,fr;q=0.9")
        .send()
        .await
        .assert_status(200);
    assert_eq!(csv_lines(french.bytes())[0], "Adresse e-mail,Name,Age,Vip");
    let listed = admin
        .get("/api/exports")
        .header("accept-language", "fr")
        .send()
        .await
        .json_value();
    assert_eq!(listed["data"][0]["columns"][0]["header"], "Adresse e-mail");
}

#[tokio::test]
async fn a_queued_export_runs_in_the_locale_that_queued_it() {
    let app = app().await;
    let pending = app
        .acting_as(principal(ADMIN))
        .post("/api/exports/contacts")
        .header("accept-language", "fr")
        .json(&json!({}))
        .send()
        .await
        .assert_status(202)
        .json_value();
    run_queue(&app).await;
    let done = operation(&app, id_of(&pending)).await;
    assert_eq!(done.status, OperationStatus::Succeeded);
    let result: Value = done.result.unwrap();
    let file = support::file_on_disk(&app, "files", result["path"].as_str().unwrap()).await;
    assert_eq!(csv_lines(&file)[0], "Adresse e-mail,Name,Age,Vip");
}

#[tokio::test]
async fn rows_are_checked_in_the_uploader_locale() {
    let app = app().await;
    let admin = app.acting_as(principal(ADMIN));
    let (content_type, body) = support::multipart(
        &[("importer", "contacts")],
        Some((
            "c.csv",
            b"Email,Name,Age\nnope,Bal,1\nbob@example.com,Bal,x\n",
        )),
    );
    let uploaded = admin
        .post("/api/imports")
        .header("content-type", content_type)
        .header("accept-language", "fr")
        .body(body)
        .send()
        .await
        .assert_status(202)
        .json_value();
    run_queue(&app).await;
    let rows = admin
        .get(&format!(
            "/api/imports/{}/rows?status=invalid",
            id_of(&uploaded)
        ))
        .send()
        .await
        .json_value();
    assert_eq!(
        rows["data"][0]["errors"],
        json!({ "email": ["email doit être une adresse e-mail valide"] })
    );
    assert_eq!(
        rows["data"][1]["errors"],
        json!({ "age": ["age doit être un nombre entier"] })
    );
}
