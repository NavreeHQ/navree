//! The webhook pipeline: verification, storage once, the job, the state
//! machine with duplicate and out-of-order events, and the events.

use http::{HeaderMap, HeaderValue};
use navree_billing::{
    BillingError, STRIPE_SIGNATURE_HEADER, SubscriptionStatus, WebhookError, sign_stripe_webhook,
};
use serde_json::{Value, json};

use crate::support::World;

fn signed(secret: &str, timestamp: i64, body: &[u8]) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        STRIPE_SIGNATURE_HEADER,
        HeaderValue::from_str(&sign_stripe_webhook(secret, timestamp, body)).unwrap(),
    );
    headers
}

#[tokio::test]
async fn a_completed_checkout_becomes_a_subscription_on_trial() {
    let world = World::new().await;
    let ada = world.user("Anu", "anu@example.com").await;
    let agra = world.workspace(ada.id, "Agra").await;
    let billing = world.billing().await;
    let log = world.log().await;

    let checkout = billing.checkout(&agra, "pro").create().await.unwrap();
    world
        .fake()
        .await
        .complete_checkout(&checkout.id)
        .await
        .unwrap();
    // Stored, not processed until the queue runs.
    assert!(billing.subscription(&agra).await.unwrap().is_none());
    let events = billing.events(false).await.unwrap();
    assert_eq!(events.len(), 3);
    assert!(events.iter().all(|e| e.processed_at.is_none()));
    world.run_queue().await;

    let sub = billing.subscription(&agra).await.unwrap().unwrap();
    assert!(sub.on_trial() && sub.on_plan("pro") && sub.valid());
    assert_eq!(sub.status, SubscriptionStatus::Trialing);
    assert_eq!(sub.quantity, 1);
    assert_eq!(
        sub.trial_ends_at,
        Some(crate::support::start() + chrono::TimeDelta::days(14))
    );
    assert!(billing.subscribed(&agra, "pro").await.unwrap());
    assert!(!billing.subscribed(&agra, "business").await.unwrap());
    let events = billing.events(false).await.unwrap();
    assert!(
        events
            .iter()
            .all(|e| e.processed_at.is_some() && e.attempts == 1)
    );

    let emitted = log.take();
    let names: Vec<&str> = emitted.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["subscription.created", "invoice.paid"]);
    let created = &emitted[0].1;
    assert_eq!(created["billable_type"], "workspace");
    assert_eq!(created["billable_id"], json!(agra.id));
    assert_eq!(created["subscription_id"], json!(sub.id));
    assert_eq!(created["plan"], "pro");
    let paid = &emitted[1].1;
    assert_eq!(paid["amount_cents"], 0, "nothing to pay during the trial");
    assert_eq!(paid["subscription_id"], json!(sub.id));
    assert!(paid["invoice_id"].as_str().unwrap().starts_with("in_fake_"));
}

#[tokio::test]
async fn a_redelivered_event_is_stored_and_processed_once() {
    let world = World::new().await;
    let ada = world.user("Anu", "anu@example.com").await;
    let agra = world.workspace(ada.id, "Agra").await;
    let billing = world.billing().await;
    world.subscribe(&agra, "starter").await;
    world.log().await.take();

    let fake = world.fake().await;
    let body = serde_json::to_vec(&json!({
        "id": "evt_redelivered",
        "object": "event",
        "type": "invoice.paid",
        "created": crate::support::start().timestamp(),
        "data": { "object": {
            "id": "in_manual", "customer": billing.customer(&agra).await.unwrap().unwrap().provider_customer_id,
            "amount_paid": 2900, "currency": "usd", "status": "paid"
        } }
    }))
    .unwrap();
    let headers = signed(
        fake.webhook_secret(),
        crate::support::start().timestamp(),
        &body,
    );
    let first = billing.receive_webhook(&headers, &body).await.unwrap();
    assert!(!first.duplicate);
    let second = billing.receive_webhook(&headers, &body).await.unwrap();
    assert!(second.duplicate);
    assert_eq!(second.event_id, first.event_id);
    world.run_queue().await;
    assert_eq!(
        billing
            .events(false)
            .await
            .unwrap()
            .iter()
            .filter(|e| e.provider_event_id == "evt_redelivered")
            .count(),
        1
    );
    let emitted = world.log().await.take();
    assert_eq!(emitted.len(), 1);
    assert_eq!(emitted[0].0, "invoice.paid");
    assert_eq!(emitted[0].1["amount_cents"], 2900);
    assert_eq!(emitted[0].1["subscription_id"], Value::Null);
    // Processing again is a no-op.
    billing.process_event(first.event_id).await.unwrap();
    assert!(world.log().await.take().is_empty());
}

#[tokio::test]
async fn bad_signatures_and_payloads_are_refused() {
    let world = World::new().await;
    let billing = world.billing().await;
    let fake = world.fake().await;
    let body = br#"{"id":"evt_1","type":"customer.created","created":1,"data":{"object":{}}}"#;
    let now = crate::support::start().timestamp();

    let error = billing
        .receive_webhook(&signed("whsec_wrong", now, body), body)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        BillingError::InvalidWebhook(WebhookError::SignatureMismatch)
    ));
    assert_eq!(error.to_app_error().status.as_u16(), 400);

    let error = billing
        .receive_webhook(&signed(fake.webhook_secret(), now - 301, body), body)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        BillingError::InvalidWebhook(WebhookError::Expired)
    ));
    // Within the tolerance it is fine.
    billing
        .receive_webhook(&signed(fake.webhook_secret(), now - 299, body), body)
        .await
        .unwrap();

    let error = billing
        .receive_webhook(&HeaderMap::new(), body)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        BillingError::InvalidWebhook(WebhookError::MissingSignature(_))
    ));

    let garbage = b"not json";
    let error = billing
        .receive_webhook(&signed(fake.webhook_secret(), now, garbage), garbage)
        .await
        .unwrap_err();
    assert!(matches!(error, BillingError::InvalidPayload(_)));
    assert_eq!(error.to_app_error().status.as_u16(), 400);
}

#[tokio::test]
async fn older_events_do_not_overwrite_newer_state() {
    let world = World::new().await;
    let ada = world.user("Anu", "anu@example.com").await;
    let agra = world.workspace(ada.id, "Agra").await;
    let billing = world.billing().await;
    let sub = world.subscribe(&agra, "starter").await;
    let fake = world.fake().await;
    let provider_id = sub.provider_id.clone().unwrap();
    let customer = sub.provider_customer_id.clone().unwrap();
    let object = |status: &str, price: &str| {
        json!({
            "id": provider_id, "object": "subscription", "customer": customer, "status": status,
            "items": { "data": [{ "id": "si_x", "quantity": 1,
                "price": { "id": price, "recurring": { "usage_type": "licensed" } },
                "current_period_start": 1_790_000_000, "current_period_end": 1_792_600_000 }] }
        })
    };
    let now = crate::support::start().timestamp();
    let send = |created: i64, object: Value| {
        let body = serde_json::to_vec(&json!({
            "id": format!("evt_at_{created}"), "object": "event",
            "type": "customer.subscription.updated", "created": created,
            "data": { "object": object }
        }))
        .unwrap();
        let headers = signed(fake.webhook_secret(), now, &body);
        let billing = billing.clone();
        async move { billing.receive_webhook(&headers, &body).await.unwrap() }
    };
    // The newer state arrives first, the older one after it.
    send(now + 20, object("active", "price_business_m")).await;
    send(now + 10, object("past_due", "price_starter_m")).await;
    world.run_queue().await;
    let current = billing.subscription(&agra).await.unwrap().unwrap();
    assert!(current.on_plan("business"));
    assert_eq!(current.status, SubscriptionStatus::Active);

    // A deletion ends it; a stale update after it changes nothing.
    send(now + 30, object("canceled", "price_business_m")).await;
    send(now + 25, object("active", "price_business_m")).await;
    world.run_queue().await;
    let ended = billing.subscription(&agra).await.unwrap().unwrap();
    assert_eq!(ended.status, SubscriptionStatus::Canceled);
    assert!(!ended.valid());
}

#[tokio::test]
async fn a_failing_event_records_its_error_and_retries() {
    let world = World::new().await;
    let billing = world.billing().await;
    let fake = world.fake().await;
    // A subscription event whose status the mapping does not know: stored,
    // then the job fails on it.
    let receipt = fake
        .send_webhook(
            "customer.subscription.updated",
            json!({ "id": "sub_odd", "customer": "cus_odd", "status": "mystery", "items": { "data": [] } }),
        )
        .await;
    // Verification passes, but parsing the body fails before storage.
    assert!(matches!(receipt, Err(BillingError::InvalidPayload(_))));

    // An invoice of an unknown customer is skipped, not failed.
    let ok = fake
        .send_webhook(
            "invoice.payment_failed",
            json!({ "id": "in_1", "customer": "cus_unknown", "amount_due": 100, "currency": "usd" }),
        )
        .await
        .unwrap();
    world.run_queue().await;
    let stored = billing.events(false).await.unwrap();
    let row = stored.iter().find(|e| e.id == ok.event_id).unwrap();
    assert!(row.processed_at.is_some());
    assert!(billing.events(true).await.unwrap().is_empty());

    // A stored event that the job cannot process keeps its error and stays
    // unprocessed until an attempt succeeds.
    let stuck = fake
        .send_webhook(
            "invoice.paid",
            json!({ "id": "in_2", "customer": "cus_unknown", "amount_paid": 100, "currency": "usd" }),
        )
        .await
        .unwrap();
    let db = world.db().await;
    let set_payload = |payload: String| {
        use navree_billing::entity::event;
        use navree_orm::Model as _;
        let db = db.clone();
        async move {
            event::Model::query()
                .where_(event::Model::ID.eq(stuck.event_id))
                .update(&*db, [event::Model::PAYLOAD.set(payload)])
                .await
                .unwrap();
        }
    };
    let original = billing
        .events(false)
        .await
        .unwrap()
        .into_iter()
        .find(|e| e.id == stuck.event_id)
        .unwrap()
        .payload;
    set_payload("{}".to_owned()).await;
    let error = billing.process_event(stuck.event_id).await.unwrap_err();
    assert!(matches!(error, BillingError::Provider(_)), "{error}");
    let failed = billing.events(true).await.unwrap();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].attempts, 1);
    assert!(failed[0].error.as_deref().unwrap().contains("missing `id`"));
    set_payload(original).await;
    billing.process_event(stuck.event_id).await.unwrap();
    assert!(billing.events(true).await.unwrap().is_empty());
    let row = billing
        .events(false)
        .await
        .unwrap()
        .into_iter()
        .find(|e| e.id == stuck.event_id)
        .unwrap();
    assert_eq!(row.attempts, 2);
    assert_eq!(row.error, None);
}

#[tokio::test]
async fn the_trial_ending_and_failed_payments_reach_listeners() {
    let world = World::new().await;
    let ada = world.user("Anu", "anu@example.com").await;
    let agra = world.workspace(ada.id, "Agra").await;
    let billing = world.billing().await;
    world.subscribe(&agra, "pro").await;
    let log = world.log().await;
    log.take();
    let fake = world.fake().await;

    world.advance_days(14);
    fake.fail_next_payment(&agra);
    fake.end_trial(&agra).await.unwrap();
    world.run_queue().await;
    let names = log.names();
    assert_eq!(
        names,
        [
            "subscription.trial_ending",
            "subscription.updated",
            "invoice.payment_failed"
        ]
    );
    let sub = billing.subscription(&agra).await.unwrap().unwrap();
    assert!(
        sub.past_due() && sub.valid(),
        "past due within the grace days"
    );
    assert_eq!(sub.past_due_since, Some(world.clock_now()));

    world.advance_days(8);
    let sub = billing.subscription(&agra).await.unwrap().unwrap();
    assert!(sub.past_due() && !sub.valid(), "past the grace days");
}

/// `billing.queue` names the queue of the `billing.process_event` jobs.
#[tokio::test]
async fn the_jobs_go_on_the_configured_queue() {
    let world = World::with(crate::support::builder().config("billing.queue", "billing")).await;
    let ada = world.user("Anu", "anu@example.com").await;
    let agra = world.workspace(ada.id, "Agra").await;
    let billing = world.billing().await;
    let queue = world.app.get::<navree_queue::Queue>().await;
    // The seat listener's job for the owner joining is on `default`.
    let default_before = queue.pending_count("default").await.unwrap();
    let checkout = billing.checkout(&agra, "starter").create().await.unwrap();
    world
        .fake()
        .await
        .complete_checkout(&checkout.id)
        .await
        .unwrap();

    assert_eq!(queue.pending_count("billing").await.unwrap(), 3);
    assert_eq!(
        queue.pending_count("default").await.unwrap(),
        default_before
    );
    let stats = queue
        .work(navree_queue::WorkOptions {
            queues: vec!["billing".to_owned()],
            stop_when_empty: true,
            ..navree_queue::WorkOptions::default()
        })
        .await
        .unwrap();
    assert_eq!((stats.processed, stats.succeeded), (3, 3));
    assert!(billing.subscribed(&agra, "starter").await.unwrap());
}

/// Without a queue module the job runs at once, inside the webhook request.
#[tokio::test]
async fn without_a_queue_the_event_is_processed_at_once() {
    use std::sync::Arc;

    use navree_billing::{BillableRef, Billing, BillingModule};
    use navree_contracts::{Clock, FixedClock};
    use navree_db::DatabaseModule;
    use navree_testing::TestApp;

    let app = crate::servers::database(TestApp::builder())
        .config("billing.provider", "fake")
        .config("billing.routes", false)
        .config(
            "billing.return_url",
            "https://app.acme.test/settings/billing",
        )
        .config("billing.plans", crate::support::plans())
        .module(DatabaseModule::from_config())
        .module(BillingModule::from_config())
        .override_value::<dyn Clock>(Arc::new(FixedClock::new(crate::support::start())))
        .build()
        .await;
    let billing = app.get::<Billing>().await;
    let agra = BillableRef {
        billable_type: "organization".into(),
        billable_id: uuid::Uuid::now_v7(),
        name: "Agra".into(),
        email: Some("billing@acme.test".into()),
    };
    let checkout = billing.checkout(&agra, "starter").create().await.unwrap();
    billing
        .fake()
        .unwrap()
        .complete_checkout(&checkout.id)
        .await
        .unwrap();

    let sub = billing.subscription(&agra).await.unwrap().unwrap();
    assert!(sub.valid() && sub.on_plan("starter"));
    assert!(
        billing
            .events(false)
            .await
            .unwrap()
            .iter()
            .all(|e| e.processed_at.is_some())
    );
}

/// `billing.webhook_tolerance_secs` bounds the signature's age.
#[tokio::test]
async fn the_tolerance_is_configurable() {
    let world =
        World::with(crate::support::builder().config("billing.webhook_tolerance_secs", 60)).await;
    let billing = world.billing().await;
    let fake = world.fake().await;
    let body = br#"{"id":"evt_t","type":"customer.created","created":1,"data":{"object":{}}}"#;
    let now = crate::support::start().timestamp();

    let error = billing
        .receive_webhook(&signed(fake.webhook_secret(), now - 61, body), body)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        BillingError::InvalidWebhook(WebhookError::Expired)
    ));
    billing
        .receive_webhook(&signed(fake.webhook_secret(), now + 59, body), body)
        .await
        .unwrap();
}

trait ClockNow {
    fn clock_now(&self) -> chrono::DateTime<chrono::Utc>;
}

impl ClockNow for World {
    fn clock_now(&self) -> chrono::DateTime<chrono::Utc> {
        use navree_contracts::Clock;
        self.clock.now()
    }
}
