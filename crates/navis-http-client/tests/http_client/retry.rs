//! Retries: exponential backoff, idempotent methods only, on connect errors,
//! 5xx and 429.

use std::time::Duration;

use http::{Method, StatusCode};
use navis_http_client::{FakeResponse, HttpClient, HttpClientError};
use tokio::time::Instant;

#[tokio::test(start_paused = true)]
async fn retries_on_503_then_succeeds() {
    let (client, server) = HttpClient::fake();
    server.on_sequence(
        Method::GET,
        "https://api.example.com/flaky",
        [
            FakeResponse::new(503),
            FakeResponse::new(503),
            FakeResponse::new(200).text("finally"),
        ],
    );
    let started = Instant::now();

    let response = client
        .get("https://api.example.com/flaky")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.text(), "finally");
    assert_eq!(server.recorded().len(), 3);
    // Default backoff: 200 ms, then 400 ms (plus jitter).
    assert!(
        started.elapsed() >= Duration::from_millis(600),
        "{:?}",
        started.elapsed()
    );
}

#[tokio::test(start_paused = true)]
async fn does_not_retry_post() {
    let (client, server) = HttpClient::fake();
    server.on(Method::POST, "*", FakeResponse::new(503));

    let response = client
        .post("https://api.example.com/charges")
        .retries(5)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(server.recorded().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn does_not_retry_patch() {
    let (client, server) = HttpClient::fake();
    server.on(Method::PATCH, "*", FakeResponse::new(500));

    client
        .patch("https://api.example.com/x")
        .send()
        .await
        .unwrap();

    assert_eq!(server.recorded().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn retries_put_delete_and_429() {
    let (client, server) = HttpClient::fake();
    server.on(Method::PUT, "*", FakeResponse::new(502));
    server.on(Method::DELETE, "*", FakeResponse::new(429));

    let put = client
        .put("https://api.example.com/x")
        .send()
        .await
        .unwrap();
    let delete = client
        .delete("https://api.example.com/x")
        .send()
        .await
        .unwrap();

    assert_eq!(
        put.status(),
        StatusCode::BAD_GATEWAY,
        "the last response is returned"
    );
    assert_eq!(delete.status(), StatusCode::TOO_MANY_REQUESTS);
    let recorded = server.recorded();
    assert_eq!(
        recorded
            .iter()
            .filter(|r| r.method() == Method::PUT)
            .count(),
        3
    );
    assert_eq!(
        recorded
            .iter()
            .filter(|r| r.method() == Method::DELETE)
            .count(),
        3
    );
}

#[tokio::test(start_paused = true)]
async fn does_not_retry_client_errors() {
    let (client, server) = HttpClient::fake();
    server.on(Method::GET, "*", FakeResponse::new(404));

    let response = client
        .get("https://api.example.com/x")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(server.recorded().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn per_request_retries_override_the_default() {
    let (client, server) = HttpClient::fake();
    server.on(
        Method::GET,
        "https://a.example.com/*",
        FakeResponse::new(503),
    );
    server.on(
        Method::GET,
        "https://b.example.com/*",
        FakeResponse::new(503),
    );

    client
        .get("https://a.example.com/")
        .retries(0)
        .send()
        .await
        .unwrap();
    client
        .get("https://b.example.com/")
        .retries(4)
        .send()
        .await
        .unwrap();

    let recorded = server.recorded();
    let count = |host: &str| {
        recorded
            .iter()
            .filter(|r| r.url().host_str() == Some(host))
            .count()
    };
    assert_eq!(count("a.example.com"), 1);
    assert_eq!(count("b.example.com"), 5);
}

#[tokio::test(start_paused = true)]
async fn retries_connection_failures() {
    let (client, server) = HttpClient::fake();
    server.on_sequence(
        Method::GET,
        "https://a.example.com/*",
        [FakeResponse::connection_failed(), FakeResponse::new(200)],
    );
    server.on(
        Method::GET,
        "https://b.example.com/*",
        FakeResponse::connection_failed(),
    );

    let recovered = client.get("https://a.example.com/").send().await.unwrap();
    let failed = client
        .get("https://b.example.com/")
        .send()
        .await
        .unwrap_err();

    assert_eq!(recovered.status(), StatusCode::OK);
    assert!(
        matches!(failed, HttpClientError::Connect { .. }),
        "{failed:?}"
    );
    let recorded = server.recorded();
    assert_eq!(recorded.len(), 2 + 3);
}

#[tokio::test(start_paused = true)]
async fn slow_fake_response_times_out_without_retry() {
    let (client, server) = HttpClient::fake();
    server.on(
        Method::GET,
        "*",
        FakeResponse::new(200).delay(Duration::from_secs(10)),
    );
    let started = Instant::now();

    let error = client
        .get("https://api.example.com/slow?key=secret")
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .unwrap_err();

    assert!(
        matches!(error, HttpClientError::Timeout { .. }),
        "{error:?}"
    );
    assert!(!error.to_string().contains("secret"), "{error}");
    assert_eq!(
        server.recorded().len(),
        1,
        "a timed-out attempt is not retried"
    );
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[tokio::test(start_paused = true)]
async fn default_timeout_comes_from_options() {
    let options = navis_http_client::HttpClientOptions {
        timeout_secs: 1,
        ..Default::default()
    };
    let (client, server) = HttpClient::fake_with(options);
    server.on(
        Method::GET,
        "*",
        FakeResponse::new(200).delay(Duration::from_secs(5)),
    );

    let error = client
        .get("https://api.example.com/")
        .send()
        .await
        .unwrap_err();

    assert!(
        matches!(error, HttpClientError::Timeout { .. }),
        "{error:?}"
    );
}
