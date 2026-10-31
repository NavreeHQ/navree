//! Navree's extractors against the previous framework's, on the same
//! requests: the same value, or a rejection with the same status (its 422s
//! are 400s here, as the error layer has always rendered them) and the same
//! message.
//!
//! Accepted differences, each tested on its own below:
//!
//! 1. **Path parameters are checked by the router.** A value that decodes
//!    to a NUL, a backslash, a slash (in a `{name}`) or a dot segment, and
//!    any path with a dot segment or a backslash, is a 400 before any
//!    handler runs; the previous router passed them on.
//! 2. **Forms read their media type.** `Content-Type` must be
//!    `application/x-www-form-urlencoded` (any case, parameters allowed);
//!    the previous `Form` tested the header text for that prefix, so it
//!    took `application/x-www-form-urlencodedx` and refused other cases.
//! 3. **`HEAD` forms read the query string**, as `GET` ones do; the
//!    previous `Form` wanted a body for `HEAD`.
//! 4. **Two `Content-Type` headers** name no media type; the previous
//!    extractors read the first.

use std::collections::BTreeMap;

use axum::extract::FromRequest as _;
use http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use navree_http::body::Body;
use navree_http::extract::FromRequest;
use proptest::prelude::*;
use serde::Deserialize;
use tower::ServiceExt;

/// `(status, message)` of a rejection, or the value's debug text.
type Outcome = Result<String, (u16, String)>;

/// The previous framework's rejection: 422 read as 400.
fn previous(status: StatusCode, text: String) -> (u16, String) {
    let status = if status == StatusCode::UNPROCESSABLE_ENTITY {
        StatusCode::BAD_REQUEST
    } else {
        status
    };
    (status.as_u16(), text)
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime")
}

// ---------------------------------------------------------------------------
// Json
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Deal {
    title: String,
    amount: Option<u32>,
    tags: Option<Vec<String>>,
}

fn json_request(content_type: Option<&str>, body: &[u8]) -> Request<Body> {
    let mut builder = Request::post("/deals");
    if let Some(content_type) = content_type {
        builder = builder.header("content-type", content_type);
    }
    builder.body(Body::from(body.to_vec())).unwrap()
}

async fn json_both<T>(content_type: Option<&str>, body: &[u8]) -> (Outcome, Outcome)
where
    T: serde::de::DeserializeOwned + Send + std::fmt::Debug,
{
    let ours =
        <navree_http::Json<T> as FromRequest>::from_request(json_request(content_type, body))
            .await
            .map(|json| format!("{:?}", json.0))
            .map_err(|rejection| (rejection.status().as_u16(), rejection.body_text()));
    let request = json_request(content_type, body).map(axum::body::Body::new);
    let theirs = axum::Json::<T>::from_request(request, &())
        .await
        .map(|json| format!("{:?}", json.0))
        .map_err(|rejection| previous(rejection.status(), rejection.body_text()));
    (ours, theirs)
}

fn content_type() -> impl Strategy<Value = Option<String>> {
    let kind = prop::sample::select(vec![
        "application",
        "Application",
        "text",
        "",
        "*",
        "app lication",
    ]);
    let subtype = prop::sample::select(vec![
        "json",
        "JSON",
        "x+json",
        "+json",
        "json+",
        "jsonx",
        "merge-patch+json",
        "",
        "*",
        "plain",
    ]);
    let params = prop::sample::select(vec![
        "",
        ";",
        "; ",
        ";charset=utf-8",
        "; charset=utf-8",
        "; charset",
        ";=x",
        "; a=\"b c\"",
        "; a=\"\"",
        ";;",
        " ",
        "; a=b ; c=d",
        ";a=;b=c",
        "; a=\"b\" ; c=d",
    ]);
    prop::option::weighted(
        0.9,
        (kind, subtype, params)
            .prop_map(|(kind, subtype, params)| format!("{kind}/{subtype}{params}")),
    )
}

fn json_body() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        prop::sample::select(vec![
            r#"{"title":"A"}"#,
            r#"{"title":"A","amount":3}"#,
            r#"{"title":1}"#,
            r#"{"amount":-1,"title":"x"}"#,
            r#"{"title":"A","tags":["a",2]}"#,
            r#"{"title":"A"} x"#,
            r#"{"title":"A"}   "#,
            "{",
            "",
            "null",
            "[]",
            r#"{"title":"\ud800"}"#,
        ])
        .prop_map(|body| body.as_bytes().to_vec()),
        any::<Vec<u8>>(),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1024))]

    #[test]
    fn json_matches_the_previous_extractor(content_type in content_type(), body in json_body()) {
        let (ours, theirs) = runtime().block_on(json_both::<Deal>(content_type.as_deref(), &body));
        prop_assert_eq!(ours, theirs, "content type {:?}", content_type);
    }

    #[test]
    fn query_matches_the_previous_extractor(query in query()) {
        let uri: http::Uri = format!("/deals?{query}").parse().unwrap();
        same_query::<BTreeMap<String, String>>(&uri)?;
        same_query::<Filter>(&uri)?;
        same_query::<Vec<(String, i32)>>(&uri)?;
    }

    #[test]
    fn form_bodies_match_the_previous_extractor(body in query()) {
        let (ours, theirs) = runtime().block_on(form_both::<Filter>(
            Method::POST,
            Some("application/x-www-form-urlencoded"),
            body.as_bytes(),
        ));
        prop_assert_eq!(ours, theirs);
    }

    #[test]
    fn path_values_match_the_previous_extractor(a in segment(), b in segment()) {
        let path = format!("/deals/{a}/notes/{b}");
        prop_assume!(!router_refuses(&path));
        let rt = runtime();
        prop_assert_eq!(
            rt.block_on(path_both::<(u32, String)>(&path)),
            rt.block_on(previous_path::<(u32, String)>(&path))
        );
        prop_assert_eq!(
            rt.block_on(path_both::<Ids>(&path)),
            rt.block_on(previous_path::<Ids>(&path))
        );
        prop_assert_eq!(
            rt.block_on(path_both::<BTreeMap<String, String>>(&path)),
            rt.block_on(previous_path::<BTreeMap<String, String>>(&path))
        );
        prop_assert_eq!(
            rt.block_on(path_both::<u32>(&path)),
            rt.block_on(previous_path::<u32>(&path))
        );
    }
}

// ---------------------------------------------------------------------------
// Query and Form
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Filter {
    q: Option<String>,
    page: Option<u32>,
    active: Option<bool>,
}

fn query() -> impl Strategy<Value = String> {
    let name = prop::sample::select(vec!["q", "page", "active", "x", "", "p%61ge"]);
    let value = prop::sample::select(vec![
        "",
        "1",
        "-1",
        "two",
        "true",
        "yes",
        "a+b",
        "%20",
        "%ZZ",
        "%FF",
        "caf%C3%A9",
    ]);
    prop::collection::vec((name, value), 0..5).prop_map(|pairs| {
        pairs
            .into_iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("&")
    })
}

fn same_query<T>(uri: &http::Uri) -> Result<(), TestCaseError>
where
    T: serde::de::DeserializeOwned + std::fmt::Debug,
{
    let ours = navree_http::Query::<T>::try_from_uri(uri)
        .map(|query| format!("{:?}", query.0))
        .map_err(|rejection| (rejection.status().as_u16(), rejection.body_text()));
    let theirs = axum::extract::Query::<T>::try_from_uri(uri)
        .map(|query| format!("{:?}", query.0))
        .map_err(|rejection| previous(rejection.status(), rejection.body_text()));
    prop_assert_eq!(ours, theirs, "{}", uri);
    Ok(())
}

fn form_request(method: Method, content_type: Option<&str>, body: &[u8]) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri("/form?q=from-query");
    if let Some(content_type) = content_type {
        builder = builder.header("content-type", content_type);
    }
    builder.body(Body::from(body.to_vec())).unwrap()
}

async fn form_both<T>(method: Method, content_type: Option<&str>, body: &[u8]) -> (Outcome, Outcome)
where
    T: serde::de::DeserializeOwned + Send + std::fmt::Debug,
{
    let ours = <navree_http::Form<T> as FromRequest>::from_request(form_request(
        method.clone(),
        content_type,
        body,
    ))
    .await
    .map(|form| format!("{:?}", form.0))
    .map_err(|rejection| (rejection.status().as_u16(), rejection.body_text()));
    let request = form_request(method, content_type, body).map(axum::body::Body::new);
    let theirs = axum::Form::<T>::from_request(request, &())
        .await
        .map(|form| format!("{:?}", form.0))
        .map_err(|rejection| previous(rejection.status(), rejection.body_text()));
    (ours, theirs)
}

#[tokio::test]
async fn form_content_types_are_media_types() {
    // Difference 2.
    let body = b"page=2";
    for (content_type, ours_ok, theirs_ok) in [
        ("application/x-www-form-urlencoded", true, true),
        (
            "application/x-www-form-urlencoded; charset=utf-8",
            true,
            true,
        ),
        ("Application/X-WWW-Form-Urlencoded", true, false),
        ("application/x-www-form-urlencodedx", false, true),
        ("text/plain", false, false),
    ] {
        let (ours, theirs) = form_both::<Filter>(Method::POST, Some(content_type), body).await;
        assert_eq!(ours.is_ok(), ours_ok, "{content_type}: {ours:?}");
        assert_eq!(theirs.is_ok(), theirs_ok, "{content_type}: {theirs:?}");
        if !ours_ok {
            assert_eq!(
                ours.unwrap_err(),
                (
                    415,
                    "Form requests must have `Content-Type: application/x-www-form-urlencoded`"
                        .to_owned()
                )
            );
        }
    }
}

#[tokio::test]
async fn get_and_head_forms_read_the_query_string() {
    let (ours, theirs) = form_both::<Filter>(Method::GET, None, b"").await;
    assert_eq!(ours, theirs);
    assert!(ours.unwrap().contains("from-query"));
    // Difference 3.
    let (ours, theirs) = form_both::<Filter>(Method::HEAD, None, b"").await;
    assert!(ours.unwrap().contains("from-query"));
    assert_eq!(theirs.unwrap_err().0, 415);
}

#[tokio::test]
async fn two_content_type_headers_name_none() {
    // Difference 4.
    let mut request = json_request(Some("application/json"), br#"{"title":"A"}"#);
    request
        .headers_mut()
        .append("content-type", "application/json".parse().unwrap());
    let rejection = <navree_http::Json<Deal> as FromRequest>::from_request(request)
        .await
        .unwrap_err();
    assert_eq!(rejection.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

// ---------------------------------------------------------------------------
// Path
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Ids {
    deal: u32,
    note: String,
}

fn segment() -> impl Strategy<Value = String> {
    prop::sample::select(vec![
        "7",
        "42",
        "-1",
        "x",
        "caf%C3%A9",
        "a%20b",
        "a+b",
        "%FF",
        "4294967296",
        "%34%32",
        "é",
        "%",
        "%2",
        "a%2Fb",
        "%2e",
        "a%5Cb",
        "%00",
    ])
    .prop_map(str::to_owned)
}

/// Difference 1: what the router refuses before any handler runs.
fn router_refuses(path: &str) -> bool {
    path.split('/').any(|segment| {
        let decoded = percent_encoding::percent_decode_str(segment).decode_utf8_lossy();
        decoded.contains(['/', '\\', '\0']) || decoded == "." || decoded == ".."
    })
}

/// The response's status and its message (the JSON error's `message`, or
/// the text body).
async fn outcome(response: http::Response<Body>) -> Outcome {
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8_lossy(&body).into_owned();
    if status.is_success() {
        return Ok(text);
    }
    let message = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|json| json["message"].as_str().map(str::to_owned))
        .unwrap_or(text);
    Err((status.as_u16(), message))
}

async fn path_both<T>(path: &str) -> Outcome
where
    T: serde::de::DeserializeOwned + Send + std::fmt::Debug + 'static,
{
    let router = navree_http::routing::Router::new().route(
        Method::GET,
        "/deals/{deal}/notes/{note}",
        |navree_http::Path(value): navree_http::Path<T>| async move { format!("{value:?}") },
    );
    let response = router
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    server_errors_without_detail(outcome(response).await)
}

async fn previous_path<T>(path: &str) -> Outcome
where
    T: serde::de::DeserializeOwned + Send + std::fmt::Debug + 'static,
{
    let router = axum::Router::new().route(
        "/deals/{deal}/notes/{note}",
        axum::routing::get(
            |axum::extract::Path(value): axum::extract::Path<T>| async move { format!("{value:?}") },
        ),
    );
    let response = router
        .oneshot(Request::get(path).body(axum::body::Body::empty()).unwrap())
        .await
        .unwrap();
    // The error layer turns the previous framework's text rejections into
    // the JSON error format; server errors show no detail.
    server_errors_without_detail(outcome(response.map(Body::new)).await)
}

/// A 5xx shows `"Internal server error"`, whatever its text.
fn server_errors_without_detail(outcome: Outcome) -> Outcome {
    outcome.map_err(|(status, message)| {
        if status >= 500 {
            (status, "Internal server error".to_owned())
        } else {
            (status, message)
        }
    })
}

#[tokio::test]
async fn what_the_router_refuses_is_a_400_before_the_handler() {
    // Difference 1.
    for path in [
        "/deals/a%2Fb/notes/1",
        "/deals/7/notes/a%5Cb",
        "/deals/7/notes/%00",
        "/deals/%2e/notes/1",
        "/deals/../notes/1",
    ] {
        let ours = path_both::<(String, String)>(path).await;
        assert_eq!(ours.as_ref().unwrap_err().0, 400, "{path}: {ours:?}");
    }
    // The previous router passed an encoded slash to the handler.
    assert_eq!(
        previous_path::<(String, String)>("/deals/a%2Fb/notes/1").await,
        Ok(r#"("a/b", "1")"#.to_owned())
    );
}
