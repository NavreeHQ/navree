//! `Billing::checkout` and its builder.

use navree_billing::{BillingError, CheckoutUrl, Interval};

use crate::support::{World, platform};

#[tokio::test]
async fn a_checkout_charges_the_monthly_price_with_the_plans_trial() {
    let world = World::new().await;
    let ada = world.user("Anu", "anu@example.com").await;
    let agra = world.workspace(ada.id, "Agra").await;
    let billing = world.billing().await;

    let checkout = billing
        .checkout(&agra, "pro")
        .customer_email(&ada.email)
        .create()
        .await
        .unwrap();
    assert_eq!(checkout.plan, "pro");
    assert_eq!(checkout.interval, Interval::Monthly);
    assert_eq!(checkout.trial_days, Some(14));
    assert_eq!(checkout.quantity, 1, "seats: one member");
    assert!(checkout.url.starts_with("https://checkout.fake.test/c/"));
    assert_eq!(
        CheckoutUrl::from(checkout.clone()).url,
        checkout.url,
        "CheckoutUrl carries the url"
    );

    let fake = world.fake().await;
    let recorded = fake.assert_checkout(|c| c.plan == "pro" && c.trial_days == Some(14));
    assert_eq!(recorded.id, checkout.id);
    assert_eq!(recorded.price_id, "price_pro_m");
    assert_eq!(recorded.metered_prices, ["price_ai"]);
    assert_eq!(recorded.billable_type, "workspace");
    assert_eq!(recorded.billable_id, agra.id);
    assert_eq!(recorded.customer_email.as_deref(), Some("anu@example.com"));
    assert_eq!(
        recorded.success_url,
        "https://app.acme.test/settings/billing"
    );
    assert_eq!(
        recorded.cancel_url,
        "https://app.acme.test/settings/billing"
    );
    assert_eq!(recorded.metadata["billable_id"], agra.id.to_string());
    assert_eq!(recorded.metadata["plan"], "pro");
    assert!(!recorded.allow_promotion_codes);

    // The customer was created and linked on the first checkout.
    let customer = billing.customer(&agra).await.unwrap().unwrap();
    assert_eq!(customer.provider.as_deref(), Some("fake"));
    assert_eq!(customer.provider_customer_id, recorded.customer_id);
    // A second checkout reuses it.
    let again = billing.checkout(&agra, "business").create().await.unwrap();
    let second = fake.assert_checkout(|c| c.id == again.id);
    assert_eq!(second.customer_id, customer.provider_customer_id);
}

#[tokio::test]
async fn every_builder_option_reaches_the_provider() {
    let world = World::new().await;
    let ada = world.user("Anu", "anu@example.com").await;
    let grace = world.user("Gauri", "gauri@example.com").await;
    let agra = world.workspace(ada.id, "Agra").await;
    world.join(agra.id, grace.id, "member").await;
    let billing = world.billing().await;

    let checkout = billing
        .checkout(&agra, "pro")
        .yearly()
        .trial_days(30)
        .coupon("LAUNCH")
        .allow_promotion_codes()
        .success_url("https://app.acme.test/ok")
        .cancel_url("https://app.acme.test/back")
        .metadata("source", "pricing-page")
        .create()
        .await
        .unwrap();
    assert_eq!(checkout.interval, Interval::Yearly);
    assert_eq!(checkout.quantity, 2, "seats default to the member count");
    let recorded = world.fake().await.assert_checkout(|c| c.id == checkout.id);
    assert_eq!(recorded.price_id, "price_pro_y");
    assert_eq!(recorded.trial_days, Some(30));
    assert_eq!(recorded.coupon.as_deref(), Some("LAUNCH"));
    assert!(recorded.allow_promotion_codes);
    assert_eq!(recorded.success_url, "https://app.acme.test/ok");
    assert_eq!(recorded.cancel_url, "https://app.acme.test/back");
    assert_eq!(recorded.metadata["source"], "pricing-page");

    let skip_trial = billing
        .checkout(&agra, "pro")
        .trial_days(0)
        .quantity(5)
        .monthly()
        .create()
        .await
        .unwrap();
    assert_eq!(skip_trial.trial_days, None);
    assert_eq!(skip_trial.quantity, 5);
    let no_trial = billing.checkout(&agra, "business").create().await.unwrap();
    assert_eq!(no_trial.trial_days, None, "business has no trial");
    assert_eq!(no_trial.quantity, 1, "business has no seats");
}

#[tokio::test]
async fn unknown_plans_missing_prices_and_bad_quantities_are_refused() {
    let world = World::new().await;
    let ada = world.user("Anu", "anu@example.com").await;
    let agra = world.workspace(ada.id, "Agra").await;
    let billing = world.billing().await;

    let error = billing
        .checkout(&agra, "enterprise")
        .create()
        .await
        .unwrap_err();
    assert!(matches!(&error, BillingError::UnknownPlan(plan) if plan == "enterprise"));
    assert_eq!(error.to_app_error().status.as_u16(), 400);

    let error = billing
        .checkout(&agra, "starter")
        .yearly()
        .create()
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        BillingError::NoPrice {
            interval: "yearly",
            ..
        }
    ));

    let error = billing
        .checkout(&agra, "starter")
        .quantity(0)
        .create()
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        BillingError::Invalid {
            field: "quantity",
            ..
        }
    ));
    assert!(world.fake().await.checkouts().is_empty());
}

#[tokio::test]
async fn a_billable_with_a_valid_subscription_gets_a_409() {
    let world = World::new().await;
    let ada = world.user("Anu", "anu@example.com").await;
    let agra = world.workspace(ada.id, "Agra").await;
    world.subscribe(&agra, "starter").await;
    let error = world
        .billing()
        .await
        .checkout(&agra, "pro")
        .create()
        .await
        .unwrap_err();
    assert!(matches!(error, BillingError::AlreadySubscribed));
    assert_eq!(error.to_app_error().status.as_u16(), 409);
}

#[tokio::test]
async fn return_urls_default_to_app_url_and_are_required() {
    let world = World::with(
        platform()
            .module(crate::support::WorkspaceBilling)
            .config("billing.return_url", serde_json::Value::Null)
            .config("app.url", "https://acme.test"),
    )
    .await;
    let ada = world.user("Anu", "anu@example.com").await;
    let agra = world.workspace(ada.id, "Agra").await;
    let billing = world.billing().await;
    assert_eq!(billing.return_url(), Some("https://acme.test"));
    let checkout = billing.checkout(&agra, "starter").create().await.unwrap();
    let recorded = world.fake().await.assert_checkout(|c| c.id == checkout.id);
    assert_eq!(recorded.success_url, "https://acme.test");

    let world = World::with(
        platform()
            .module(crate::support::WorkspaceBilling)
            .config("billing.return_url", serde_json::Value::Null),
    )
    .await;
    let ada = world.user("Anu", "anu@example.com").await;
    let agra = world.workspace(ada.id, "Agra").await;
    let error = world
        .billing()
        .await
        .checkout(&agra, "starter")
        .create()
        .await
        .unwrap_err();
    assert!(matches!(error, BillingError::Config(_)), "{error}");
    let ok = world
        .billing()
        .await
        .checkout(&agra, "starter")
        .success_url("https://x.test/ok")
        .cancel_url("https://x.test/back")
        .create()
        .await;
    assert!(ok.is_ok());
}

#[tokio::test]
async fn users_are_billable_with_their_email() {
    let world = World::with(platform().module(crate::support::UserBilling)).await;
    let ada = world.user("Anu", "anu@example.com").await;
    let checkout = world
        .billing()
        .await
        .checkout(&ada, "starter")
        .create()
        .await
        .unwrap();
    let recorded = world.fake().await.assert_checkout(|c| c.id == checkout.id);
    assert_eq!(recorded.billable_type, "user");
    assert_eq!(recorded.billable_id, ada.id);
    assert_eq!(recorded.customer_email.as_deref(), Some("anu@example.com"));
}
