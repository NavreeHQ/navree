//! Delivery attempts reported as telemetry: the endpoint, the event, the
//! attempt, the answer and how long it took, never the payload.

use http::Method;
use navree_http_client::FakeResponse;
use navree_telemetry::{Outcome, Telemetry, TelemetryContext, TelemetryRecorder};
use navree_webhooks::NewEndpoint;

use crate::support::{deal_won, harness};

#[tokio::test]
async fn each_attempt_is_reported_with_its_answer_and_outcome() {
    let recorder = TelemetryRecorder::new();
    let h = harness(|b| b.module(recorder.module())).await;
    h.server.on(
        Method::POST,
        "https://hooks.lovelace.io/*",
        FakeResponse::new(503),
    );
    let created = h
        .endpoint(NewEndpoint::new("https://hooks.lovelace.io/navree").events(["deal.won"]))
        .await;
    let telemetry = Telemetry::from_app(h.app.ctx()).await;

    h.emit(deal_won(None)).await;
    TelemetryContext::new(telemetry).scope(h.run_queue()).await;

    let attempts = recorder.webhook_deliveries();
    assert_eq!(attempts.len(), 1);
    let attempt = &attempts[0];
    assert_eq!(attempt.endpoint, created.endpoint.id.to_string());
    assert_eq!(attempt.event, "deal.won");
    assert_eq!(attempt.attempt, 1);
    assert_eq!(attempt.status, Some(503));
    assert_eq!(attempt.outcome, Outcome::Failure);
    let text = serde_json::to_string(attempt).unwrap();
    assert!(
        !text.contains("Lovelace renewal"),
        "the payload stays out: {text}"
    );
}

#[tokio::test]
async fn an_answer_of_204_is_a_success() {
    let recorder = TelemetryRecorder::new();
    let h = harness(|b| b.module(recorder.module())).await;
    h.server.on(
        Method::POST,
        "https://hooks.lovelace.io/*",
        FakeResponse::new(204),
    );
    h.endpoint(NewEndpoint::new("https://hooks.lovelace.io/navree").events(["deal.won"]))
        .await;
    let telemetry = Telemetry::from_app(h.app.ctx()).await;

    h.emit(deal_won(None)).await;
    TelemetryContext::new(telemetry).scope(h.run_queue()).await;

    let attempts = recorder.webhook_deliveries();
    assert_eq!(attempts[0].status, Some(204));
    assert_eq!(attempts[0].outcome, Outcome::Success);
}
