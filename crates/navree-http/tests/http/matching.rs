//! Navree's router in the full pipeline: what reaches a handler, what is
//! refused before one runs, methods, body limits and route conflicts.

use http::{Method, Request, StatusCode};
use navree_http::body::Body;
use navree_http::{Json, Path, Query, RouteDef};
use serde::Deserialize;

use crate::support::{Setup, json_request};

fn file_routes() -> Vec<RouteDef> {
    vec![
        RouteDef::on(Method::GET, "/{id}", |Path(id): Path<String>| async move {
            format!("file {id}")
        })
        .public(),
        RouteDef::on(
            Method::GET,
            "/{id}/raw/{*path}",
            |Path((id, path)): Path<(String, String)>| async move { format!("{id}:{path}") },
        )
        .public(),
        RouteDef::on(Method::POST, "/{id}", |Path(id): Path<String>| async move {
            format!("posted {id}")
        })
        .public(),
    ]
}

#[tokio::test]
async fn paths_reach_handlers_decoded() {
    let app = Setup::new(file_routes()).prefix("/files").boot().await;
    assert_eq!(app.get("/api/files/caf%C3%A9").await.text(), "file café");
    assert_eq!(
        app.get("/api/files/7/raw/a%2Fb/c%20d.txt").await.text(),
        "7:a/b/c d.txt"
    );
}

#[tokio::test]
async fn ambiguous_paths_are_refused_before_any_handler() {
    let app = Setup::new(file_routes()).prefix("/files").boot().await;
    for (path, message) in [
        (
            "/api/files/../files/7",
            "Invalid URL: the path has a `.` or `..` segment",
        ),
        (
            "/api/files/%2e%2e/7",
            "Invalid URL: the path has a `.` or `..` segment",
        ),
        (
            "/api/files/7/raw/a/../../etc/passwd",
            "Invalid URL: the path has a `.` or `..` segment",
        ),
        ("/api/files\\7", "Invalid URL: the path has a backslash"),
        (
            "/api/files/a%2Fb",
            "Invalid URL: `id` holds an encoded slash",
        ),
        ("/api/files/a%5Cb", "Invalid URL: `id` holds a backslash"),
        ("/api/files/a%00", "Invalid URL: `id` holds a NUL character"),
        ("/api/files/%FF", "Invalid URL: Invalid UTF-8 in `id`"),
        (
            "/api/files/7/raw/a%2F..%2Fsecret",
            "Invalid URL: `path` is or holds a `.` or `..` segment",
        ),
    ] {
        let body = app.get(path).await.assert_error(400);
        assert_eq!(body["message"], message, "{path}");
        assert_eq!(body["code"], "bad_request", "{path}");
    }
    // An empty segment or a trailing slash reaches no route.
    for path in [
        "/api/files/",
        "/api/files//raw/x",
        "/api//files/7",
        "/api/files/7/",
    ] {
        app.get(path).await.assert_error(404);
    }
}

#[tokio::test]
async fn head_options_and_405_answer_from_the_routes() {
    let app = Setup::new(file_routes()).prefix("/files").boot().await;
    let head = app
        .send(Request::head("/api/files/7").body(Body::empty()).unwrap())
        .await;
    assert_eq!(head.status, StatusCode::OK);
    assert!(head.body.is_empty());
    assert_eq!(head.header("content-length"), Some("6"));
    assert!(head.header("x-request-id").is_some());

    let options = app
        .send(
            Request::options("/api/files/7")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(options.status, StatusCode::NO_CONTENT);
    assert_eq!(options.header("allow"), Some("GET, HEAD, POST, OPTIONS"));

    let response = app
        .send(Request::delete("/api/files/7").body(Body::empty()).unwrap())
        .await;
    let body = response.assert_error(405);
    assert_eq!(body["code"], "method_not_allowed");
    assert_eq!(response.header("allow"), Some("GET, HEAD, POST, OPTIONS"));
}

#[derive(Debug, Deserialize, serde::Serialize)]
struct Note {
    title: String,
}

#[derive(Debug, Deserialize)]
struct Page {
    page: Option<u32>,
}

#[tokio::test]
async fn native_extractors_and_route_body_limits() {
    let app = Setup::new(vec![
        RouteDef::on(
            Method::POST,
            "/{deal}",
            |Path(deal): Path<u32>, Query(page): Query<Page>, Json(note): Json<Note>| async move {
                Json(serde_json::json!({ "deal": deal, "page": page.page, "title": note.title }))
            },
        )
        .public()
        .body_limit(32),
    ])
    .prefix("/notes")
    .boot()
    .await;

    let ok = app
        .send(json_request(
            "POST",
            "/api/notes/7?page=2",
            serde_json::json!({"title": "A"}),
        ))
        .await;
    assert_eq!(
        ok.json(),
        serde_json::json!({ "deal": 7, "page": 2, "title": "A" })
    );

    let body = app
        .send(json_request(
            "POST",
            "/api/notes/x",
            serde_json::json!({"title": "A"}),
        ))
        .await
        .assert_error(400);
    assert_eq!(body["message"], "Invalid URL: Cannot parse `x` to a `u32`");

    let body = app
        .send(json_request(
            "POST",
            "/api/notes/7?page=x",
            serde_json::json!({"title": "A"}),
        ))
        .await
        .assert_error(400);
    assert_eq!(
        body["message"],
        "Failed to deserialize query string: page: invalid digit found in string"
    );

    let body = app
        .send(json_request(
            "POST",
            "/api/notes/7",
            serde_json::json!({"title": 1}),
        ))
        .await
        .assert_error(400);
    assert_eq!(
        body["message"],
        "Failed to deserialize the JSON body into the target type: title: invalid type: integer `1`, expected a string at line 1 column 10"
    );

    // Past the route's limit with a length: refused before the handler.
    let long = serde_json::json!({ "title": "x".repeat(64) });
    app.send(json_request("POST", "/api/notes/7", long.clone()))
        .await
        .assert_error(413);
    // Without a length (streamed): the extractor stops at the limit.
    let chunks = futures_util::stream::iter(vec![Ok::<_, std::io::Error>(bytes::Bytes::from(
        long.to_string(),
    ))]);
    let streamed = Request::post("/api/notes/7")
        .header("content-type", "application/json")
        .body(Body::from_stream(chunks))
        .unwrap();
    let body = app.send(streamed).await.assert_error(413);
    assert_eq!(
        body["message"],
        "Failed to buffer the request body: length limit exceeded"
    );
}

#[tokio::test]
async fn routes_of_the_same_shape_and_method_fail_the_boot() {
    let error = Setup::new(vec![
        RouteDef::on(Method::GET, "/{id}", || async { "a" }).public(),
        RouteDef::on(Method::GET, "/{slug}", || async { "b" }).public(),
        RouteDef::on(Method::POST, "/{slug}", || async { "c" }).public(),
        RouteDef::on(Method::GET, "/v{n}", || async { "d" }).public(),
    ])
    .prefix("/things")
    .boot_error()
    .await;
    assert!(
        error.contains(
            "`GET /api/things/{slug}` matches the same paths as `GET /api/things/{id}` (by TestController)"
        ),
        "{error}"
    );
    assert!(
        !error.contains("POST /api/things/{slug}` matches"),
        "{error}"
    );
    assert!(
        error.contains("mixes text and a parameter; a parameter is a whole segment"),
        "{error}"
    );
}
