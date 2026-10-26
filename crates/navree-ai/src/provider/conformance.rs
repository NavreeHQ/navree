//! The provider conformance suite: every driver, against a local mock
//! server answering with the vendor's recorded shapes, must give the same
//! text, tool calls, streams, usage, errors and embeddings.

use std::time::Duration;

use futures_util::StreamExt;
use navree_http_client::{HttpClient, HttpClientOptions, SsrfPolicy};
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

use super::eventstream::encode;
use super::{
    ChatRequest, ChunkStream, Driver, EmbedRequest, StreamChunk, ToolChoice, ToolDefinition,
    build_driver,
};
use crate::error::ProviderError;
use crate::message::{Message, ToolCall};
use crate::options::ProviderConfig;
use crate::response::{Finish, Usage};

/// One vendor's recorded answers.
struct Fixture {
    driver: &'static str,
    model: &'static str,
    chat_path: &'static str,
    stream_path: &'static str,
    text: Value,
    tool: Value,
    stream: ResponseTemplate,
    tool_stream: ResponseTemplate,
    embed: Option<(&'static str, Value)>,
    error: Value,
}

fn sse(events: &[Value]) -> ResponseTemplate {
    let body: String = events.iter().map(|e| format!("data: {e}\n\n")).collect();
    ResponseTemplate::new(200).set_body_raw(body, "text/event-stream")
}

fn named_sse(events: &[(&str, Value)]) -> ResponseTemplate {
    let body: String = events
        .iter()
        .map(|(n, e)| format!("event: {n}\ndata: {e}\n\n"))
        .collect();
    ResponseTemplate::new(200).set_body_raw(body, "text/event-stream")
}

fn anthropic() -> Fixture {
    let start = json!({ "type": "message_start", "message": { "usage": { "input_tokens": 12, "output_tokens": 1 } } });
    let stop = json!({ "type": "message_stop" });
    Fixture {
        driver: "anthropic",
        model: "claude-sonnet-5-5",
        chat_path: "/v1/messages",
        stream_path: "/v1/messages",
        text: json!({
            "content": [{ "type": "text", "text": "Hello" }],
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 12, "output_tokens": 5 }
        }),
        tool: json!({
            "content": [{ "type": "tool_use", "id": "toolu_1", "name": "lookup", "input": { "id": 3 } }],
            "stop_reason": "tool_use",
            "usage": { "input_tokens": 12, "output_tokens": 5 }
        }),
        stream: named_sse(&[
            ("message_start", start.clone()),
            (
                "content_block_start",
                json!({ "type": "content_block_start", "index": 0, "content_block": { "type": "text", "text": "" } }),
            ),
            (
                "content_block_delta",
                json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": "Hel" } }),
            ),
            ("ping", json!({ "type": "ping" })),
            (
                "content_block_delta",
                json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": "lo" } }),
            ),
            (
                "content_block_stop",
                json!({ "type": "content_block_stop", "index": 0 }),
            ),
            (
                "message_delta",
                json!({ "type": "message_delta", "delta": { "stop_reason": "end_turn" }, "usage": { "output_tokens": 5 } }),
            ),
            ("message_stop", stop.clone()),
        ]),
        tool_stream: named_sse(&[
            ("message_start", start),
            (
                "content_block_start",
                json!({ "type": "content_block_start", "index": 0, "content_block": { "type": "tool_use", "id": "toolu_1", "name": "lookup", "input": {} } }),
            ),
            (
                "content_block_delta",
                json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "input_json_delta", "partial_json": "{\"id\": 3}" } }),
            ),
            (
                "content_block_stop",
                json!({ "type": "content_block_stop", "index": 0 }),
            ),
            (
                "message_delta",
                json!({ "type": "message_delta", "delta": { "stop_reason": "tool_use" }, "usage": { "output_tokens": 5 } }),
            ),
            ("message_stop", stop),
        ]),
        embed: None,
        error: json!({ "type": "error", "error": { "type": "invalid_request_error", "message": "bad things" } }),
    }
}

fn openai_like(driver: &'static str) -> Fixture {
    let usage = json!({ "prompt_tokens": 12, "completion_tokens": 5 });
    Fixture {
        driver,
        model: "gpt-5",
        chat_path: "/chat/completions",
        stream_path: "/chat/completions",
        text: json!({
            "choices": [{ "message": { "role": "assistant", "content": "Hello" }, "finish_reason": "stop" }],
            "usage": usage
        }),
        tool: json!({
            "choices": [{ "message": { "role": "assistant", "content": null, "tool_calls": [
                { "id": "call_1", "type": "function", "function": { "name": "lookup", "arguments": "{\"id\":3}" } }
            ]}, "finish_reason": "tool_calls" }],
            "usage": usage
        }),
        stream: {
            let body = format!(
                "data: {}\n\ndata: {}\n\ndata: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                json!({ "choices": [{ "delta": { "role": "assistant", "content": "Hel" } }] }),
                json!({ "choices": [{ "delta": { "content": "lo" } }] }),
                json!({ "choices": [{ "delta": {}, "finish_reason": "stop" }] }),
                json!({ "choices": [], "usage": usage })
            );
            ResponseTemplate::new(200).set_body_raw(body, "text/event-stream")
        },
        tool_stream: {
            let body = format!(
                "data: {}\n\ndata: {}\n\ndata: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                json!({ "choices": [{ "delta": { "tool_calls": [{ "index": 0, "id": "call_1", "function": { "name": "lookup", "arguments": "{\"id\"" } }] } }] }),
                json!({ "choices": [{ "delta": { "tool_calls": [{ "index": 0, "function": { "arguments": ":3}" } }] } }] }),
                json!({ "choices": [{ "delta": {}, "finish_reason": "tool_calls" }] }),
                json!({ "choices": [], "usage": usage })
            );
            ResponseTemplate::new(200).set_body_raw(body, "text/event-stream")
        },
        embed: Some((
            "/embeddings",
            json!({ "data": [{ "index": 0, "embedding": [1.0, 0.0] }, { "index": 1, "embedding": [0.0, 1.0] }], "usage": { "prompt_tokens": 4 } }),
        )),
        error: json!({ "error": { "message": "bad things", "type": "invalid_request_error" } }),
    }
}

fn gemini() -> Fixture {
    let usage = json!({ "promptTokenCount": 12, "candidatesTokenCount": 5 });
    Fixture {
        driver: "gemini",
        model: "gemini-3-flash",
        chat_path: "/v1beta/models/gemini-3-flash:generateContent",
        stream_path: "/v1beta/models/gemini-3-flash:streamGenerateContent",
        text: json!({
            "candidates": [{ "content": { "role": "model", "parts": [{ "text": "Hello" }] }, "finishReason": "STOP" }],
            "usageMetadata": usage
        }),
        tool: json!({
            "candidates": [{ "content": { "role": "model", "parts": [
                { "functionCall": { "id": "fc_1", "name": "lookup", "args": { "id": 3 } } }
            ] }, "finishReason": "STOP" }],
            "usageMetadata": usage
        }),
        stream: sse(&[
            json!({ "candidates": [{ "content": { "role": "model", "parts": [{ "text": "Hel" }] } }] }),
            json!({ "candidates": [{ "content": { "role": "model", "parts": [{ "text": "lo" }] }, "finishReason": "STOP" }], "usageMetadata": usage }),
        ]),
        tool_stream: sse(&[json!({
            "candidates": [{ "content": { "role": "model", "parts": [
                { "functionCall": { "id": "fc_1", "name": "lookup", "args": { "id": 3 } } }
            ] }, "finishReason": "STOP" }],
            "usageMetadata": usage
        })]),
        embed: Some((
            "/v1beta/models/gemini-3-flash:batchEmbedContents",
            json!({ "embeddings": [{ "values": [1.0, 0.0] }, { "values": [0.0, 1.0] }] }),
        )),
        error: json!({ "error": { "code": 400, "message": "bad things", "status": "INVALID_ARGUMENT" } }),
    }
}

fn ollama() -> Fixture {
    let done = |reason: &str| json!({ "message": { "role": "assistant", "content": "" }, "done": true, "done_reason": reason, "prompt_eval_count": 12, "eval_count": 5 });
    let lines = |items: Vec<Value>| {
        let body: String = items.iter().map(|v| format!("{v}\n")).collect();
        ResponseTemplate::new(200).set_body_raw(body, "application/x-ndjson")
    };
    Fixture {
        driver: "ollama",
        model: "llama3.3",
        chat_path: "/api/chat",
        stream_path: "/api/chat",
        text: json!({ "message": { "role": "assistant", "content": "Hello" }, "done": true, "done_reason": "stop", "prompt_eval_count": 12, "eval_count": 5 }),
        tool: json!({
            "message": { "role": "assistant", "content": "", "tool_calls": [{ "function": { "name": "lookup", "arguments": { "id": 3 } } }] },
            "done": true, "done_reason": "stop", "prompt_eval_count": 12, "eval_count": 5
        }),
        stream: lines(vec![
            json!({ "message": { "role": "assistant", "content": "Hel" }, "done": false }),
            json!({ "message": { "role": "assistant", "content": "lo" }, "done": false }),
            done("stop"),
        ]),
        tool_stream: lines(vec![
            json!({ "message": { "role": "assistant", "content": "", "tool_calls": [{ "function": { "name": "lookup", "arguments": { "id": 3 } } }] }, "done": false }),
            done("stop"),
        ]),
        embed: Some((
            "/api/embed",
            json!({ "embeddings": [[1.0, 0.0], [0.0, 1.0]], "prompt_eval_count": 4 }),
        )),
        error: json!({ "error": "bad things" }),
    }
}

fn bedrock() -> Fixture {
    let frame = |event: &str, payload: Value| {
        encode(
            &[(":event-type", event), (":message-type", "event")],
            payload.to_string().as_bytes(),
        )
    };
    let usage = json!({ "usage": { "inputTokens": 12, "outputTokens": 5 } });
    let stream: Vec<u8> = [
        frame("messageStart", json!({ "role": "assistant" })),
        frame(
            "contentBlockDelta",
            json!({ "contentBlockIndex": 0, "delta": { "text": "Hel" } }),
        ),
        frame(
            "contentBlockDelta",
            json!({ "contentBlockIndex": 0, "delta": { "text": "lo" } }),
        ),
        frame("contentBlockStop", json!({ "contentBlockIndex": 0 })),
        frame("messageStop", json!({ "stopReason": "end_turn" })),
        frame("metadata", usage.clone()),
    ]
    .concat();
    let tool_stream: Vec<u8> = [
        frame("messageStart", json!({ "role": "assistant" })),
        frame("contentBlockStart", json!({ "contentBlockIndex": 0, "start": { "toolUse": { "toolUseId": "t1", "name": "lookup" } } })),
        frame("contentBlockDelta", json!({ "contentBlockIndex": 0, "delta": { "toolUse": { "input": "{\"id\":3}" } } })),
        frame("contentBlockStop", json!({ "contentBlockIndex": 0 })),
        frame("messageStop", json!({ "stopReason": "tool_use" })),
        frame("metadata", usage),
    ]
    .concat();
    let usage = json!({ "inputTokens": 12, "outputTokens": 5 });
    Fixture {
        driver: "bedrock",
        model: "anthropic.claude-sonnet-5-5",
        chat_path: "/model/anthropic.claude-sonnet-5-5/converse",
        stream_path: "/model/anthropic.claude-sonnet-5-5/converse-stream",
        text: json!({ "output": { "message": { "role": "assistant", "content": [{ "text": "Hello" }] } }, "stopReason": "end_turn", "usage": usage }),
        tool: json!({
            "output": { "message": { "role": "assistant", "content": [{ "toolUse": { "toolUseId": "t1", "name": "lookup", "input": { "id": 3 } } }] } },
            "stopReason": "tool_use", "usage": usage
        }),
        stream: ResponseTemplate::new(200)
            .set_body_raw(stream, "application/vnd.amazon.eventstream"),
        tool_stream: ResponseTemplate::new(200)
            .set_body_raw(tool_stream, "application/vnd.amazon.eventstream"),
        embed: None,
        error: json!({ "message": "bad things" }),
    }
}

fn fixtures() -> Vec<Fixture> {
    vec![
        anthropic(),
        openai_like("openai"),
        openai_like("mistral"),
        gemini(),
        ollama(),
        bedrock(),
    ]
}

fn http() -> HttpClient {
    HttpClient::new(HttpClientOptions {
        retries: 0,
        ssrf: SsrfPolicy {
            block_private: true,
            allow_hosts: vec!["127.0.0.1".into()],
        },
        ..HttpClientOptions::default()
    })
    .unwrap()
}

fn driver(fixture: &Fixture, server: &MockServer) -> std::sync::Arc<dyn Driver> {
    let mut config = ProviderConfig {
        driver: Some(fixture.driver.to_owned()),
        url: Some(server.uri()),
        api_key: Some("test-key".into()),
        ..ProviderConfig::default()
    };
    if fixture.driver == "bedrock" {
        config.region = Some("us-east-1".into());
        config.access_key_id = Some("AKIDEXAMPLE".into());
        config.secret_access_key = Some("secret".into());
    }
    build_driver(fixture.driver, &config, &http()).unwrap()
}

fn request(fixture: &Fixture, tools: bool) -> ChatRequest {
    ChatRequest {
        model_name: "m".into(),
        model: fixture.model.into(),
        system: Some("Be brief.".into()),
        messages: vec![Message::user("Hi")],
        max_tokens: 64,
        temperature: None,
        stop: Vec::new(),
        tools: if tools {
            vec![ToolDefinition {
                name: "lookup".into(),
                description: "Looks a record up.".into(),
                parameters: json!({ "type": "object", "properties": { "id": { "type": "integer" } } }),
            }]
        } else {
            Vec::new()
        },
        tool_choice: ToolChoice::Auto,
        output: None,
        user: None,
        timeout: Duration::from_secs(5),
    }
}

async fn serve(server: &MockServer, at: &str, response: ResponseTemplate) {
    server.reset().await;
    Mock::given(method("POST"))
        .and(path(at))
        .respond_with(response)
        .mount(server)
        .await;
}

async fn collect(stream: ChunkStream) -> Vec<StreamChunk> {
    stream.map(|chunk| chunk.unwrap()).collect().await
}

fn assert_authenticated(fixture: &Fixture, request: &Request) {
    let header = |name: &str| {
        request
            .headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    };
    match fixture.driver {
        "anthropic" => {
            assert_eq!(header("x-api-key").as_deref(), Some("test-key"));
            assert_eq!(header("anthropic-version").as_deref(), Some("2026-10-21"));
        }
        "openai" | "mistral" => {
            assert_eq!(header("authorization").as_deref(), Some("Bearer test-key"))
        }
        "gemini" => assert_eq!(header("x-goog-api-key").as_deref(), Some("test-key")),
        "bedrock" => {
            let auth = header("authorization").unwrap();
            assert!(
                auth.starts_with("AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/"),
                "{auth}"
            );
            assert!(auth.contains("/us-east-1/bedrock/aws4_request"), "{auth}");
            assert!(
                auth.contains("SignedHeaders=content-type;host;x-amz-date"),
                "{auth}"
            );
            assert!(header("x-amz-date").is_some());
        }
        _ => {}
    }
}

#[tokio::test]
async fn every_driver_answers_text_with_usage() {
    let server = MockServer::start().await;
    for fixture in fixtures() {
        serve(
            &server,
            fixture.chat_path,
            ResponseTemplate::new(200).set_body_json(fixture.text.clone()),
        )
        .await;
        let response = driver(&fixture, &server)
            .chat(&request(&fixture, false))
            .await;
        let response = response.unwrap_or_else(|e| panic!("{}: {e:?}", fixture.driver));
        assert_eq!(response.text(), "Hello", "{}", fixture.driver);
        assert_eq!(response.usage, Usage::new(12, 5), "{}", fixture.driver);
        assert_eq!(response.finish, Finish::Stop, "{}", fixture.driver);
        let received = server.received_requests().await.unwrap();
        assert_eq!(received.len(), 1, "{}", fixture.driver);
        assert_authenticated(&fixture, &received[0]);
    }
}

#[tokio::test]
async fn every_driver_returns_tool_calls() {
    let server = MockServer::start().await;
    for fixture in fixtures() {
        serve(
            &server,
            fixture.chat_path,
            ResponseTemplate::new(200).set_body_json(fixture.tool.clone()),
        )
        .await;
        let response = driver(&fixture, &server)
            .chat(&request(&fixture, true))
            .await
            .unwrap();
        let calls = response.tool_calls();
        assert_eq!(calls.len(), 1, "{}", fixture.driver);
        assert_eq!(calls[0].name, "lookup", "{}", fixture.driver);
        assert_eq!(calls[0].arguments, json!({ "id": 3 }), "{}", fixture.driver);
        assert!(!calls[0].id.is_empty(), "{}", fixture.driver);
        assert_eq!(response.finish, Finish::ToolCalls, "{}", fixture.driver);
        let body: Value =
            serde_json::from_slice(&server.received_requests().await.unwrap()[0].body).unwrap();
        assert!(
            body.to_string().contains("Looks a record up."),
            "{}: {body}",
            fixture.driver
        );
    }
}

#[tokio::test]
async fn every_driver_streams_text_then_done() {
    let server = MockServer::start().await;
    for fixture in fixtures() {
        serve(&server, fixture.stream_path, fixture.stream.clone()).await;
        let stream = driver(&fixture, &server)
            .stream(&request(&fixture, false))
            .await;
        let chunks = collect(stream.unwrap_or_else(|e| panic!("{}: {e:?}", fixture.driver))).await;
        let text: String = chunks
            .iter()
            .filter_map(|c| match c {
                StreamChunk::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text, "Hello", "{}", fixture.driver);
        assert!(
            chunks
                .iter()
                .filter(|c| matches!(c, StreamChunk::Text(_)))
                .count()
                >= 1,
            "{}",
            fixture.driver
        );
        assert_eq!(
            chunks.last(),
            Some(&StreamChunk::Done {
                usage: Usage::new(12, 5),
                finish: Finish::Stop
            }),
            "{}",
            fixture.driver
        );
    }
}

#[tokio::test]
async fn every_driver_streams_tool_calls() {
    let server = MockServer::start().await;
    for fixture in fixtures() {
        serve(&server, fixture.stream_path, fixture.tool_stream.clone()).await;
        let chunks = collect(
            driver(&fixture, &server)
                .stream(&request(&fixture, true))
                .await
                .unwrap(),
        )
        .await;
        let calls: Vec<&ToolCall> = chunks
            .iter()
            .filter_map(|c| match c {
                StreamChunk::ToolCall(call) => Some(call),
                _ => None,
            })
            .collect();
        assert_eq!(calls.len(), 1, "{}: {chunks:?}", fixture.driver);
        assert_eq!(calls[0].name, "lookup", "{}", fixture.driver);
        assert_eq!(calls[0].arguments, json!({ "id": 3 }), "{}", fixture.driver);
        assert!(
            matches!(
                chunks.last(),
                Some(StreamChunk::Done {
                    finish: Finish::ToolCalls,
                    ..
                })
            ),
            "{}: {chunks:?}",
            fixture.driver
        );
    }
}

#[tokio::test]
async fn every_driver_classifies_errors() {
    let server = MockServer::start().await;
    for fixture in fixtures() {
        let cases = [
            (
                ResponseTemplate::new(429)
                    .insert_header("retry-after", "3")
                    .set_body_json(fixture.error.clone()),
                "rate",
            ),
            (
                ResponseTemplate::new(503).set_body_json(fixture.error.clone()),
                "unavailable",
            ),
            (
                ResponseTemplate::new(400).set_body_json(fixture.error.clone()),
                "request",
            ),
            (
                ResponseTemplate::new(401).set_body_json(fixture.error.clone()),
                "auth",
            ),
        ];
        for (response, expected) in cases {
            serve(&server, fixture.chat_path, response.clone()).await;
            let error = driver(&fixture, &server)
                .chat(&request(&fixture, false))
                .await
                .unwrap_err();
            let ok = match expected {
                "rate" => {
                    error
                        == ProviderError::RateLimited {
                            retry_after: Some(Duration::from_secs(3)),
                        }
                }
                "unavailable" => matches!(error, ProviderError::Unavailable { .. }),
                "request" => {
                    matches!(&error, ProviderError::Request { message } if message.contains("bad things"))
                }
                _ => matches!(error, ProviderError::Auth { .. }),
            };
            assert!(ok, "{} {expected}: {error:?}", fixture.driver);
            // Streams classify the same way before the first event.
            serve(&server, fixture.stream_path, response).await;
            let stream_error = driver(&fixture, &server)
                .stream(&request(&fixture, false))
                .await
                .err();
            assert_eq!(
                stream_error,
                Some(error),
                "{} stream {expected}",
                fixture.driver
            );
        }
    }
}

#[tokio::test]
async fn drivers_with_embeddings_return_vectors_in_order() {
    let server = MockServer::start().await;
    for fixture in fixtures() {
        let Some((at, answer)) = &fixture.embed else {
            continue;
        };
        serve(
            &server,
            at,
            ResponseTemplate::new(200).set_body_json(answer.clone()),
        )
        .await;
        let response = driver(&fixture, &server)
            .embed(&EmbedRequest {
                model: fixture.model.into(),
                texts: vec!["a".into(), "b".into()],
                dimensions: Some(2),
                timeout: Duration::from_secs(5),
            })
            .await
            .unwrap_or_else(|e| panic!("{}: {e:?}", fixture.driver));
        assert_eq!(
            response.vectors,
            [vec![1.0, 0.0], vec![0.0, 1.0]],
            "{}",
            fixture.driver
        );
    }
}

#[tokio::test]
async fn anthropic_has_no_embeddings() {
    let server = MockServer::start().await;
    let error = driver(&anthropic(), &server)
        .embed(&EmbedRequest {
            model: "x".into(),
            texts: vec!["a".into()],
            dimensions: None,
            timeout: Duration::from_secs(5),
        })
        .await
        .unwrap_err();
    assert!(matches!(error, ProviderError::Unsupported { .. }));
}

#[tokio::test]
async fn bedrock_embeds_with_titan_one_text_per_call() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/model/amazon.titan-embed-text-v2%3A0/invoke"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "embedding": [0.5, 0.5], "inputTextTokenCount": 2 })),
        )
        .mount(&server)
        .await;
    let response = driver(&bedrock(), &server)
        .embed(&EmbedRequest {
            model: "amazon.titan-embed-text-v2:0".into(),
            texts: vec!["a".into(), "b".into()],
            dimensions: Some(2),
            timeout: Duration::from_secs(5),
        })
        .await
        .unwrap();
    assert_eq!(response.vectors.len(), 2);
    assert_eq!(response.input_tokens, 4);
    let bodies = server.received_requests().await.unwrap();
    assert_eq!(bodies.len(), 2);
    let first: Value = serde_json::from_slice(&bodies[0].body).unwrap();
    assert_eq!(first, json!({ "inputText": "a", "dimensions": 2 }));
}

#[tokio::test]
async fn an_ssrf_blocked_address_is_an_invalid_request() {
    let config = ProviderConfig::openai_compatible("http://10.1.2.3:8000/v1");
    let strict = HttpClient::new(HttpClientOptions::default()).unwrap();
    let driver = build_driver("local", &config, &strict).unwrap();
    let error = driver
        .chat(&request(&openai_like("openai"), false))
        .await
        .unwrap_err();
    assert!(
        matches!(&error, ProviderError::Request { message } if message.contains("SSRF")),
        "{error:?}"
    );
}
