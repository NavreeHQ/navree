//! Everything that turns into a response: its status, its headers in
//! order, its data frames and its size, one line each (`Seen::line`).

use std::borrow::Cow;
use std::convert::Infallible;

use bytes::{Bytes, BytesMut};
use http::header::{CONTENT_TYPE, SET_COOKIE};
use http::{Extensions, HeaderMap, HeaderName, HeaderValue, StatusCode};
use navree_http::body::Body;
use navree_http::response::{AppendHeaders, Html, IntoResponse as _, Redirect};
use proptest::prelude::*;

use crate::support::{ours, runtime};

/// `$value` as a response reads as `$expected`.
macro_rules! shape {
    ($value:expr, $expected:expr) => {{
        let response = navree_http::response::IntoResponse::into_response($value);
        let seen = ours(response).await;
        assert_eq!(seen.line(), $expected, "{}", stringify!($value));
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
    shape!((), r#"200 |  | [] | 0..0"#);
    shape!(StatusCode::NO_CONTENT, r#"204 |  | [] | 0..0"#);
    shape!(StatusCode::IM_A_TEAPOT, r#"418 |  | [] | 0..0"#);
    shape!(
        "text",
        r#"200 | content-type: text/plain; charset=utf-8 | ["text"] | 4..4"#
    );
    shape!(
        String::from("text"),
        r#"200 | content-type: text/plain; charset=utf-8 | ["text"] | 4..4"#
    );
    shape!(
        String::new(),
        r#"200 | content-type: text/plain; charset=utf-8 | [] | 0..0"#
    );
    shape!(
        Box::<str>::from("text"),
        r#"200 | content-type: text/plain; charset=utf-8 | ["text"] | 4..4"#
    );
    shape!(
        Cow::<'static, str>::Borrowed("text"),
        r#"200 | content-type: text/plain; charset=utf-8 | ["text"] | 4..4"#
    );
    shape!(
        Cow::<'static, str>::Owned("text".to_owned()),
        r#"200 | content-type: text/plain; charset=utf-8 | ["text"] | 4..4"#
    );
    shape!(
        Bytes::from_static(b"\xff\x00"),
        r#"200 | content-type: application/octet-stream | ["\xff\x00"] | 2..2"#
    );
    shape!(
        BytesMut::from(&b"bytes"[..]),
        r#"200 | content-type: application/octet-stream | ["bytes"] | 5..5"#
    );
    shape!(
        &b"bytes"[..],
        r#"200 | content-type: application/octet-stream | ["bytes"] | 5..5"#
    );
    shape!(
        b"bytes",
        r#"200 | content-type: application/octet-stream | ["bytes"] | 5..5"#
    );
    shape!(
        *b"bytes",
        r#"200 | content-type: application/octet-stream | ["bytes"] | 5..5"#
    );
    shape!(
        vec![0_u8, 1, 2],
        r#"200 | content-type: application/octet-stream | ["\x00\x01\x02"] | 3..3"#
    );
    shape!(
        vec![1_u8].into_boxed_slice(),
        r#"200 | content-type: application/octet-stream | ["\x01"] | 1..1"#
    );
    shape!(
        Cow::<'static, [u8]>::Borrowed(b"x"),
        r#"200 | content-type: application/octet-stream | ["x"] | 1..1"#
    );
    shape!(
        headers(),
        r#"200 | x-one: 1 / x-many: a / x-many: b / content-type: text/csv | [] | 0..0"#
    );
    shape!(
        Ok::<_, StatusCode>("fine"),
        r#"200 | content-type: text/plain; charset=utf-8 | ["fine"] | 4..4"#
    );
    shape!(Err::<&str, _>(StatusCode::GONE), r#"410 |  | [] | 0..0"#);
    shape!(Ok::<_, Infallible>(()), r#"200 |  | [] | 0..0"#);
}

#[tokio::test]
async fn tuples_apply_their_parts_in_order() {
    shape!(
        (StatusCode::CREATED, "made"),
        r#"201 | content-type: text/plain; charset=utf-8 | ["made"] | 4..4"#
    );
    shape!(
        (StatusCode::ACCEPTED, headers(), "body"),
        r#"202 | content-type: text/csv / x-one: 1 / x-many: a / x-many: b | ["body"] | 4..4"#
    );
    shape!(
        (headers(), "the map replaces the content type"),
        r#"200 | content-type: text/csv / x-one: 1 / x-many: a / x-many: b | ["the map replaces the content type"] | 33..33"#
    );
    shape!(
        (
            [("x-a", "1"), ("x-a", "2"), ("x-b", "3")],
            headers(),
            [(CONTENT_TYPE, "application/x-first")],
            "later parts win"
        ),
        r#"200 | content-type: application/x-first / x-a: 2 / x-b: 3 / x-one: 1 / x-many: a / x-many: b | ["later parts win"] | 15..15"#
    );
    shape!(
        (
            StatusCode::CREATED,
            [("x-a", "1")],
            [("x-b", "2")],
            [("x-a", "3")],
            "body"
        ),
        r#"201 | content-type: text/plain; charset=utf-8 / x-a: 3 / x-b: 2 | ["body"] | 4..4"#
    );
    shape!(([("x-a", "1")],), r#"200 | x-a: 1 | [] | 0..0"#);
    shape!(
        ("only a body",),
        r#"200 | content-type: text/plain; charset=utf-8 | ["only a body"] | 11..11"#
    );
    shape!(
        ((StatusCode::CREATED, "inner"),),
        r#"201 | content-type: text/plain; charset=utf-8 | ["inner"] | 5..5"#
    );
    shape!(
        [("x-a", "1"), ("x-b", "2")],
        r#"200 | x-a: 1 / x-b: 2 | [] | 0..0"#
    );
    shape!(
        (StatusCode::OK, Bytes::from_static(b"x")),
        r#"200 | content-type: application/octet-stream | ["x"] | 1..1"#
    );
    // A part may do nothing.
    shape!(
        (Option::<[(&str, &str); 1]>::None, "body"),
        r#"200 | content-type: text/plain; charset=utf-8 | ["body"] | 4..4"#
    );
    shape!(
        (Some([("x-a", "1")]), "body"),
        r#"200 | content-type: text/plain; charset=utf-8 / x-a: 1 | ["body"] | 4..4"#
    );
    shape!(
        ((), "body"),
        r#"200 | content-type: text/plain; charset=utf-8 | ["body"] | 4..4"#
    );
    shape!(
        (([("x-a", "1")], [("x-b", "2")]), "a tuple of parts"),
        r#"200 | content-type: text/plain; charset=utf-8 / x-a: 1 / x-b: 2 | ["a tuple of parts"] | 16..16"#
    );
    shape!(
        (Extensions::new(), "extensions"),
        r#"200 | content-type: text/plain; charset=utf-8 | ["extensions"] | 10..10"#
    );
}

#[tokio::test]
async fn a_status_set_inside_is_replaced_outside() {
    shape!(
        (StatusCode::CREATED, (StatusCode::ACCEPTED, "inner")),
        r#"201 | content-type: text/plain; charset=utf-8 | ["inner"] | 5..5"#
    );
    shape!(
        (StatusCode::CREATED, StatusCode::ACCEPTED),
        r#"201 |  | [] | 0..0"#
    );
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
    shape!(
        (template(), "body"),
        r#"208 | content-type: text/plain; charset=utf-8 / x-t: 1 | ["body"] | 4..4"#
    );
    shape!(
        (template(), [("x-t", "2")], "body"),
        r#"208 | content-type: text/plain; charset=utf-8 / x-t: 1 | ["body"] | 4..4"#
    );
    shape!(
        (template().into_parts().0, "body"),
        r#"208 | content-type: text/plain; charset=utf-8 / x-t: 1 | ["body"] | 4..4"#
    );
    shape!(
        (template().into_parts().0, [("x-a", "1")], "body"),
        r#"208 | content-type: text/plain; charset=utf-8 / x-a: 1 / x-t: 1 | ["body"] | 4..4"#
    );
    shape!(template().into_parts().0, r#"208 | x-t: 1 | [] | 0..0"#);

    let response = navree_http::response::IntoResponse::into_response((template(), "body"));
    assert_eq!(response.extensions().get::<u8>(), Some(&7));
}

#[tokio::test]
async fn bad_headers_are_a_500() {
    // A part that is no header is the answer: a 500 naming the problem.
    shape!(
        [("bad name", "x")],
        r#"500 | content-type: text/plain; charset=utf-8 | ["invalid HTTP header name"] | 24..24"#
    );
    shape!(
        [("x-a", "bad\nvalue")],
        r#"500 | content-type: text/plain; charset=utf-8 | ["failed to parse header value"] | 28..28"#
    );
    shape!(
        ([("bad name", "x")], "body"),
        r#"500 | content-type: text/plain; charset=utf-8 | ["invalid HTTP header name"] | 24..24"#
    );
    // A bad part is the answer: the status around it does not apply.
    shape!(
        (StatusCode::CREATED, [("x-a", "bad\nvalue")], "body"),
        r#"500 | content-type: text/plain; charset=utf-8 | ["failed to parse header value"] | 28..28"#
    );
    shape!(
        (
            StatusCode::CREATED,
            [("x-first", "1")],
            [("bad name", "x")],
            "body"
        ),
        r#"500 | content-type: text/plain; charset=utf-8 | ["invalid HTTP header name"] | 24..24"#
    );
    shape!(
        (
            http::Response::new(()).into_parts().0,
            [("bad name", "x")],
            "body"
        ),
        r#"500 | content-type: text/plain; charset=utf-8 | ["invalid HTTP header name"] | 24..24"#
    );
}

#[tokio::test]
async fn append_headers_keep_what_is_there() {
    let map = || {
        let mut map = HeaderMap::new();
        map.insert(SET_COOKIE, HeaderValue::from_static("first=1"));
        map
    };
    shape!(
        (
            map(),
            AppendHeaders([(SET_COOKIE, "a=1"), (SET_COOKIE, "b=2")]),
            "body"
        ),
        r#"200 | content-type: text/plain; charset=utf-8 / set-cookie: first=1 / set-cookie: a=1 / set-cookie: b=2 | ["body"] | 4..4"#
    );
    shape!(
        AppendHeaders([("x-a", "1"), ("x-a", "2")]),
        r#"200 | x-a: 1 / x-a: 2 | [] | 0..0"#
    );
    shape!(
        AppendHeaders([("bad name", "1")]),
        r#"500 | content-type: text/plain; charset=utf-8 | ["invalid HTTP header name"] | 24..24"#
    );
    shape!(
        (StatusCode::CREATED, AppendHeaders([("x", "\n")]), "body"),
        r#"500 | content-type: text/plain; charset=utf-8 | ["failed to parse header value"] | 28..28"#
    );
}

#[tokio::test]
async fn html_replaces_the_content_type() {
    shape!(
        Html("<p>"),
        r#"200 | content-type: text/html; charset=utf-8 | ["<p>"] | 3..3"#
    );
    shape!(
        Html(format!("<p>{}", 1)),
        r#"200 | content-type: text/html; charset=utf-8 | ["<p>1"] | 4..4"#
    );
    shape!(
        Html((StatusCode::NOT_FOUND, "<p>")),
        r#"404 | content-type: text/html; charset=utf-8 | ["<p>"] | 3..3"#
    );
    assert_eq!(Html::from("x").0, "x");
}

#[tokio::test]
async fn redirects() {
    let refused = r#"500 | content-type: text/plain; charset=utf-8 | ["failed to parse header value"] | 28..28"#;
    for (location, to, temporary, permanent) in [
        (
            "/next",
            r#"303 | location: /next | [] | 0..0"#,
            r#"307 | location: /next | [] | 0..0"#,
            r#"308 | location: /next | [] | 0..0"#,
        ),
        (
            "https://example.com/?a=1#b",
            r#"303 | location: https://example.com/?a=1#b | [] | 0..0"#,
            r#"307 | location: https://example.com/?a=1#b | [] | 0..0"#,
            r#"308 | location: https://example.com/?a=1#b | [] | 0..0"#,
        ),
        (
            "",
            r#"303 | location:  | [] | 0..0"#,
            r#"307 | location:  | [] | 0..0"#,
            r#"308 | location:  | [] | 0..0"#,
        ),
        (
            "/caf\u{e9}",
            r#"303 | location: /caf\xc3\xa9 | [] | 0..0"#,
            r#"307 | location: /caf\xc3\xa9 | [] | 0..0"#,
            r#"308 | location: /caf\xc3\xa9 | [] | 0..0"#,
        ),
        ("/a\nb", refused, refused, refused),
    ] {
        for (redirect, status, expected) in [
            (Redirect::to(location), StatusCode::SEE_OTHER, to),
            (
                Redirect::temporary(location),
                StatusCode::TEMPORARY_REDIRECT,
                temporary,
            ),
            (
                Redirect::permanent(location),
                StatusCode::PERMANENT_REDIRECT,
                permanent,
            ),
        ] {
            assert_eq!(redirect.status_code(), status, "{location:?}");
            assert_eq!(redirect.location(), location);
            assert_eq!(
                ours(redirect.into_response()).await.line(),
                expected,
                "{location:?}"
            );
        }
    }
}

#[tokio::test]
async fn a_response_keeps_its_parts_and_its_body() {
    let response = http::Response::builder()
        .status(StatusCode::PARTIAL_CONTENT)
        .header("x-a", "1")
        .extension(9_u8)
        .body(Body::from("body"))
        .unwrap();
    let response = navree_http::response::IntoResponse::into_response(response);
    assert_eq!(response.extensions().get::<u8>(), Some(&9));
    assert_eq!(
        ours(response).await.line(),
        r#"206 | x-a: 1 | ["body"] | 4..4"#
    );

    // Another body type.
    shape!(
        http::Response::builder()
            .status(StatusCode::PARTIAL_CONTENT)
            .header("x-a", "1")
            .body(http_body_util::Full::new(Bytes::from_static(b"full")))
            .unwrap(),
        r#"206 | x-a: 1 | ["full"] | 4..4"#
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Any status, any header (valid or not), any text. A part the `http`
    /// types refuse is the answer, a 500 naming the first one (a name is
    /// read before its value); else the parts apply in order, replacing
    /// (arrays) or adding (`AppendHeaders`) after the body's own content
    /// type.
    #[test]
    fn generated_tuples_apply_their_parts(
        status in 200_u16..600,
        first in "[a-zA-Z0-9 _-]{0,8}",
        first_value in "[ -~\n]{0,8}",
        second in "[a-z-]{1,8}",
        second_value in "[ -~]{0,8}",
        body in ".{0,32}",
        replace in any::<bool>(),
    ) {
        let status = StatusCode::from_u16(status).unwrap();
        let parts = || [
            (first.clone(), first_value.clone()),
            (second.clone(), second_value.clone()),
        ];
        let response = if replace {
            (status, parts(), body.clone()).into_response()
        } else {
            (status, AppendHeaders(parts()), body.clone()).into_response()
        };
        let seen = runtime().block_on(crate::support::seen(response));

        let mut expected = HeaderMap::new();
        expected.insert(CONTENT_TYPE, HeaderValue::from_static("text/plain; charset=utf-8"));
        let mut problem = None;
        for (name, value) in parts() {
            let Ok(name) = HeaderName::from_bytes(name.as_bytes()) else {
                problem = Some("invalid HTTP header name");
                break;
            };
            let Ok(value) = HeaderValue::from_str(&value) else {
                problem = Some("failed to parse header value");
                break;
            };
            if replace {
                expected.insert(name, value);
            } else {
                expected.append(name, value);
            }
        }
        let (status, frames, headers) = match problem {
            Some(text) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                vec![Bytes::from(text)],
                vec![(CONTENT_TYPE, HeaderValue::from_static("text/plain; charset=utf-8"))],
            ),
            None => (
                status,
                if body.is_empty() { vec![] } else { vec![Bytes::from(body.clone())] },
                expected.iter().map(|(name, value)| (name.clone(), value.clone())).collect(),
            ),
        };
        prop_assert_eq!(seen.status, status);
        prop_assert_eq!(seen.frames, frames);
        prop_assert_eq!(seen.headers, headers);
        prop_assert_eq!(seen.error, None);
    }

    /// A redirect's location is taken as it is, whatever it holds.
    #[test]
    fn generated_redirects_keep_their_location(location in ".{0,32}") {
        let seen = runtime().block_on(crate::support::seen(Redirect::to(&location).into_response()));
        match HeaderValue::from_str(&location) {
            Ok(value) => {
                prop_assert_eq!(seen.status, StatusCode::SEE_OTHER);
                prop_assert_eq!(seen.headers, vec![(http::header::LOCATION, value)]);
                prop_assert!(seen.frames.is_empty());
            }
            Err(_) => prop_assert_eq!(seen.status, StatusCode::INTERNAL_SERVER_ERROR),
        }
    }
}
