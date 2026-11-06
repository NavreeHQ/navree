//! `TestResponse`: accessors and assertions, and the failure messages they give.

use std::panic::{AssertUnwindSafe, catch_unwind};

use navree_testing::{TestApp, TestResponse};
use serde::Deserialize;
use serde_json::json;

use crate::support::DemoModule;

/// The message of the panic `f` raises.
fn panic_message(f: impl FnOnce()) -> String {
    let payload = catch_unwind(AssertUnwindSafe(f)).expect_err("the assertion must fail");
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
        .expect("a string panic message")
}

async fn items() -> TestResponse {
    let app = TestApp::builder().module(DemoModule).build().await;
    app.client().get("/api/demo/items").send().await
}

#[tokio::test]
async fn assert_json_path_reports_actual_on_failure() {
    let response = items().await;

    let message = panic_message(|| {
        response
            .clone()
            .assert_json_path("data.0.name", json!("Bal"));
    });
    assert!(message.contains("data.0.name"), "{message}");
    assert!(message.contains(r#"expected: "Bal""#), "{message}");
    assert!(message.contains(r#"actual: "Aarti""#), "{message}");
    assert!(
        message.contains(r#""meta":{"total":2}"#),
        "the body is shown: {message}"
    );

    let missing = panic_message(|| {
        response
            .clone()
            .assert_json_path("data.5.name", json!("Ela"));
    });
    assert!(
        missing.contains("actual: <missing>") && missing.contains("`data.5`"),
        "names the first missing segment: {missing}"
    );
}

#[tokio::test]
async fn assert_json_path_walks_objects_and_arrays() {
    let response = items()
        .await
        .assert_status(200)
        .assert_json_path("data.0.name", json!("Aarti"))
        .assert_json_path("data.0.tags.1", json!("sales"))
        .assert_json_path("data.1.tags", json!([]))
        .assert_json_path("meta", json!({ "total": 2 }));
    assert_eq!(response.json_value()["meta"]["total"], 2);
}

#[tokio::test]
async fn assert_status_shows_the_body_on_failure() {
    let response = items().await;
    let message = panic_message(|| {
        response.clone().assert_status(201);
    });
    assert!(message.contains("expected status 201"), "{message}");
    assert!(message.contains("200"), "{message}");
    assert!(message.contains("Aarti"), "the body is shown: {message}");
}

#[tokio::test]
async fn json_deserializes_typed_bodies() {
    #[derive(Debug, Deserialize, PartialEq)]
    struct Item {
        name: String,
        tags: Vec<String>,
    }
    #[derive(Debug, Deserialize)]
    struct Items {
        data: Vec<Item>,
    }

    let items: Items = items().await.json();
    assert_eq!(
        items.data[0],
        Item {
            name: "Aarti".into(),
            tags: vec!["admin".into(), "sales".into()]
        }
    );

    let response = TestApp::builder()
        .module(DemoModule)
        .build()
        .await
        .client()
        .get("/api/demo/items")
        .send()
        .await;
    let message = panic_message(|| {
        let _: Vec<u8> = response.json();
    });
    assert!(message.contains("Vec<u8>"), "names the type: {message}");
}

#[tokio::test]
async fn assert_error_checks_status_message_and_shape() {
    let app = TestApp::builder().module(DemoModule).build().await;
    let response = app
        .client()
        .get("/api/nowhere")
        .send()
        .await
        .assert_error(404, "Not Found");
    assert!(response.header("x-request-id").is_some());

    let response = app.client().get("/api/nowhere").send().await;
    response.clone().assert_error(404, None);

    let wrong_message = panic_message(|| {
        response.clone().assert_error(404, "Deal not found");
    });
    assert!(
        wrong_message.contains(r#"expected message "Deal not found""#)
            && wrong_message.contains(r#""message":"Not Found""#),
        "{wrong_message}"
    );

    let wrong_status = panic_message(|| {
        response.clone().assert_error(403, None);
    });
    assert!(
        wrong_status.contains("expected status 403"),
        "{wrong_status}"
    );

    let not_an_error = panic_message(|| {
        plain_json_response().assert_error(200, None);
    });
    assert!(not_an_error.contains("statusCode"), "{not_an_error}");

    let wrong_type = panic_message(|| {
        TestResponse::from_parts(
            http::StatusCode::NOT_FOUND,
            http::HeaderMap::new(),
            r#"{"statusCode":404,"message":"Not Found","error":"Not Found"}"#.into(),
        )
        .assert_error(404, None);
    });
    assert!(wrong_type.contains("application/json"), "{wrong_type}");
}

#[tokio::test]
async fn assert_error_matches_one_of_several_messages() {
    let app = TestApp::builder().module(DemoModule).build().await;
    let response = app.client().post("/api/demo/invalid").send().await;
    response
        .clone()
        .assert_error(400, "name should not be empty")
        .assert_error(400, "email must be a valid email address");

    let missing = panic_message(|| {
        response.clone().assert_error(400, "phone is required");
    });
    assert!(
        missing.contains(r#"expected message "phone is required""#),
        "{missing}"
    );
}

#[tokio::test]
async fn assert_validation_error_checks_400_and_the_field() {
    let app = TestApp::builder().module(DemoModule).build().await;
    let response = app.client().post("/api/demo/invalid").send().await;
    response
        .clone()
        .assert_validation_error("email")
        .assert_validation_error("name");

    let missing = panic_message(|| {
        response.clone().assert_validation_error("phone");
    });
    assert!(
        missing.contains("no validation error for `phone`") && missing.contains("email"),
        "{missing}"
    );

    let not_validation = panic_message(|| {
        not_found_response().assert_validation_error("email");
    });
    assert!(
        not_validation.contains("expected status 400"),
        "{not_validation}"
    );
}

#[tokio::test]
async fn assert_error_code_checks_the_machine_readable_code() {
    let app = TestApp::builder().module(DemoModule).build().await;
    let response = app.client().get("/api/nowhere").send().await;
    response
        .clone()
        .assert_error_code(404, "not_found")
        .assert_error(404, "Not Found");
    app.client()
        .post("/api/demo/invalid")
        .send()
        .await
        .assert_error_code(400, "validation_failed");

    let wrong = panic_message(|| {
        response.clone().assert_error_code(404, "deal_not_found");
    });
    assert!(
        wrong.contains(r#"expected code "deal_not_found""#)
            && wrong.contains(r#""code":"not_found""#),
        "{wrong}"
    );

    let missing = panic_message(|| {
        not_found_response().assert_error_code(404, "not_found");
    });
    assert!(missing.contains("no `code`"), "{missing}");

    let wrong_status = panic_message(|| {
        response.clone().assert_error_code(410, "not_found");
    });
    assert!(
        wrong_status.contains("expected status 410"),
        "{wrong_status}"
    );
}

#[tokio::test]
#[allow(deprecated)]
async fn assert_problem_is_a_deprecated_alias_of_assert_error() {
    let app = TestApp::builder().module(DemoModule).build().await;
    app.client()
        .get("/api/nowhere")
        .send()
        .await
        .assert_problem(404, "not_found")
        .assert_problem(404, None);
    let wrong = panic_message(|| {
        not_found_response().assert_problem(404, "gone");
    });
    assert!(wrong.contains("no `code`"), "checks the code: {wrong}");
}

/// A 404 error response like the one an app returns for unknown paths.
fn not_found_response() -> TestResponse {
    let mut headers = http::HeaderMap::new();
    headers.insert("content-type", "application/json".parse().unwrap());
    headers.insert("x-request-id", "abc".parse().unwrap());
    TestResponse::from_parts(
        http::StatusCode::NOT_FOUND,
        headers,
        r#"{"statusCode":404,"message":"Not Found","error":"Not Found"}"#.into(),
    )
}

/// A plain JSON (not an error) response.
fn plain_json_response() -> TestResponse {
    let mut headers = http::HeaderMap::new();
    headers.insert("content-type", "application/json".parse().unwrap());
    headers.insert("x-request-id", "abc".parse().unwrap());
    TestResponse::from_parts(http::StatusCode::OK, headers, r#"{"ok":true}"#.into())
}

#[tokio::test]
async fn set_cookie_exposes_the_attributes_the_server_sent() {
    let app = TestApp::builder().module(DemoModule).build().await;
    let client = app.client();
    let response = client.get("/api/demo/visits").send().await;
    let cookie = response.set_cookie("visits").expect("visits is set");
    assert_eq!(cookie.value(), "1");
    assert_eq!(cookie.http_only(), Some(true));
    assert_eq!(cookie.path(), Some("/"));

    let response = client.csrf_cookie().await;
    let xsrf = response
        .set_cookie("XSRF-TOKEN")
        .expect("XSRF-TOKEN is set");
    assert_eq!(xsrf.http_only(), None, "readable by JavaScript");
    assert_eq!(
        xsrf.same_site(),
        Some(navree_testing::cookie::SameSite::Lax)
    );
    assert!(response.set_cookie("visits").is_none());
}
