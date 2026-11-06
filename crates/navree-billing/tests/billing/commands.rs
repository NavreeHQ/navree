//! The `billing:*` commands and invoice caching.

use navree_billing::SubscriptionStatus;

use crate::support::World;

#[tokio::test]
async fn plans_webhook_and_report_usage_commands_run() {
    let world = World::new().await;
    world.app.run_command("billing:plans", &[]).await.unwrap();
    world
        .app
        .run_command(
            "billing:webhook",
            &["https://api.acme.test/api/billing/webhook"],
        )
        .await
        .unwrap();
    assert!(world.app.run_command("billing:webhook", &[]).await.is_err());
    world
        .app
        .run_command("billing:report-usage", &[])
        .await
        .unwrap();
}

#[tokio::test]
async fn billing_plans_checks_every_price_id() {
    // The fake knows the configured prices (base and metered); a price it
    // does not know is reported missing.
    let world = World::new().await;
    world
        .app
        .run_command("billing:plans", &["--verbose"])
        .await
        .unwrap();
    let billing = world.billing().await;
    assert!(billing.provider().price_exists("price_ai").await.unwrap());
    assert!(!billing.provider().price_exists("price_nope").await.unwrap());
}

#[tokio::test]
async fn sync_pulls_the_providers_state() {
    let world = World::new().await;
    let ada = world.user("Anu", "anu@example.com").await;
    let agra = world.workspace(ada.id, "Agra").await;
    let billing = world.billing().await;
    world.subscribe(&agra, "starter").await;

    // The provider changed something the local copy missed: a webhook that
    // never arrived. Cancel at the fake directly, behind Billing's back.
    let sub = billing.subscription(&agra).await.unwrap().unwrap();
    billing
        .provider()
        .cancel_subscription(sub.provider_id.as_deref().unwrap(), false)
        .await
        .unwrap();
    assert!(billing.subscription(&agra).await.unwrap().unwrap().valid());

    world.advance_days(1);
    let id = agra.id.to_string();
    world
        .app
        .run_command("billing:sync", &["workspace", id.as_str()])
        .await
        .unwrap();
    let synced = billing.subscription(&agra).await.unwrap().unwrap();
    assert_eq!(synced.status, SubscriptionStatus::Canceled);
    assert!(
        world
            .app
            .run_command("billing:sync", &["workspace"])
            .await
            .is_err()
    );
    assert!(
        world
            .app
            .run_command("billing:sync", &["workspace", "not-a-uuid"])
            .await
            .is_err()
    );
}

#[tokio::test]
async fn events_are_listed_and_pruned() {
    let world = World::new().await;
    let ada = world.user("Anu", "anu@example.com").await;
    let agra = world.workspace(ada.id, "Agra").await;
    let billing = world.billing().await;
    world.subscribe(&agra, "starter").await;
    world.app.run_command("billing:events", &[]).await.unwrap();
    world
        .app
        .run_command("billing:events", &["--failed"])
        .await
        .unwrap();
    assert_eq!(billing.events(false).await.unwrap().len(), 3);

    // Not old enough yet.
    world
        .app
        .run_command("billing:prune-events", &["--days", "30"])
        .await
        .unwrap();
    assert_eq!(billing.events(false).await.unwrap().len(), 3);
    world.advance_days(31);
    world
        .app
        .run_command("billing:prune-events", &["--days", "30"])
        .await
        .unwrap();
    assert!(billing.events(false).await.unwrap().is_empty());
    assert!(
        world
            .app
            .run_command("billing:prune-events", &["--days"])
            .await
            .is_err()
    );
    assert_eq!(billing.prune_events(90).await.unwrap(), 0);
}

#[tokio::test]
async fn invoices_are_cached_until_a_payment_event() {
    let world = World::new().await;
    let ada = world.user("Anu", "anu@example.com").await;
    let agra = world.workspace(ada.id, "Agra").await;
    let billing = world.billing().await;
    assert!(
        billing.invoices(&agra).await.unwrap().is_empty(),
        "no customer yet"
    );
    world.subscribe(&agra, "starter").await;
    let first = billing.invoices(&agra).await.unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].currency, "usd");
    assert!(first[0].pdf_url.is_some() && first[0].hosted_url.is_some());
    assert!(first[0].number.starts_with("FAKE-"));

    // The renewal's invoice.paid forgets the cache.
    world.advance_days(31);
    world.fake().await.renew(&agra).await.unwrap();
    world.run_queue().await;
    let second = billing.invoices(&agra).await.unwrap();
    assert_eq!(second.len(), 2);
    assert!(second[0].created_at > second[1].created_at, "newest first");
}
