use std::convert::Infallible;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::body::Body;
use http::StatusCode;
use tower::{ServiceBuilder, ServiceExt, service_fn};

use super::*;

const APP: &str = "https://app.example.com";
const ADMIN: &str = "https://admin.example.com";

fn layer() -> CorsLayer {
    CorsLayer::new([
        HeaderValue::from_static(APP),
        HeaderValue::from_static(ADMIN),
    ])
    .allow_credentials(true)
    .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
    .allow_headers([
        HeaderName::from_static("content-type"),
        HeaderName::from_static("authorization"),
    ])
    .expose_headers([HeaderName::from_static("etag")])
    .max_age(Duration::from_secs(600))
}

fn request(method: Method, headers: &[(&str, &str)]) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri("/api/deals");
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    builder.body(Body::empty()).unwrap()
}

/// Sends `request` through `layer` over an inner service that marks its
/// responses with `x-inner` and an existing `Vary`. Returns the response
/// and whether the inner service ran.
async fn send(layer: &CorsLayer, request: Request<Body>) -> (Response<Body>, bool) {
    let called = Arc::new(AtomicBool::new(false));
    let seen = called.clone();
    let inner = service_fn(move |_: Request<Body>| {
        seen.store(true, Ordering::SeqCst);
        async {
            Ok::<_, Infallible>(
                Response::builder()
                    .header("x-inner", "1")
                    .header("vary", "accept-encoding")
                    .body(Body::empty())
                    .unwrap(),
            )
        }
    });
    let response = ServiceBuilder::new()
        .layer(layer.clone())
        .service(inner)
        .oneshot(request)
        .await
        .unwrap();
    (response, called.load(Ordering::SeqCst))
}

fn header<'a>(response: &'a Response<Body>, name: &str) -> Option<&'a str> {
    response.headers().get(name).map(|v| v.to_str().unwrap())
}

fn vary(response: &Response<Body>) -> Vec<String> {
    response
        .headers()
        .get_all(VARY)
        .iter()
        .map(|v| v.to_str().unwrap().to_owned())
        .collect()
}

/// The CORS headers a response carries.
fn cors_headers(response: &Response<Body>) -> Vec<String> {
    response
        .headers()
        .keys()
        .map(|name| name.as_str())
        .filter(|name| name.starts_with("access-control-"))
        .map(str::to_owned)
        .collect()
}

#[tokio::test]
async fn a_simple_request_from_an_allowed_origin_gets_the_cors_headers() {
    let (response, called) = send(&layer(), request(Method::GET, &[("origin", APP)])).await;
    assert!(called);
    assert_eq!(header(&response, "access-control-allow-origin"), Some(APP));
    assert_eq!(
        header(&response, "access-control-allow-credentials"),
        Some("true")
    );
    assert_eq!(
        header(&response, "access-control-expose-headers"),
        Some("etag")
    );
    // Preflight-only headers stay off ordinary responses.
    assert_eq!(header(&response, "access-control-allow-methods"), None);
    assert_eq!(header(&response, "access-control-max-age"), None);
    assert_eq!(header(&response, "x-inner"), Some("1"));
    // The inner Vary is kept and Origin is added.
    assert_eq!(vary(&response), ["accept-encoding", "Origin"]);
}

#[tokio::test]
async fn each_configured_origin_is_echoed_exactly() {
    for origin in [APP, ADMIN] {
        let (response, _) = send(&layer(), request(Method::GET, &[("origin", origin)])).await;
        assert_eq!(
            header(&response, "access-control-allow-origin"),
            Some(origin)
        );
    }
}

#[tokio::test]
async fn a_request_without_origin_reaches_the_handler_with_only_vary() {
    let (response, called) = send(&layer(), request(Method::GET, &[])).await;
    assert!(called);
    assert!(cors_headers(&response).is_empty(), "{response:?}");
    assert_eq!(vary(&response), ["accept-encoding", "Origin"]);
}

#[tokio::test]
async fn a_disallowed_origin_gets_no_cors_headers_but_reaches_the_handler() {
    for origin in [
        "https://evil.example",
        "http://app.example.com",
        "https://app.example.com:8443",
        "https://app.example.com/",
        "HTTPS://APP.EXAMPLE.COM",
        "null",
        "*",
        "",
    ] {
        let (response, called) = send(&layer(), request(Method::GET, &[("origin", origin)])).await;
        assert!(called, "{origin:?}");
        assert!(
            cors_headers(&response).is_empty(),
            "{origin:?}: {response:?}"
        );
        assert_eq!(header(&response, "x-inner"), Some("1"));
        assert!(vary(&response).contains(&"Origin".to_owned()), "{origin:?}");
    }
}

#[tokio::test]
async fn two_origin_headers_are_a_disallowed_origin() {
    let (response, called) = send(
        &layer(),
        request(Method::GET, &[("origin", APP), ("origin", ADMIN)]),
    )
    .await;
    assert!(called);
    assert!(cors_headers(&response).is_empty(), "{response:?}");
}

#[tokio::test]
async fn a_preflight_is_answered_without_calling_the_inner_service() {
    let (response, called) = send(
        &layer(),
        request(
            Method::OPTIONS,
            &[
                ("origin", APP),
                ("access-control-request-method", "POST"),
                ("access-control-request-headers", "content-type, x-odd"),
            ],
        ),
    )
    .await;
    assert!(!called);
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header(&response, "access-control-allow-origin"), Some(APP));
    assert_eq!(
        header(&response, "access-control-allow-credentials"),
        Some("true")
    );
    assert_eq!(
        header(&response, "access-control-allow-methods"),
        Some("GET,POST,OPTIONS")
    );
    assert_eq!(
        header(&response, "access-control-allow-headers"),
        Some("content-type,authorization")
    );
    assert_eq!(header(&response, "access-control-max-age"), Some("600"));
    assert_eq!(header(&response, "access-control-expose-headers"), None);
    assert_eq!(header(&response, "x-inner"), None);
    assert_eq!(
        vary(&response),
        ["Origin, Access-Control-Request-Method, Access-Control-Request-Headers"]
    );
}

#[tokio::test]
async fn a_preflight_from_a_disallowed_origin_gets_only_vary() {
    let (response, called) = send(
        &layer(),
        request(
            Method::OPTIONS,
            &[
                ("origin", "https://evil.example"),
                ("access-control-request-method", "POST"),
            ],
        ),
    )
    .await;
    assert!(!called);
    assert_eq!(response.status(), StatusCode::OK);
    assert!(cors_headers(&response).is_empty(), "{response:?}");
    assert_eq!(
        vary(&response),
        ["Origin, Access-Control-Request-Method, Access-Control-Request-Headers"]
    );
}

#[tokio::test]
async fn options_without_a_request_method_is_not_a_preflight() {
    // With an allowed origin: an ordinary request, so the handler runs and
    // the response carries the ordinary CORS headers.
    let (response, called) = send(&layer(), request(Method::OPTIONS, &[("origin", APP)])).await;
    assert!(called);
    assert_eq!(header(&response, "access-control-allow-origin"), Some(APP));
    assert_eq!(header(&response, "access-control-allow-methods"), None);
    // Without an origin: nothing for CORS to do.
    let (response, called) = send(&layer(), request(Method::OPTIONS, &[])).await;
    assert!(called);
    assert!(cors_headers(&response).is_empty());
    // A request method on a non-OPTIONS request means nothing.
    let (_, called) = send(
        &layer(),
        request(
            Method::POST,
            &[("origin", APP), ("access-control-request-method", "POST")],
        ),
    )
    .await;
    assert!(called);
}

#[tokio::test]
async fn credentials_off_sends_no_credentials_header_and_never_a_wildcard() {
    let layer = layer().allow_credentials(false);
    for headers in [
        vec![("origin", APP)],
        vec![("origin", APP), ("access-control-request-method", "GET")],
    ] {
        for method in [Method::GET, Method::OPTIONS] {
            let (response, _) = send(&layer, request(method, &headers)).await;
            assert_eq!(header(&response, "access-control-allow-origin"), Some(APP));
            assert_eq!(header(&response, "access-control-allow-credentials"), None);
        }
    }
}

#[tokio::test]
async fn nothing_listed_means_nothing_sent() {
    let bare = CorsLayer::new([HeaderValue::from_static(APP)]);
    let (response, _) = send(
        &bare,
        request(
            Method::OPTIONS,
            &[("origin", APP), ("access-control-request-method", "GET")],
        ),
    )
    .await;
    assert_eq!(
        cors_headers(&response),
        ["access-control-allow-origin"],
        "{response:?}"
    );
    let none = CorsLayer::new([]);
    let (response, called) = send(&none, request(Method::GET, &[("origin", APP)])).await;
    assert!(called);
    assert!(cors_headers(&response).is_empty());
}

#[tokio::test]
async fn an_existing_vary_origin_is_not_repeated() {
    let inner = service_fn(|_: Request<Body>| async {
        Ok::<_, Infallible>(
            Response::builder()
                .header("vary", "Accept, ORIGIN")
                .body(Body::empty())
                .unwrap(),
        )
    });
    let response = ServiceBuilder::new()
        .layer(layer())
        .service(inner)
        .oneshot(request(Method::GET, &[("origin", APP)]))
        .await
        .unwrap();
    assert_eq!(vary(&response), ["Accept, ORIGIN"]);
}

#[tokio::test]
async fn the_inner_services_own_cors_headers_are_replaced_for_allowed_origins() {
    let inner = service_fn(|_: Request<Body>| async {
        Ok::<_, Infallible>(
            Response::builder()
                .header("access-control-allow-origin", "*")
                .body(Body::empty())
                .unwrap(),
        )
    });
    let response = ServiceBuilder::new()
        .layer(layer())
        .service(inner)
        .oneshot(request(Method::GET, &[("origin", APP)]))
        .await
        .unwrap();
    let all: Vec<_> = response
        .headers()
        .get_all("access-control-allow-origin")
        .iter()
        .collect();
    assert_eq!(all, [APP]);
}

#[tokio::test]
async fn inner_errors_pass_through() {
    let inner = service_fn(|_: Request<Body>| async { Err::<Response<Body>, _>("boom") });
    let error = ServiceBuilder::new()
        .layer(layer())
        .service(inner)
        .oneshot(request(Method::GET, &[("origin", APP)]))
        .await
        .unwrap_err();
    assert_eq!(error, "boom");
}

#[test]
fn from_options_follows_the_http_options() {
    let mut options = CorsOptions::default();
    assert!(CorsLayer::from_options(&options).unwrap().is_none());
    options.allowed_origins = vec![format!("  {APP} ")];
    options.allow_credentials = false;
    options.max_age_secs = 5;
    let layer = CorsLayer::from_options(&options).unwrap().unwrap();
    assert_eq!(layer.rules.origins, [HeaderValue::from_static(APP)]);
    assert!(!layer.rules.credentials);
    assert_eq!(layer.rules.max_age, Some(HeaderValue::from_static("5")));
    options.allowed_origins = vec!["bad\u{7f}origin".to_owned()];
    assert!(CorsLayer::from_options(&options).is_err());
}
