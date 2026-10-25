//! problem+json errors and request ids.

use std::collections::BTreeMap;

use axum::Json;
use axum::body::Body;
use axum::routing::{get, post};
use http::{Method, Request};
use keel_http::{AppError, ModuleBuilderHttpExt, RouteDef};
use serde::Deserialize;

use crate::support::{Setup, json_request};

fn validation_route() -> RouteDef {
    RouteDef::new(
        Method::POST,
        "/validate",
        post(|| async {
            let mut errors = BTreeMap::new();
            errors.insert(
                "email".to_owned(),
                vec!["The email field is required.".to_owned()],
            );
            Err::<(), _>(AppError::validation(errors))
        }),
    )
    .public()
}

#[tokio::test]
async fn problem_json_shape_and_content_type() {
    let app = Setup::new(vec![validation_route()]).boot().await;

    let response = app
        .send(Request::post("/api/validate").body(Body::empty()).unwrap())
        .await;

    let body = response.assert_problem(422);
    assert_eq!(body["type"], "validation_failed");
    assert_eq!(body["title"], "The given data was invalid.");
    assert_eq!(
        body["errors"]["email"],
        serde_json::json!(["The email field is required."])
    );
    assert!(body.get("detail").is_none(), "no detail key: {body}");
    assert_eq!(
        body.as_object().unwrap().len(),
        5,
        "no other members: {body}"
    );
    let text = response.text();
    let positions: Vec<usize> = [
        "\"type\"",
        "\"title\"",
        "\"status\"",
        "\"request_id\"",
        "\"errors\"",
    ]
    .iter()
    .map(|key| text.find(key).expect(key))
    .collect();
    assert!(positions.is_sorted(), "RFC 9457 member order: {text}");
}

#[tokio::test]
async fn request_id_is_generated_and_echoed() {
    let app = Setup::new(vec![validation_route()]).boot().await;

    let response = app
        .send(Request::post("/api/validate").body(Body::empty()).unwrap())
        .await;
    let generated = response.header("x-request-id").expect("generated id");
    assert_eq!(generated.len(), 32, "a UUID v7 in simple form: {generated}");
    assert!(generated.bytes().all(|b| b.is_ascii_hexdigit()));
    response.assert_problem(422);

    let response = app
        .send(
            Request::post("/api/validate")
                .header("x-request-id", "client-id_123")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(response.header("x-request-id"), Some("client-id_123"));
    assert_eq!(response.json()["request_id"], "client-id_123");
}

#[tokio::test]
async fn forged_request_id_is_replaced() {
    let app = Setup::new(vec![validation_route()]).boot().await;

    for forged in [
        "bad id\nINFO forged log line",
        &"a".repeat(65),
        "semi;colon",
    ] {
        let response = app
            .send(
                Request::post("/api/validate")
                    .header("x-request-id", forged.replace('\n', " "))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
        let echoed = response.header("x-request-id").unwrap();
        assert_ne!(echoed, forged);
        assert_eq!(echoed.len(), 32, "replaced by a generated id");
        response.assert_problem(422);
    }
}

#[tokio::test]
async fn internal_errors_hide_their_detail() {
    let app = Setup::new(vec![
        RouteDef::new(
            Method::GET,
            "/io",
            get(|| async {
                Err::<(), AppError>(std::io::Error::other("disk /secret/path is full").into())
            }),
        )
        .public(),
        RouteDef::new(
            Method::GET,
            "/question-mark",
            get(|| async {
                let n: i32 = "not a number".parse()?;
                Ok::<_, AppError>(n.to_string())
            }),
        )
        .public(),
    ])
    .boot()
    .await;

    for path in ["/api/io", "/api/question-mark"] {
        let response = app.get(path).await;
        let body = response.assert_problem(500);
        assert_eq!(body["type"], "internal_error");
        assert!(body.get("detail").is_none(), "detail hidden: {body}");
        assert!(!response.text().contains("secret"));
    }
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct NewThing {
    name: String,
}

#[tokio::test]
async fn framework_rejections_become_problem_json() {
    let app = Setup::new(vec![
        RouteDef::new(
            Method::POST,
            "/things",
            post(|Json(_thing): Json<NewThing>| async { "created" }),
        )
        .public(),
    ])
    .boot()
    .await;

    // Missing field: axum's JSON rejection (422, text/plain) is normalised.
    let response = app
        .send(json_request(
            "POST",
            "/api/things",
            serde_json::json!({"other": 1}),
        ))
        .await;
    let body = response.assert_problem(422);
    assert!(
        body["detail"].as_str().unwrap().contains("name"),
        "detail explains the rejection: {body}"
    );

    // Wrong method: 405 keeps its Allow header.
    let response = app.get("/api/things").await;
    response.assert_problem(405);
    assert!(response.header("allow").unwrap().contains("POST"));
}

/// An error type from "another crate" that maps to a 409.
#[derive(Debug)]
struct DuplicateKey;

impl std::fmt::Display for DuplicateKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("duplicate key value violates unique constraint")
    }
}

impl std::error::Error for DuplicateKey {}

#[tokio::test]
async fn contributed_error_mapper_maps_typed_errors() {
    let app = Setup::new(vec![
        RouteDef::new(
            Method::POST,
            "/dup",
            post(|| async { Err::<(), AppError>(DuplicateKey.into()) }),
        )
        .public(),
    ])
    .register(|m| {
        m.map_error::<DuplicateKey>(|_| Some(AppError::conflict("The record already exists.")));
    })
    .boot()
    .await;

    let response = app
        .send(Request::post("/api/dup").body(Body::empty()).unwrap())
        .await;
    let body = response.assert_problem(409);
    assert_eq!(body["detail"], "The record already exists.");
}

#[tokio::test]
async fn rejections_propagated_with_question_mark_keep_their_status() {
    use axum::extract::Path;
    use axum::extract::rejection::{JsonRejection, PathRejection};

    let app = Setup::new(vec![
        RouteDef::on(
            Method::POST,
            "/things",
            |body: std::result::Result<Json<NewThing>, JsonRejection>| async move {
                let Json(thing) = body?;
                Ok::<_, AppError>(thing.name)
            },
        )
        .public(),
        RouteDef::on(
            Method::GET,
            "/things/{id}",
            |id: std::result::Result<Path<u32>, PathRejection>| async move {
                let Path(id) = id?;
                Ok::<_, AppError>(id.to_string())
            },
        )
        .public(),
    ])
    .boot()
    .await;

    let response = app
        .send(json_request(
            "POST",
            "/api/things",
            serde_json::json!({"x": 1}),
        ))
        .await;
    let body = response.assert_problem(422);
    assert!(body["detail"].as_str().unwrap().contains("name"), "{body}");

    let body = app
        .get("/api/things/not-a-number")
        .await
        .assert_problem(400);
    assert!(body["detail"].is_string(), "{body}");
}
