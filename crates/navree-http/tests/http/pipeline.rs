//! The fixed global layers: CORS, panic catcher, timeout, security headers,
//! compression, maintenance mode, per-IP rate limit, body limit.

use std::collections::BTreeMap;
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use http::{Method, Request, StatusCode};
use navree_http::{AppError, HttpOptions, RouteDef};

use crate::support::{ORIGIN, Setup, bytes, from_origin, options};

fn error_routes() -> Vec<RouteDef> {
    vec![
        RouteDef::new(
            Method::GET,
            "/panic",
            get(|| async {
                if true {
                    panic!("handler exploded");
                }
                "unreachable"
            }),
        )
        .public(),
        RouteDef::new(
            Method::GET,
            "/slow-route",
            get(|| async {
                tokio::time::sleep(Duration::from_secs(5)).await;
                "late"
            }),
        )
        .public()
        .timeout(Duration::from_millis(50)),
        RouteDef::new(
            Method::GET,
            "/slow",
            get(|| async {
                tokio::time::sleep(Duration::from_secs(5)).await;
                "late"
            }),
        )
        .public(),
        RouteDef::new(
            Method::GET,
            "/invalid",
            get(|| async {
                let mut errors = BTreeMap::new();
                errors.insert("name".to_owned(), vec!["Required.".to_owned()]);
                AppError::validation(errors)
            }),
        )
        .public(),
    ]
}

fn assert_cors(response: &crate::support::TestResponse, what: &str) {
    assert_eq!(
        response.header("access-control-allow-origin"),
        Some(ORIGIN),
        "{what}: allow-origin"
    );
    assert_eq!(
        response.header("access-control-allow-credentials"),
        Some("true"),
        "{what}: allow-credentials"
    );
    let exposed = response
        .header("access-control-expose-headers")
        .unwrap_or_default()
        .to_ascii_lowercase();
    assert!(
        exposed.contains("x-request-id"),
        "{what}: exposes x-request-id"
    );
}

/// Review Focus 4: error responses keep CORS and the request id.
#[tokio::test]
async fn cors_headers_on_error_responses() {
    let app = Setup::new(error_routes())
        .options(HttpOptions {
            timeout_secs: 1,
            ..options()
        })
        .boot()
        .await;

    let cases = [
        ("/api/panic", 500, "panic"),
        ("/api/slow-route", 503, "route timeout"),
        ("/api/slow", 503, "global timeout"),
        ("/api/nope", 404, "404"),
        ("/api/invalid", 400, "400"),
    ];
    for (path, status, what) in cases {
        let response = app.send(from_origin("GET", path, ORIGIN)).await;
        response.assert_error(status);
        assert_cors(&response, what);
    }
}

#[tokio::test]
async fn panics_and_timeouts_are_json_errors() {
    let app = Setup::new(error_routes()).boot().await;

    let body = app.get("/api/panic").await.assert_error(500);
    assert_eq!(
        body,
        serde_json::json!({
            "statusCode": 500,
            "message": "Internal server error",
            "error": "Internal Server Error",
            "code": "internal_error",
        }),
        "panic message not sent"
    );

    let response = app.get("/api/slow-route").await;
    let body = response.assert_error(503);
    assert_eq!(body["error"], "Service Unavailable");
    assert_eq!(body["code"], "request_timeout");
    assert_eq!(
        body["message"],
        "The server did not finish the request in time."
    );
}

#[tokio::test]
async fn cors_rejects_unknown_origin() {
    let app = Setup::new(error_routes()).boot().await;

    let response = app
        .send(from_origin("GET", "/api/invalid", "https://evil.example"))
        .await;
    // Without Access-Control-Allow-Origin the browser blocks the response.
    assert_eq!(response.header("access-control-allow-origin"), None);
    response.assert_error(400);

    let preflight = Request::builder()
        .method(Method::OPTIONS)
        .uri("/api/invalid")
        .header("origin", "https://evil.example")
        .header("access-control-request-method", "POST")
        .body(Body::empty())
        .unwrap();
    let response = app.send(preflight).await;
    assert_eq!(response.header("access-control-allow-origin"), None);
}

#[tokio::test]
async fn cors_preflight_lists_explicit_headers_for_allowed_origin() {
    let app = Setup::new(error_routes()).boot().await;

    let preflight = Request::builder()
        .method(Method::OPTIONS)
        .uri("/api/invalid")
        .header("origin", ORIGIN)
        .header("access-control-request-method", "POST")
        .header(
            "access-control-request-headers",
            "x-xsrf-token, content-type",
        )
        .body(Body::empty())
        .unwrap();
    let response = app.send(preflight).await;
    assert!(response.status.is_success(), "{:?}", response.status);
    assert_eq!(response.header("access-control-allow-origin"), Some(ORIGIN));
    assert_eq!(
        response.header("access-control-allow-credentials"),
        Some("true")
    );
    let allowed = response.header("access-control-allow-headers").unwrap();
    assert!(allowed.contains("x-xsrf-token"), "{allowed}");
    assert!(!allowed.contains('*'), "no wildcards with credentials");
    assert!(response.header("x-request-id").is_some());
}

#[tokio::test]
async fn cors_wildcard_origin_fails_boot() {
    let mut options = options();
    options.cors.allowed_origins = vec!["*".to_owned()];
    let error = Setup::new(error_routes())
        .options(options)
        .boot_error()
        .await;
    assert!(error.contains("allowed_origins"), "{error}");
}

#[tokio::test]
async fn security_headers_present() {
    let app = Setup::new(vec![
        RouteDef::new(Method::GET, "/ok", get(|| async { "ok" })).public(),
        RouteDef::new(
            Method::GET,
            "/docs",
            get(|| async { ([("content-security-policy", "default-src 'self'")], "docs") }),
        )
        .public(),
    ])
    .boot()
    .await;

    for path in ["/api/ok", "/api/missing"] {
        let response = app.get(path).await;
        assert_eq!(response.header("x-content-type-options"), Some("nosniff"));
        assert_eq!(response.header("x-frame-options"), Some("DENY"));
        assert_eq!(response.header("referrer-policy"), Some("no-referrer"));
        assert!(
            response
                .header("strict-transport-security")
                .unwrap()
                .starts_with("max-age=")
        );
        assert!(
            response
                .header("content-security-policy")
                .unwrap()
                .contains("default-src 'none'")
        );
    }

    let response = app.get("/api/docs").await;
    assert_eq!(
        response.header("content-security-policy"),
        Some("default-src 'self'"),
        "a route's own header wins"
    );

    let off = Setup::new(vec![
        RouteDef::new(Method::GET, "/ok", get(|| async { "ok" })).public(),
    ])
    .options(HttpOptions {
        security_headers: false,
        ..options()
    })
    .boot()
    .await;
    assert_eq!(off.get("/api/ok").await.header("x-frame-options"), None);
}

#[tokio::test]
async fn compression_skipped_for_event_stream() {
    let payload = "data: hello\n\n".repeat(200);
    let sse_payload = payload.clone();
    let app = Setup::new(vec![
        RouteDef::new(
            Method::GET,
            "/events",
            get(move || {
                let body = sse_payload.clone();
                async move { ([("content-type", "text/event-stream")], body).into_response() }
            }),
        )
        .public(),
        RouteDef::new(
            Method::GET,
            "/big",
            get(move || {
                let body = payload.clone();
                async move { axum::Json(serde_json::json!({ "data": body })) }
            }),
        )
        .public(),
    ])
    .boot()
    .await;

    let gzip = |path: &str| {
        Request::get(path)
            .header("accept-encoding", "gzip")
            .body(Body::empty())
            .unwrap()
    };
    let json = app.send(gzip("/api/big")).await;
    assert_eq!(json.header("content-encoding"), Some("gzip"));

    let events = app.send(gzip("/api/events")).await;
    assert_eq!(events.status, StatusCode::OK);
    assert_eq!(events.header("content-encoding"), None, "SSE must stream");
    assert!(events.text().starts_with("data: hello"));
}

#[tokio::test]
async fn maintenance_mode_returns_503() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("down");
    std::fs::write(&file, r#"{"retry": 120, "message": "Back soon."}"#).unwrap();
    let app = Setup::new(vec![
        RouteDef::new(Method::GET, "/ok", get(|| async { "ok" })).public(),
    ])
    .options(HttpOptions {
        maintenance_file: file.clone(),
        ..options()
    })
    .boot()
    .await;

    let response = app.send(from_origin("GET", "/api/ok", ORIGIN)).await;
    let body = response.assert_error(503);
    assert_eq!(body["message"], "Back soon.");
    assert_eq!(body["error"], "Service Unavailable");
    assert_eq!(body["code"], "maintenance");
    assert_eq!(response.header("retry-after"), Some("120"));
    assert_eq!(response.header("access-control-allow-origin"), Some(ORIGIN));
}

#[tokio::test]
async fn maintenance_mode_skips_excepted_paths() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("down");
    std::fs::write(&file, "{}").unwrap();
    let routes = || {
        vec![
            RouteDef::new(Method::GET, "!/health/live", get(|| async { "live" })).public(),
            RouteDef::new(Method::GET, "/ok", get(|| async { "ok" })).public(),
            RouteDef::new(Method::GET, "/status", get(|| async { "status" })).public(),
        ]
    };

    // By default the liveness probe stays up, so an orchestrator does not
    // restart instances that are only in maintenance.
    let app = Setup::new(routes())
        .options(HttpOptions {
            maintenance_file: file.clone(),
            ..options()
        })
        .boot()
        .await;
    assert_eq!(app.get("/health/live").await.status, StatusCode::OK);
    app.get("/api/ok").await.assert_error(503);

    // An explicit list replaces the default; matching is on the exact path.
    let app = Setup::new(routes())
        .options(HttpOptions {
            maintenance_file: file.clone(),
            maintenance_except: vec!["/api/status".into()],
            ..options()
        })
        .boot()
        .await;
    assert_eq!(app.get("/api/status").await.status, StatusCode::OK);
    app.get("/api/status/").await.assert_error(503);
    app.get("/health/live").await.assert_error(503);
}

#[test]
fn maintenance_except_reads_a_comma_separated_environment_value() {
    let options: HttpOptions = serde_json::from_value(serde_json::json!({
        "maintenance_except": "/health/live, /up"
    }))
    .unwrap();
    assert_eq!(options.maintenance_except, ["/health/live", "/up"]);
    assert_eq!(
        HttpOptions::default().maintenance_except,
        ["/health/live", "/health/ready", "/metrics"]
    );
}

#[tokio::test]
async fn down_and_up_commands_toggle_maintenance() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("framework").join("down");
    let app = Setup::new(vec![
        RouteDef::new(Method::GET, "/ok", get(|| async { "ok" })).public(),
    ])
    .options(HttpOptions {
        maintenance_file: file.clone(),
        ..options()
    })
    .boot()
    .await;
    assert_eq!(app.get("/api/ok").await.status, StatusCode::OK);

    app.app
        .run_command("down", vec!["--retry".into(), "30".into()])
        .await
        .unwrap();
    assert!(file.exists());
    let response = app.get("/api/ok").await;
    response.assert_error(503);
    assert_eq!(response.header("retry-after"), Some("30"));

    app.app.run_command("up", vec![]).await.unwrap();
    assert!(!file.exists());
    assert_eq!(app.get("/api/ok").await.status, StatusCode::OK);
}

#[tokio::test]
async fn per_ip_rate_limit_returns_429_with_retry_after() {
    let app = Setup::new(vec![
        RouteDef::new(Method::GET, "/ok", get(|| async { "ok" })).public(),
    ])
    .options(HttpOptions {
        per_ip_rate_limit: Some(2),
        ..options()
    })
    .boot()
    .await;

    assert_eq!(app.get("/api/ok").await.status, StatusCode::OK);
    assert_eq!(app.get("/api/ok").await.status, StatusCode::OK);
    let response = app.send(from_origin("GET", "/api/ok", ORIGIN)).await;
    let body = response.assert_error(429);
    assert_eq!(body["message"], "Too Many Requests");
    assert_eq!(body["error"], "Too Many Requests");
    assert_eq!(body["code"], "too_many_requests");
    let retry_after: u64 = response.header("retry-after").unwrap().parse().unwrap();
    assert!((1..=60).contains(&retry_after), "retry after {retry_after}");
    assert_eq!(response.header("access-control-allow-origin"), Some(ORIGIN));
}

#[tokio::test]
async fn body_limit_returns_413() {
    let app = Setup::new(vec![
        RouteDef::new(
            Method::POST,
            "/echo",
            post(|body: Bytes| async move { body.len().to_string() }),
        )
        .public(),
        RouteDef::new(
            Method::POST,
            "/ignore",
            post(|| async { "ignored the body" }),
        )
        .public(),
        RouteDef::new(
            Method::POST,
            "/upload",
            post(|body: Bytes| async move { body.len().to_string() }),
        )
        .public()
        .body_limit(1024),
    ])
    .options(HttpOptions {
        body_limit_bytes: 16,
        ..options()
    })
    .boot()
    .await;

    // Declared length over the limit: rejected before the handler runs,
    // even when the handler never reads the body.
    let declared = Request::post("/api/ignore")
        .header("content-length", "100")
        .body(Body::from(bytes(100)))
        .unwrap();
    let body = app.send(declared).await.assert_error(413);
    assert_eq!(body["error"], "Payload Too Large");
    assert_eq!(body["code"], "payload_too_large");

    // Streamed body without a length: the extractor's limit applies.
    let streamed = Request::post("/api/echo")
        .body(Body::from_stream(futures_util::stream::iter([Ok::<
            _,
            std::io::Error,
        >(
            bytes(100),
        )])))
        .unwrap();
    app.send(streamed).await.assert_error(413);

    // Under the limit, and a route with a larger limit.
    let small = Request::post("/api/echo")
        .body(Body::from(bytes(8)))
        .unwrap();
    assert_eq!(app.send(small).await.text(), "8");
    let upload = Request::post("/api/upload")
        .header("content-length", "100")
        .body(Body::from(bytes(100)))
        .unwrap();
    assert_eq!(app.send(upload).await.text(), "100");
}

#[test]
fn client_ip_of_a_request_follows_the_proxy_settings() {
    use std::net::{IpAddr, SocketAddr};

    use axum::extract::ConnectInfo;
    use navree_http::ClientIp;

    let mut request = Request::get("/")
        .header("x-forwarded-for", "203.0.113.9")
        .body(Body::empty())
        .unwrap();
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 4000))));
    let (parts, _) = request.into_parts();
    let proxy: IpAddr = [127, 0, 0, 1].into();
    let client: IpAddr = [203, 0, 113, 9].into();

    let direct = ClientIp::of(&parts.headers, &parts.extensions, &HttpOptions::default());
    assert_eq!(direct, ClientIp(proxy), "untrusted peers name themselves");
    let behind_proxy = HttpOptions {
        trusted_proxies: vec!["127.0.0.1".to_owned()],
        ..HttpOptions::default()
    };
    let forwarded = ClientIp::of(&parts.headers, &parts.extensions, &behind_proxy);
    assert_eq!(forwarded, ClientIp(client));
}
