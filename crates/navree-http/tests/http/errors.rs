//! JSON errors and request ids.

use std::collections::BTreeMap;
use std::time::Duration;

use axum::Json;
use axum::body::Body;
use axum::routing::{get, post};
use http::{Method, Request};
use navree_config::ConfigSources;
use navree_http::{AppError, ErrorOptions, HttpOptions, ModuleBuilderHttpExt, RouteDef};
use serde::Deserialize;
use serde_json::json;

use crate::support::{Setup, json_request, options};

fn validation_route() -> RouteDef {
    RouteDef::new(
        Method::POST,
        "/validate",
        post(|| async {
            let mut errors = BTreeMap::new();
            errors.insert(
                "name".to_owned(),
                vec![
                    "name should not be empty".to_owned(),
                    "name must be a string".to_owned(),
                ],
            );
            errors.insert(
                "email".to_owned(),
                vec!["email must be a valid email address".to_owned()],
            );
            Err::<(), _>(AppError::validation(errors))
        }),
    )
    .public()
}

#[tokio::test]
async fn json_error_shape_and_content_type() {
    let app = Setup::new(vec![validation_route()]).boot().await;

    let response = app
        .send(Request::post("/api/validate").body(Body::empty()).unwrap())
        .await;

    let body = response.assert_error(400);
    assert_eq!(
        body,
        json!({
            "statusCode": 400,
            "message": [
                "email must be a valid email address",
                "name should not be empty",
                "name must be a string",
            ],
            "error": "Bad Request",
            "code": "validation_failed",
            "errors": {
                "email": ["email must be a valid email address"],
                "name": ["name should not be empty", "name must be a string"],
            },
        })
    );
    let text = response.text();
    let positions: Vec<usize> = [
        "\"statusCode\"",
        "\"message\"",
        "\"error\"",
        "\"code\"",
        "\"errors\"",
    ]
        .iter()
        .map(|key| text.find(key).expect(key))
        .collect();
    assert!(positions.is_sorted(), "error member order: {text}");
}

#[tokio::test]
async fn plain_errors_carry_message_and_reason_phrase() {
    let app = Setup::new(vec![
        RouteDef::on(Method::GET, "/missing", || async {
            Err::<(), _>(AppError::not_found())
        })
        .public(),
        RouteDef::on(Method::GET, "/custom", || async {
            Err::<(), _>(AppError::not_found().with_detail("Deal not found"))
        })
        .public(),
    ])
    .boot()
    .await;

    let body = app.get("/api/missing").await.assert_error(404);
    assert_eq!(
        body,
        json!({"statusCode": 404, "message": "Not Found", "error": "Not Found", "code": "not_found"})
    );
    let body = app.get("/api/custom").await.assert_error(404);
    assert_eq!(
        body,
        json!({"statusCode": 404, "message": "Deal not found", "error": "Not Found", "code": "not_found"})
    );
    let body = app.get("/api/no-such-route").await.assert_error(404);
    assert_eq!(
        body,
        json!({"statusCode": 404, "message": "Not Found", "error": "Not Found", "code": "not_found"})
    );
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
    response.assert_error(400);

    let response = app
        .send(
            Request::post("/api/validate")
                .header("x-request-id", "client-id_123")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(response.header("x-request-id"), Some("client-id_123"));
    response.assert_error(400);
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
        response.assert_error(400);
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
        let body = response.assert_error(500);
        assert_eq!(
            body,
            json!({
                "statusCode": 500,
                "message": "Internal server error",
                "error": "Internal Server Error",
                "code": "internal_error",
            })
        );
        assert!(!response.text().contains("secret"));
    }
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct NewThing {
    name: String,
}

#[tokio::test]
async fn framework_rejections_become_json_errors() {
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

    // Missing field: axum's JSON rejection (422, text/plain) becomes a 400.
    let response = app
        .send(json_request(
            "POST",
            "/api/things",
            serde_json::json!({"other": 1}),
        ))
        .await;
    let body = response.assert_error(400);
    assert_eq!(body["error"], "Bad Request");
    assert!(
        body["message"].as_str().unwrap().contains("name"),
        "the message explains the rejection: {body}"
    );

    // Malformed JSON: 400.
    let response = app
        .send(
            Request::post("/api/things")
                .header("content-type", "application/json")
                .body(Body::from("{not json"))
                .unwrap(),
        )
        .await;
    response.assert_error(400);

    // No JSON content type: 415.
    let response = app
        .send(Request::post("/api/things").body(Body::from("x")).unwrap())
        .await;
    let body = response.assert_error(415);
    assert_eq!(body["error"], "Unsupported Media Type");

    // Wrong method: 405 keeps its Allow header.
    let response = app.get("/api/things").await;
    let body = response.assert_error(405);
    assert_eq!(body["error"], "Method Not Allowed");
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
    let body = response.assert_error(409);
    assert_eq!(
        body,
        json!({"statusCode": 409, "message": "The record already exists.", "error": "Conflict", "code": "conflict"})
    );
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

    // axum's 422 JSON data rejection is a 400, like every input error.
    let response = app
        .send(json_request(
            "POST",
            "/api/things",
            serde_json::json!({"x": 1}),
        ))
        .await;
    let body = response.assert_error(400);
    assert!(body["message"].as_str().unwrap().contains("name"), "{body}");

    let body = app.get("/api/things/not-a-number").await.assert_error(400);
    assert!(body["message"].is_string(), "{body}");
}

#[derive(Debug)]
struct Missing;

impl std::fmt::Display for Missing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("missing record")
    }
}

impl std::error::Error for Missing {}

#[test]
fn error_mappers_apply_to_the_source_chain_outside_the_pipeline() {
    use navree_http::ErrorMapper;

    let mappers = [ErrorMapper::new::<Missing>(|_| Some(AppError::not_found()))];
    let mapped = ErrorMapper::apply(&mappers, &AppError::from(Missing)).expect("mapped");
    assert_eq!(mapped.status, http::StatusCode::NOT_FOUND);
    let unrelated = AppError::internal("something else");
    assert!(ErrorMapper::apply(&mappers, &unrelated).is_none());
}

/// Routes that fail in every way the framework knows.
fn failing_routes() -> Vec<RouteDef> {
    vec![
        validation_route(),
        RouteDef::on(Method::GET, "/missing", || async {
            Err::<(), _>(AppError::not_found().with_detail("Deal not found"))
        })
        .public(),
        RouteDef::on(Method::GET, "/private", || async { "secret" }).authenticated(),
        RouteDef::new(
            Method::POST,
            "/things",
            post(|Json(_thing): Json<NewThing>| async { "created" }),
        )
        .public(),
        RouteDef::on(Method::GET, "/panic", || async {
            if true {
                panic!("handler exploded");
            }
            "unreachable"
        })
        .public(),
        RouteDef::on(Method::GET, "/slow-route", || async {
            tokio::time::sleep(Duration::from_secs(5)).await;
            "late"
        })
        .public()
        .timeout(Duration::from_millis(50)),
        RouteDef::on(Method::GET, "/slow", || async {
            tokio::time::sleep(Duration::from_secs(5)).await;
            "late"
        })
        .public(),
        RouteDef::on(Method::POST, "/campaigns/start", || async {
            Err::<(), _>(
                AppError::conflict("The campaign is already running.")
                    .with_code("campaign_already_running"),
            )
        })
        .public(),
        RouteDef::on(Method::GET, "/question-mark", || async {
            let n: i32 = "not a number".parse()?;
            Ok::<_, AppError>(n.to_string())
        })
        .public(),
    ]
}

fn get_request(path: &str) -> Request<Body> {
    Request::get(path).body(Body::empty()).unwrap()
}

/// Every request of [`failing_routes`] with the status and code it gets.
fn failing_requests() -> Vec<(Request<Body>, u16, &'static str)> {
    vec![
        (
            Request::post("/api/validate").body(Body::empty()).unwrap(),
            400,
            "validation_failed",
        ),
        (
            json_request("POST", "/api/things", json!({"other": 1})),
            400,
            "bad_request",
        ),
        (
            Request::post("/api/things")
                .header("content-type", "application/json")
                .body(Body::from("{not json"))
                .unwrap(),
            400,
            "bad_request",
        ),
        (get_request("/api/private"), 401, "unauthenticated"),
        (get_request("/api/missing"), 404, "not_found"),
        (get_request("/api/no-such-route"), 404, "not_found"),
        (get_request("/api/things"), 405, "method_not_allowed"),
        (
            Request::post("/api/campaigns/start")
                .body(Body::empty())
                .unwrap(),
            409,
            "campaign_already_running",
        ),
        (
            Request::post("/api/things").body(Body::from("x")).unwrap(),
            415,
            "unsupported_media_type",
        ),
        (get_request("/api/panic"), 500, "internal_error"),
        (get_request("/api/question-mark"), 500, "internal_error"),
        (get_request("/api/slow-route"), 503, "request_timeout"),
        (get_request("/api/slow"), 503, "request_timeout"),
    ]
}

#[tokio::test(start_paused = true)]
async fn framework_errors_carry_their_stable_code() {
    let app = Setup::new(failing_routes())
        .options(HttpOptions {
            timeout_secs: 1,
            ..options()
        })
        .authenticated()
        .boot()
        .await;

    for (request, status, code) in failing_requests() {
        let what = format!("{} {}", request.method(), request.uri());
        let body = app.send(request).await.assert_error(status);
        assert_eq!(body["code"], code, "{what}: {body}");
    }
}

#[tokio::test(start_paused = true)]
async fn http_errors_code_false_leaves_the_code_out() {
    let app = Setup::new(failing_routes())
        .options(HttpOptions {
            timeout_secs: 1,
            errors: ErrorOptions { code: false },
            ..options()
        })
        .authenticated()
        .boot()
        .await;

    for (request, status, _) in failing_requests() {
        let what = format!("{} {}", request.method(), request.uri());
        let body = app.send(request).await.assert_error(status);
        assert!(body.get("code").is_none(), "{what}: {body}");
    }
    let body = app.get("/api/missing").await.assert_error(404);
    assert_eq!(
        body,
        json!({"statusCode": 404, "message": "Deal not found", "error": "Not Found"})
    );
}

#[tokio::test]
async fn the_environment_turns_the_code_off() {
    let sources = ConfigSources::new().env_vars("APP_", [("APP_HTTP__ERRORS__CODE", "false")]);
    let app = Setup::new(failing_routes())
        .configured_by(sources)
        .authenticated()
        .boot()
        .await;

    let body = app.get("/api/no-such-route").await.assert_error(404);
    assert!(body.get("code").is_none(), "{body}");
    let body = app.get("/api/panic").await.assert_error(500);
    assert!(body.get("code").is_none(), "{body}");
}
