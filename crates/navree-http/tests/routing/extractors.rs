//! Navree's extractors against independent references, on generated
//! requests: the same value, or a rejection with the same status and the
//! same message. The references are the `mime` crate (what a `Content-Type`
//! says), `serde_json` and `serde_path_to_error` (a JSON body and the field
//! a failure names), `serde_urlencoded` and `form_urlencoded` (a query
//! string or a form), and `percent-encoding` (a path parameter).
//!
//! Accepted differences from reading these references plainly, each tested
//! on its own below:
//!
//! 1. **Path parameters are checked by the router.** A value that decodes
//!    to a NUL, a backslash, a slash (in a `{name}`) or a dot segment, and
//!    any path with a dot segment or a backslash, is a 400 before any
//!    handler runs.
//! 2. **Forms read their media type.** `Content-Type` must be
//!    `application/x-www-form-urlencoded` (any case, parameters allowed);
//!    a header that only starts with that text names another type.
//! 3. **`HEAD` forms read the query string**, as `GET` ones do.
//! 4. **Two `Content-Type` headers** name no media type.

use std::collections::BTreeMap;

use http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use navree_http::body::Body;
use navree_http::extract::FromRequest;
use proptest::prelude::*;
use serde::Deserialize;
use tower::ServiceExt;

/// `(status, message)` of a rejection, or the value's debug text.
type Outcome = Result<String, (u16, String)>;

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

async fn json_outcome<T>(content_type: Option<&str>, body: &[u8]) -> Outcome
where
    T: serde::de::DeserializeOwned + Send + std::fmt::Debug,
{
    <navree_http::Json<T> as FromRequest>::from_request(json_request(content_type, body))
        .await
        .map(|json| format!("{:?}", json.0))
        .map_err(|rejection| (rejection.status().as_u16(), rejection.body_text()))
}

/// What the references say: JSON when the one `Content-Type` is an
/// `application/` type named `json` or with a `+json` suffix; the body is
/// one JSON value of the right shape and nothing after it but whitespace.
fn json_reference<T>(content_type: Option<&str>, body: &[u8]) -> Outcome
where
    T: serde::de::DeserializeOwned + std::fmt::Debug,
{
    let says_json = content_type
        .and_then(|value| value.parse::<mime::Mime>().ok())
        .is_some_and(|media| {
            media.type_() == mime::APPLICATION
                && (media.subtype() == mime::JSON || media.suffix() == Some(mime::JSON))
        });
    if !says_json {
        return Err((
            415,
            "Expected request with `Content-Type: application/json`".to_owned(),
        ));
    }
    let mut deserializer = serde_json::Deserializer::from_slice(body);
    let value: T = match serde_path_to_error::deserialize(&mut deserializer) {
        Ok(value) => value,
        Err(error) => {
            let what = match error.inner().classify() {
                serde_json::error::Category::Data => {
                    "Failed to deserialize the JSON body into the target type"
                }
                _ => "Failed to parse the request body as JSON",
            };
            return Err((400, format!("{what}: {error}")));
        }
    };
    if let Err(error) = deserializer.end() {
        return Err((
            400,
            format!("Failed to parse the request body as JSON: {error}"),
        ));
    }
    Ok(format!("{value:?}"))
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
    fn json_matches_the_reference(content_type in content_type(), body in json_body()) {
        let ours = runtime().block_on(json_outcome::<Deal>(content_type.as_deref(), &body));
        let reference = json_reference::<Deal>(content_type.as_deref(), &body);
        prop_assert_eq!(ours, reference, "content type {:?}", content_type);
    }

    #[test]
    fn query_matches_the_reference(query in query()) {
        let uri: http::Uri = format!("/deals?{query}").parse().unwrap();
        same_query::<BTreeMap<String, String>>(&uri)?;
        same_query::<Filter>(&uri)?;
        same_query::<Vec<(String, i32)>>(&uri)?;
    }

    #[test]
    fn form_bodies_match_the_reference(body in query()) {
        let ours = runtime().block_on(form_outcome::<Filter>(
            Method::POST,
            Some("application/x-www-form-urlencoded"),
            body.as_bytes(),
        ));
        prop_assert_eq!(ours, urlencoded_reference::<Filter>(&body, "Failed to deserialize form body"));
    }

    #[test]
    fn path_values_match_the_reference(a in segment(), b in segment()) {
        let path = format!("/deals/{a}/notes/{b}");
        prop_assume!(!router_refuses(&path));
        let rt = runtime();
        let values = decoded_pair(&a, &b);
        prop_assert_eq!(
            rt.block_on(path_outcome::<(u32, String)>(&path)),
            values.clone().and_then(|(deal, note)| match deal.parse::<u32>() {
                Ok(deal) => Ok(format!("({deal}, {note:?})")),
                Err(_) => Err((
                    400,
                    format!("Invalid URL: Cannot parse value at index 0 with value `{deal}` to a `u32`"),
                )),
            })
        );
        prop_assert_eq!(
            rt.block_on(path_outcome::<Ids>(&path)),
            values.clone().and_then(|(deal, note)| match deal.parse::<u32>() {
                Ok(deal) => Ok(format!("Ids {{ deal: {deal}, note: {note:?} }}")),
                Err(_) => Err((
                    400,
                    format!("Invalid URL: Cannot parse `deal` with value `{deal}` to a `u32`"),
                )),
            })
        );
        prop_assert_eq!(
            rt.block_on(path_outcome::<BTreeMap<String, String>>(&path)),
            values.clone().map(|(deal, note)| {
                format!("{:?}", BTreeMap::from([("deal", deal), ("note", note)]))
            })
        );
        // One value for two parameters does not fit the route: a 500.
        prop_assert_eq!(
            rt.block_on(path_outcome::<u32>(&path)),
            values.and_then(|_| Err((500, "Internal server error".to_owned())))
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

/// `input` read by `serde_urlencoded` over `form_urlencoded`'s pairs, with
/// the field a failure names, as `Err((400, "{what}: {error}"))`.
fn urlencoded_reference<T>(input: &str, what: &str) -> Outcome
where
    T: serde::de::DeserializeOwned + std::fmt::Debug,
{
    let deserializer =
        serde_urlencoded::Deserializer::new(form_urlencoded::parse(input.as_bytes()));
    serde_path_to_error::deserialize::<_, T>(deserializer)
        .map(|value| format!("{value:?}"))
        .map_err(|error| (400, format!("{what}: {error}")))
}

fn same_query<T>(uri: &http::Uri) -> Result<(), TestCaseError>
where
    T: serde::de::DeserializeOwned + std::fmt::Debug,
{
    let ours = navree_http::Query::<T>::try_from_uri(uri)
        .map(|query| format!("{:?}", query.0))
        .map_err(|rejection| (rejection.status().as_u16(), rejection.body_text()));
    let reference = urlencoded_reference::<T>(
        uri.query().unwrap_or_default(),
        "Failed to deserialize query string",
    );
    prop_assert_eq!(ours, reference, "{}", uri);
    Ok(())
}

fn form_request(method: Method, content_type: Option<&str>, body: &[u8]) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri("/form?q=from-query");
    if let Some(content_type) = content_type {
        builder = builder.header("content-type", content_type);
    }
    builder.body(Body::from(body.to_vec())).unwrap()
}

async fn form_outcome<T>(method: Method, content_type: Option<&str>, body: &[u8]) -> Outcome
where
    T: serde::de::DeserializeOwned + Send + std::fmt::Debug,
{
    <navree_http::Form<T> as FromRequest>::from_request(form_request(method, content_type, body))
        .await
        .map(|form| format!("{:?}", form.0))
        .map_err(|rejection| (rejection.status().as_u16(), rejection.body_text()))
}

#[tokio::test]
async fn form_content_types_are_media_types() {
    // Difference 2.
    let body = b"page=2";
    for (content_type, accepted) in [
        ("application/x-www-form-urlencoded", true),
        ("application/x-www-form-urlencoded; charset=utf-8", true),
        ("Application/X-WWW-Form-Urlencoded", true),
        ("application/x-www-form-urlencodedx", false),
        ("text/plain", false),
    ] {
        let ours = form_outcome::<Filter>(Method::POST, Some(content_type), body).await;
        assert_eq!(ours.is_ok(), accepted, "{content_type}: {ours:?}");
        if !accepted {
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
    let ours = form_outcome::<Filter>(Method::GET, None, b"").await;
    assert_eq!(
        ours,
        urlencoded_reference::<Filter>("q=from-query", "Failed to deserialize form")
    );
    assert!(ours.unwrap().contains("from-query"));
    // Difference 3.
    let ours = form_outcome::<Filter>(Method::HEAD, None, b"").await;
    assert!(ours.unwrap().contains("from-query"));
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

async fn path_outcome<T>(path: &str) -> Outcome
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

/// The two parameters' values from the references: percent-decoded, and
/// refused by name when they are not UTF-8 (`deal` first).
fn decoded_pair(deal: &str, note: &str) -> Result<(String, String), (u16, String)> {
    let decode = |name: &str, raw: &str| {
        percent_encoding::percent_decode_str(raw)
            .decode_utf8()
            .map(std::borrow::Cow::into_owned)
            .map_err(|_| (400, format!("Invalid URL: Invalid UTF-8 in `{name}`")))
    };
    Ok((decode("deal", deal)?, decode("note", note)?))
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
        let ours = path_outcome::<(String, String)>(path).await;
        assert_eq!(ours.as_ref().unwrap_err().0, 400, "{path}: {ours:?}");
    }
}
