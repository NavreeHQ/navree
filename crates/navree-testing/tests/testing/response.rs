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
            .assert_json_path("data.0.name", json!("Bob"));
    });
    assert!(message.contains("data.0.name"), "{message}");
    assert!(message.contains(r#"expected: "Bob""#), "{message}");
    assert!(message.contains(r#"actual: "Alice""#), "{message}");
    assert!(
        message.contains(r#""meta":{"total":2}"#),
        "the body is shown: {message}"
    );

    let missing = panic_message(|| {
        response
            .clone()
            .assert_json_path("data.5.name", json!("Eve"));
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
        .assert_json_path("data.0.name", json!("Alice"))
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
    assert!(message.contains("Alice"), "the body is shown: {message}");
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
            name: "Alice".into(),
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
async fn assert_problem_checks_status_type_and_request_id() {
    let app = TestApp::builder().module(DemoModule).build().await;
    let response = app
        .client()
        .get("/api/nowhere")
        .send()
        .await
        .assert_problem(404, "not_found");
    assert!(response.header("x-request-id").is_some());

    let response = app.client().get("/api/nowhere").send().await;
    response.clone().assert_problem(404, None);

    let wrong_code = panic_message(|| {
        response.clone().assert_problem(404, "forbidden");
    });
    assert!(
        wrong_code.contains(r#"expected problem type "forbidden""#)
            && wrong_code.contains("not_found"),
        "{wrong_code}"
    );

    let not_a_problem = panic_message(|| {
        plain_json_response().assert_problem(200, None);
    });
    assert!(
        not_a_problem.contains("application/problem+json"),
        "{not_a_problem}"
    );
}

/// A plain JSON (not problem+json) response.
fn plain_json_response() -> TestResponse {
    TestResponse::from_parts(
        http::StatusCode::OK,
        http::HeaderMap::new(),
        r#"{"ok":true}"#.into(),
    )
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
    assert_eq!(xsrf.same_site(), Some(navree_testing::cookie::SameSite::Lax));
    assert!(response.set_cookie("visits").is_none());
}
