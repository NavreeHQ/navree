//! Everything that turns into a response, on both sides.

use std::borrow::Cow;
use std::convert::Infallible;

use axum::response::IntoResponse as _;
use bytes::{Bytes, BytesMut};
use http::header::{CONTENT_TYPE, SET_COOKIE};
use http::{Extensions, HeaderMap, HeaderValue, StatusCode};
use navree_http::body::Body;
use navree_http::response::{AppendHeaders, Html, IntoResponse as _, Redirect};
use proptest::prelude::*;

use crate::support::{ours, runtime, seen, theirs};

/// The same expression, through each side's `IntoResponse`.
macro_rules! same {
    ($value:expr) => {{
        let a = navree_http::response::IntoResponse::into_response($value);
        let b = axum::response::IntoResponse::into_response($value);
        assert_eq!(ours(a).await, theirs(b).await, stringify!($value));
    }};
}

fn headers() -> HeaderMap {
    let mut map = HeaderMap::new();
    map.insert("x-one", HeaderValue::from_static("1"));
    map.append("x-many", HeaderValue::from_static("a"));
    map.append("x-many", HeaderValue::from_static("b"));
    map.insert(CONTENT_TYPE, HeaderValue::from_static("text/csv"));
    map
}

#[tokio::test]
async fn plain_values() {
    same!(());
    same!(StatusCode::NO_CONTENT);
    same!(StatusCode::IM_A_TEAPOT);
    same!("text");
    same!(String::from("text"));
    same!(String::new());
    same!(Box::<str>::from("text"));
    same!(Cow::<'static, str>::Borrowed("text"));
    same!(Cow::<'static, str>::Owned("text".to_owned()));
    same!(Bytes::from_static(b"\xff\x00"));
    same!(BytesMut::from(&b"bytes"[..]));
    same!(&b"bytes"[..]);
    same!(b"bytes");
    same!(*b"bytes");
    same!(vec![0_u8, 1, 2]);
    same!(vec![1_u8].into_boxed_slice());
    same!(Cow::<'static, [u8]>::Borrowed(b"x"));
    same!(headers());
    same!(Ok::<_, StatusCode>("fine"));
    same!(Err::<&str, _>(StatusCode::GONE));
    same!(Ok::<_, Infallible>(()));
}

#[tokio::test]
async fn tuples_apply_their_parts_in_order() {
    same!((StatusCode::CREATED, "made"));
    same!((StatusCode::ACCEPTED, headers(), "body"));
    same!((headers(), "the map replaces the content type"));
    same!((
        [("x-a", "1"), ("x-a", "2"), ("x-b", "3")],
        headers(),
        [(CONTENT_TYPE, "application/x-first")],
        "later parts win"
    ));
    same!((
        StatusCode::CREATED,
        [("x-a", "1")],
        [("x-b", "2")],
        [("x-a", "3")],
        "body"
    ));
    same!(([("x-a", "1")],));
    same!(("only a body",));
    same!(((StatusCode::CREATED, "inner"),));
    same!([("x-a", "1"), ("x-b", "2")]);
    same!((StatusCode::OK, Bytes::from_static(b"x")));
    // A part may do nothing.
    same!((Option::<[(&str, &str); 1]>::None, "body"));
    same!((Some([("x-a", "1")]), "body"));
    same!(((), "body"));
    same!((([("x-a", "1")], [("x-b", "2")]), "a tuple of parts"));
    same!((Extensions::new(), "extensions"));
}

#[tokio::test]
async fn a_status_set_inside_is_replaced_outside() {
    same!((StatusCode::CREATED, (StatusCode::ACCEPTED, "inner")));
    same!((StatusCode::CREATED, StatusCode::ACCEPTED));
}

#[tokio::test]
async fn templates_set_status_headers_and_extensions() {
    let template = || {
        let mut response = http::Response::new(());
        *response.status_mut() = StatusCode::ALREADY_REPORTED;
        response.headers_mut().insert("x-t", "1".parse().unwrap());
        response.extensions_mut().insert(7_u8);
        response
    };
    same!((template(), "body"));
    same!((template(), [("x-t", "2")], "body"));
    same!((template().into_parts().0, "body"));
    same!((template().into_parts().0, [("x-a", "1")], "body"));
    same!(template().into_parts().0);

    let response = navree_http::response::IntoResponse::into_response((template(), "body"));
    assert_eq!(response.extensions().get::<u8>(), Some(&7));
}

#[tokio::test]
async fn bad_headers_are_a_500() {
    // Difference 2.
    same!([("bad name", "x")]);
    same!([("x-a", "bad\nvalue")]);
    same!(([("bad name", "x")], "body"));
    // A bad part is the answer: the status around it does not apply.
    same!((StatusCode::CREATED, [("x-a", "bad\nvalue")], "body"));
    same!((
        StatusCode::CREATED,
        [("x-first", "1")],
        [("bad name", "x")],
        "body"
    ));
    same!((
        http::Response::new(()).into_parts().0,
        [("bad name", "x")],
        "body"
    ));
}

#[tokio::test]
async fn append_headers_keep_what_is_there() {
    let map = || {
        let mut map = HeaderMap::new();
        map.insert(SET_COOKIE, HeaderValue::from_static("first=1"));
        map
    };
    let a = (
        map(),
        AppendHeaders([(SET_COOKIE, "a=1"), (SET_COOKIE, "b=2")]),
        "body",
    );
    let b = (
        map(),
        axum::response::AppendHeaders([(SET_COOKIE, "a=1"), (SET_COOKIE, "b=2")]),
        "body",
    );
    let a = ours(a.into_response()).await;
    assert_eq!(a, theirs(b.into_response()).await);
    let cookies = a.headers.iter().filter(|(name, _)| name == SET_COOKIE);
    assert_eq!(cookies.count(), 3);

    let a = AppendHeaders([("x-a", "1"), ("x-a", "2")]).into_response();
    let b = axum::response::AppendHeaders([("x-a", "1"), ("x-a", "2")]).into_response();
    assert_eq!(ours(a).await, theirs(b).await);
    let a = AppendHeaders([("bad name", "1")]).into_response();
    let b = axum::response::AppendHeaders([("bad name", "1")]).into_response();
    assert_eq!(ours(a).await, theirs(b).await);
    let a = (StatusCode::CREATED, AppendHeaders([("x", "\n")]), "body").into_response();
    let b = (
        StatusCode::CREATED,
        axum::response::AppendHeaders([("x", "\n")]),
        "body",
    )
        .into_response();
    assert_eq!(ours(a).await, theirs(b).await);
}

#[tokio::test]
async fn html_replaces_the_content_type() {
    let a = Html("<p>").into_response();
    let b = axum::response::Html("<p>").into_response();
    assert_eq!(ours(a).await, theirs(b).await);
    let a = Html(format!("<p>{}", 1)).into_response();
    let b = axum::response::Html(format!("<p>{}", 1)).into_response();
    assert_eq!(ours(a).await, theirs(b).await);
    let a = Html((StatusCode::NOT_FOUND, "<p>")).into_response();
    let b = axum::response::Html((StatusCode::NOT_FOUND, "<p>")).into_response();
    assert_eq!(ours(a).await, theirs(b).await);
    assert_eq!(Html::from("x").0, axum::response::Html::from("x").0);
}

#[tokio::test]
async fn redirects() {
    for location in [
        "/next",
        "https://example.com/?a=1#b",
        "",
        "/caf\u{e9}",
        "/a\nb",
    ] {
        let pairs = [
            (
                Redirect::to(location),
                axum::response::Redirect::to(location),
            ),
            (
                Redirect::temporary(location),
                axum::response::Redirect::temporary(location),
            ),
            (
                Redirect::permanent(location),
                axum::response::Redirect::permanent(location),
            ),
        ];
        for (a, b) in pairs {
            assert_eq!(a.status_code(), b.status_code());
            assert_eq!(a.location(), b.location());
            assert_eq!(
                ours(a.into_response()).await,
                theirs(b.into_response()).await,
                "{location:?}"
            );
        }
    }
}

#[tokio::test]
async fn a_response_keeps_its_parts_and_its_body() {
    let response = || {
        http::Response::builder()
            .status(StatusCode::PARTIAL_CONTENT)
            .header("x-a", "1")
            .extension(9_u8)
    };
    let a = response().body(Body::from("body")).unwrap();
    let a = navree_http::response::IntoResponse::into_response(a);
    assert_eq!(a.extensions().get::<u8>(), Some(&9));
    let b = response().body(axum::body::Body::from("body")).unwrap();
    let b = axum::response::IntoResponse::into_response(b);
    assert_eq!(ours(a).await, theirs(b).await);

    // Another body type, through each side's `IntoResponse`.
    let full = |text: &'static str| {
        response()
            .body(http_body_util::Full::new(Bytes::from_static(
                text.as_bytes(),
            )))
            .unwrap()
    };
    same!(full("full"));
}

#[tokio::test]
async fn each_side_answers_with_the_others_response() {
    // What the bridge counts on: a response body is a body for either side.
    let previous = axum::response::IntoResponse::into_response("text");
    let a = navree_http::response::IntoResponse::into_response(previous);
    let b = axum::response::IntoResponse::into_response("text");
    assert_eq!(ours(a).await, theirs(b).await);

    let native = navree_http::response::IntoResponse::into_response("text");
    let a = axum::response::IntoResponse::into_response(native);
    let b = axum::response::IntoResponse::into_response("text");
    assert_eq!(theirs(a).await, theirs(b).await);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Any status, any header (valid or not), any text.
    #[test]
    fn generated_tuples_read_the_same(
        status in 200_u16..600,
        first in "[a-zA-Z0-9 _-]{0,8}",
        first_value in "[ -~\n]{0,8}",
        second in "[a-z-]{1,8}",
        second_value in "[ -~]{0,8}",
        body in ".{0,32}",
        replace in any::<bool>(),
    ) {
        let status = StatusCode::from_u16(status).unwrap();
        let (a, b) = if replace {
            let parts = || [
                (first.clone(), first_value.clone()),
                (second.clone(), second_value.clone()),
            ];
            (
                navree_http::response::IntoResponse::into_response((status, parts(), body.clone())),
                axum::response::IntoResponse::into_response((status, parts(), body.clone())),
            )
        } else {
            let parts = || [
                (first.clone(), first_value.clone()),
                (second.clone(), second_value.clone()),
            ];
            (
                (status, AppendHeaders(parts()), body.clone()).into_response(),
                (status, axum::response::AppendHeaders(parts()), body.clone()).into_response(),
            )
        };
        let (a, b) = runtime().block_on(async { (seen(a).await, seen(b).await) });
        prop_assert_eq!(a, b);
    }

    /// A redirect's location is taken as it is, whatever it holds.
    #[test]
    fn generated_redirects_read_the_same(location in ".{0,32}") {
        let a = Redirect::to(&location).into_response();
        let b = axum::response::IntoResponse::into_response(
            axum::response::Redirect::to(&location),
        );
        let (a, b) = runtime().block_on(async { (seen(a).await, seen(b).await) });
        prop_assert_eq!(a, b);
    }
}
