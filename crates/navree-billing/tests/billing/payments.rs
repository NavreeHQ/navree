//! One-time payments, payment links and refunds through `Billing` on the
//! fake provider (CONN-101), with idempotency keys honoured end to end
//! (CONN-103).

use navree_billing::{
    BillingError, Capability, Money, PaymentAction, PaymentMethod, PaymentStatus, ProviderError,
    RefundReason, RefundStatus,
};
use serde_json::json;

use crate::support::World;

// CONN-101
#[tokio::test]
async fn a_one_time_payment_is_created_paid_and_reported() {
    let world = World::new().await;
    let ada = world.user("Harpreet Kaur", "harpreet@example.in").await;
    let acme = world.workspace(ada.id, "Grewal Traders").await;
    let billing = world.billing().await;
    let log = world.log().await;
    assert!(billing.supports(Capability::OneTimePayments));
    assert!(billing.supports(Capability::PartialRefunds));

    let payment = billing
        .payment(Money::inr(149_900), "Annual support, Ludhiana office")
        .billable(&acme)
        .reference("INV-2026-0042")
        .customer_phone("+919876543210")
        .metadata("order", "42")
        .create()
        .await
        .unwrap();
    assert_eq!(payment.status, PaymentStatus::Pending);
    assert_eq!(payment.amount, Money::inr(149_900));
    assert_eq!(payment.reference.as_deref(), Some("INV-2026-0042"));
    let Some(PaymentAction::Redirect { url }) = &payment.next_action else {
        panic!("{:?}", payment.next_action);
    };
    assert!(url.starts_with("https://checkout.fake.test/"), "{url}");

    let fake = world.fake().await;
    let recorded = fake.assert_payment(|p| p.reference == "INV-2026-0042");
    assert_eq!(recorded.metadata["billable_type"], "workspace");
    assert_eq!(recorded.metadata["billable_id"], acme.id.to_string());
    assert_eq!(recorded.metadata["order"], "42");
    assert_eq!(recorded.metadata["navree_kind"], "payment");

    fake.complete_payment(&payment.id).await.unwrap();
    world.run_queue().await;
    let emitted = log.take();
    let [(name, paid)] = emitted.as_slice() else {
        panic!("{emitted:?}");
    };
    assert_eq!(name, "payment.updated");
    assert_eq!(paid["status"], "paid");
    assert_eq!(paid["provider"], "fake");
    assert_eq!(paid["reference"], "INV-2026-0042");
    assert_eq!(
        paid["amount"],
        json!({ "amount": 149_900, "currency": "inr" })
    );
    assert_eq!(paid["billable_type"], "workspace");
    assert_eq!(paid["billable_id"], json!(acme.id));
    assert_eq!(paid["payment_id"], json!(recorded.payment_id));
    assert_eq!(paid["metadata"]["order"], "42");
}

// CONN-101
#[tokio::test]
async fn failed_and_expired_payments_are_reported() {
    let world = World::new().await;
    let billing = world.billing().await;
    let log = world.log().await;
    let fake = world.fake().await;

    let failed = billing
        .payment(Money::inr(50_000), "Workshop seat")
        .create()
        .await
        .unwrap();
    let expired = billing
        .payment(Money::inr(50_000), "Workshop seat")
        .create()
        .await
        .unwrap();
    assert_ne!(failed.id, expired.id, "each call is a new attempt");
    fake.fail_payment(&failed.id).await.unwrap();
    fake.expire_payment(&expired.id).await.unwrap();
    world.run_queue().await;

    let statuses: Vec<String> = log
        .take()
        .into_iter()
        .map(|(_, payload)| payload["status"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(statuses, ["failed", "expired"]);
    assert!(fake.complete_payment(&failed.id).await.is_err(), "settled");
}

// CONN-103
#[tokio::test]
async fn the_same_idempotency_key_creates_one_payment() {
    let world = World::new().await;
    let billing = world.billing().await;
    let fake = world.fake().await;

    let create = || {
        billing
            .payment(Money::inr(25_000), "Consultation, Jalandhar")
            .reference("ORD-77")
            .idempotency_key("order-77-attempt-1")
            .create()
    };
    let first = create().await.unwrap();
    let again = create().await.unwrap();
    assert_eq!(first.id, again.id);
    assert_eq!(fake.payments().len(), 1);
    assert_eq!(
        fake.idempotency_keys(),
        ["order-77-attempt-1", "order-77-attempt-1"]
    );

    // A new attempt for the same order needs a new key.
    let retry = billing
        .payment(Money::inr(25_000), "Consultation, Jalandhar")
        .reference("ORD-77")
        .idempotency_key("order-77-attempt-2")
        .create()
        .await
        .unwrap();
    assert_ne!(retry.id, first.id);
    assert_eq!(fake.payments().len(), 2);
}

// CONN-101
#[tokio::test]
async fn refunds_take_part_or_the_rest_of_a_payment_once() {
    let world = World::new().await;
    let billing = world.billing().await;
    let log = world.log().await;
    let fake = world.fake().await;
    let payment = billing
        .payment(Money::inr(100_000), "Annual plan, Chandigarh")
        .create()
        .await
        .unwrap();
    fake.complete_payment(&payment.id).await.unwrap();
    world.run_queue().await;
    let paid = log.take();
    let payment_id = paid[0].1["payment_id"].as_str().unwrap().to_owned();

    let part = billing
        .refund(&payment_id)
        .amount(Money::inr(40_000))
        .reason(RefundReason::Duplicate)
        .note("charged twice at the counter")
        .reference("RF-2026-0007")
        .create()
        .await
        .unwrap();
    assert_eq!(part.status, RefundStatus::Succeeded);
    assert_eq!(part.amount, Money::inr(40_000));
    assert_eq!(part.reference.as_deref(), Some("RF-2026-0007"));
    // The same reference refunds once: the default key follows it.
    let again = billing
        .refund(&payment_id)
        .amount(Money::inr(40_000))
        .reference("RF-2026-0007")
        .create()
        .await
        .unwrap();
    assert_eq!(again.id, part.id);
    assert_eq!(fake.refunds().len(), 1);

    let rest = billing.refund(&payment_id).create().await.unwrap();
    assert_eq!(rest.amount, Money::inr(60_000), "what is left");
    let error = billing
        .refund(&payment_id)
        .amount(Money::inr(1))
        .create()
        .await
        .unwrap_err();
    assert!(
        matches!(
            error,
            BillingError::Provider(ProviderError::Api { status: 400, .. })
        ),
        "{error:?}"
    );
    assert_eq!(fake.refunds().len(), 2);
}

// CONN-101
#[tokio::test]
async fn a_payment_link_is_paid_and_reported_with_its_link() {
    let world = World::new().await;
    let ada = world.user("Simran Bedi", "simran@example.in").await;
    let billing = world.billing().await;
    let log = world.log().await;
    let fake = world.fake().await;

    let link = billing
        .payment_link(Money::inr(250_000), "Wedding photography deposit, Patiala")
        .billable(&ada)
        .reference("DEP-311")
        .notify_by_sms()
        .create()
        .await
        .unwrap();
    assert!(
        link.url.contains("client_reference_id=DEP-311"),
        "{}",
        link.url
    );
    // The default key follows the reference: the same link again.
    let again = billing
        .payment_link(Money::inr(250_000), "Wedding photography deposit, Patiala")
        .reference("DEP-311")
        .create()
        .await
        .unwrap();
    assert_eq!(again.id, link.id);
    assert_eq!(fake.payment_links().len(), 1);

    fake.pay_link(&link.id).await.unwrap();
    world.run_queue().await;
    let emitted = log.take();
    let [(_, paid)] = emitted.as_slice() else {
        panic!("{emitted:?}");
    };
    assert_eq!(paid["status"], "paid");
    assert_eq!(paid["link_id"], json!(link.id));
    assert_eq!(paid["reference"], "DEP-311");
    assert_eq!(paid["billable_type"], "user");
    assert_eq!(paid["billable_id"], json!(ada.id));
}

// CONN-101
#[tokio::test]
async fn bad_payment_input_is_a_validation_error() {
    let world = World::new().await;
    let billing = world.billing().await;
    for (builder, field) in [
        (billing.payment(Money::inr(0), "Nothing"), "amount"),
        (
            billing.payment(Money::new(100, "rupees"), "Bad code"),
            "amount",
        ),
        (billing.payment(Money::inr(100), " "), "description"),
        (
            billing.payment(Money::inr(100), "Tea").reference("INV 42"),
            "reference",
        ),
        (
            billing
                .payment(Money::inr(100), "Tea")
                .idempotency_key("two words"),
            "idempotency_key",
        ),
    ] {
        let error = builder.create().await.unwrap_err();
        assert!(
            matches!(&error, BillingError::Invalid { field: f, .. } if *f == field),
            "{field}: {error:?}"
        );
        assert_eq!(error.to_app_error().status.as_u16(), 400);
    }
    // The fake checks methods as Stripe does: cards and UPI.
    let error = billing
        .payment(Money::inr(100), "Chai")
        .methods([PaymentMethod::NetBanking])
        .create()
        .await
        .unwrap_err();
    assert!(
        matches!(
            error,
            BillingError::Provider(ProviderError::Unsupported { .. })
        ),
        "{error:?}"
    );
    assert!(world.fake().await.payments().is_empty());
}
