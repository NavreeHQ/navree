//! `Multipart` behind the whole pipeline: the error format, the route's body
//! limit, and `http.multipart`.

use http::{Method, Request};
use navree_config::ConfigSources;
use navree_http::body::{Body, Bytes};
use navree_http::extract::Multipart;
use navree_http::{AppError, HttpOptions, Json, MultipartLimits, RouteDef};
use serde_json::json;

use crate::support::{Setup, options};

const CONTENT_TYPE: &str = "multipart/form-data; boundary=b";

/// One text part per value.
fn form(values: &[&str]) -> String {
    let mut body = String::new();
    for value in values {
        body.push_str(&format!(
            "--b\r\nContent-Disposition: form-data; name=\"field\"\r\n\r\n{value}\r\n"
        ));
    }
    body.push_str("--b--\r\n");
    body
}

fn post(body: String) -> Request<Body> {
    // No `Content-Length`: the limits have to hold without one.
    let chunks = body
        .into_bytes()
        .chunks(16)
        .map(|chunk| Ok::<_, std::io::Error>(Bytes::copy_from_slice(chunk)))
        .collect::<Vec<_>>();
    Request::post("/api/forms")
        .header("content-type", CONTENT_TYPE)
        .body(Body::from_stream(futures_util::stream::iter(chunks)))
        .unwrap()
}

/// A route that reads every part with `?`.
fn route() -> RouteDef {
    RouteDef::on(Method::POST, "/forms", |mut form: Multipart| async move {
        let mut values = Vec::new();
        while let Some(field) = form.next_field().await? {
            values.push(field.text().await?);
        }
        Ok::<_, AppError>(Json(json!({ "values": values })))
    })
    .public()
}

#[tokio::test]
async fn a_form_reaches_the_handler() {
    let app = Setup::new(vec![route()]).boot().await;
    let response = app.send(post(form(&["one", "two"]))).await;
    assert_eq!(response.json(), json!({ "values": ["one", "two"] }));
}

/// `?` in a handler answers what the extractor would have.
#[tokio::test]
async fn a_failure_keeps_its_status_through_question_mark() {
    let app = Setup::new(vec![route()]).boot().await;

    let cut = "--b\r\nContent-Disposition: form-data; name=\"field\"\r\n\r\nvalue".to_owned();
    let body = app.send(post(cut)).await.assert_error(400);
    assert_eq!(
        body["message"],
        "field \"field\" received with incomplete data"
    );

    let large = "x".repeat(1024 * 1024 + 1);
    let body = app.send(post(form(&[&large]))).await.assert_error(413);
    assert_eq!(
        body["message"],
        "A form field may have at most 1048576 bytes"
    );
    assert_eq!(body["code"], "payload_too_large");
}

#[tokio::test]
async fn a_request_without_a_boundary_is_a_400_in_the_error_format() {
    let app = Setup::new(vec![route()]).boot().await;
    let request = Request::post("/api/forms")
        .header("content-type", "multipart/form-data")
        .body(Body::empty())
        .unwrap();
    let body = app.send(request).await.assert_error(400);
    assert_eq!(
        body["message"],
        "Invalid `boundary` for `multipart/form-data` request"
    );
    assert_eq!(body["statusCode"], 400);
}

#[tokio::test]
async fn the_route_body_limit_holds_without_a_content_length() {
    let app = Setup::new(vec![route().body_limit(64)]).boot().await;
    let body = app
        .send(post(form(&["a value long enough to pass the limit"])))
        .await
        .assert_error(413);
    assert_eq!(body["message"], "Request payload is too large");
}

#[tokio::test]
async fn a_route_may_raise_the_body_limit_past_the_default() {
    let app = Setup::new(vec![route().body_limit(4 * 1024 * 1024)])
        .boot()
        .await;
    let value = "x".repeat(1024 * 1024);
    let values = [value.as_str(), value.as_str(), value.as_str()];
    let response = app.send(post(form(&values))).await;
    assert_eq!(response.json()["values"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn the_app_sets_the_limits_in_its_options() {
    let app = Setup::new(vec![route()])
        .options(HttpOptions {
            multipart: MultipartLimits {
                max_parts: 2,
                ..MultipartLimits::default()
            },
            ..options()
        })
        .boot()
        .await;
    let response = app.send(post(form(&["1", "2"]))).await;
    assert_eq!(response.json(), json!({ "values": ["1", "2"] }));
    let body = app
        .send(post(form(&["1", "2", "3"])))
        .await
        .assert_error(413);
    assert_eq!(body["message"], "A form may have at most 2 parts");
}

#[tokio::test]
async fn the_limits_come_from_configuration() {
    let app = Setup::new(vec![route()])
        .configured_by(ConfigSources::new().set("http.multipart.max_field_bytes", 4))
        .boot()
        .await;
    let body = app.send(post(form(&["12345"]))).await.assert_error(413);
    assert_eq!(body["message"], "A form field may have at most 4 bytes");
}

#[test]
fn limits_that_refuse_everything_fail_the_boot() {
    use navree_config::ConfigSection;

    let mut options = HttpOptions::default();
    assert!(options.validate().is_ok());
    options.multipart.max_parts = 0;
    options.multipart.max_header_bytes = 0;
    let error = options.validate().unwrap_err();
    assert!(error.contains("http.multipart.max_parts"), "{error}");
    assert!(error.contains("http.multipart.max_header_bytes"), "{error}");
}
