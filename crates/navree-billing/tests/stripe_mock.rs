//! The Stripe client against stripe-mock, Stripe's mock server: it checks
//! every request against Stripe's OpenAPI description (unknown or malformed
//! parameters are a 400) and answers with static fixtures.
//!
//! Runs when `NAVREE_TEST_STRIPE_MOCK_URL` is set, for example in CI's
//! services job with `stripe/stripe-mock` on port 12111
//! (`NAVREE_TEST_STRIPE_MOCK_URL=http://127.0.0.1:12111`). Without it the
//! test prints `SKIPPED` and passes, unless `NAVREE_TEST_REQUIRE_SERVICES=1`
//! turns the missing service into a failure.
//!
//! The suite asserts that Stripe accepts each request the client sends. The
//! fixtures don't follow the requests (a canceled subscription comes back in
//! the fixture's state), so a fixture the client can't map is printed, not
//! failed; the mapping is covered by `tests/providers` with recorded
//! payloads.

use std::collections::BTreeMap;

use chrono::Utc;
use navree_billing::{
    BillingProvider, CheckoutRequest, Interval, Money, NewCustomer, PaymentCustomer,
    PaymentLinkRequest, PaymentMethod, PaymentRequest, Proration, ProviderError, RefundReason,
    RefundRequest, StripeProvider, SubscriptionChange, UsageReport,
};
use navree_contracts::Secret;
use navree_http_client::{HttpClient, HttpClientOptions, SsrfPolicy};
use uuid::Uuid;

/// The stripe-mock URL, or `None` after printing why the test is skipped.
fn service(test: &str) -> Option<String> {
    match std::env::var("NAVREE_TEST_STRIPE_MOCK_URL") {
        Ok(url) if !url.trim().is_empty() => Some(url),
        _ => {
            assert!(
                std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() != Ok("1"),
                "{test}: NAVREE_TEST_REQUIRE_SERVICES=1 but NAVREE_TEST_STRIPE_MOCK_URL is not set"
            );
            eprintln!("SKIPPED {test}: set NAVREE_TEST_STRIPE_MOCK_URL");
            None
        }
    }
}

fn provider(base: String) -> StripeProvider {
    let client = HttpClient::new(HttpClientOptions {
        retries: 0,
        ssrf: SsrfPolicy {
            block_private: false,
            allow_hosts: Vec::new(),
        },
        ..HttpClientOptions::default()
    })
    .expect("an HTTP client");
    StripeProvider::new(
        client,
        Secret::new("sk_test_123".to_owned()),
        Some(Secret::new("whsec_test".to_owned())),
        base,
    )
}

/// Fails when Stripe refused the request or it did not get there; prints a
/// fixture the client could not map.
fn accepted<T>(operation: &str, result: Result<T, ProviderError>) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(error @ ProviderError::Decode { .. }) => {
            eprintln!("{operation}: accepted; the fixture does not map: {error}");
            None
        }
        Err(error) => panic!("{operation}: stripe-mock refused the request: {error}"),
    }
}

fn checkout(customer: Option<String>) -> CheckoutRequest {
    CheckoutRequest {
        billable_type: "workspace".into(),
        billable_id: Uuid::now_v7(),
        customer_id: customer,
        customer_email: Some("anu@example.com".into()),
        plan: "pro".into(),
        interval: Interval::Monthly,
        price_id: "price_pro_monthly".into(),
        metered_prices: vec!["price_ai_summaries".into()],
        quantity: 3,
        trial_days: Some(14),
        coupon: Some("promo_launch".into()),
        allow_promotion_codes: false,
        success_url: "https://app.acme.test/settings/billing?upgraded=1".into(),
        cancel_url: "https://app.acme.test/settings/billing".into(),
        metadata: BTreeMap::from([("source".to_owned(), "pricing".to_owned())]),
        idempotency_key: format!("navree-checkout-mock-{}", Uuid::now_v7().simple()),
    }
}

#[tokio::test]
async fn stripe_accepts_every_request_the_client_sends() {
    let Some(base) = service("stripe_accepts_every_request_the_client_sends") else {
        return;
    };
    let stripe = provider(base);

    let customer = accepted(
        "create a customer",
        stripe
            .create_customer(&NewCustomer {
                billable_type: "workspace".into(),
                billable_id: Uuid::now_v7(),
                name: "Agra".into(),
                email: Some("anu@example.com".into()),
                idempotency_key: format!("navree-customer-mock-{}", Uuid::now_v7().simple()),
            })
            .await,
    )
    .flatten()
    .unwrap_or_else(|| "cus_123".to_owned());

    accepted(
        "a checkout for a customer, with a promotion code",
        stripe
            .create_checkout(&checkout(Some(customer.clone())))
            .await,
    );
    let mut by_email = checkout(None);
    by_email.coupon = Some("LAUNCH".into());
    accepted(
        "a checkout by email, with a coupon",
        stripe.create_checkout(&by_email).await,
    );
    let mut open = checkout(None);
    open.coupon = None;
    open.allow_promotion_codes = true;
    open.trial_days = None;
    accepted(
        "a checkout that lets the customer type a code",
        stripe.create_checkout(&open).await,
    );

    for proration in [
        Proration::NextInvoice,
        Proration::InvoiceNow,
        Proration::None,
    ] {
        accepted(
            "a swap",
            stripe
                .update_subscription(
                    "sub_123",
                    &SubscriptionChange {
                        base_item_id: Some("si_base".into()),
                        price_id: "price_business_monthly".into(),
                        quantity: 4,
                        add_prices: vec!["price_ai_summaries".into()],
                        remove_item_ids: vec!["si_meter".into()],
                        proration,
                    },
                )
                .await,
        );
    }
    accepted(
        "cancel at the period's end",
        stripe.cancel_subscription("sub_123", true).await,
    );
    accepted(
        "cancel now",
        stripe.cancel_subscription("sub_123", false).await,
    );
    accepted("resume", stripe.resume_subscription("sub_123").await);
    accepted("pause", stripe.pause_subscription("sub_123").await);
    accepted("unpause", stripe.unpause_subscription("sub_123").await);
    accepted("subscriptions", stripe.subscriptions(&customer).await);
    accepted("invoices", stripe.invoices(&customer).await);
    accepted(
        "a portal session",
        stripe
            .portal_url(&customer, "https://app.acme.test/settings/billing", &[])
            .await,
    );
    accepted(
        "a price lookup",
        stripe.price_exists("price_pro_monthly").await,
    );
    accepted(
        "usage, as a meter event",
        stripe
            .report_usage(&UsageReport {
                customer_id: customer.clone(),
                subscription_id: "sub_123".into(),
                meter: "ai_summaries".into(),
                price_id: "price_ai_summaries".into(),
                quantity: 12,
                idempotency_key: "navree-usage-0123456789abcdef".into(),
                timestamp: Utc::now(),
            })
            .await,
    );
    accepted(
        "a webhook endpoint on the pinned version",
        stripe
            .create_webhook_endpoint("https://api.acme.test/api/billing/webhook")
            .await,
    );
}

/// A one-time payment request for stripe-mock.
fn payment(reference: &str) -> PaymentRequest {
    PaymentRequest {
        billable: Some(("workspace".into(), Uuid::now_v7())),
        amount: Money::inr(149_900),
        description: "Annual support, Ludhiana office".into(),
        reference: reference.into(),
        customer: PaymentCustomer {
            customer_id: None,
            name: Some("Harpreet Kaur".into()),
            email: Some("harpreet@example.in".into()),
            phone: Some("+919876543210".into()),
        },
        methods: vec![PaymentMethod::Card],
        success_url: Some("https://app.acme.test/orders/42?paid=1".into()),
        cancel_url: Some("https://app.acme.test/orders/42".into()),
        expires_at: Some(Utc::now() + chrono::TimeDelta::hours(2)),
        metadata: BTreeMap::from([("order".to_owned(), "42".to_owned())]),
        idempotency_key: format!("navree-pay-mock-{}", Uuid::now_v7().simple()),
    }
}

// One-time payments, payment links and refunds, each
// write with its Idempotency-Key, as Stripe's own description accepts them.
#[tokio::test]
async fn stripe_accepts_the_payment_requests_the_client_sends() {
    let Some(base) = service("stripe_accepts_the_payment_requests_the_client_sends") else {
        return;
    };
    let stripe = provider(base);

    accepted(
        "a one-time payment as a payment-mode checkout session",
        stripe.create_payment(&payment("INV-2026-0042")).await,
    );
    let mut any_method = payment("INV-2026-0043");
    any_method.methods.clear();
    any_method.customer.customer_id = Some("cus_123".into());
    accepted(
        "a one-time payment for a customer, with the dashboard's methods",
        stripe.create_payment(&any_method).await,
    );
    accepted(
        "a payment link (a price, then the link)",
        stripe
            .create_payment_link(&PaymentLinkRequest {
                billable: None,
                amount: Money::inr(250_000),
                description: "Wedding photography deposit, Patiala".into(),
                reference: "DEP-311".into(),
                customer: PaymentCustomer::default(),
                methods: Vec::new(),
                expires_at: None,
                notify_by_email: false,
                notify_by_sms: false,
                return_url: Some("https://app.acme.test/thanks".into()),
                metadata: BTreeMap::new(),
                idempotency_key: format!("navree-link-mock-{}", Uuid::now_v7().simple()),
            })
            .await,
    );
    for (payment_id, amount) in [("pi_123", Some(Money::inr(40_000))), ("ch_123", None)] {
        accepted(
            "a refund",
            stripe
                .refund(&RefundRequest {
                    payment_id: payment_id.into(),
                    amount,
                    reason: RefundReason::RequestedByCustomer,
                    note: Some("order canceled".into()),
                    reference: "RF-2026-0007".into(),
                    billable: None,
                    metadata: BTreeMap::new(),
                    idempotency_key: format!("navree-refund-mock-{}", Uuid::now_v7().simple()),
                })
                .await,
        );
    }
}
