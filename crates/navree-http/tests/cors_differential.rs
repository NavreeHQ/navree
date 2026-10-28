//! Our `CorsLayer` against tower-http's, on a matrix of configurations and
//! requests.
//!
//! The two must produce the same status and headers, except where
//! tower-http is less strict than the Fetch standard. Those differences are
//! written as three rules in [`standard`], applied to tower-http's answer,
//! and every request in the matrix is checked against the result:
//!
//! 1. **CORS headers only for allowed origins.** tower-http sends
//!    `Access-Control-Allow-Credentials`, `-Expose-Headers`, `-Allow-Methods`,
//!    `-Allow-Headers` and `-Max-Age` to every origin, and for requests
//!    without one. We send no CORS header to an origin that is not allowed,
//!    and none without an `Origin` (only `Vary: Origin`).
//! 2. **A preflight is `OPTIONS` with `Origin` and
//!    `Access-Control-Request-Method`.** tower-http answers every `OPTIONS`
//!    request itself. We pass the others to the handler.
//! 3. **`Vary`.** tower-http 0.6 (the version in the tree) varies every
//!    response with `Origin`, `Access-Control-Request-Method` and
//!    `Access-Control-Request-Headers`. We vary ordinary responses with
//!    `Origin` and preflight responses with all three.
//!
//! A request with two `Origin` headers is a fourth case: tower-http reads
//! the first, we treat it as a disallowed origin (rule 1 then applies).

use std::convert::Infallible;
use std::time::Duration;

use axum::body::Body;
use http::{HeaderName, HeaderValue, Method, Request, Response, StatusCode};
use navree_http::CorsLayer;
use tower::{ServiceBuilder, ServiceExt, service_fn};
use tower_http::cors::{AllowOrigin, CorsLayer as TowerCors};

const APP: &str = "https://app.example.com";
const ADMIN: &str = "https://admin.example.com";

const METHODS: [Method; 7] = [
    Method::GET,
    Method::HEAD,
    Method::POST,
    Method::PUT,
    Method::PATCH,
    Method::DELETE,
    Method::OPTIONS,
];
const REQUEST_HEADERS: [&str; 4] = ["accept", "authorization", "content-type", "x-xsrf-token"];
const EXPOSED: [&str; 3] = ["x-request-id", "etag", "location"];

struct Config {
    name: &'static str,
    origins: &'static [&'static str],
    credentials: bool,
}

const CONFIGS: &[Config] = &[
    Config {
        name: "no origins, credentials on",
        origins: &[],
        credentials: true,
    },
    Config {
        name: "no origins, credentials off",
        origins: &[],
        credentials: false,
    },
    Config {
        name: "one origin, credentials on",
        origins: &[APP],
        credentials: true,
    },
    Config {
        name: "one origin, credentials off",
        origins: &[APP],
        credentials: false,
    },
    Config {
        name: "two origins, credentials on",
        origins: &[APP, ADMIN],
        credentials: true,
    },
    Config {
        name: "two origins, credentials off",
        origins: &[APP, ADMIN],
        credentials: false,
    },
];

fn ours(config: &Config) -> CorsLayer {
    CorsLayer::new(config.origins.iter().map(|o| HeaderValue::from_static(o)))
        .allow_credentials(config.credentials)
        .allow_methods(METHODS)
        .allow_headers(REQUEST_HEADERS.map(HeaderName::from_static))
        .expose_headers(EXPOSED.map(HeaderName::from_static))
        .max_age(Duration::from_secs(600))
}

fn theirs(config: &Config) -> TowerCors {
    TowerCors::new()
        .allow_origin(AllowOrigin::list(
            config.origins.iter().map(|o| HeaderValue::from_static(o)),
        ))
        .allow_credentials(config.credentials)
        .allow_methods(METHODS)
        .allow_headers(REQUEST_HEADERS.map(HeaderName::from_static))
        .expose_headers(EXPOSED.map(HeaderName::from_static))
        .max_age(Duration::from_secs(600))
}

struct Case {
    name: &'static str,
    method: Method,
    headers: &'static [(&'static str, &'static str)],
}

const CASES: &[Case] = &[
    Case {
        name: "simple, first origin",
        method: Method::GET,
        headers: &[("origin", APP)],
    },
    Case {
        name: "simple, second origin",
        method: Method::POST,
        headers: &[("origin", ADMIN)],
    },
    Case {
        name: "simple, no origin",
        method: Method::GET,
        headers: &[],
    },
    Case {
        name: "simple, disallowed origin",
        method: Method::GET,
        headers: &[("origin", "https://evil.example")],
    },
    Case {
        name: "simple, null origin",
        method: Method::GET,
        headers: &[("origin", "null")],
    },
    Case {
        name: "simple, origin in another case",
        method: Method::GET,
        headers: &[("origin", "HTTPS://APP.EXAMPLE.COM")],
    },
    Case {
        name: "simple, origin with a trailing slash",
        method: Method::GET,
        headers: &[("origin", "https://app.example.com/")],
    },
    Case {
        name: "simple, two origin headers",
        method: Method::GET,
        headers: &[("origin", APP), ("origin", "https://evil.example")],
    },
    Case {
        name: "preflight, allowed origin",
        method: Method::OPTIONS,
        headers: &[
            ("origin", APP),
            ("access-control-request-method", "POST"),
            (
                "access-control-request-headers",
                "content-type, x-xsrf-token",
            ),
        ],
    },
    Case {
        name: "preflight, second origin, no request headers",
        method: Method::OPTIONS,
        headers: &[("origin", ADMIN), ("access-control-request-method", "GET")],
    },
    Case {
        name: "preflight, odd request headers",
        method: Method::OPTIONS,
        headers: &[
            ("origin", APP),
            ("access-control-request-method", "PUT"),
            ("access-control-request-headers", "x-unlisted,  , X-Odd-One"),
        ],
    },
    Case {
        name: "preflight, unlisted method",
        method: Method::OPTIONS,
        headers: &[("origin", APP), ("access-control-request-method", "TRACE")],
    },
    Case {
        name: "preflight, disallowed origin",
        method: Method::OPTIONS,
        headers: &[
            ("origin", "https://evil.example"),
            ("access-control-request-method", "POST"),
        ],
    },
    Case {
        name: "preflight, disallowed origin and odd headers",
        method: Method::OPTIONS,
        headers: &[
            ("origin", "https://evil.example"),
            ("access-control-request-method", "POST"),
            ("access-control-request-headers", "authorization, x-odd"),
        ],
    },
    Case {
        name: "options, allowed origin, missing request method",
        method: Method::OPTIONS,
        headers: &[("origin", APP)],
    },
    Case {
        name: "options, disallowed origin, missing request method",
        method: Method::OPTIONS,
        headers: &[("origin", "https://evil.example")],
    },
    Case {
        name: "options, no origin, no request method",
        method: Method::OPTIONS,
        headers: &[],
    },
    Case {
        name: "options, request method but no origin",
        method: Method::OPTIONS,
        headers: &[("access-control-request-method", "POST")],
    },
    Case {
        name: "preflight, two origin headers",
        method: Method::OPTIONS,
        headers: &[
            ("origin", APP),
            ("origin", ADMIN),
            ("access-control-request-method", "POST"),
        ],
    },
    Case {
        name: "request method header on a GET",
        method: Method::GET,
        headers: &[("origin", APP), ("access-control-request-method", "POST")],
    },
];

fn request(method: &Method, headers: &[(&str, &str)]) -> Request<Body> {
    let mut builder = Request::builder().method(method.clone()).uri("/api/deals");
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    builder.body(Body::empty()).unwrap()
}

/// The inner service: marks its answer and sets an unrelated `Vary`.
async fn inner(_: Request<Body>) -> Result<Response<Body>, Infallible> {
    Ok(Response::builder()
        .status(StatusCode::NO_CONTENT)
        .header("x-inner", "1")
        .header("vary", "accept-encoding")
        .body(Body::empty())
        .unwrap())
}

/// Status and headers as sorted `name: value` lines (`Vary` lower-cased).
fn summary(response: &Response<Body>) -> Vec<String> {
    let mut lines: Vec<String> = response
        .headers()
        .iter()
        .map(|(name, value)| {
            let value = value.to_str().unwrap();
            if name.as_str() == "vary" {
                format!("{name}: {}", value.to_ascii_lowercase())
            } else {
                format!("{name}: {value}")
            }
        })
        .collect();
    lines.push(format!("status: {}", response.status().as_u16()));
    lines.sort();
    lines
}

async fn run_ours(config: &Config, method: &Method, headers: &[(&str, &str)]) -> Vec<String> {
    let response = ServiceBuilder::new()
        .layer(ours(config))
        .service(service_fn(inner))
        .oneshot(request(method, headers))
        .await
        .unwrap();
    summary(&response)
}

async fn run_theirs(config: &Config, method: &Method, headers: &[(&str, &str)]) -> Vec<String> {
    let response = ServiceBuilder::new()
        .layer(theirs(config))
        .service(service_fn(inner))
        .oneshot(request(method, headers))
        .await
        .unwrap();
    summary(&response)
}

fn header_values<'a>(headers: &'a [(&str, &str)], name: &str) -> Vec<&'a str> {
    headers
        .iter()
        .filter(|(n, _)| *n == name)
        .map(|(_, v)| *v)
        .collect()
}

/// What tower-http's answer becomes under the Fetch standard.
async fn standard(config: &Config, case: &Case) -> Vec<String> {
    let origins = header_values(case.headers, "origin");
    let allowed = matches!(origins.as_slice(), [one] if config.origins.contains(one));
    let preflight = case.method == Method::OPTIONS
        && !origins.is_empty()
        && !header_values(case.headers, "access-control-request-method").is_empty();

    // Rule 2: an OPTIONS request that is not a preflight is an ordinary
    // request. tower-http handles one as an ordinary request when it is
    // not OPTIONS, so ask it that.
    let method = if case.method == Method::OPTIONS && !preflight {
        Method::GET
    } else {
        case.method.clone()
    };
    let mut lines = run_theirs(config, &method, case.headers).await;

    // Rule 1: CORS headers only for an allowed origin.
    if !allowed {
        lines.retain(|line| !line.starts_with("access-control-"));
    }
    // Rule 3: `Vary` is `Origin`, and on a preflight also the request
    // method and headers. tower-http 0.6 sends all three on every response.
    let vary = if preflight {
        "vary: origin, access-control-request-method, access-control-request-headers"
    } else {
        "vary: origin"
    };
    for line in &mut lines {
        if line.starts_with("vary: origin") {
            *line = vary.to_owned();
        }
    }
    lines.sort();
    lines
}

#[tokio::test]
async fn our_layer_equals_tower_http_apart_from_the_standard_rules() {
    let mut compared = 0;
    let mut identical = 0;
    let mut fixed = Vec::new();
    for config in CONFIGS {
        for case in CASES {
            let ours = run_ours(config, &case.method, case.headers).await;
            let expected = standard(config, case).await;
            assert_eq!(
                ours, expected,
                "config `{}`, request `{}`",
                config.name, case.name
            );
            compared += 1;
            let raw = run_theirs(config, &case.method, case.headers).await;
            if raw == ours {
                identical += 1;
            } else {
                fixed.push(format!("{} / {}", config.name, case.name));
            }
        }
    }
    println!(
        "{compared} (config, request) pairs: {identical} identical to tower-http, {} differ by the standard rules",
        fixed.len()
    );
    // Both outcomes must be exercised: plain agreement on allowed simple
    // requests, and the standard rules on everything else.
    assert!(identical >= 10, "{identical} of {compared}");
    assert!(!fixed.is_empty());
}

#[tokio::test]
async fn an_allowed_simple_request_and_preflight_are_identical_to_tower_http() {
    // The cases the browser-facing protocol is about: apart from `Vary`,
    // no rule applies to an allowed simple request or preflight.
    let config = &CONFIGS[2];
    for case in [&CASES[0], &CASES[8]] {
        let mut ours = run_ours(config, &case.method, case.headers).await;
        let mut theirs = run_theirs(config, &case.method, case.headers).await;
        ours.retain(|line| !line.starts_with("vary: origin"));
        theirs.retain(|line| !line.starts_with("vary: origin"));
        assert_eq!(ours, theirs, "{}", case.name);
    }
}
