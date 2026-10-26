//! Changing subscriptions: swap, quantity, cancel, resume, pause, renewals
//! and grace periods.

use navree_billing::{BillingError, Interval, Proration, SubscriptionStatus};

use crate::support::World;

#[tokio::test]
async fn swaps_change_the_plan_interval_and_meters() {
    let world = World::new().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let acme = world.workspace(ada.id, "Acme").await;
    let billing = world.billing().await;
    let started = world.subscribe(&acme, "pro").await;
    let log = world.log().await;
    log.take();

    let swapped = billing.swap(&acme, "business").await.unwrap();
    assert!(swapped.on_plan("business"));
    assert_eq!(swapped.interval, Interval::Monthly);
    assert_eq!(swapped.id, started.id, "the same subscription");
    assert_eq!(log.names(), ["subscription.updated"]);
    // The local copy is what `subscription` reads.
    let read = billing.subscription(&acme).await.unwrap().unwrap();
    assert!(read.on_plan("business"));

    let yearly = billing.swap_yearly(&acme, "pro").await.unwrap();
    assert!(yearly.on_plan("pro"));
    assert_eq!(yearly.interval, Interval::Yearly);
    let monthly = billing.swap_monthly(&acme, "pro").await.unwrap();
    assert_eq!(monthly.interval, Interval::Monthly);
    let now = billing
        .swap_with(&acme, "business", Proration::InvoiceNow)
        .await
        .unwrap();
    assert!(now.on_plan("business"));

    let error = billing.swap(&acme, "enterprise").await.unwrap_err();
    assert!(matches!(error, BillingError::UnknownPlan(_)));
    let error = billing.swap_yearly(&acme, "starter").await.unwrap_err();
    assert!(matches!(error, BillingError::NoPrice { .. }));
    // The confirming webhook of a provider would be a no-op: the fake sends
    // none, and nothing else was emitted.
    world.run_queue().await;
    assert_eq!(log.names(), ["subscription.updated"; 3]);
}

#[tokio::test]
async fn quantity_changes_reach_the_provider_and_the_copy() {
    let world = World::new().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let acme = world.workspace(ada.id, "Acme").await;
    let billing = world.billing().await;
    world.subscribe(&acme, "business").await;
    let sub = billing.update_quantity(&acme, 12).await.unwrap();
    assert_eq!(sub.quantity, 12);
    assert_eq!(
        billing.subscription(&acme).await.unwrap().unwrap().quantity,
        12
    );
    let error = billing.update_quantity(&acme, 0).await.unwrap_err();
    assert!(matches!(
        error,
        BillingError::Invalid {
            field: "quantity",
            ..
        }
    ));
    // A renewal now bills 12 seats.
    world.fake().await.renew(&acme).await.unwrap();
    world.run_queue().await;
    let invoices = billing.invoices(&acme).await.unwrap();
    assert_eq!(
        invoices[0].total_cents,
        12 * navree_billing::FAKE_UNIT_AMOUNT_CENTS
    );
}

#[tokio::test]
async fn a_cancel_keeps_access_until_the_period_ends_and_resume_undoes_it() {
    let world = World::new().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let acme = world.workspace(ada.id, "Acme").await;
    let billing = world.billing().await;
    let started = world.subscribe(&acme, "business").await;
    let log = world.log().await;
    log.take();

    let canceled = billing.cancel(&acme).await.unwrap();
    assert!(canceled.canceled() && canceled.on_grace_period() && canceled.valid());
    assert_eq!(canceled.ends_at, started.current_period_end);
    let emitted = log.take();
    assert_eq!(emitted.len(), 1);
    assert_eq!(emitted[0].0, "subscription.canceled");
    assert_eq!(
        emitted[0].1["ends_at"],
        serde_json::json!(started.current_period_end.unwrap())
    );

    let resumed = billing.resume(&acme).await.unwrap();
    assert!(!resumed.canceled() && resumed.valid());
    assert_eq!(log.names(), ["subscription.updated"]);
    let error = billing.resume(&acme).await.unwrap_err();
    assert!(
        matches!(error, BillingError::Invalid { .. }),
        "nothing to resume"
    );

    // Canceled again, the renewal ends it.
    billing.cancel(&acme).await.unwrap();
    world.advance_days(31);
    world.fake().await.renew(&acme).await.unwrap();
    world.run_queue().await;
    let ended = billing.subscription(&acme).await.unwrap().unwrap();
    assert_eq!(ended.status, SubscriptionStatus::Canceled);
    assert!(!ended.valid() && ended.ended());
    assert!(!billing.subscribed(&acme, "business").await.unwrap());
    let error = billing.cancel(&acme).await.unwrap_err();
    assert!(matches!(error, BillingError::NoSubscription));
    // A new checkout is possible again.
    billing.checkout(&acme, "starter").create().await.unwrap();
}

#[tokio::test]
async fn cancel_now_ends_it_at_once() {
    let world = World::new().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let acme = world.workspace(ada.id, "Acme").await;
    let billing = world.billing().await;
    world.subscribe(&acme, "starter").await;
    let sub = billing.cancel_now(&acme).await.unwrap();
    assert_eq!(sub.status, SubscriptionStatus::Canceled);
    assert!(!sub.valid() && !sub.on_grace_period());
    assert_eq!(
        world.log().await.names().last().unwrap(),
        "subscription.canceled"
    );
}

#[tokio::test]
async fn pause_stops_access_and_unpause_restores_it() {
    let world = World::new().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let acme = world.workspace(ada.id, "Acme").await;
    let billing = world.billing().await;
    world.subscribe(&acme, "starter").await;
    let paused = billing.pause(&acme).await.unwrap();
    assert!(paused.paused() && !paused.valid());
    let unpaused = billing.unpause(&acme).await.unwrap();
    assert!(!unpaused.paused() && unpaused.valid());
}

#[tokio::test]
async fn changes_without_a_subscription_are_404s() {
    let world = World::new().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let acme = world.workspace(ada.id, "Acme").await;
    let billing = world.billing().await;
    for result in [
        billing.swap(&acme, "pro").await,
        billing.update_quantity(&acme, 2).await,
        billing.cancel(&acme).await,
        billing.cancel_now(&acme).await,
        billing.resume(&acme).await,
        billing.pause(&acme).await,
        billing.unpause(&acme).await,
    ] {
        let error = result.unwrap_err();
        assert!(matches!(error, BillingError::NoSubscription), "{error}");
        assert_eq!(error.to_app_error().status.as_u16(), 404);
    }
    assert!(billing.subscription(&acme).await.unwrap().is_none());
    assert!(billing.subscriptions(&acme).await.unwrap().is_empty());
}

#[tokio::test]
async fn renewals_move_the_period_and_failed_payments_start_the_grace_days() {
    let world = World::new().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let acme = world.workspace(ada.id, "Acme").await;
    let billing = world.billing().await;
    let first = world.subscribe(&acme, "business").await;
    let fake = world.fake().await;

    world.advance_days(31);
    fake.renew(&acme).await.unwrap();
    world.run_queue().await;
    let renewed = billing.subscription(&acme).await.unwrap().unwrap();
    assert_eq!(renewed.current_period_start, first.current_period_end);
    assert!(renewed.current_period_end > first.current_period_end);
    assert_eq!(renewed.status, SubscriptionStatus::Active);

    let log = world.log().await;
    log.take();
    fake.fail_next_payment(&acme);
    world.advance_days(30);
    fake.renew(&acme).await.unwrap();
    world.run_queue().await;
    assert_eq!(
        log.names(),
        ["subscription.updated", "invoice.payment_failed"]
    );
    let late = billing.subscription(&acme).await.unwrap().unwrap();
    assert!(late.past_due() && late.valid());
    world.advance_days(7);
    let late = billing.subscription(&acme).await.unwrap().unwrap();
    assert!(!late.valid(), "billing.past_due_grace_days is 7");

    // Paying recovers it.
    world.advance_days(1);
    fake.renew(&acme).await.unwrap();
    world.run_queue().await;
    let paid = billing.subscription(&acme).await.unwrap().unwrap();
    assert!(paid.valid() && !paid.past_due());
    assert_eq!(paid.past_due_since, None);
}
