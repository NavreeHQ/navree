//! Safe retries of writes (CONN-103): a `POST` or `PATCH` that carries an
//! idempotency key is retried like an idempotent method, with the same key,
//! the same body, backoff and jitter; `Retry-After` is honoured on every
//! retried request.

use std::time::Duration;

use http::{Method, StatusCode};
use navree_http_client::{FakeResponse, HttpClient, HttpClientError, HttpClientOptions};
use serde_json::json;
use tokio::time::Instant;

// CONN-103
#[tokio::test(start_paused = true)]
async fn a_post_with_an_idempotency_key_is_retried_with_the_same_key_and_body() {
    let (client, server) = HttpClient::fake();
    server.on_sequence(
        Method::POST,
        "https://api.example.com/charges",
        [
            FakeResponse::new(503),
            FakeResponse::connection_failed(),
            FakeResponse::new(200).json(&json!({ "id": "ch_1" })),
        ],
    );

    let response = client
        .post("https://api.example.com/charges")
        .idempotency_key("order-42-attempt-1")
        .json(&json!({ "amount": 149_900, "currency": "inr" }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let recorded = server.recorded();
    assert_eq!(recorded.len(), 3, "two retries");
    for request in &recorded {
        assert_eq!(
            request.header("idempotency-key"),
            Some("order-42-attempt-1")
        );
        assert_eq!(
            request.json::<serde_json::Value>().unwrap(),
            json!({ "amount": 149_900, "currency": "inr" })
        );
    }
}

// CONN-103
#[tokio::test(start_paused = true)]
async fn a_patch_with_an_idempotency_key_is_retried() {
    let (client, server) = HttpClient::fake();
    server.on_sequence(
        Method::PATCH,
        "*",
        [FakeResponse::new(500), FakeResponse::new(200)],
    );

    let response = client
        .patch("https://api.example.com/orders/42")
        .idempotency_key("order-42-ship")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(server.recorded().len(), 2);
}

// CONN-103
#[tokio::test(start_paused = true)]
async fn a_post_without_a_key_is_still_sent_once() {
    let (client, server) = HttpClient::fake();
    server.on(Method::POST, "*", FakeResponse::new(503));

    let response = client
        .post("https://api.example.com/charges")
        .retries(5)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    server.assert_sent_count(1);
    server.assert_not_sent(|request| request.header("idempotency-key").is_some());
}

// CONN-103
#[tokio::test(start_paused = true)]
async fn a_provider_header_name_can_carry_the_key() {
    let (client, server) = HttpClient::fake();
    server.on_sequence(
        Method::POST,
        "*",
        [FakeResponse::new(502), FakeResponse::new(201)],
    );

    let response = client
        .post("https://api.example.com/pg/orders")
        .idempotency_key_header("x-idempotency-key", "rcpt-2026-0042")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    let recorded = server.recorded();
    assert_eq!(recorded.len(), 2);
    for request in &recorded {
        assert_eq!(request.header("x-idempotency-key"), Some("rcpt-2026-0042"));
        assert_eq!(request.header("idempotency-key"), None);
    }
}

// CONN-103
#[tokio::test(start_paused = true)]
async fn a_keyed_post_backs_off_with_jitter() {
    let options = HttpClientOptions {
        retry_base_ms: 200,
        ..HttpClientOptions::default()
    };
    let (client, server) = HttpClient::fake_with(options);
    server.on(Method::POST, "*", FakeResponse::new(503));
    let started = Instant::now();

    let response = client
        .post("https://api.example.com/charges")
        .idempotency_key("k-1")
        .retries(3)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    server.assert_sent_count(4);
    // 200, 400 and 800 ms, each plus up to as much again in jitter.
    let spent = started.elapsed();
    assert!(spent >= Duration::from_millis(1_400), "{spent:?}");
    assert!(spent < Duration::from_millis(2_800), "{spent:?}");
}

// CONN-103
#[tokio::test(start_paused = true)]
async fn a_keyed_post_answered_with_a_client_error_is_not_retried() {
    let (client, server) = HttpClient::fake();
    server.on(Method::POST, "*", FakeResponse::new(400));

    let response = client
        .post("https://api.example.com/charges")
        .idempotency_key("k-2")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    server.assert_sent_count(1);
}

// CONN-103
#[tokio::test(start_paused = true)]
async fn a_timed_out_keyed_post_is_not_retried() {
    let (client, server) = HttpClient::fake();
    server.on(
        Method::POST,
        "*",
        FakeResponse::new(200).delay(Duration::from_secs(10)),
    );

    let error = client
        .post("https://api.example.com/charges")
        .idempotency_key("k-3")
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .unwrap_err();

    assert!(
        matches!(error, HttpClientError::Timeout { .. }),
        "{error:?}"
    );
    server.assert_sent_count(1);
}

// CONN-103
#[tokio::test(start_paused = true)]
async fn an_invalid_idempotency_key_fails_the_request_before_sending() {
    let (client, server) = HttpClient::fake();
    server.on(Method::POST, "*", FakeResponse::new(200));

    let too_long = "k".repeat(256);
    for key in ["", "two words", "line\nbreak", too_long.as_str()] {
        let error = client
            .post("https://api.example.com/charges")
            .idempotency_key(key)
            .send()
            .await
            .unwrap_err();
        assert!(
            matches!(error, HttpClientError::Build(_)),
            "{key:?}: {error:?}"
        );
    }
    server.assert_nothing_sent();
}

// CONN-103
#[tokio::test(start_paused = true)]
async fn retry_after_in_seconds_is_waited_for() {
    let (client, server) = HttpClient::fake();
    server.on_sequence(
        Method::GET,
        "*",
        [
            FakeResponse::new(429).header("retry-after", "5"),
            FakeResponse::new(200),
        ],
    );
    let started = Instant::now();

    let response = client.get("https://api.example.com/").send().await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    server.assert_sent_count(2);
    // The server's 5 s, not the 200 ms backoff.
    let spent = started.elapsed();
    assert!(spent >= Duration::from_secs(5), "{spent:?}");
    assert!(spent < Duration::from_millis(5_500), "{spent:?}");
}

// CONN-103
#[tokio::test(start_paused = true)]
async fn retry_after_on_a_keyed_post_is_waited_for() {
    let (client, server) = HttpClient::fake();
    server.on_sequence(
        Method::POST,
        "*",
        [
            FakeResponse::new(503).header("retry-after", "3"),
            FakeResponse::new(200),
        ],
    );
    let started = Instant::now();

    let response = client
        .post("https://api.example.com/charges")
        .idempotency_key("k-4")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert!(started.elapsed() >= Duration::from_secs(3));
}

// CONN-103
#[tokio::test(start_paused = true)]
async fn a_short_retry_after_never_shortens_the_backoff() {
    let options = HttpClientOptions {
        retry_base_ms: 2_000,
        ..HttpClientOptions::default()
    };
    let (client, server) = HttpClient::fake_with(options);
    server.on_sequence(
        Method::GET,
        "*",
        [
            FakeResponse::new(503).header("retry-after", "0"),
            FakeResponse::new(200),
        ],
    );
    let started = Instant::now();

    client.get("https://api.example.com/").send().await.unwrap();

    assert!(started.elapsed() >= Duration::from_secs(2));
}

// CONN-103
#[tokio::test(start_paused = true)]
async fn a_retry_after_beyond_thirty_seconds_returns_the_response_at_once() {
    let (client, server) = HttpClient::fake();
    server.on(
        Method::GET,
        "*",
        FakeResponse::new(503).header("retry-after", "120"),
    );
    let started = Instant::now();

    let response = client
        .get("https://api.example.com/")
        .retries(5)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.header("retry-after"), Some("120"));
    server.assert_sent_count(1);
    assert_eq!(started.elapsed(), Duration::ZERO);
}

// CONN-103
#[tokio::test(start_paused = true)]
async fn retry_after_as_a_date_is_read() {
    // A date in the past asks for no wait: the backoff schedule decides.
    let (client, server) = HttpClient::fake();
    server.on_sequence(
        Method::GET,
        "https://past.example.com/*",
        [
            FakeResponse::new(503).header("retry-after", "Wed, 21 Oct 2015 07:28:00 GMT"),
            FakeResponse::new(200),
        ],
    );
    // A date far ahead is more than the client waits: no retry.
    server.on(
        Method::GET,
        "https://future.example.com/*",
        FakeResponse::new(503).header("retry-after", "Fri, 31 Dec 9999 23:59:59 GMT"),
    );
    let started = Instant::now();

    let past = client
        .get("https://past.example.com/")
        .send()
        .await
        .unwrap();
    assert_eq!(past.status(), StatusCode::OK);
    assert!(started.elapsed() < Duration::from_millis(400));
    let future = client
        .get("https://future.example.com/")
        .send()
        .await
        .unwrap();
    assert_eq!(future.status(), StatusCode::SERVICE_UNAVAILABLE);

    let recorded = server.recorded();
    let count = |host: &str| {
        recorded
            .iter()
            .filter(|r| r.url().host_str() == Some(host))
            .count()
    };
    assert_eq!(count("past.example.com"), 2);
    assert_eq!(count("future.example.com"), 1);
}

// CONN-103
#[tokio::test(start_paused = true)]
async fn an_unreadable_retry_after_is_ignored() {
    let (client, server) = HttpClient::fake();
    server.on_sequence(
        Method::GET,
        "*",
        [
            FakeResponse::new(503).header("retry-after", "soon"),
            FakeResponse::new(200),
        ],
    );
    let started = Instant::now();

    let response = client.get("https://api.example.com/").send().await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert!(started.elapsed() < Duration::from_millis(400));
}
