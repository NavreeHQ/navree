//! CSRF protection: the `XSRF-TOKEN` cookie endpoint and the 403 check on
//! cookie-authenticated writes.

use keel_testing::cookie::SameSite;
use keel_testing::cookie::time::Duration;
use serde_json::json;

use crate::support::{self, COOKIE, XSRF};

#[tokio::test]
async fn csrf_cookie_endpoint_sets_xsrf_token() {
    let app = support::app().await;
    let client = app.client();

    let response = client
        .get("/sanctum/csrf-cookie")
        .send()
        .await
        .assert_status(204);
    let xsrf = response
        .set_cookie(XSRF)
        .expect("the XSRF-TOKEN cookie is set");
    assert_ne!(xsrf.http_only(), Some(true), "JavaScript must read it");
    assert_eq!(xsrf.path(), Some("/"));
    assert_eq!(xsrf.same_site(), Some(SameSite::Lax));
    assert_eq!(xsrf.max_age(), Some(Duration::minutes(120)));
    assert_eq!(xsrf.value().len(), 40);
    assert!(
        response.set_cookie(COOKIE).is_some(),
        "the token lives in a new session"
    );

    let body = client
        .get("/api/probe/token")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(
        body["token"],
        json!(xsrf.value()),
        "the cookie carries the session's token"
    );

    // Asking again keeps the token and hands the cookie out again.
    let again = client
        .get("/sanctum/csrf-cookie")
        .send()
        .await
        .assert_status(204);
    assert_eq!(again.set_cookie(XSRF).unwrap().value(), xsrf.value());
}

#[tokio::test]
async fn post_with_session_without_xsrf_is_403() {
    let app = support::app().await;
    let client = app.client();
    client.csrf_cookie().await;

    let response = client
        .post("/api/probe/write")
        .json(&json!({}))
        .send()
        .await
        .assert_error(403, "Invalid CSRF token");
    assert_eq!(
        response.json_value(),
        json!({"statusCode": 403, "message": "Invalid CSRF token", "error": "Forbidden"})
    );
    assert!(response.header("x-request-id").is_some());
}

#[tokio::test]
async fn post_with_wrong_xsrf_is_403() {
    let app = support::app().await;
    let client = app.client();
    let token = client.csrf_cookie().await.set_cookie(XSRF).unwrap();

    let mut wrong = token.value().to_owned();
    let last = wrong.pop().unwrap();
    wrong.push(if last == 'a' { 'b' } else { 'a' });
    for (header, value) in [
        ("x-xsrf-token", wrong.as_str()),
        ("x-csrf-token", wrong.as_str()),
        ("x-xsrf-token", ""),
        ("x-csrf-token", &token.value()[..39]),
    ] {
        client
            .post("/api/probe/write")
            .header(header, value)
            .send()
            .await
            .assert_error(403, "Invalid CSRF token");
    }

    // Another session's token does not fit this session.
    let other = app.client();
    let other_token = other.csrf_cookie().await.set_cookie(XSRF).unwrap();
    client
        .post("/api/probe/write")
        .header("x-xsrf-token", other_token.value())
        .send()
        .await
        .assert_error(403, "Invalid CSRF token");
}

#[tokio::test]
async fn post_with_matching_xsrf_passes() {
    let app = support::app().await;
    let client = app.client();
    let token = client.csrf_cookie().await.set_cookie(XSRF).unwrap();

    client
        .post("/api/probe/write")
        .xsrf()
        .send()
        .await
        .assert_status(200);
    client
        .post("/api/probe/write")
        .header("x-csrf-token", token.value())
        .send()
        .await
        .assert_status(200);
    for method in ["PUT", "PATCH", "DELETE"] {
        let method = method.parse().unwrap();
        client
            .request(method, "/api/probe/write")
            .xsrf()
            .send()
            .await
            .assert_status(200);
    }
}

#[tokio::test]
async fn xsrf_header_is_url_decoded() {
    let app = support::app().await;
    let client = app.client();
    let token = client.csrf_cookie().await.set_cookie(XSRF).unwrap();
    let encoded: String = token.value().bytes().map(|b| format!("%{b:02X}")).collect();
    client
        .post("/api/probe/write")
        .header("x-xsrf-token", encoded.as_str())
        .send()
        .await
        .assert_status(200);
}

#[tokio::test]
async fn every_unsafe_method_is_checked() {
    let app = support::app().await;
    let client = app.client();
    client.csrf_cookie().await;
    for method in ["POST", "PUT", "PATCH", "DELETE"] {
        client
            .request(method.parse().unwrap(), "/api/probe/write")
            .send()
            .await
            .assert_error(403, "Invalid CSRF token");
    }
}

#[tokio::test]
async fn bearer_request_skips_csrf() {
    let app = support::app().await;
    let client = app.client();
    client.csrf_cookie().await;
    client
        .post("/api/probe/write")
        .bearer("1|plain-text-token")
        .send()
        .await
        .assert_status(200);
    client
        .post("/api/probe/write")
        .header("authorization", "Basic dXNlcjpwYXNz")
        .send()
        .await
        .assert_error(403, "Invalid CSRF token");
    client
        .post("/api/probe/write")
        .header("authorization", "Bearer ")
        .send()
        .await
        .assert_error(403, "Invalid CSRF token");
}

#[tokio::test]
async fn get_is_never_csrf_checked() {
    let app = support::app().await;
    let client = app.client();
    client.csrf_cookie().await;
    client
        .get("/api/probe/token")
        .send()
        .await
        .assert_status(200);
    client
        .get("/api/probe/token")
        .header("x-xsrf-token", "wrong")
        .send()
        .await
        .assert_status(200);
    client
        .request("HEAD".parse().unwrap(), "/api/probe/token")
        .send()
        .await
        .assert_status(200);
}

#[tokio::test]
async fn post_without_session_cookie_is_not_csrf_checked() {
    let app = support::app().await;
    let client = app.client();
    client
        .post("/api/probe/write")
        .send()
        .await
        .assert_status(200);
    // Other cookies do not count.
    client.set_cookie("theme", "dark");
    client
        .post("/api/probe/write")
        .send()
        .await
        .assert_status(200);
}

#[tokio::test]
async fn a_stale_session_cookie_still_needs_the_token() {
    let app = support::app().await;
    let client = app.client();
    client.set_cookie(COOKIE, "v1.not-a-valid.cookie.value");
    client
        .post("/api/probe/write")
        .send()
        .await
        .assert_error(403, "Invalid CSRF token");
    // The handshake replaces the stale cookie, and the write then passes.
    client.csrf_cookie().await;
    client
        .post("/api/probe/write")
        .xsrf()
        .send()
        .await
        .assert_status(200);
}

#[tokio::test]
async fn csrf_except_path_skips() {
    let app = support::app_with(keel_session::SessionOptions {
        csrf_except: vec!["/webhooks/*".into(), "api/probe/put".into()],
        ..keel_session::SessionOptions::default()
    })
    .await;
    let client = app.client();
    client.csrf_cookie().await;

    client
        .post("/webhooks/incoming")
        .send()
        .await
        .assert_status(200);
    client
        .post("/api/probe/put")
        .json(&json!({ "key": "k", "value": 1 }))
        .send()
        .await
        .assert_status(200);
    client
        .post("/api/probe/write")
        .send()
        .await
        .assert_error(403, "Invalid CSRF token");
}

#[tokio::test]
async fn a_failed_check_does_not_save_the_session() {
    let app = support::app().await;
    let client = app.client();
    client.csrf_cookie().await;
    let response = client
        .post("/api/probe/put")
        .json(&json!({ "key": "k", "value": 1 }))
        .send()
        .await
        .assert_error(403, "Invalid CSRF token");
    assert!(response.set_cookie(COOKIE).is_none());
    let (_, value) = support::get(&client, "k").await;
    assert_eq!(value, json!(null), "the handler did not run");
}

#[tokio::test]
async fn cross_site_browser_writes_without_a_session_are_rejected() {
    // Login CSRF: a cross-site form must not sign the victim in, even
    // though the victim's browser has no session cookie yet.
    let app = support::base()
        .config(
            "http.cors.allowed_origins",
            json!(["http://localhost:3000"]),
        )
        .module(keel_session::SessionModule::for_root(
            keel_session::SessionOptions {
                csrf_except: vec!["/webhooks/*".into()],
                ..keel_session::SessionOptions::default()
            },
        ))
        .build()
        .await;
    let client = app.client();
    let response = client
        .post("/api/probe/write")
        .header("sec-fetch-site", "cross-site")
        .header("origin", "https://evil.example")
        .send()
        .await
        .assert_error(403, "Invalid CSRF token");
    assert!(response.set_cookie(COOKIE).is_none());

    // An allowed front-end origin, same-site and same-origin requests,
    // non-browser clients and exempt paths pass.
    client
        .post("/api/probe/write")
        .header("sec-fetch-site", "cross-site")
        .header("origin", "http://localhost:3000")
        .send()
        .await
        .assert_status(200);
    for site in ["same-site", "same-origin", "none"] {
        client
            .post("/api/probe/write")
            .header("sec-fetch-site", site)
            .header("origin", "https://evil.example")
            .send()
            .await
            .assert_status(200);
    }
    client
        .post("/api/probe/write")
        .header("origin", "https://evil.example")
        .send()
        .await
        .assert_status(200);
    client
        .post("/webhooks/incoming")
        .header("sec-fetch-site", "cross-site")
        .header("origin", "https://payments.example")
        .send()
        .await
        .assert_status(200);
    client
        .post("/api/probe/write")
        .header("sec-fetch-site", "cross-site")
        .header("origin", "https://evil.example")
        .bearer("1|token")
        .send()
        .await
        .assert_status(200);
    client
        .get("/api/probe/untouched")
        .header("sec-fetch-site", "cross-site")
        .header("origin", "https://evil.example")
        .send()
        .await
        .assert_status(200);
}
