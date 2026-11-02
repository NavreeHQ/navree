//! Outgoing webhooks: fan-out, signing, retries, disabling, rotation,
//! scoping, dispatch by name and the module's events.

use std::sync::Arc;
use std::time::Duration;

use http::Method;
use navree_crypto::CryptoModule;
use navree_db::Db;
use navree_events::EventBus;
use navree_http_client::FakeResponse;
use navree_orm::Model as _;
use navree_queue::QueueFake;
use navree_webhooks::entity::endpoint;
use navree_webhooks::{
    DeliveryStatus, EndpointChanges, FanOut, NewEndpoint, RETRY_SCHEDULE, StandardWebhooks,
    Tolerance, WebhookDeliveryFailed, WebhookEndpointDisabled, Webhooks, WebhooksError,
};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::{DealLost, Harness, deal_won, harness};

const URL: &str = "https://hooks.lovelace.io/navree";

fn ok(h: &Harness, status: u16) {
    h.server.on(
        Method::POST,
        "https://hooks.lovelace.io/*",
        FakeResponse::new(status),
    );
}

#[tokio::test]
async fn winning_a_deal_notifies_subscribed_endpoints() {
    let h = harness(|b| b).await;
    ok(&h, 204);
    let created = h.endpoint(NewEndpoint::new(URL).events(["deal.won"])).await;
    let other = h
        .endpoint(NewEndpoint::new("https://hooks.lovelace.io/lost").events(["deal.lost"]))
        .await;

    let event = deal_won(None);
    h.emit(event.clone()).await;
    h.run_queue().await;

    h.server.assert_sent_count(1);
    let request = h.server.recorded().pop().unwrap();
    assert_eq!(request.url().as_str(), URL);
    assert_eq!(request.header("content-type"), Some("application/json"));
    assert_eq!(request.header("user-agent"), Some("Navree-Webhooks/1"));
    assert_eq!(
        request.header("webhook-timestamp"),
        Some(h.clock_now().to_string().as_str())
    );
    let verified = StandardWebhooks::verify(
        &created.secret,
        request.headers(),
        request.body(),
        Some(Tolerance::at(navree_contracts::Clock::now(&*h.clock), 300)),
    )
    .unwrap();
    let message_id = verified.id.unwrap();
    assert!(message_id.starts_with("msg_"), "{message_id}");
    let body: Value = request.json().unwrap();
    assert_eq!(body["type"], "deal.won");
    assert_eq!(body["timestamp"], "2026-10-26T08:00:00Z");
    assert_eq!(body["data"]["deal"]["title"], "Lovelace renewal");
    assert_eq!(body["data"]["deal"]["id"], json!(event.deal_id));
    assert_eq!(body["data"]["endpoints_seen"], 2);
    // Keys are sent in order: type, timestamp, data.
    assert!(
        request
            .text()
            .starts_with(r#"{"type":"deal.won","timestamp":"2026-10-26T08:00:00Z","data":{"#),
        "{}",
        request.text()
    );

    let delivery = h.last_delivery(created.endpoint.id).await;
    assert_eq!(delivery.status, DeliveryStatus::Succeeded);
    assert_eq!(delivery.attempts, 1);
    assert_eq!(delivery.message_id, message_id);
    assert_eq!(delivery.last_status_code, Some(204));
    let attempts = h.webhooks().await.attempts(delivery.id).await.unwrap();
    assert_eq!(attempts.len(), 1);
    assert!(attempts[0].succeeded);
    assert_eq!(attempts[0].status_code, Some(204));
    assert!(
        h.webhooks()
            .await
            .deliveries(other.endpoint.id, Default::default())
            .await
            .unwrap()
            .data
            .is_empty()
    );
}

impl Harness {
    fn clock_now(&self) -> i64 {
        navree_contracts::Clock::now(&*self.clock).timestamp()
    }
}

#[tokio::test]
async fn the_default_payload_is_the_event_and_star_subscribes_to_everything() {
    let h = harness(|b| b).await;
    ok(&h, 200);
    let created = h.endpoint(NewEndpoint::new(URL).events(["*"])).await;

    let lost = DealLost {
        deal_id: Uuid::now_v7(),
        reason: "budget".to_owned(),
    };
    h.emit(lost.clone()).await;
    h.run_queue().await;

    let request = h.server.recorded().pop().unwrap();
    let body: Value = request.json().unwrap();
    assert_eq!(body["type"], "deal.lost");
    assert_eq!(body["data"], json!(lost));
    StandardWebhooks::verify(&created.secret, request.headers(), request.body(), None).unwrap();
}

#[tokio::test]
async fn failed_attempts_follow_the_retry_schedule_then_fail() {
    let h = harness(|b| b.config("webhooks.disable_after_days", 0)).await;
    ok(&h, 500);
    let created = h.endpoint(NewEndpoint::new(URL).events(["deal.won"])).await;
    let bus = h.app.get::<EventBus>().await;
    let mut failed_events =
        bus.subscribe_json(<WebhookDeliveryFailed as navree_events::Event>::NAME);

    h.emit(deal_won(None)).await;
    h.run_queue().await;

    let delivery = h.last_delivery(created.endpoint.id).await;
    assert_eq!(delivery.status, DeliveryStatus::Pending);
    assert_eq!(delivery.attempts, 1);
    assert_eq!(delivery.last_status_code, Some(500));
    assert_eq!(
        delivery.next_attempt_at,
        Some(super::support::start() + chrono::TimeDelta::seconds(5))
    );

    // Not due yet: nothing happens a second early.
    h.advance(Duration::from_secs(4)).await;
    h.server.assert_sent_count(1);

    h.advance(Duration::from_secs(1)).await;
    h.server.assert_sent_count(2);
    for (sent, delay) in RETRY_SCHEDULE.iter().enumerate().skip(2) {
        h.advance(*delay).await;
        h.server.assert_sent_count(sent + 1);
    }

    let delivery = h.last_delivery(created.endpoint.id).await;
    assert_eq!(delivery.status, DeliveryStatus::Failed);
    assert_eq!(delivery.attempts, 8);
    assert_eq!(delivery.next_attempt_at, None);
    let ids: Vec<_> = h
        .server
        .recorded()
        .iter()
        .map(|r| r.header("webhook-id").unwrap().to_owned())
        .collect();
    assert!(ids.iter().all(|id| *id == delivery.message_id), "{ids:?}");

    h.advance(Duration::from_secs(48 * 3600)).await;
    h.server.assert_sent_count(8);
    let event: WebhookDeliveryFailed =
        serde_json::from_value(failed_events.try_recv().unwrap()).unwrap();
    assert_eq!(event.delivery_id, delivery.id);
    assert_eq!(event.attempts, 8);
    assert_eq!(event.last_status_code, Some(500));
    let endpoint = h
        .webhooks()
        .await
        .endpoint(created.endpoint.id)
        .await
        .unwrap();
    assert!(endpoint.enabled, "disable_after_days = 0 never disables");
    assert!(endpoint.failing_since.is_some());
}

#[tokio::test]
async fn a_success_after_failures_succeeds_and_clears_failing_since() {
    let h = harness(|b| b).await;
    h.server.on_sequence(
        Method::POST,
        "https://hooks.lovelace.io/*",
        [
            FakeResponse::connection_failed(),
            FakeResponse::new(503).text("busy"),
            FakeResponse::new(200).text("ok"),
        ],
    );
    let created = h.endpoint(NewEndpoint::new(URL).events(["deal.won"])).await;

    h.emit(deal_won(None)).await;
    h.run_queue().await;
    let endpoint = h
        .webhooks()
        .await
        .endpoint(created.endpoint.id)
        .await
        .unwrap();
    assert_eq!(endpoint.failing_since, Some(super::support::start()));

    h.advance(RETRY_SCHEDULE[1]).await;
    h.advance(RETRY_SCHEDULE[2]).await;

    let delivery = h.last_delivery(created.endpoint.id).await;
    assert_eq!(delivery.status, DeliveryStatus::Succeeded);
    assert_eq!(delivery.attempts, 3);
    let attempts = h.webhooks().await.attempts(delivery.id).await.unwrap();
    assert_eq!(attempts.len(), 3);
    assert_eq!(attempts[0].status_code, None);
    assert!(attempts[0].error.as_deref().unwrap().contains("connect"));
    assert_eq!(attempts[1].status_code, Some(503));
    assert_eq!(attempts[1].response_body.as_deref(), Some("busy"));
    assert_eq!(attempts[2].response_body.as_deref(), Some("ok"));
    let endpoint = h
        .webhooks()
        .await
        .endpoint(created.endpoint.id)
        .await
        .unwrap();
    assert_eq!(endpoint.failing_since, None);
}

#[tokio::test]
async fn an_endpoint_failing_for_disable_after_days_is_disabled() {
    let h = harness(|b| b.config("webhooks.disable_after_days", 1)).await;
    ok(&h, 500);
    let created = h.endpoint(NewEndpoint::new(URL).events(["deal.won"])).await;
    let mut disabled_events = h
        .app
        .get::<EventBus>()
        .await
        .subscribe_json(<WebhookEndpointDisabled as navree_events::Event>::NAME);

    h.emit(deal_won(None)).await;
    h.run_queue().await;
    for delay in RETRY_SCHEDULE.iter().skip(1) {
        h.advance(*delay).await;
    }

    // The eighth attempt came about 27 hours after the first failure.
    let endpoint = h
        .webhooks()
        .await
        .endpoint(created.endpoint.id)
        .await
        .unwrap();
    assert!(!endpoint.enabled);
    assert_eq!(endpoint.disabled_reason.as_deref(), Some("failing"));
    let event: WebhookEndpointDisabled =
        serde_json::from_value(disabled_events.try_recv().unwrap()).unwrap();
    assert_eq!(event.endpoint_id, created.endpoint.id);
    assert_eq!(event.reason, "failing");

    // A disabled endpoint receives nothing.
    let sent = h.server.recorded().len();
    h.emit(deal_won(None)).await;
    h.run_queue().await;
    h.server.assert_sent_count(sent);
}

#[tokio::test]
async fn an_endpoint_answering_410_is_disabled_at_once() {
    let h = harness(|b| b).await;
    ok(&h, 410);
    let created = h.endpoint(NewEndpoint::new(URL).events(["deal.won"])).await;

    h.emit(deal_won(None)).await;
    h.run_queue().await;

    let endpoint = h
        .webhooks()
        .await
        .endpoint(created.endpoint.id)
        .await
        .unwrap();
    assert!(!endpoint.enabled);
    assert_eq!(endpoint.disabled_reason.as_deref(), Some("gone"));
    let delivery = h.last_delivery(created.endpoint.id).await;
    assert_eq!(delivery.status, DeliveryStatus::Failed);
    assert_eq!(delivery.attempts, 1);

    // Enabling it again forgets the failure.
    let uow = h.app.get::<Db>().await.uow(None, None).await;
    let enabled = h
        .webhooks()
        .await
        .enable(&uow, created.endpoint.id)
        .await
        .unwrap();
    uow.commit().await.unwrap();
    assert!(enabled.enabled);
    assert_eq!(enabled.disabled_reason, None);
    assert_eq!(enabled.failing_since, None);
}

#[tokio::test]
async fn a_rotated_secret_keeps_signing_for_24_hours() {
    let h = harness(|b| b).await;
    ok(&h, 204);
    let created = h.endpoint(NewEndpoint::new(URL).events(["deal.won"])).await;
    let uow = h.app.get::<Db>().await.uow(None, None).await;
    let new_secret = h
        .webhooks()
        .await
        .rotate_secret(&uow, created.endpoint.id)
        .await
        .unwrap();
    uow.commit().await.unwrap();
    assert_ne!(new_secret, created.secret);

    h.emit(deal_won(None)).await;
    h.run_queue().await;
    let request = h.server.recorded().pop().unwrap();
    let signatures = request.header("webhook-signature").unwrap();
    assert_eq!(signatures.split(' ').count(), 2, "{signatures}");
    StandardWebhooks::verify(&created.secret, request.headers(), request.body(), None).unwrap();
    StandardWebhooks::verify(&new_secret, request.headers(), request.body(), None).unwrap();

    h.clock.advance(Duration::from_secs(24 * 3600 + 1));
    h.emit(deal_won(None)).await;
    h.run_queue().await;
    let request = h.server.recorded().pop().unwrap();
    assert_eq!(
        request
            .header("webhook-signature")
            .unwrap()
            .split(' ')
            .count(),
        1
    );
    StandardWebhooks::verify(&new_secret, request.headers(), request.body(), None).unwrap();
    assert!(
        StandardWebhooks::verify(&created.secret, request.headers(), request.body(), None).is_err()
    );
}

#[tokio::test]
async fn events_go_to_the_endpoints_of_their_workspace_only() {
    let h = harness(|b| b).await;
    ok(&h, 204);
    let acme = Uuid::now_v7();
    let globex = Uuid::now_v7();
    let acme_endpoint = h
        .endpoint(
            NewEndpoint::new("https://hooks.lovelace.io/acme")
                .events(["deal.won"])
                .workspace(acme),
        )
        .await;
    h.endpoint(
        NewEndpoint::new("https://hooks.lovelace.io/globex")
            .events(["deal.won"])
            .workspace(globex),
    )
    .await;
    h.endpoint(NewEndpoint::new("https://hooks.lovelace.io/app").events(["deal.won"]))
        .await;

    h.emit(deal_won(Some(acme))).await;
    h.run_queue().await;

    h.server.assert_sent_count(1);
    assert_eq!(
        h.server.recorded()[0].url().as_str(),
        "https://hooks.lovelace.io/acme"
    );
    assert_eq!(
        h.last_delivery(acme_endpoint.endpoint.id)
            .await
            .workspace_id,
        Some(acme)
    );

    h.emit(deal_won(None)).await;
    h.run_queue().await;
    h.server.assert_sent_count(2);
    assert_eq!(
        h.server.recorded()[1].url().as_str(),
        "https://hooks.lovelace.io/app"
    );
    let webhooks = h.webhooks().await;
    assert_eq!(webhooks.endpoints(Some(acme)).await.unwrap().len(), 1);
    assert_eq!(webhooks.endpoints(None).await.unwrap().len(), 1);
}

#[tokio::test]
async fn an_event_emitted_in_a_rolled_back_unit_of_work_sends_nothing() {
    let h = harness(|b| b).await;
    ok(&h, 204);
    h.endpoint(NewEndpoint::new(URL).events(["deal.won"])).await;
    let bus = h.app.get::<EventBus>().await;

    let uow = h.app.get::<Db>().await.uow(None, None).await;
    uow.scope(bus.emit(deal_won(None))).await.unwrap();
    uow.rollback().await.unwrap();
    h.run_queue().await;
    h.server.assert_nothing_sent();

    let uow = h.app.get::<Db>().await.uow(None, None).await;
    uow.scope(bus.emit(deal_won(None))).await.unwrap();
    uow.commit().await.unwrap();
    h.run_queue().await;
    h.server.assert_sent_count(1);
}

#[tokio::test]
async fn a_queue_fake_records_the_fan_out_job() {
    let fake = QueueFake::new();
    let h = harness(|b| b.override_value(Arc::new(fake.queue()))).await;
    h.endpoint(NewEndpoint::new(URL).events(["deal.won"])).await;
    let acme = Uuid::now_v7();

    h.emit(deal_won(Some(acme))).await;

    fake.assert_dispatched::<FanOut>(|job| {
        job.event_type == "deal.won"
            && job.workspace_id == Some(acme)
            && job.message_id.starts_with("msg_")
            && job.event.is_some()
    });
    let envelope = fake.envelopes().pop().unwrap();
    assert_eq!(envelope.job_type, "webhooks.fan_out");
    assert_eq!(envelope.options.queue, "default");
    h.server.assert_nothing_sent();
}

#[tokio::test]
async fn jobs_run_on_the_configured_queue() {
    let fake = QueueFake::new();
    let h = harness(|b| {
        b.config("webhooks.queue", "webhooks")
            .override_value(Arc::new(fake.queue()))
    })
    .await;
    h.emit(deal_won(None)).await;
    assert_eq!(fake.envelopes()[0].options.queue, "webhooks");
}

#[tokio::test]
async fn dispatch_json_sends_registered_types_by_name() {
    let h = harness(|b| b).await;
    ok(&h, 204);
    let acme = Uuid::now_v7();
    let created = h
        .endpoint(
            NewEndpoint::new(URL)
                .events(["export.ready"])
                .workspace(acme),
        )
        .await;
    let webhooks = h.webhooks().await;

    let message_id = webhooks
        .dispatch_json(
            "export.ready",
            json!({ "export_id": 7, "url": "https://x/7.csv" }),
        )
        .workspace(acme)
        .send()
        .await
        .unwrap();
    h.run_queue().await;

    let request = h.server.recorded().pop().unwrap();
    assert_eq!(request.header("webhook-id"), Some(message_id.as_str()));
    let body: Value = request.json().unwrap();
    assert_eq!(body["type"], "export.ready");
    assert_eq!(body["data"]["export_id"], 7);
    StandardWebhooks::verify(&created.secret, request.headers(), request.body(), None).unwrap();

    let unknown = webhooks
        .dispatch_json("export.started", json!({}))
        .send()
        .await
        .unwrap_err();
    assert!(matches!(unknown, WebhooksError::UnknownEventType(name) if name == "export.started"));
}

#[tokio::test]
async fn a_payload_over_max_payload_bytes_fails_the_dispatch() {
    let h = harness(|b| b.config("webhooks.max_payload_bytes", 100)).await;
    let error = h
        .webhooks()
        .await
        .dispatch_json("export.ready", json!({ "blob": "x".repeat(200) }))
        .send()
        .await
        .unwrap_err();
    assert!(
        matches!(error, WebhooksError::PayloadTooLarge { limit: 100, .. }),
        "{error:?}"
    );
}

#[tokio::test]
async fn endpoints_are_checked_when_they_are_created_and_updated() {
    let h = harness(|b| b).await;
    let webhooks = h.webhooks().await;
    let uow = h.app.get::<Db>().await.uow(None, None).await;
    let invalid = |error: WebhooksError| match error {
        WebhooksError::Invalid { field, message } => format!("{field}: {message}"),
        other => panic!("expected an invalid field, got {other:?}"),
    };

    let http = webhooks
        .create_endpoint(
            &uow,
            NewEndpoint::new("http://hooks.example.com/").events(["*"]),
        )
        .await
        .unwrap_err();
    assert_eq!(invalid(http), "url: url must use https");
    let private = webhooks
        .create_endpoint(
            &uow,
            NewEndpoint::new("https://10.0.0.7/hooks").events(["*"]),
        )
        .await
        .unwrap_err();
    assert!(
        invalid(private).contains("SSRF"),
        "private addresses are refused"
    );
    let metadata = webhooks
        .create_endpoint(
            &uow,
            NewEndpoint::new("https://169.254.169.254/latest").events(["*"]),
        )
        .await
        .unwrap_err();
    assert!(invalid(metadata).starts_with("url:"));
    let credentials = webhooks
        .create_endpoint(
            &uow,
            NewEndpoint::new("https://u:p@hooks.example.com/").events(["*"]),
        )
        .await
        .unwrap_err();
    assert!(invalid(credentials).contains("credentials"));
    let no_events = webhooks
        .create_endpoint(&uow, NewEndpoint::new(URL))
        .await
        .unwrap_err();
    assert!(invalid(no_events).starts_with("events:"));
    let unknown = webhooks
        .create_endpoint(&uow, NewEndpoint::new(URL).events(["deal.wonn"]))
        .await
        .unwrap_err();
    assert!(matches!(unknown, WebhooksError::UnknownEventType(_)));
    let reserved = webhooks
        .create_endpoint(
            &uow,
            NewEndpoint::new(URL)
                .events(["*"])
                .header("webhook-signature", "x"),
        )
        .await
        .unwrap_err();
    assert!(invalid(reserved).starts_with("headers:"));

    let created = webhooks
        .create_endpoint(
            &uow,
            NewEndpoint::new(URL)
                .events(["deal.won", "deal.won", "deal.lost"])
                .description("Revenue dashboard")
                .header("X-Team", "revenue"),
        )
        .await
        .unwrap();
    assert_eq!(created.endpoint.events, ["deal.won", "deal.lost"]);
    assert_eq!(created.endpoint.headers["x-team"], "revenue");
    assert!(created.secret.starts_with("whsec_"));
    assert!(!format!("{created:?}").contains(&created.secret));

    let updated = webhooks
        .update_endpoint(
            &uow,
            created.endpoint.id,
            EndpointChanges::new()
                .url("https://hooks.lovelace.io/v2")
                .events(["*"])
                .description(""),
        )
        .await
        .unwrap();
    assert_eq!(updated.url, "https://hooks.lovelace.io/v2");
    assert_eq!(updated.events, ["*"]);
    assert_eq!(updated.description, None);
    let refused = webhooks
        .update_endpoint(
            &uow,
            created.endpoint.id,
            EndpointChanges::new().url("https://127.0.0.1/"),
        )
        .await
        .unwrap_err();
    assert!(invalid(refused).starts_with("url:"));
    let missing = webhooks
        .update_endpoint(&uow, Uuid::now_v7(), EndpointChanges::new())
        .await
        .unwrap_err();
    assert!(matches!(missing, WebhooksError::EndpointNotFound(_)));
    uow.commit().await.unwrap();
}

#[tokio::test]
async fn plain_http_is_allowed_when_require_https_is_off() {
    let h = harness(|b| b.config("webhooks.require_https", false)).await;
    h.server
        .on(Method::POST, "http://*", FakeResponse::new(200));
    let created = h
        .endpoint(NewEndpoint::new("http://hooks.example.com/dev").events(["deal.won"]))
        .await;
    h.emit(deal_won(None)).await;
    h.run_queue().await;
    assert_eq!(
        h.last_delivery(created.endpoint.id).await.status,
        DeliveryStatus::Succeeded
    );
}

#[tokio::test]
async fn deleting_an_endpoint_drops_its_deliveries() {
    let h = harness(|b| b).await;
    ok(&h, 500);
    let created = h.endpoint(NewEndpoint::new(URL).events(["deal.won"])).await;
    h.emit(deal_won(None)).await;
    h.run_queue().await;
    let delivery = h.last_delivery(created.endpoint.id).await;

    let uow = h.app.get::<Db>().await.uow(None, None).await;
    h.webhooks()
        .await
        .delete_endpoint(&uow, created.endpoint.id)
        .await
        .unwrap();
    uow.commit().await.unwrap();

    let webhooks = h.webhooks().await;
    assert!(matches!(
        webhooks.delivery(delivery.id).await,
        Err(WebhooksError::DeliveryNotFound(_))
    ));
    // The pending retry finds nothing to do.
    h.advance(Duration::from_secs(60)).await;
    h.server.assert_sent_count(1);
}

#[tokio::test]
async fn a_failed_delivery_can_be_retried_once_more() {
    let h = harness(|b| b).await;
    h.server.on_sequence(
        Method::POST,
        "https://hooks.lovelace.io/*",
        [FakeResponse::new(410), FakeResponse::new(200)],
    );
    let created = h.endpoint(NewEndpoint::new(URL).events(["deal.won"])).await;
    h.emit(deal_won(None)).await;
    h.run_queue().await;
    let delivery = h.last_delivery(created.endpoint.id).await;
    assert_eq!(delivery.status, DeliveryStatus::Failed);

    let webhooks = h.webhooks().await;
    let uow = h.app.get::<Db>().await.uow(None, None).await;
    webhooks.enable(&uow, created.endpoint.id).await.unwrap();
    uow.commit().await.unwrap();
    let queued = webhooks.retry(delivery.id).await.unwrap();
    assert_eq!(queued.id, delivery.id);
    assert_eq!(queued.status, DeliveryStatus::Pending);
    assert!(matches!(
        webhooks.retry(delivery.id).await,
        Err(WebhooksError::NotRetryable(DeliveryStatus::Pending))
    ));
    h.run_queue().await;

    let delivery = webhooks.delivery(delivery.id).await.unwrap();
    assert_eq!(delivery.status, DeliveryStatus::Succeeded);
    assert_eq!(delivery.attempts, 2);
    let ids: Vec<_> = h
        .server
        .recorded()
        .iter()
        .map(|r| r.header("webhook-id").unwrap().to_owned())
        .collect();
    assert_eq!(ids[0], ids[1], "a retry keeps the message id");
}

#[tokio::test]
async fn deliveries_to_a_disabled_endpoint_fail_without_an_attempt() {
    let h = harness(|b| b).await;
    ok(&h, 500);
    let created = h.endpoint(NewEndpoint::new(URL).events(["deal.won"])).await;
    h.emit(deal_won(None)).await;
    h.run_queue().await;

    let uow = h.app.get::<Db>().await.uow(None, None).await;
    let disabled = h
        .webhooks()
        .await
        .disable(&uow, created.endpoint.id)
        .await
        .unwrap();
    uow.commit().await.unwrap();
    assert_eq!(disabled.disabled_reason.as_deref(), Some("manual"));

    h.advance(RETRY_SCHEDULE[1]).await;
    h.server.assert_sent_count(1);
    let delivery = h.last_delivery(created.endpoint.id).await;
    assert_eq!(delivery.status, DeliveryStatus::Failed);
    assert_eq!(
        delivery.last_error.as_deref(),
        Some("the endpoint is disabled")
    );
}

#[tokio::test]
async fn send_test_posts_a_signed_test_message_at_once() {
    let h = harness(|b| b).await;
    h.server.on(
        Method::POST,
        "https://hooks.lovelace.io/*",
        FakeResponse::new(200).text("pong"),
    );
    let created = h.endpoint(NewEndpoint::new(URL).events(["deal.won"])).await;

    let attempt = h
        .webhooks()
        .await
        .send_test(created.endpoint.id)
        .await
        .unwrap();

    assert!(attempt.succeeded);
    assert_eq!(attempt.status_code, Some(200));
    assert_eq!(attempt.response_body.as_deref(), Some("pong"));
    let request = h.server.recorded().pop().unwrap();
    StandardWebhooks::verify(&created.secret, request.headers(), request.body(), None).unwrap();
    let body: Value = request.json().unwrap();
    assert_eq!(body["type"], "webhook.test");
    assert_eq!(body["data"]["endpoint_id"], json!(created.endpoint.id));
    let delivery = h.last_delivery(created.endpoint.id).await;
    assert_eq!(delivery.event_type, "webhook.test");
    assert_eq!(delivery.status, DeliveryStatus::Succeeded);
}

#[tokio::test]
async fn secrets_are_encrypted_at_rest_with_the_app_key() {
    let h = harness(|b| {
        b.config("app.key", navree_crypto::generate_key())
            .module(CryptoModule::from_config())
    })
    .await;
    ok(&h, 204);
    let created = h.endpoint(NewEndpoint::new(URL).events(["deal.won"])).await;

    let db = h.app.get::<Db>().await;
    let row = endpoint::Model::find(&*db, created.endpoint.id)
        .await
        .unwrap()
        .unwrap()
        .into_inner();
    assert!(!row.secret.contains(&created.secret[6..]));
    assert!(!row.secret.starts_with("whsec_"));
    let serialized = serde_json::to_value(&row).unwrap();
    assert!(serialized.get("secret").is_none());

    h.emit(deal_won(None)).await;
    h.run_queue().await;
    let request = h.server.recorded().pop().unwrap();
    StandardWebhooks::verify(&created.secret, request.headers(), request.body(), None).unwrap();
}

#[tokio::test]
async fn deleting_a_workspace_deletes_its_endpoints() {
    let h = harness(|b| b).await;
    let acme = Uuid::now_v7();
    h.endpoint(NewEndpoint::new(URL).events(["*"]).workspace(acme))
        .await;
    h.endpoint(NewEndpoint::new(URL).events(["*"])).await;

    h.emit(navree_workspace::events::WorkspaceDeleted {
        workspace_id: acme,
        slug: "acme".to_owned(),
    })
    .await;

    let webhooks: Arc<Webhooks> = h.webhooks().await;
    assert!(webhooks.endpoints(Some(acme)).await.unwrap().is_empty());
    assert_eq!(webhooks.endpoints(None).await.unwrap().len(), 1);
}

#[tokio::test]
async fn event_types_list_every_registration() {
    let h = harness(|b| b).await;
    let types = h.webhooks().await.event_types();
    let names: Vec<_> = types.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["deal.lost", "deal.won", "export.ready"]);
    assert_eq!(types[1].description, "A deal moved to won.");
}

#[tokio::test]
async fn change_events_never_carry_the_secrets() {
    for key in [true, false] {
        let h = harness(|b| {
            if key {
                b.config("app.key", navree_crypto::generate_key())
                    .module(CryptoModule::from_config())
            } else {
                b
            }
        })
        .await;
        let webhooks = h.webhooks().await;
        let db = h.app.get::<Db>().await;
        let uow = db.uow(None, None).await;
        let created = webhooks
            .create_endpoint(&uow, NewEndpoint::new(URL).events(["deal.won"]))
            .await
            .unwrap();
        let id = created.endpoint.id;
        let rotated = webhooks.rotate_secret(&uow, id).await.unwrap();
        webhooks.disable(&uow, id).await.unwrap();
        webhooks.delete_endpoint(&uow, id).await.unwrap();

        let events = uow.changes();
        let ops: Vec<_> = events.iter().map(|e| format!("{:?}", e.op)).collect();
        assert_eq!(ops, ["Created", "Updated", "Updated", "Deleted"]);
        let written = serde_json::to_string(&events).unwrap();
        for secret in [&created.secret, &rotated] {
            assert!(!written.contains(&secret[6..]), "{written}");
        }
        assert!(!written.contains("\"previous_secret\""), "{written}");
        assert!(!written.contains("\"secret\""), "{written}");
        assert!(written.contains("hooks.lovelace.io"), "{written}");
        uow.rollback().await.unwrap();
    }
}

/// An endpoint's URL is often a secret of its own: its path doesn't reach what
/// an attempt stores or what is shown of it.
#[tokio::test]
async fn a_failed_attempt_does_not_store_the_path_of_the_endpoint() {
    let h = harness(|b| b).await;
    h.server.on(
        Method::POST,
        "https://hooks.lovelace.io/*",
        FakeResponse::connection_failed(),
    );
    let created = h
        .endpoint(NewEndpoint::new("https://hooks.lovelace.io/SECRETPATH?t=QUERYSECRET").events(["deal.won"]))
        .await;
    h.emit(deal_won(None)).await;
    h.run_queue().await;
    let delivery = h.last_delivery(created.endpoint.id).await;
    let last_error = delivery.last_error.clone().unwrap();
    let attempts = h.webhooks().await.attempts(delivery.id).await.unwrap();
    let error = attempts[0].error.clone().unwrap();
    for text in [&last_error, &error] {
        assert!(text.contains("connect"), "{text}");
        assert!(!text.contains("SECRETPATH"), "{text}");
        assert!(!text.contains("QUERYSECRET"), "{text}");
    }
}
