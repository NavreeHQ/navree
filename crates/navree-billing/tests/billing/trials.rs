//! Trials with and without a subscription.

use chrono::TimeDelta;
use navree_billing::{BillingError, SubscriptionStatus};

use crate::support::{World, builder, start};

#[tokio::test]
async fn a_trial_without_a_subscription_lasts_its_days() {
    let world = World::new().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let acme = world.workspace(ada.id, "Acme").await;
    let billing = world.billing().await;
    assert!(!billing.on_trial(&acme).await.unwrap());
    assert_eq!(billing.trial_ends_at(&acme).await.unwrap(), None);
    assert!(billing.access(&acme).await.unwrap().is_none());

    let ends = billing.start_trial(&acme, 14).await.unwrap();
    assert_eq!(ends, start() + TimeDelta::days(14));
    assert!(billing.on_trial(&acme).await.unwrap());
    assert_eq!(billing.trial_ends_at(&acme).await.unwrap(), Some(ends));
    // It never reaches the provider: no customer id.
    let customer = billing.customer(&acme).await.unwrap().unwrap();
    assert_eq!(customer.provider_customer_id, None);
    assert_eq!(customer.trial_ends_at, Some(ends));
    // There is no subscription, but `access` grants the trial plan
    // (default: the first plan by key).
    assert!(billing.subscription(&acme).await.unwrap().is_none());
    let access = billing.access(&acme).await.unwrap().unwrap();
    assert!(access.is_trial_without_subscription() && access.on_trial() && access.valid());
    assert_eq!(access.plan, "business");
    assert_eq!(access.status, SubscriptionStatus::Trialing);

    world.advance_days(14);
    assert!(!billing.on_trial(&acme).await.unwrap());
    assert!(billing.access(&acme).await.unwrap().is_none());
    assert_eq!(
        billing.trial_ends_at(&acme).await.unwrap(),
        Some(ends),
        "the end stays readable"
    );
    let error = billing.start_trial(&acme, 0).await.unwrap_err();
    assert!(matches!(error, BillingError::Invalid { field: "days", .. }));
}

#[tokio::test]
async fn the_trial_plan_is_configurable_and_a_checkout_keeps_the_trial_row() {
    let world = World::with(builder().config("billing.trial_plan", "pro")).await;
    let ada = world.user("Ada", "ada@example.com").await;
    let acme = world.workspace(ada.id, "Acme").await;
    let billing = world.billing().await;
    billing.start_trial(&acme, 7).await.unwrap();
    assert_eq!(billing.trial_plan().unwrap().key, "pro");
    assert!(billing.access(&acme).await.unwrap().unwrap().on_plan("pro"));
    // A checkout during the trial links the customer to the same row.
    world.subscribe(&acme, "pro").await;
    let customer = billing.customer(&acme).await.unwrap().unwrap();
    assert!(customer.provider_customer_id.is_some());
    assert!(customer.trial_ends_at.is_some());
    let access = billing.access(&acme).await.unwrap().unwrap();
    assert!(
        !access.is_trial_without_subscription(),
        "the subscription wins"
    );
}

#[tokio::test]
async fn a_subscriptions_trial_counts_as_on_trial() {
    let world = World::new().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let acme = world.workspace(ada.id, "Acme").await;
    let billing = world.billing().await;
    let sub = world.subscribe(&acme, "pro").await;
    assert!(billing.on_trial(&acme).await.unwrap());
    assert_eq!(
        billing.trial_ends_at(&acme).await.unwrap(),
        sub.trial_ends_at
    );
    // Once the trial ends and the first payment goes through, it is active.
    world.advance_days(14);
    world.fake().await.end_trial(&acme).await.unwrap();
    world.run_queue().await;
    let active = billing.subscription(&acme).await.unwrap().unwrap();
    assert_eq!(active.status, SubscriptionStatus::Active);
    assert!(!billing.on_trial(&acme).await.unwrap());
    let invoices = billing.invoices(&acme).await.unwrap();
    assert_eq!(invoices.len(), 2);
    assert_eq!(
        invoices[0].total_cents,
        navree_billing::FAKE_UNIT_AMOUNT_CENTS
    );
}
