//! `ai.stream`: the Rust stream, the SSE response, `chunked`, `on_finish`,
//! failover before the first event and errors after it.

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::response::IntoResponse;
use futures_util::StreamExt;
use http_body_util::BodyExt;
use navree_ai::{Ai, AiError, AiFake, AiModule, FakeReply, StreamEvent, TextStream};
use navree_http::{Controller, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{BoxError, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_testing::TestApp;
use serde_json::json;
use tokio::sync::Mutex;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::support::{Sequence, provider_options, sse};

async fn body_text(stream: TextStream) -> (http::HeaderMap, String) {
    let response = stream.into_response();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (headers, String::from_utf8(bytes.to_vec()).unwrap())
}

#[tokio::test]
async fn a_stream_yields_text_then_done() {
    let fake = AiFake::new();
    fake.respond(FakeReply::text("Acme moved to negotiation.").with_usage(912, 41));

    let mut stream = fake.ai().stream("Summarize").model("fast").await.unwrap();
    let mut text = String::new();
    let mut done = None;
    while let Some(event) = stream.next().await {
        match event.unwrap() {
            StreamEvent::Text(piece) => text.push_str(&piece),
            StreamEvent::Done(completion) => done = Some(completion),
            other => panic!("{other:?}"),
        }
    }

    assert_eq!(text, "Acme moved to negotiation.");
    let done = done.unwrap();
    assert_eq!(done.model, "fast");
    assert_eq!(done.usage.input_tokens, 912);
    assert_eq!(done.usage.output_tokens, 41);
}

#[tokio::test]
async fn the_response_is_server_sent_events() {
    let fake = AiFake::new();
    fake.respond(FakeReply::text("Acme moved on.").with_usage(5, 3));

    let stream = fake.ai().stream("Summarize").model("fast").await.unwrap();
    let (headers, body) = body_text(stream).await;

    assert_eq!(headers["content-type"], "text/event-stream");
    assert_eq!(headers["cache-control"], "no-cache");
    assert_eq!(headers["x-accel-buffering"], "no");
    assert!(
        body.starts_with("event: text\ndata: {\"text\":\"Acme\"}\n\n"),
        "{body}"
    );
    assert!(
        body.contains("event: text\ndata: {\"text\":\" moved\"}\n\n"),
        "{body}"
    );
    let done = body
        .split("\n\n")
        .find_map(|event| event.strip_prefix("event: done\ndata: "))
        .expect("a done event");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(done).unwrap(),
        json!({
            "model": "fast",
            "provider": "fake",
            "usage": { "input_tokens": 5, "output_tokens": 3, "cached_input_tokens": 0 },
            "finish": "stop"
        })
    );
}

#[tokio::test]
async fn an_error_after_the_first_event_is_an_error_event() {
    let server = MockServer::start().await;
    // The stream starts, then the provider reports an overload.
    Mock::given(method("POST"))
        .respond_with(sse(&[
            ("message_start", json!({ "type": "message_start", "message": { "usage": { "input_tokens": 3 } } })),
            ("content_block_delta", json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": "Hi" } })),
            ("error", json!({ "type": "error", "error": { "type": "overloaded_error", "message": "Overloaded" } })),
        ]))
        .mount(&server)
        .await;
    let ai = Ai::new(provider_options(
        "anthropic",
        &server.uri(),
        "fast",
        "claude-sonnet-5-5",
    ))
    .unwrap();

    let stream = ai.stream("hi").await.unwrap();
    let (_, body) = body_text(stream).await;

    assert!(
        body.starts_with("event: text\ndata: {\"text\":\"Hi\"}\n\n"),
        "{body}"
    );
    assert!(
        body.ends_with("event: error\ndata: {\"error\":\"Service Unavailable\",\"message\":\"The AI provider is unavailable\",\"statusCode\":503}\n\n"),
        "{body}"
    );
}

#[tokio::test]
async fn a_stream_fails_over_before_its_first_event() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(Sequence::new(vec![
            ResponseTemplate::new(529).set_body_json(json!({ "type": "error", "error": { "type": "overloaded_error", "message": "Overloaded" } })),
            sse(&[
                ("message_start", json!({ "type": "message_start", "message": { "usage": { "input_tokens": 3 } } })),
                ("content_block_delta", json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": "Back" } })),
                ("message_delta", json!({ "type": "message_delta", "delta": { "stop_reason": "end_turn" }, "usage": { "output_tokens": 1 } })),
                ("message_stop", json!({ "type": "message_stop" })),
            ]),
        ]))
        .mount(&server)
        .await;
    let ai = Ai::new(provider_options(
        "anthropic",
        &server.uri(),
        "fast",
        "claude-sonnet-5-5",
    ))
    .unwrap();

    let events: Vec<StreamEvent> = ai
        .stream("hi")
        .await
        .unwrap()
        .map(Result::unwrap)
        .collect()
        .await;

    assert_eq!(events[0], StreamEvent::Text("Back".into()));
    assert!(matches!(events[1], StreamEvent::Done(_)));
}

#[tokio::test]
async fn errors_before_the_first_event_are_ordinary_errors() {
    let fake = AiFake::new();
    fake.respond_when(
        |_| true,
        FakeReply::error(AiError::unavailable("anthropic")),
    );

    let error = fake.ai().stream("hi").await.unwrap_err();

    assert_eq!(error.status().as_u16(), 503);
}

#[tokio::test(start_paused = true)]
async fn chunked_merges_pieces() {
    let fake = AiFake::new();
    fake.respond(FakeReply::text("one two three four"));

    let events: Vec<StreamEvent> = fake
        .ai()
        .stream("x")
        .await
        .unwrap()
        .chunked(Duration::from_millis(100))
        .map(Result::unwrap)
        .collect()
        .await;

    // The fake's words arrive at once, so they merge into one piece.
    assert_eq!(events[0], StreamEvent::Text("one two three four".into()));
    assert!(matches!(events[1], StreamEvent::Done(_)));
}

#[tokio::test]
async fn on_finish_runs_with_the_whole_answer() {
    let fake = AiFake::new();
    fake.respond(FakeReply::text("Saved summary."));
    let saved = Arc::new(Mutex::new(None));
    let sink = saved.clone();

    let stream = fake
        .ai()
        .stream("x")
        .model("fast")
        .await
        .unwrap()
        .on_finish(move |response| async move {
            *sink.lock().await = Some(response);
            Ok::<(), BoxError>(())
        });
    let _ = body_text(stream).await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let response = saved.lock().await.clone().expect("on_finish ran");
    assert_eq!(response.text, "Saved summary.");
    assert_eq!(response.model, "fast");
}

#[tokio::test]
async fn on_finish_does_not_run_when_the_client_leaves_first() {
    let fake = AiFake::new();
    fake.respond(FakeReply::text("a b c d"));
    let ran = Arc::new(Mutex::new(false));
    let flag = ran.clone();

    let mut stream = fake
        .ai()
        .stream("x")
        .await
        .unwrap()
        .on_finish(move |_| async move {
            *flag.lock().await = true;
            Ok::<(), BoxError>(())
        });
    let _first = stream.next().await;
    drop(stream);
    tokio::time::sleep(Duration::from_millis(50)).await;

    assert!(!*ran.lock().await);
}

/// A controller returning a `TextStream` from a route.
struct SummaryController {
    ai: Arc<Ai>,
}

impl Injectable for SummaryController {
    fn construct(r: Resolver) -> navree_kernel::BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(Self {
                ai: r.get::<Ai>().await?,
            })
        })
    }
}

impl Controller for SummaryController {
    fn prefix(&self) -> &'static str {
        "/summary"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let this = self.clone();
        vec![
            RouteDef::on(http::Method::GET, "/", move || {
                let ai = this.ai.clone();
                async move {
                    match ai.stream("Summarize the deal").model("fast").await {
                        Ok(stream) => stream.into_response(),
                        Err(error) => error.to_app_error().into_response(),
                    }
                }
            })
            .public(),
        ]
    }
}

struct SummaryModule;

impl Module for SummaryModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<SummaryController>();
    }
}

#[tokio::test]
async fn a_text_stream_is_served_through_the_http_pipeline() {
    let app = TestApp::builder()
        .config("ai.fake", true)
        .module(AiModule::from_config())
        .module(SummaryModule)
        .build()
        .await;
    let fake = app.get::<Ai>().await.fake().unwrap();
    fake.respond(FakeReply::text("Acme is close."));

    let response = app
        .client()
        .get("/api/summary")
        .header("accept", "text/event-stream")
        .send()
        .await;

    let response = response.assert_status(200);
    assert_eq!(response.header("content-type"), Some("text/event-stream"));
    assert_eq!(response.header("x-accel-buffering"), Some("no"));
    let body = response.text();
    assert!(
        body.contains("event: text\ndata: {\"text\":\"Acme\"}"),
        "{body}"
    );
    assert!(body.contains("event: done"), "{body}");
}

#[test]
fn bodies_are_plain() {
    // A compile-time check that TextStream works with axum's Body.
    let _ = |stream: TextStream| -> Body { stream.into_response().into_body() };
}
