//! Idempotency on Stripe (CONN-103): every write carries an
//! `Idempotency-Key`, what creates something carries the request's own key,
//! and a write that fails with a retryable answer is sent again with the
//! same key and body.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use navree_billing::{
    BillingProvider, Money, NewCustomer, PaymentCustomer, PaymentLinkRequest, PaymentRequest,
    RefundReason, RefundRequest, StripeProvider, UsageReport,
};
use navree_contracts::Secret;
use navree_http_client::{HttpClient, HttpClientOptions, SsrfPolicy};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

use crate::fixtures::{NOW, WORKSPACE, stripe as fx};
use crate::support::{change, checkout_request, client};

fn ok(body: serde_json::Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(body)
}

fn key(request: &Request) -> Option<String> {
    request
        .headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

fn stripe(server: &MockServer, client: HttpClient) -> StripeProvider {
    StripeProvider::new(
        client,
        Secret::new("sk_test_51Q8".to_owned()),
        Some(Secret::new("whsec_test_Jx2".to_owned())),
        server.uri(),
    )
}

fn payment_session() -> serde_json::Value {
    json!({
        "id": "cs_test_pay1", "object": "checkout.session", "mode": "payment",
        "status": "open", "payment_status": "unpaid", "amount_total": 149_900,
        "currency": "inr", "customer": null, "payment_intent": null,
        "client_reference_id": "INV-2026-0042",
        "metadata": { "reference": "INV-2026-0042", "navree_kind": "payment" },
        "url": "https://checkout.stripe.com/c/pay/cs_test_pay1"
    })
}

fn payment_request(key: &str) -> PaymentRequest {
    PaymentRequest {
        billable: Some(("workspace".into(), WORKSPACE)),
        amount: Money::inr(149_900),
        description: "Annual support, Ludhiana office".into(),
        reference: "INV-2026-0042".into(),
        customer: PaymentCustomer::default(),
        methods: Vec::new(),
        success_url: Some("https://app.acme.test/paid".into()),
        cancel_url: None,
        expires_at: None,
        metadata: BTreeMap::new(),
        idempotency_key: key.into(),
    }
}

// CONN-103
#[tokio::test]
async fn every_stripe_write_carries_an_idempotency_key() {
    let server = MockServer::start().await;
    let sub = "/v1/subscriptions/sub_1Q8XyzAbC";
    for (verb, route, body) in [
        ("POST", "/v1/customers", fx::customer()),
        ("POST", "/v1/checkout/sessions", fx::checkout_session()),
        ("POST", sub, fx::subscription("active", "price_pro_m", 2)),
        ("DELETE", sub, fx::canceled("price_pro_m")),
        ("POST", "/v1/billing_portal/sessions", fx::portal_session()),
        ("POST", "/v1/webhook_endpoints", fx::webhook_endpoint()),
        ("GET", "/v1/prices/price_ai", fx::metered_price()),
        (
            "GET",
            "/v1/billing/meters/mtr_test_61Q8AiSummaries",
            fx::meter(),
        ),
        ("POST", "/v1/billing/meter_events", fx::meter_event()),
        (
            "POST",
            "/v1/prices",
            json!({ "id": "price_link1", "object": "price" }),
        ),
        (
            "POST",
            "/v1/payment_links",
            json!({ "id": "plink_1", "object": "payment_link", "active": true,
                    "url": "https://buy.stripe.com/test_link1" }),
        ),
        (
            "POST",
            "/v1/refunds",
            json!({ "id": "re_1", "object": "refund", "amount": 40_000, "currency": "inr",
                    "status": "succeeded", "payment_intent": "pi_1", "metadata": {} }),
        ),
    ] {
        Mock::given(method(verb))
            .and(path(route))
            .respond_with(ok(body))
            .mount(&server)
            .await;
    }
    let stripe = stripe(&server, client());

    stripe
        .create_customer(&NewCustomer {
            billable_type: "workspace".into(),
            billable_id: WORKSPACE,
            name: "Acme".into(),
            email: Some("ada@example.com".into()),
            idempotency_key: "navree-customer-workspace-1".into(),
        })
        .await
        .unwrap();
    let mut checkout = checkout_request(Some("cus_QX1aBcDeF"), "price_pro_m");
    checkout.idempotency_key = "navree-checkout-attempt-1".into();
    stripe.create_checkout(&checkout).await.unwrap();
    let id = "sub_1Q8XyzAbC";
    stripe
        .update_subscription(id, &change(Some("si_QX1base"), "price_pro_m", 3))
        .await
        .unwrap();
    stripe.cancel_subscription(id, true).await.unwrap();
    stripe.resume_subscription(id).await.unwrap();
    stripe.pause_subscription(id).await.unwrap();
    stripe.unpause_subscription(id).await.unwrap();
    stripe.cancel_subscription(id, false).await.unwrap();
    stripe
        .portal_url("cus_QX1aBcDeF", "https://app.acme.test/billing", &[])
        .await
        .unwrap();
    stripe
        .create_webhook_endpoint("https://api.acme.test/api/billing/webhook")
        .await
        .unwrap();
    stripe
        .report_usage(&UsageReport {
            customer_id: "cus_QX1aBcDeF".into(),
            subscription_id: id.into(),
            meter: "ai_summaries".into(),
            price_id: "price_ai".into(),
            quantity: 12,
            idempotency_key: "navree-usage-0123456789abcdef".into(),
            timestamp: DateTime::<Utc>::from_timestamp(NOW, 0).unwrap(),
        })
        .await
        .unwrap();
    stripe
        .create_payment_link(&PaymentLinkRequest {
            billable: None,
            amount: Money::inr(50_000),
            description: "Workshop seat, Amritsar".into(),
            reference: "SEAT-7".into(),
            customer: PaymentCustomer::default(),
            methods: Vec::new(),
            expires_at: None,
            notify_by_email: false,
            notify_by_sms: false,
            return_url: None,
            metadata: BTreeMap::new(),
            idempotency_key: "navree-link-SEAT-7".into(),
        })
        .await
        .unwrap();
    stripe
        .refund(&RefundRequest {
            payment_id: "pi_1".into(),
            amount: Some(Money::inr(40_000)),
            reason: RefundReason::RequestedByCustomer,
            note: None,
            reference: "RF-9".into(),
            billable: None,
            metadata: BTreeMap::new(),
            idempotency_key: "navree-refund-RF-9".into(),
        })
        .await
        .unwrap();

    let requests = server.received_requests().await.unwrap();
    let mut toggles = Vec::new();
    for request in &requests {
        let route = request.url.path();
        match request.method.as_str() {
            "POST" => {
                let key = key(request).unwrap_or_else(|| panic!("no key on POST {route}"));
                match route {
                    "/v1/customers" => assert_eq!(key, "navree-customer-workspace-1"),
                    "/v1/checkout/sessions" => assert_eq!(key, "navree-checkout-attempt-1"),
                    "/v1/billing/meter_events" => {
                        assert_eq!(key, "navree-usage-0123456789abcdef");
                    }
                    "/v1/prices" => assert_eq!(key, "navree-link-SEAT-7-price"),
                    "/v1/payment_links" => assert_eq!(key, "navree-link-SEAT-7-link"),
                    "/v1/refunds" => assert_eq!(key, "navree-refund-RF-9"),
                    _ => {
                        assert!(key.starts_with("navree-"), "{route}: {key}");
                        toggles.push(key);
                    }
                }
            }
            // GET and DELETE are idempotent; Stripe ignores keys on them.
            _ => assert_eq!(key(request), None, "{} {route}", request.method),
        }
    }
    // Subscription changes, the portal and the endpoint: a key per call.
    assert_eq!(toggles.len(), 7, "{toggles:?}");
    let mut unique = toggles.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), toggles.len(), "{toggles:?}");
}

// CONN-103
#[tokio::test]
async fn a_stripe_write_answered_503_is_retried_with_the_same_key_and_body() {
    let server = MockServer::start().await;
    // The first attempt fails, the retry gets the session.
    Mock::given(method("POST"))
        .and(path("/v1/checkout/sessions"))
        .respond_with(ResponseTemplate::new(503).set_body_json(fx::error("try again")))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/checkout/sessions"))
        .respond_with(ok(payment_session()))
        .with_priority(2)
        .mount(&server)
        .await;
    let retrying = HttpClient::new(HttpClientOptions {
        retries: 2,
        retry_base_ms: 1,
        ssrf: SsrfPolicy {
            block_private: true,
            allow_hosts: vec!["127.0.0.1".to_owned()],
        },
        ..HttpClientOptions::default()
    })
    .unwrap();
    let stripe = stripe(&server, retrying);

    let payment = stripe
        .create_payment(&payment_request("navree-pay-INV-2026-0042-a1"))
        .await
        .unwrap();
    assert_eq!(payment.id, "cs_test_pay1");

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2, "one retry");
    assert_eq!(
        key(&requests[0]).as_deref(),
        Some("navree-pay-INV-2026-0042-a1")
    );
    assert_eq!(key(&requests[0]), key(&requests[1]));
    assert_eq!(requests[0].body, requests[1].body);
}

// CONN-103
#[tokio::test]
async fn a_stripe_write_asked_to_wait_too_long_is_returned_not_retried() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/checkout/sessions"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "120")
                .set_body_json(fx::error("Too many requests")),
        )
        .mount(&server)
        .await;
    let retrying = HttpClient::new(HttpClientOptions {
        retries: 2,
        retry_base_ms: 1,
        ssrf: SsrfPolicy {
            block_private: true,
            allow_hosts: vec!["127.0.0.1".to_owned()],
        },
        ..HttpClientOptions::default()
    })
    .unwrap();
    let stripe = stripe(&server, retrying);

    let error = stripe
        .create_payment(&payment_request("navree-pay-INV-2026-0042-a2"))
        .await
        .unwrap_err();
    assert!(
        matches!(
            error,
            navree_billing::ProviderError::Api { status: 429, .. }
        ),
        "{error:?}"
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}
