//! Retries and failover across models, against mock provider servers and
//! with the fake's scripted errors.

use std::time::Duration;

use navree_ai::{
    Ai, AiError, AiFake, AiOptions, Attachment, FakeReply, ModelConfig, ProviderConfig,
};
use navree_contracts::Secret;
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::support::{Sequence, json, provider_options};

fn chat_answer(text: &str) -> Value {
    json!({
        "choices": [{ "message": { "role": "assistant", "content": text }, "finish_reason": "stop" }],
        "usage": { "prompt_tokens": 10, "completion_tokens": 3 }
    })
}

/// Two OpenAI-compatible servers: `primary` (model `smart`, falling back
/// to `backup`) and `secondary`.
fn two_servers(primary: &MockServer, secondary: &MockServer) -> AiOptions {
    AiOptions {
        default: "smart".into(),
        timeout_secs: 5,
        retry_base_ms: 5,
        providers: [
            (
                "one".to_owned(),
                ProviderConfig::openai_compatible(primary.uri()),
            ),
            (
                "two".to_owned(),
                ProviderConfig::openai_compatible(secondary.uri()),
            ),
        ]
        .into(),
        models: [
            (
                "smart".to_owned(),
                ModelConfig::new("one", "big").fallbacks(["backup"]),
            ),
            ("backup".to_owned(), ModelConfig::new("two", "small")),
        ]
        .into(),
        ..AiOptions::default()
    }
}

#[tokio::test]
async fn a_5xx_is_retried_on_the_same_model() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(Sequence::new(vec![
            json(503, json!({ "error": { "message": "overloaded" } })),
            json(200, chat_answer("second try")),
        ]))
        .mount(&server)
        .await;
    let ai = Ai::new(provider_options("openai", &server.uri(), "fast", "gpt-5")).unwrap();

    let response = ai.text("hi").await.unwrap();

    assert_eq!(response.text, "second try");
    assert_eq!(response.model, "fast");
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn after_the_retries_the_fallback_answers() {
    let (primary, secondary) = (MockServer::start().await, MockServer::start().await);
    Mock::given(method("POST"))
        .respond_with(json(500, json!({ "error": { "message": "boom" } })))
        .mount(&primary)
        .await;
    Mock::given(method("POST"))
        .respond_with(json(200, chat_answer("from backup")))
        .mount(&secondary)
        .await;
    let ai = Ai::new(two_servers(&primary, &secondary)).unwrap();

    let response = ai.text("hi").await.unwrap();

    assert_eq!(response.text, "from backup");
    assert_eq!(response.model, "backup");
    assert_eq!(response.provider, "two");
    // One attempt and `max_retries` (2) retries on the first model.
    assert_eq!(primary.received_requests().await.unwrap().len(), 3);
}

#[tokio::test]
async fn when_every_model_fails_the_request_is_unavailable() {
    let (primary, secondary) = (MockServer::start().await, MockServer::start().await);
    for server in [&primary, &secondary] {
        Mock::given(method("POST"))
            .respond_with(json(502, json!({ "error": { "message": "down" } })))
            .mount(server)
            .await;
    }
    let ai = Ai::new(two_servers(&primary, &secondary)).unwrap();

    let error = ai.text("hi").await.unwrap_err();

    assert!(
        matches!(&error, AiError::Unavailable { provider, .. } if provider == "two"),
        "{error:?}"
    );
    let body = error.to_app_error().body();
    assert_eq!(
        body,
        json!({ "statusCode": 503, "message": "The AI provider is unavailable", "error": "Service Unavailable", "code": "service_unavailable" })
    );
}

#[tokio::test]
async fn rate_limits_honor_retry_after_and_end_as_rate_limited() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after-ms", "20")
                .set_body_json(json!({ "error": { "message": "slow down" } })),
        )
        .mount(&server)
        .await;
    let ai = Ai::new(provider_options("openai", &server.uri(), "fast", "gpt-5")).unwrap();

    let error = ai.text("hi").await.unwrap_err();

    assert_eq!(
        error,
        AiError::RateLimited {
            provider: "openai".into(),
            retry_after: Some(Duration::from_millis(20))
        }
    );
    assert_eq!(error.to_app_error().status.as_u16(), 429);
    assert_eq!(server.received_requests().await.unwrap().len(), 3);
}

#[tokio::test]
async fn an_invalid_request_fails_at_once_without_failover() {
    let (primary, secondary) = (MockServer::start().await, MockServer::start().await);
    Mock::given(method("POST"))
        .respond_with(json(
            400,
            json!({ "error": { "message": "prompt is too long" } }),
        ))
        .mount(&primary)
        .await;
    let ai = Ai::new(two_servers(&primary, &secondary)).unwrap();

    let error = ai.text("hi").await.unwrap_err();

    match &error {
        AiError::Request { provider, message } => {
            assert_eq!(provider, "one");
            assert!(message.contains("prompt is too long"));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(primary.received_requests().await.unwrap().len(), 1);
    assert!(secondary.received_requests().await.unwrap().is_empty());
    // The provider's text never reaches the client.
    assert!(!error.to_app_error().body().to_string().contains("too long"));
}

#[tokio::test]
async fn a_rejected_key_is_a_configuration_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(json(
            401,
            json!({ "error": { "message": "invalid api key" } }),
        ))
        .mount(&server)
        .await;
    let ai = Ai::new(provider_options("openai", &server.uri(), "fast", "gpt-5")).unwrap();

    let error = ai.text("hi").await.unwrap_err();

    assert!(matches!(error, AiError::Config { .. }), "{error:?}");
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn a_missing_key_fails_requests_not_the_boot() {
    let mut options = provider_options(
        "anthropic",
        "http://127.0.0.1:9",
        "fast",
        "claude-sonnet-5-5",
    );
    options.providers.get_mut("anthropic").unwrap().api_key = None;
    let ai = Ai::new(options).unwrap();

    let error = ai.text("hi").await.unwrap_err();

    match error {
        AiError::Config { message } => assert!(
            message.contains("APP_AI__PROVIDERS__ANTHROPIC__API_KEY"),
            "{message}"
        ),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn connection_failures_fail_over() {
    let secondary = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(json(200, chat_answer("up")))
        .mount(&secondary)
        .await;
    let mut options = AiOptions {
        default: "smart".into(),
        retry_base_ms: 1,
        max_retries: 0,
        timeout_secs: 2,
        providers: [
            (
                "one".to_owned(),
                ProviderConfig::openai_compatible("http://127.0.0.1:9"),
            ),
            (
                "two".to_owned(),
                ProviderConfig::openai_compatible(secondary.uri()),
            ),
        ]
        .into(),
        ..AiOptions::default()
    };
    options.models.insert(
        "smart".into(),
        ModelConfig::new("one", "a").fallbacks(["backup"]),
    );
    options
        .models
        .insert("backup".into(), ModelConfig::new("two", "b"));

    let response = Ai::new(options).unwrap().text("hi").await.unwrap();

    assert_eq!(response.model, "backup");
}

#[tokio::test]
async fn models_that_cannot_read_an_attachment_are_skipped() {
    let (primary, secondary) = (MockServer::start().await, MockServer::start().await);
    let mut options = two_servers(&primary, &secondary);
    // `one` speaks OpenAI (no PDFs); `backup` is Gemini-compatible here.
    options.providers.insert(
        "two".into(),
        ProviderConfig {
            driver: Some("gemini".into()),
            url: Some(secondary.uri()),
            api_key: Some(Secret::new("k".to_owned())),
            ..ProviderConfig::default()
        },
    );
    Mock::given(method("POST"))
        .respond_with(json(200, json!({
            "candidates": [{ "content": { "role": "model", "parts": [{ "text": "read it" }] }, "finishReason": "STOP" }]
        })))
        .mount(&secondary)
        .await;
    let ai = Ai::new(options).unwrap();

    let response = ai
        .text("Summarize")
        .attach(Attachment::pdf(vec![1u8]))
        .await
        .unwrap();

    assert_eq!(response.model, "backup");
    assert!(primary.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn no_model_supporting_an_attachment_is_unsupported() {
    let server = MockServer::start().await;
    let ai = Ai::new(provider_options("openai", &server.uri(), "fast", "gpt-5")).unwrap();

    let error = ai
        .text("x")
        .attach(Attachment::pdf(vec![1u8]))
        .await
        .unwrap_err();

    assert!(matches!(error, AiError::Unsupported { .. }), "{error:?}");
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn fake_errors_go_through_retries_and_failover() {
    let fake = AiFake::new();
    let ai = fake.ai_with(AiOptions {
        default: "smart".into(),
        models: [
            (
                "smart".to_owned(),
                ModelConfig::new("anthropic", "a").fallbacks(["backup"]),
            ),
            ("backup".to_owned(), ModelConfig::new("openai", "b")),
        ]
        .into(),
        max_retries: 1,
        ..AiOptions::default()
    });
    fake.respond_with([
        FakeReply::error(AiError::unavailable("anthropic")),
        FakeReply::error(AiError::unavailable("anthropic")),
        FakeReply::text("backup answered"),
    ]);

    let response = ai.text("hi").await.unwrap();

    assert_eq!(response.text, "backup answered");
    assert_eq!(response.model, "backup");
    let models: Vec<String> = fake
        .requests()
        .iter()
        .map(|r| r.model().to_owned())
        .collect();
    assert_eq!(models, ["smart", "smart", "backup"]);
}

#[tokio::test]
async fn unknown_models_are_configuration_errors() {
    let ai = Ai::new(provider_options(
        "openai",
        "http://127.0.0.1:9",
        "fast",
        "gpt-5",
    ))
    .unwrap();

    let error = ai.text("hi").model("nope").await.unwrap_err();

    assert!(
        matches!(&error, AiError::Config { message } if message.contains("nope")),
        "{error:?}"
    );
}

#[test]
fn invalid_options_fail_ai_new() {
    let mut options = provider_options("openai", "http://127.0.0.1:9", "fast", "gpt-5");
    options.models.get_mut("fast").unwrap().fallbacks = vec!["ghost".into()];
    let error = Ai::new(options).unwrap_err();
    assert!(
        error.to_string().contains("ai.models.fast.fallbacks"),
        "{error}"
    );
}
