//! Broadcasts reported as telemetry: the channels, the event name and how
//! many subscribers were reached, never the data.

use std::sync::Arc;

use navree_telemetry::{Outcome, Telemetry, TelemetryContext, TelemetryRecorder};
use serde_json::json;

use crate::support::Server;

#[tokio::test]
async fn a_broadcast_reports_its_channels_and_the_subscribers_it_reached() {
    let recorder = TelemetryRecorder::new();
    let telemetry = Telemetry::with_sinks(vec![Arc::new(recorder.clone())]);
    let server = Server::start().await;
    let mut subscriber = server.connect().await;
    subscriber.subscribe("news", None, None).await;
    subscriber
        .expect("pusher_internal:subscription_succeeded")
        .await;

    TelemetryContext::new(telemetry)
        .scope(async {
            server
                .realtime
                .broadcast(
                    ["news", "sport"],
                    "story.published",
                    json!({ "headline": "Ludhiana wins the final" }),
                )
                .unwrap();
        })
        .await;

    let sent = recorder.broadcasts();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].channels, ["news", "sport"]);
    assert_eq!(sent[0].event, "story.published");
    assert_eq!(
        sent[0].delivered, 1,
        "one subscriber on one of the channels"
    );
    assert_eq!(sent[0].outcome, Outcome::Success);
    let text = serde_json::to_string(&sent[0]).unwrap();
    assert!(!text.contains("Ludhiana"), "the data stays out: {text}");
}

#[tokio::test]
async fn a_broadcast_outside_a_request_or_job_reports_nothing() {
    let server = Server::start().await;
    server
        .realtime
        .broadcast(["news"], "story.published", json!({}))
        .unwrap();
}
