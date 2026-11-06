//! The model calls reported as telemetry: tokens, latency and outcome, never
//! the prompt or the answer.

use std::sync::Arc;

use navree_ai::{AiFake, FakeReply};
use navree_telemetry::{Outcome, Telemetry, TelemetryContext, TelemetryRecorder};

#[tokio::test]
async fn a_text_request_reports_an_ai_call_with_its_tokens_and_no_text() {
    let recorder = TelemetryRecorder::new();
    let telemetry = Telemetry::with_sinks(vec![Arc::new(recorder.clone())]);
    let fake = AiFake::new();
    let ai = fake.ai();
    fake.respond(FakeReply::text("Acme is in negotiation.").with_usage(120, 9));

    TelemetryContext::new(telemetry)
        .scope(async {
            ai.text("Summarize the Acme deal")
                .model("fast")
                .await
                .unwrap();
        })
        .await;

    let calls = recorder.ai_calls();
    assert_eq!(calls.len(), 1);
    let call = &calls[0];
    assert_eq!(call.provider, "fake");
    assert_eq!(call.model, "fast");
    assert_eq!(call.operation, "generate");
    assert_eq!(call.input_tokens, Some(120));
    assert_eq!(call.output_tokens, Some(9));
    assert_eq!(call.outcome, Outcome::Success);
    let json = serde_json::to_string(call).unwrap();
    assert!(!json.contains("Acme"), "the prompt and the answer stay out: {json}");
}

#[tokio::test]
async fn outside_a_request_or_job_nothing_is_reported() {
    let fake = AiFake::new();
    fake.respond(FakeReply::text("Fine."));
    fake.ai().text("Hello").await.unwrap();
}
