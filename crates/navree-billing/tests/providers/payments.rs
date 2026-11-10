//! The payment half of the provider conformance suite: one-time
//! payments, payment links, refunds and their webhooks, on the fake, Stripe
//! and Paddle. What a provider lists in `capabilities` must work, and what
//! it does not list must answer `Unsupported`. A provider that implements
//! only the three required methods gets the documented defaults.

use std::any::Any;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use http::{HeaderMap, HeaderName, HeaderValue};
use navree_billing::{
    Billing, BillingModule, BillingProvider, Capability, Money, PADDLE_SIGNATURE_HEADER,
    PaddleProvider, PaymentAction, PaymentCustomer, PaymentLinkRequest, PaymentLinkStatus,
    PaymentRequest, PaymentStatus, ProviderError, RefundReason, RefundRequest,
    STRIPE_SIGNATURE_HEADER, StripeProvider, WebhookAction, WebhookError, WebhookEvent,
    sign_paddle_webhook, sign_stripe_webhook,
};
use navree_contracts::{Clock, FixedClock, Secret};
use navree_db::DatabaseModule;
use navree_testing::TestApp;
use serde_json::{Value, json};
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::fixtures::{NOW, WORKSPACE, paddle as pfx, stripe as sfx};
use crate::support::client;

/// One provider under the payment conformance suite.
struct Case {
    name: &'static str,
    provider: Arc<dyn BillingProvider>,
    /// A paid payment `refund` can name, once `pay` has run.
    pay: Box<dyn Fn(String) -> Paid + Send + Sync>,
    secret: String,
    sign: fn(&str, i64, &[u8]) -> String,
    header: &'static str,
    /// A raw "payment paid" event for `INV-2026-0042`, ₹1,499.00, the
    /// fixture workspace.
    paid_event: Vec<u8>,
    _keep: Box<dyn Any>,
}

/// What `pay` returns: the payment id refunds name.
type Paid = std::pin::Pin<Box<dyn std::future::Future<Output = String> + Send>>;

fn at(secs: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(secs, 0).unwrap()
}

fn payment_request(key: &str) -> PaymentRequest {
    PaymentRequest {
        billable: Some(("workspace".into(), WORKSPACE)),
        amount: Money::inr(149_900),
        description: "Annual support, Ludhiana office".into(),
        reference: "INV-2026-0042".into(),
        customer: PaymentCustomer {
            customer_id: None,
            name: Some("Harpreet Kaur".into()),
            email: Some("harpreet@example.in".into()),
            phone: Some("+919876543210".into()),
        },
        methods: Vec::new(),
        success_url: Some("https://app.acme.test/orders/42?paid=1".into()),
        cancel_url: Some("https://app.acme.test/orders/42".into()),
        expires_at: None,
        metadata: BTreeMap::from([("order".to_owned(), "42".to_owned())]),
        idempotency_key: key.into(),
    }
}

fn link_request() -> PaymentLinkRequest {
    PaymentLinkRequest {
        billable: Some(("workspace".into(), WORKSPACE)),
        amount: Money::inr(250_000),
        description: "Wedding photography deposit, Patiala".into(),
        reference: "DEP-311".into(),
        customer: PaymentCustomer::default(),
        methods: Vec::new(),
        expires_at: None,
        notify_by_email: false,
        notify_by_sms: true,
        return_url: Some("https://app.acme.test/thanks".into()),
        metadata: BTreeMap::new(),
        idempotency_key: "navree-link-DEP-311".into(),
    }
}

fn refund_request(payment_id: &str, amount: Option<Money>, key: &str) -> RefundRequest {
    RefundRequest {
        payment_id: payment_id.into(),
        amount,
        reason: RefundReason::RequestedByCustomer,
        note: Some("order canceled".into()),
        reference: "RF-2026-0007".into(),
        billable: None,
        metadata: BTreeMap::new(),
        idempotency_key: key.into(),
    }
}

/// The Stripe Checkout Session of `INV-2026-0042`.
fn stripe_session(status: &str, paid: &str) -> Value {
    json!({
        "id": "cs_test_pay1", "object": "checkout.session", "mode": "payment",
        "status": status, "payment_status": paid, "amount_total": 149_900,
        "currency": "inr", "customer": null,
        "payment_intent": if status == "open" { Value::Null } else { json!("pi_3Q8pay1") },
        "payment_link": null,
        "client_reference_id": "INV-2026-0042",
        "metadata": { "billable_type": "workspace", "billable_id": WORKSPACE.to_string(),
                      "reference": "INV-2026-0042", "navree_kind": "payment", "order": "42" },
        "url": if status == "open" { json!("https://checkout.stripe.com/c/pay/cs_test_pay1") } else { Value::Null }
    })
}

fn stripe_refund(amount: i64) -> Value {
    json!({ "id": format!("re_{amount}"), "object": "refund", "amount": amount, "currency": "inr",
            "status": "pending", "payment_intent": "pi_3Q8pay1",
            "metadata": { "reference": "RF-2026-0007" } })
}

async fn fake_case() -> Case {
    let app = crate::servers::database(TestApp::builder())
        .config("billing.provider", "fake")
        .config("billing.routes", false)
        .module(DatabaseModule::from_config())
        .module(BillingModule::from_config())
        .override_value::<dyn Clock>(Arc::new(FixedClock::new(at(NOW))))
        .build()
        .await;
    let billing = app.get::<Billing>().await;
    let fake = billing.fake().unwrap();
    let paid_event = serde_json::to_vec(&sfx::event(
        "evt_paid",
        "checkout.session.completed",
        stripe_session("complete", "paid"),
    ))
    .unwrap();
    let paying = fake.clone();
    Case {
        name: "fake",
        provider: fake.clone(),
        pay: Box::new(move |session: String| {
            let fake = paying.clone();
            Box::pin(async move {
                fake.complete_payment(&session).await.unwrap();
                fake.assert_payment(|p| p.id == session).payment_id
            })
        }),
        secret: fake.webhook_secret().to_owned(),
        sign: sign_stripe_webhook,
        header: STRIPE_SIGNATURE_HEADER,
        paid_event,
        _keep: Box::new(app),
    }
}

async fn stripe_case() -> Case {
    let server = MockServer::start().await;
    let ok = |body| ResponseTemplate::new(200).set_body_json(body);
    Mock::given(method("POST"))
        .and(path("/v1/checkout/sessions"))
        .respond_with(ok(stripe_session("open", "unpaid")))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/prices"))
        .respond_with(ok(json!({ "id": "price_dep311", "object": "price" })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/payment_links"))
        .respond_with(ok(
            json!({ "id": "plink_dep311", "object": "payment_link", "active": true,
                                  "url": "https://buy.stripe.com/test_dep311" }),
        ))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/refunds"))
        .and(body_string_contains("amount=40000"))
        .respond_with(ok(stripe_refund(40_000)))
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/refunds"))
        .respond_with(ok(stripe_refund(149_900)))
        .with_priority(2)
        .mount(&server)
        .await;
    let provider = StripeProvider::new(
        client(),
        Secret::new("sk_test_51Q8".to_owned()),
        Some(Secret::new("whsec_payments".to_owned())),
        server.uri(),
    );
    Case {
        name: "stripe",
        provider: Arc::new(provider),
        pay: Box::new(|_| Box::pin(async { "pi_3Q8pay1".to_owned() })),
        secret: "whsec_payments".into(),
        sign: sign_stripe_webhook,
        header: STRIPE_SIGNATURE_HEADER,
        paid_event: serde_json::to_vec(&sfx::event(
            "evt_paid",
            "checkout.session.completed",
            stripe_session("complete", "paid"),
        ))
        .unwrap(),
        _keep: Box::new(server),
    }
}

/// The Paddle transaction of `INV-2026-0042`.
fn paddle_transaction(status: &str) -> Value {
    json!({
        "id": "txn_01j8inv42", "status": status, "customer_id": null, "subscription_id": null,
        "currency_code": "INR", "collection_mode": "automatic",
        "custom_data": { "billable_type": "workspace", "billable_id": WORKSPACE.to_string(),
                         "reference": "INV-2026-0042", "navree_kind": "payment", "order": "42" },
        "items": [{ "price": { "unit_price": { "amount": "149900", "currency_code": "INR" } }, "quantity": 1 }],
        "details": { "totals": { "grand_total": "149900" } },
        "checkout": { "url": "https://pay.acme.test/checkout?_ptxn=txn_01j8inv42" },
        "created_at": "2026-10-26T09:00:00Z"
    })
}

async fn paddle_case() -> Case {
    let server = MockServer::start().await;
    let ok = |data| ResponseTemplate::new(200).set_body_json(pfx::envelope(data));
    Mock::given(method("POST"))
        .and(path("/transactions"))
        .respond_with(ok(paddle_transaction("ready")))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/adjustments"))
        .respond_with(ok(json!({
            "id": "adj_01j8", "action": "refund", "type": "full", "status": "pending_approval",
            "transaction_id": "txn_01j8inv42", "currency_code": "INR",
            "totals": { "total": "149900" }
        })))
        .mount(&server)
        .await;
    let provider = PaddleProvider::new(
        client(),
        Secret::new("pdl_sdbx_apikey_01j8".to_owned()),
        Some(Secret::new("pdl_ntfset_payments".to_owned())),
        server.uri(),
    );
    Case {
        name: "paddle",
        provider: Arc::new(provider),
        pay: Box::new(|_| Box::pin(async { "txn_01j8inv42".to_owned() })),
        secret: "pdl_ntfset_payments".into(),
        sign: sign_paddle_webhook,
        header: PADDLE_SIGNATURE_HEADER,
        paid_event: serde_json::to_vec(&pfx::event(
            "evt_paid",
            "transaction.completed",
            paddle_transaction("completed"),
        ))
        .unwrap(),
        _keep: Box::new(server),
    }
}

fn signed(case: &Case, body: &[u8]) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static(case.header),
        HeaderValue::from_str(&(case.sign)(&case.secret, NOW, body)).unwrap(),
    );
    headers
}

fn unsupported<T: std::fmt::Debug>(name: &str, what: &str, result: Result<T, ProviderError>) {
    assert!(
        matches!(result, Err(ProviderError::Unsupported { .. })),
        "{name}: {what} is not listed, so it must answer Unsupported: {result:?}"
    );
}

async fn payments_conform(case: Case) {
    let name = case.name;
    let provider = &case.provider;
    let can = |capability| provider.capabilities().contains(&capability);

    // One-time payments.
    let created = provider
        .create_payment(&payment_request("navree-pay-INV-2026-0042-c1"))
        .await;
    let session = if can(Capability::OneTimePayments) {
        let payment = created.unwrap();
        assert!(!payment.id.is_empty(), "{name}");
        assert_eq!(payment.status, PaymentStatus::Pending, "{name}");
        assert_eq!(payment.amount, Money::inr(149_900), "{name}");
        assert_eq!(
            payment.reference.as_deref(),
            Some("INV-2026-0042"),
            "{name}"
        );
        match &payment.next_action {
            Some(PaymentAction::Redirect { url }) => {
                assert!(url.starts_with("https://"), "{name}: {url}");
            }
            other => panic!("{name}: {other:?}"),
        }
        Some(payment.id)
    } else {
        unsupported(name, "a one-time payment", created);
        None
    };

    // Payment links.
    let link = provider.create_payment_link(&link_request()).await;
    if can(Capability::PaymentLinks) {
        let link = link.unwrap();
        assert!(link.url.starts_with("https://"), "{name}: {}", link.url);
        assert_eq!(link.status, PaymentLinkStatus::Active, "{name}");
        assert_eq!(link.amount, Money::inr(250_000), "{name}");
        assert_eq!(link.reference.as_deref(), Some("DEP-311"), "{name}");
    } else {
        unsupported(name, "a payment link", link);
    }

    // Refunds of a paid payment.
    if let Some(session) = session {
        let payment_id = (case.pay)(session).await;
        let partial = provider
            .refund(&refund_request(
                &payment_id,
                Some(Money::inr(40_000)),
                "navree-refund-p",
            ))
            .await;
        if can(Capability::PartialRefunds) {
            let refund = partial.unwrap();
            assert_eq!(refund.amount, Money::inr(40_000), "{name}");
            assert_eq!(
                refund.payment_id.as_deref(),
                Some(payment_id.as_str()),
                "{name}"
            );
        } else {
            unsupported(name, "a partial refund", partial);
        }
        let full = provider
            .refund(&refund_request(&payment_id, None, "navree-refund-f"))
            .await;
        if can(Capability::Refunds) {
            let refund = full.unwrap();
            assert!(!refund.id.is_empty(), "{name}");
            assert_eq!(refund.amount.currency, "inr", "{name}");
            assert!(refund.amount.amount > 0, "{name}");
        } else {
            unsupported(name, "a refund", full);
        }
    }

    // The payment's webhook: verified by the provider's own scheme, parsed
    // to the same values.
    let tolerance = Duration::from_secs(300);
    let body = &case.paid_event;
    provider
        .verify_webhook(&signed(&case, body), body, at(NOW), tolerance)
        .unwrap();
    assert_eq!(
        provider.verify_webhook(&signed(&case, b"{}"), body, at(NOW), tolerance),
        Err(WebhookError::SignatureMismatch),
        "{name}"
    );
    let event = provider.parse_webhook(body).unwrap();
    let payment = event
        .actions
        .iter()
        .find_map(|action| match action {
            WebhookAction::Payment(payment) => Some(payment),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{name}: no payment in {:?}", event.actions));
    assert_eq!(payment.status, PaymentStatus::Paid, "{name}");
    assert_eq!(payment.amount, Money::inr(149_900), "{name}");
    assert_eq!(
        payment.reference.as_deref(),
        Some("INV-2026-0042"),
        "{name}"
    );
    assert_eq!(
        payment.billable(),
        Some(("workspace".to_owned(), WORKSPACE)),
        "{name}"
    );
    assert_eq!(
        payment.metadata.get("order").map(String::as_str),
        Some("42"),
        "{name}"
    );
    assert!(
        !event
            .actions
            .iter()
            .any(|action| matches!(action, WebhookAction::InvoicePaid(_))),
        "{name}: a one-time payment is not an invoice"
    );
}

#[tokio::test]
async fn the_fake_takes_payments_like_stripe() {
    payments_conform(fake_case().await).await;
}

#[tokio::test]
async fn stripe_takes_payments_links_and_refunds() {
    payments_conform(stripe_case().await).await;
}

#[tokio::test]
async fn paddle_takes_payments_and_full_refunds() {
    let case = paddle_case().await;
    assert!(
        !case
            .provider
            .capabilities()
            .contains(&Capability::PartialRefunds)
    );
    assert!(
        !case
            .provider
            .capabilities()
            .contains(&Capability::IdempotencyKeys)
    );
    payments_conform(case).await;
}

/// A provider that implements only what the trait requires.
struct Minimal;

impl BillingProvider for Minimal {
    fn name(&self) -> &'static str {
        "minimal"
    }

    fn verify_webhook(
        &self,
        _headers: &HeaderMap,
        _body: &[u8],
        _now: DateTime<Utc>,
        _tolerance: Duration,
    ) -> Result<(), WebhookError> {
        Err(WebhookError::NoSecret)
    }

    fn parse_webhook(&self, _body: &[u8]) -> Result<WebhookEvent, ProviderError> {
        Err(ProviderError::Unsupported {
            provider: "minimal",
            operation: "webhooks",
        })
    }
}

#[tokio::test]
async fn a_provider_implements_only_what_it_offers() {
    let minimal = Minimal;
    assert!(minimal.capabilities().is_empty());
    assert_eq!(
        minimal
            .create_customer(&navree_billing::NewCustomer {
                billable_type: "workspace".into(),
                billable_id: WORKSPACE,
                name: "Agra".into(),
                email: None,
                idempotency_key: "k".into(),
            })
            .await
            .unwrap(),
        None
    );
    assert!(minimal.subscriptions("c").await.unwrap().is_empty());
    assert!(minimal.invoices("c").await.unwrap().is_empty());
    assert_eq!(minimal.webhook_event_id(&HeaderMap::new()), None);
    unsupported(
        "minimal",
        "a checkout",
        minimal
            .create_checkout(&crate::support::checkout_request(None, "price_pro_m"))
            .await,
    );
    unsupported(
        "minimal",
        "a change",
        minimal
            .update_subscription("s", &crate::support::change(None, "p", 1))
            .await,
    );
    unsupported(
        "minimal",
        "a cancel",
        minimal.cancel_subscription("s", true).await,
    );
    unsupported(
        "minimal",
        "a resume",
        minimal.resume_subscription("s").await,
    );
    unsupported("minimal", "a pause", minimal.pause_subscription("s").await);
    unsupported(
        "minimal",
        "an unpause",
        minimal.unpause_subscription("s").await,
    );
    unsupported(
        "minimal",
        "the portal",
        minimal.portal_url("c", "https://a.test", &[]).await,
    );
    unsupported("minimal", "a price lookup", minimal.price_exists("p").await);
    unsupported(
        "minimal",
        "an endpoint",
        minimal.create_webhook_endpoint("https://a.test/hook").await,
    );
    unsupported(
        "minimal",
        "a payment",
        minimal.create_payment(&payment_request("k")).await,
    );
    unsupported(
        "minimal",
        "a link",
        minimal.create_payment_link(&link_request()).await,
    );
    unsupported(
        "minimal",
        "a refund",
        minimal.refund(&refund_request("p", None, "k")).await,
    );
}
