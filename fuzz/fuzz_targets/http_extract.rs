//! `navree_http`'s request extractors on untrusted input: `Query`, `Form`
//! and `Json`, and the urlencoded parser under the first two. The input is
//! a `Content-Type` value and a query string (one line each), a NUL, then
//! the body.
//!
//! Invariants:
//! - nothing panics, and every rejection is a 4xx (a client's input never
//!   makes a 500);
//! - urlencoded pairs serialize and parse back to the same pairs;
//! - a JSON body accepted as a `serde_json::Value` prints as JSON that is
//!   accepted again with the same value;
//! - a form read from a body and the same pairs read from a query string
//!   give the same value.
#![no_main]

use std::collections::HashMap;

use axum::body::Body;
use http::{Method, Request};
use libfuzzer_sys::fuzz_target;
use navree_fuzz::{block_on, split_nul};
use navree_http::extract::{FromRequest, urlencoded};
use navree_http::{Form, Json, Query};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize, PartialEq)]
#[allow(dead_code)]
struct Filter {
    q: Option<String>,
    page: Option<u32>,
    active: Option<bool>,
    ratio: Option<f64>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[allow(dead_code)]
struct Deal {
    title: String,
    amount: Option<i64>,
    tags: Option<Vec<String>>,
    owner: Option<serde_json::Value>,
}

fn request(method: Method, query: &str, content_type: &str, body: &[u8]) -> Option<Request<Body>> {
    let uri: http::Uri = format!("/x?{query}").parse().ok()?;
    let mut builder = Request::builder().method(method).uri(uri);
    if !content_type.is_empty() {
        builder = builder.header("content-type", content_type);
    }
    builder.body(Body::from(body.to_vec())).ok()
}

fn client_error(status: http::StatusCode) {
    assert!(
        status.is_client_error(),
        "a rejection of client input is a 4xx, got {status}"
    );
}

/// Prints pairs the way an HTML form submits them.
fn print(pairs: &[(String, String)]) -> String {
    let encode = |text: &str| -> String {
        text.bytes()
            .map(|byte| match byte {
                b'*' | b'-' | b'.' | b'_' | b'0'..=b'9' | b'A'..=b'Z' | b'a'..=b'z' => {
                    char::from(byte).to_string()
                }
                b' ' => "+".to_owned(),
                _ => format!("%{byte:02X}"),
            })
            .collect()
    };
    pairs
        .iter()
        .map(|(name, value)| format!("{}={}", encode(name), encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Equal, but for floats, which may read back one bit apart: a parsed number
/// is the nearest float to within a step, not always the same one.
fn same(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(left), Value::Number(right)) => {
            left == right
                || match (left.as_f64(), right.as_f64()) {
                    (Some(left), Some(right)) => {
                        (left - right).abs() <= left.abs().max(right.abs()) * 1e-14
                    }
                    _ => false,
                }
        }
        (Value::Array(left), Value::Array(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| same(left, right))
        }
        (Value::Object(left), Value::Object(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .all(|(key, left)| right.get(key).is_some_and(|right| same(left, right)))
        }
        _ => left == right,
    }
}

fuzz_target!(|data: &[u8]| {
    let (head, body) = split_nul(data);
    let head = String::from_utf8_lossy(head);
    let mut lines = head.lines();
    let content_type = lines.next().unwrap_or_default();
    let query = lines.next().unwrap_or_default();

    // The parser: pairs serialize and parse back.
    for input in [query.as_bytes(), body] {
        let pairs: Vec<(String, String)> = urlencoded::parse(input)
            .map(|(name, value)| (name.into_owned(), value.into_owned()))
            .collect();
        let printed = print(&pairs);
        let again: Vec<(String, String)> = urlencoded::parse(printed.as_bytes())
            .map(|(name, value)| (name.into_owned(), value.into_owned()))
            .collect();
        assert_eq!(again, pairs, "pairs read back from {printed:?}");
        let _ = urlencoded::from_bytes::<HashMap<String, String>>(input);
        let _ = urlencoded::from_bytes::<Filter>(input);
        let _ = urlencoded::from_bytes::<Vec<(String, i32)>>(input);
    }

    // Query.
    if let Ok(uri) = format!("/x?{query}").parse::<http::Uri>() {
        for result in [
            Query::<Filter>::try_from_uri(&uri).map(drop),
            Query::<HashMap<String, String>>::try_from_uri(&uri).map(drop),
        ] {
            if let Err(rejection) = result {
                client_error(rejection.status());
            }
        }
    }

    // Form, from the body and from the query string: the same value.
    if let Some(post) = request(Method::POST, "", "application/x-www-form-urlencoded", body) {
        let from_body = block_on(<Form<Filter> as FromRequest>::from_request(post));
        if let Err(rejection) = &from_body {
            client_error(rejection.status());
        }
        let pairs: Vec<(String, String)> = urlencoded::parse(body)
            .map(|(name, value)| (name.into_owned(), value.into_owned()))
            .collect();
        if let Some(get) = request(Method::GET, &print(&pairs), "", b"") {
            let from_query = block_on(<Form<Filter> as FromRequest>::from_request(get));
            assert_eq!(
                from_body.as_ref().ok().map(|form| &form.0),
                from_query.as_ref().ok().map(|form| &form.0),
                "body and query string"
            );
        }
    }
    if let Some(post) = request(Method::POST, query, content_type, body) {
        if let Err(rejection) = block_on(<Form<Filter> as FromRequest>::from_request(post)) {
            client_error(rejection.status());
        }
    }

    // Json.
    for content_type in [content_type, "application/json"] {
        if let Some(post) = request(Method::POST, "", content_type, body) {
            match block_on(<Json<serde_json::Value> as FromRequest>::from_request(post)) {
                Ok(Json(value)) => {
                    let printed = serde_json::to_vec(&value).expect("a value prints");
                    let again = Json::<serde_json::Value>::from_bytes(&printed)
                        .expect("printed JSON reads back");
                    assert!(same(&again.0, &value), "{value} read back as {}", again.0);
                }
                Err(rejection) => client_error(rejection.status()),
            }
        }
        if let Some(post) = request(Method::POST, "", content_type, body) {
            if let Err(rejection) = block_on(<Json<Deal> as FromRequest>::from_request(post)) {
                client_error(rejection.status());
            }
        }
    }
});
