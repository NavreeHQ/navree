//! Fake mode: stubs, recording and assertions (never touches the network).

use http::{Method, StatusCode};
use navis_contracts::RequestId;
use navis_http_client::{FakeResponse, HttpClient, HttpClientError, HttpClientOptions};
use serde_json::{Value, json};

#[tokio::test]
async fn fake_records_and_responds() {
    let (client, server) = HttpClient::fake();
    server.on(
        Method::POST,
        "https://api.example.com/users*",
        FakeResponse::new(201)
            .header("x-trace", "abc")
            .json(&json!({ "id": 7, "name": "Ada" })),
    );

    let response = client
        .post("https://api.example.com/users")
        .header("x-tenant", "acme")
        .json(&json!({ "name": "Ada" }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(response.header("x-trace"), Some("abc"));
    let body: Value = response.json().unwrap();
    assert_eq!(body["id"], 7);

    let recorded = server.recorded();
    assert_eq!(recorded.len(), 1);
    let request = &recorded[0];
    assert_eq!(request.method(), &Method::POST);
    assert_eq!(request.url().as_str(), "https://api.example.com/users");
    assert_eq!(request.header("x-tenant"), Some("acme"));
    assert_eq!(request.header("content-type"), Some("application/json"));
    assert_eq!(request.json::<Value>().unwrap(), json!({ "name": "Ada" }));
    server.assert_sent(|r| r.method() == Method::POST && r.url().path() == "/users");
}

#[tokio::test]
async fn request_id_header_propagated() {
    let (client, server) = HttpClient::fake();
    server.on(Method::GET, "*", FakeResponse::new(200));
    let id = RequestId::parse("req-123_abc").unwrap();

    client
        .get("https://api.example.com/ping")
        .with_request_id(&id)
        .send()
        .await
        .unwrap();

    server.assert_sent(|r| r.header("x-request-id") == Some("req-123_abc"));
}

#[tokio::test]
async fn bearer_query_and_form_are_encoded() {
    let (client, server) = HttpClient::fake();
    server.on(
        Method::POST,
        "https://api.example.com/token?*",
        FakeResponse::new(200),
    );

    client
        .post("https://api.example.com/token")
        .bearer("s3cret")
        .query(&[("scope", "read write"), ("x", "1")])
        .form(&[("grant_type", "client_credentials")])
        .send()
        .await
        .unwrap();

    let request = &server.recorded()[0];
    assert_eq!(
        request.url().as_str(),
        "https://api.example.com/token?scope=read+write&x=1"
    );
    assert_eq!(request.header("authorization"), Some("Bearer s3cret"));
    assert_eq!(
        request.header("content-type"),
        Some("application/x-www-form-urlencoded")
    );
    assert_eq!(request.text(), "grant_type=client_credentials");
    assert!(
        !format!("{request:?}").contains("s3cret"),
        "the bearer token must not appear in Debug output"
    );
}

#[tokio::test]
async fn user_agent_sent_by_default() {
    let options = HttpClientOptions {
        user_agent: "crm/1.0".into(),
        ..HttpClientOptions::default()
    };
    let (client, server) = HttpClient::fake_with(options);
    server.on(Method::GET, "*", FakeResponse::new(204));

    client.get("https://example.com/").send().await.unwrap();
    client
        .get("https://example.com/")
        .header("user-agent", "custom")
        .send()
        .await
        .unwrap();

    let recorded = server.recorded();
    assert_eq!(recorded[0].header("user-agent"), Some("crm/1.0"));
    assert_eq!(recorded[1].header("user-agent"), Some("custom"));
}

#[tokio::test]
async fn unmatched_fake_request_errors_without_network() {
    let (client, server) = HttpClient::fake();
    server.on(
        Method::GET,
        "https://api.example.com/*",
        FakeResponse::new(200),
    );

    let error = client
        .post("https://api.example.com/users?token=secret")
        .send()
        .await
        .unwrap_err();

    assert!(
        matches!(error, HttpClientError::NoFakeResponse { .. }),
        "{error:?}"
    );
    assert!(!error.to_string().contains("secret"), "{error}");
    assert_eq!(
        server.recorded().len(),
        1,
        "unmatched requests are still recorded"
    );
}

#[tokio::test]
async fn response_text_bytes_and_status_helpers() {
    let (client, server) = HttpClient::fake();
    server.on(
        Method::GET,
        "https://example.com/missing",
        FakeResponse::new(404).text("not here"),
    );

    let response = client
        .get("https://example.com/missing")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(!response.is_success());
    assert_eq!(response.text(), "not here");
    assert_eq!(&response.bytes()[..], b"not here");
    assert_eq!(response.url().as_str(), "https://example.com/missing");
    let error = response.error_for_status().unwrap_err();
    assert!(
        matches!(error, HttpClientError::Status { status, .. } if status == StatusCode::NOT_FOUND),
        "{error:?}"
    );
}

#[tokio::test]
async fn invalid_url_is_a_build_error() {
    let (client, server) = HttpClient::fake();

    let error = client.get("not a url").send().await.unwrap_err();

    assert!(matches!(error, HttpClientError::Build(_)), "{error:?}");
    assert!(server.recorded().is_empty());
}

#[tokio::test]
#[should_panic(expected = "no recorded request matches")]
async fn assert_sent_panics_when_nothing_matches() {
    let (client, server) = HttpClient::fake();
    server.on(Method::GET, "*", FakeResponse::new(200));
    client.get("https://example.com/a").send().await.unwrap();

    server.assert_sent(|r| r.url().path() == "/b");
}

#[tokio::test]
async fn oversized_fake_body_is_refused() {
    let options = HttpClientOptions {
        max_response_bytes: 4,
        ..HttpClientOptions::default()
    };
    let (client, server) = HttpClient::fake_with(options);
    server.on(Method::GET, "*", FakeResponse::new(200).text("too long"));

    let error = client
        .get("https://example.com/big")
        .send()
        .await
        .unwrap_err();

    assert!(
        matches!(error, HttpClientError::BodyTooLarge { limit: 4, .. }),
        "{error:?}"
    );
}

#[tokio::test]
async fn extra_assertion_helpers() {
    let (client, server) = HttpClient::fake();
    server.assert_nothing_sent();
    server.on(Method::GET, "*", FakeResponse::new(200));

    client.get("https://example.com/a").send().await.unwrap();
    client.get("https://example.com/a").send().await.unwrap();

    server.assert_sent_count(2);
    server.assert_not_sent(|r| r.url().path() == "/b");
}

#[tokio::test]
#[should_panic(expected = "expected 3 requests")]
async fn assert_sent_count_panics_on_mismatch() {
    let (client, server) = HttpClient::fake();
    server.on(Method::GET, "*", FakeResponse::new(200));
    client.get("https://example.com/a").send().await.unwrap();

    server.assert_sent_count(3);
}

#[tokio::test]
async fn debug_output_hides_credentials_query_and_body() {
    let (client, server) = HttpClient::fake();
    server.on(Method::POST, "*", FakeResponse::new(200));

    let builder = client
        .post("https://api.example.com/login?token=QUERYSECRET")
        .header("authorization", "Basic BASICSECRET")
        .header("cookie", "session=COOKIESECRET")
        .json(&json!({ "password": "BODYSECRET" }));
    let shown = format!("{builder:?}");
    builder.send().await.unwrap();
    let recorded = format!("{:?}", server.recorded()[0].headers());

    for secret in ["QUERYSECRET", "BASICSECRET", "COOKIESECRET", "BODYSECRET"] {
        assert!(
            !shown.contains(secret),
            "builder Debug leaks `{secret}`: {shown}"
        );
    }
    assert!(shown.contains("https://api.example.com/login"), "{shown}");
    for secret in ["BASICSECRET", "COOKIESECRET"] {
        assert!(
            !recorded.contains(secret),
            "headers Debug leaks `{secret}`: {recorded}"
        );
    }
}

#[test]
#[should_panic(expected = "timeout_secs")]
fn fake_with_rejects_invalid_options() {
    let _ = HttpClient::fake_with(HttpClientOptions {
        timeout_secs: 0,
        ..HttpClientOptions::default()
    });
}
