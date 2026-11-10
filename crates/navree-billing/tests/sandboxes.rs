//! The Stripe and Paddle drivers against the providers' own test
//! environments: Stripe's test mode and the Paddle sandbox. They create
//! real objects there (customers, prices, checkouts, payments, links, a
//! refund, a webhook endpoint), read them back, and remove what can be
//! removed. Nothing is charged: a Stripe test key and the Paddle sandbox
//! move no money.
//!
//! Opt-in, by key. Without the key a test says so and passes, also with
//! `NAVREE_TEST_REQUIRE_SERVICES=1` (the keys live in CI secrets, not on
//! every machine):
//!
//! - `STRIPE_TEST_SECRET`: a Stripe secret key of test mode (`sk_test_...`
//!   or a restricted `rk_test_...`); a live key is refused. Prices and the
//!   test payment are in `STRIPE_TEST_CURRENCY` (default `inr`), which the
//!   account must accept. The customer portal is left out: it needs its
//!   settings saved once in the dashboard's test mode.
//! - `PADDLE_SANDBOX_KEY`: an API key of a Paddle sandbox account. The
//!   requests go to `https://sandbox-api.paddle.com`, where a live key does
//!   not work. Checkouts need the sandbox's default payment link (Checkout
//!   settings in the dashboard).
//!
//! ```sh
//! STRIPE_TEST_SECRET=sk_test_... PADDLE_SANDBOX_KEY=pdl_sdbx_apikey_... \
//!     cargo test -p navree-billing --test sandboxes -- --nocapture
//! ```
//!
//! The signed webhook round trip is not run: the providers deliver events
//! only to a public URL. The events Stripe recorded for the account are
//! parsed instead, and the endpoint Navree creates is read back for its API
//! version and events.

use std::collections::BTreeMap;

use chrono::Utc;
use http::Method;
use navree_billing::{
    BillingProvider, CheckoutRequest, Interval, Money, NewCustomer, PADDLE_API_VERSION,
    PaddleProvider, PaymentCustomer, PaymentLinkRequest, PaymentMethod, PaymentRequest,
    RefundReason, RefundRequest, RefundStatus, STRIPE_API_VERSION, STRIPE_EVENTS, StripeProvider,
};
use navree_contracts::Secret;
use navree_http_client::{HttpClient, HttpClientOptions};
use serde_json::{Value, json};
use uuid::Uuid;

/// The value of `variable`, or `None` after saying the test is skipped.
fn key(variable: &str, test: &str) -> Option<String> {
    match std::env::var(variable) {
        Ok(value) if !value.trim().is_empty() => Some(value.trim().to_owned()),
        _ => {
            println!("SKIPPED {test}: set {variable} (an opt-in test environment key)");
            None
        }
    }
}

fn client() -> HttpClient {
    HttpClient::new(HttpClientOptions::default()).expect("an HTTP client")
}

/// A key no earlier run used.
fn fresh(what: &str) -> String {
    format!("navree-sandbox-{what}-{}", Uuid::now_v7().simple())
}

/// Calls the provider's API directly (to set up what the drivers only read,
/// and to clean up), failing on anything but a 2xx answer.
struct Api {
    client: HttpClient,
    base: &'static str,
    key: String,
    stripe: bool,
}

impl Api {
    async fn send(&self, method: Method, path: &str, body: Option<Body<'_>>) -> Value {
        let mut request = self
            .client
            .request(method.clone(), format!("{}{path}", self.base))
            .bearer(&self.key)
            .retries(0);
        request = if self.stripe {
            request.header("Stripe-Version", STRIPE_API_VERSION)
        } else {
            request.header("Paddle-Version", PADDLE_API_VERSION)
        };
        request = match body {
            Some(Body::Form(form)) => request.form(form),
            Some(Body::Json(json)) => request.json(json),
            None => request,
        };
        let response = request
            .send()
            .await
            .unwrap_or_else(|error| panic!("{method} {path}: {error}"));
        let status = response.status().as_u16();
        let text = response.text();
        assert!(
            (200..300).contains(&status),
            "{method} {path}: {status} {text}"
        );
        serde_json::from_str(&text).unwrap_or(Value::Null)
    }
}

enum Body<'a> {
    Form(&'a [(&'a str, String)]),
    Json(&'a Value),
}

#[tokio::test]
async fn stripe_test_mode_takes_what_the_client_sends() {
    let test = "stripe_test_mode_takes_what_the_client_sends";
    let Some(secret) = key("STRIPE_TEST_SECRET", test) else {
        return;
    };
    assert!(
        secret.starts_with("sk_test_") || secret.starts_with("rk_test_"),
        "STRIPE_TEST_SECRET must be a test-mode key (sk_test_ or rk_test_), never a live one"
    );
    let currency = std::env::var("STRIPE_TEST_CURRENCY")
        .ok()
        .filter(|c| !c.trim().is_empty())
        .unwrap_or_else(|| "inr".to_owned())
        .to_ascii_lowercase();
    let api = Api {
        client: client(),
        base: "https://api.stripe.com",
        key: secret.clone(),
        stripe: true,
    };
    let stripe = StripeProvider::new(
        client(),
        Secret::new(secret),
        None,
        "https://api.stripe.com",
    );

    let customer = stripe
        .create_customer(&NewCustomer {
            billable_type: "workspace".into(),
            billable_id: Uuid::now_v7(),
            name: "Harpreet Kaur".into(),
            email: Some("harpreet.kaur@example.com".into()),
            idempotency_key: fresh("customer"),
        })
        .await
        .expect("Stripe creates the customer")
        .expect("a customer id");
    assert!(customer.starts_with("cus_"), "{customer}");

    // A monthly plan price, as `billing.plans` names one.
    let price = api
        .send(
            Method::POST,
            "/v1/prices",
            Some(Body::Form(&[
                ("currency", currency.clone()),
                ("unit_amount", "149900".into()),
                ("recurring[interval]", "month".into()),
                ("product_data[name]", "Navree sandbox: Pro, Ludhiana".into()),
            ])),
        )
        .await["id"]
        .as_str()
        .expect("a price id")
        .to_owned();
    assert!(stripe.price_exists(&price).await.unwrap());
    assert!(
        !stripe
            .price_exists("price_navree_sandbox_missing")
            .await
            .unwrap()
    );

    let checkout = stripe
        .create_checkout(&CheckoutRequest {
            billable_type: "workspace".into(),
            billable_id: Uuid::now_v7(),
            customer_id: Some(customer.clone()),
            customer_email: None,
            plan: "pro".into(),
            interval: Interval::Monthly,
            price_id: price.clone(),
            metered_prices: Vec::new(),
            quantity: 2,
            trial_days: Some(14),
            coupon: None,
            allow_promotion_codes: true,
            success_url: "https://app.example.in/settings/billing?upgraded=1".into(),
            cancel_url: "https://app.example.in/settings/billing".into(),
            metadata: BTreeMap::from([("source".to_owned(), "sandbox".to_owned())]),
            idempotency_key: fresh("checkout"),
        })
        .await
        .expect("Stripe creates the subscription checkout");
    assert!(checkout.id.starts_with("cs_test_"), "{}", checkout.id);
    assert!(checkout.url.starts_with("https://"), "{}", checkout.url);

    for methods in [Vec::new(), vec![PaymentMethod::Card]] {
        let payment = stripe
            .create_payment(&PaymentRequest {
                billable: Some(("workspace".into(), Uuid::now_v7())),
                amount: Money::new(149_900, &currency),
                description: "Annual support, Amritsar office".into(),
                reference: format!("INV-{}", Uuid::now_v7().simple()),
                customer: PaymentCustomer {
                    customer_id: Some(customer.clone()),
                    ..PaymentCustomer::default()
                },
                methods: methods.clone(),
                success_url: Some("https://app.example.in/orders/42?paid=1".into()),
                cancel_url: Some("https://app.example.in/orders/42".into()),
                expires_at: Some(Utc::now() + chrono::TimeDelta::hours(2)),
                metadata: BTreeMap::from([("order".to_owned(), "42".to_owned())]),
                idempotency_key: fresh("payment"),
            })
            .await
            .unwrap_or_else(|error| panic!("a payment with {methods:?}: {error}"));
        assert!(payment.id.starts_with("cs_test_"), "{}", payment.id);
        assert_eq!(payment.amount, Money::new(149_900, &currency));
    }

    let link = stripe
        .create_payment_link(&PaymentLinkRequest {
            billable: None,
            amount: Money::new(250_000, &currency),
            description: "Workshop seat, Patiala".into(),
            reference: "SEAT-7".into(),
            customer: PaymentCustomer::default(),
            methods: Vec::new(),
            expires_at: None,
            notify_by_email: false,
            notify_by_sms: false,
            return_url: Some("https://app.example.in/thanks".into()),
            metadata: BTreeMap::new(),
            idempotency_key: fresh("link"),
        })
        .await
        .expect("Stripe creates the payment link");
    assert!(
        link.url.contains("client_reference_id=SEAT-7"),
        "{}",
        link.url
    );
    api.send(
        Method::POST,
        &format!("/v1/payment_links/{}", link.id),
        Some(Body::Form(&[("active", "false".into())])),
    )
    .await;

    // A paid test payment (Stripe's test card), then part of it refunded.
    let intent = api
        .send(
            Method::POST,
            "/v1/payment_intents",
            Some(Body::Form(&[
                ("amount", "149900".into()),
                ("currency", currency.clone()),
                ("customer", customer.clone()),
                ("payment_method", "pm_card_visa".into()),
                ("confirm", "true".into()),
                ("automatic_payment_methods[enabled]", "true".into()),
                ("automatic_payment_methods[allow_redirects]", "never".into()),
            ])),
        )
        .await;
    assert_eq!(intent["status"], "succeeded", "{intent}");
    let intent_id = intent["id"]
        .as_str()
        .expect("a PaymentIntent id")
        .to_owned();
    let refund = stripe
        .refund(&RefundRequest {
            payment_id: intent_id.clone(),
            amount: Some(Money::new(40_000, &currency)),
            reason: RefundReason::RequestedByCustomer,
            note: Some("one seat returned".into()),
            reference: "RF-2026-0007".into(),
            billable: None,
            metadata: BTreeMap::new(),
            idempotency_key: fresh("refund"),
        })
        .await
        .expect("Stripe refunds part of the payment");
    assert_eq!(refund.payment_id.as_deref(), Some(intent_id.as_str()));
    assert_eq!(refund.amount, Money::new(40_000, &currency));
    assert!(
        matches!(
            refund.status,
            RefundStatus::Succeeded | RefundStatus::Pending
        ),
        "{:?}",
        refund.status
    );

    assert!(stripe.subscriptions(&customer).await.unwrap().is_empty());
    stripe
        .invoices(&customer)
        .await
        .expect("Stripe lists the customer's invoices");

    // The endpoint `billing:webhook` creates: on the pinned version, for
    // the events Navree handles.
    let endpoint = stripe
        .create_webhook_endpoint(&format!(
            "https://example.com/navree-sandbox/{}",
            Uuid::now_v7().simple()
        ))
        .await
        .expect("Stripe creates the webhook endpoint");
    assert!(
        endpoint.secret.is_some_and(|s| s.starts_with("whsec_")),
        "a signing secret"
    );
    let created = api
        .send(
            Method::GET,
            &format!("/v1/webhook_endpoints/{}", endpoint.id),
            None,
        )
        .await;
    assert_eq!(created["api_version"], STRIPE_API_VERSION);
    let mut events: Vec<&str> = created["enabled_events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    events.sort_unstable();
    let mut expected = STRIPE_EVENTS.to_vec();
    expected.sort_unstable();
    assert_eq!(events, expected);
    api.send(
        Method::DELETE,
        &format!("/v1/webhook_endpoints/{}", endpoint.id),
        None,
    )
    .await;

    // The events this run caused parse (those Navree does not handle map
    // to no action).
    let recent = api.send(Method::GET, "/v1/events?limit=20", None).await;
    for event in recent["data"].as_array().expect("a list of events") {
        let body = serde_json::to_vec(event).unwrap();
        stripe
            .parse_webhook(&body)
            .unwrap_or_else(|error| panic!("event {}: {error}", event["type"]));
    }

    api.send(Method::DELETE, &format!("/v1/customers/{customer}"), None)
        .await;
}

#[tokio::test]
async fn paddle_sandbox_takes_what_the_client_sends() {
    let test = "paddle_sandbox_takes_what_the_client_sends";
    let Some(api_key) = key("PADDLE_SANDBOX_KEY", test) else {
        return;
    };
    let base = "https://sandbox-api.paddle.com";
    let api = Api {
        client: client(),
        base,
        key: api_key.clone(),
        stripe: false,
    };
    let paddle = PaddleProvider::new(client(), Secret::new(api_key), None, base);

    let customer = paddle
        .create_customer(&NewCustomer {
            billable_type: "workspace".into(),
            billable_id: Uuid::now_v7(),
            name: "Rajinder Singh".into(),
            email: Some(format!(
                "rajinder.singh+{}@example.com",
                Uuid::now_v7().simple()
            )),
            idempotency_key: fresh("customer"),
        })
        .await
        .expect("Paddle creates the customer")
        .expect("a customer id");
    assert!(customer.starts_with("ctm_"), "{customer}");

    // A product with a monthly price, as `billing.plans` names one.
    let product = api
        .send(
            Method::POST,
            "/products",
            Some(Body::Json(&json!({
                "name": "Navree sandbox: Pro, Jalandhar",
                "tax_category": "standard",
            }))),
        )
        .await["data"]["id"]
        .as_str()
        .expect("a product id")
        .to_owned();
    let price = api
        .send(
            Method::POST,
            "/prices",
            Some(Body::Json(&json!({
                "product_id": product,
                "description": "Pro, monthly",
                "unit_price": { "amount": "149900", "currency_code": "INR" },
                "billing_cycle": { "interval": "month", "frequency": 1 },
            }))),
        )
        .await["data"]["id"]
        .as_str()
        .expect("a price id")
        .to_owned();
    assert!(paddle.price_exists(&price).await.unwrap());
    assert!(
        !paddle
            .price_exists("pri_01navreesandboxmissing000000")
            .await
            .unwrap()
    );

    let checkout = paddle
        .create_checkout(&CheckoutRequest {
            billable_type: "workspace".into(),
            billable_id: Uuid::now_v7(),
            customer_id: Some(customer.clone()),
            customer_email: None,
            plan: "pro".into(),
            interval: Interval::Monthly,
            price_id: price.clone(),
            metered_prices: Vec::new(),
            quantity: 1,
            trial_days: None,
            coupon: None,
            allow_promotion_codes: false,
            success_url: "https://app.example.in/settings/billing?upgraded=1".into(),
            cancel_url: "https://app.example.in/settings/billing".into(),
            metadata: BTreeMap::new(),
            idempotency_key: fresh("checkout"),
        })
        .await
        .expect("Paddle creates the checkout (a transaction)");
    assert!(checkout.id.starts_with("txn_"), "{}", checkout.id);

    let payment = paddle
        .create_payment(&PaymentRequest {
            billable: Some(("workspace".into(), Uuid::now_v7())),
            amount: Money::inr(99_900),
            description: "Onboarding session, Mohali".into(),
            reference: format!("INV-{}", Uuid::now_v7().simple()),
            customer: PaymentCustomer {
                customer_id: Some(customer.clone()),
                ..PaymentCustomer::default()
            },
            methods: Vec::new(),
            success_url: Some("https://app.example.in/orders/42?paid=1".into()),
            cancel_url: None,
            expires_at: None,
            metadata: BTreeMap::new(),
            idempotency_key: fresh("payment"),
        })
        .await
        .expect("Paddle creates the one-time payment (a transaction)");
    assert!(payment.id.starts_with("txn_"), "{}", payment.id);
    assert_eq!(payment.amount.amount, 99_900);

    assert!(paddle.subscriptions(&customer).await.unwrap().is_empty());
    paddle
        .invoices(&customer)
        .await
        .expect("Paddle lists the customer's transactions");
    let portal = paddle
        .portal_url(&customer, "https://app.example.in/settings/billing", &[])
        .await
        .expect("Paddle opens a customer portal session");
    assert!(portal.starts_with("https://"), "{portal}");

    let endpoint = paddle
        .create_webhook_endpoint(&format!(
            "https://example.com/navree-sandbox/{}",
            Uuid::now_v7().simple()
        ))
        .await
        .expect("Paddle creates the notification destination");
    api.send(
        Method::DELETE,
        &format!("/notification-settings/{}", endpoint.id),
        None,
    )
    .await;

    // Paddle archives rather than deletes.
    for path in [format!("/prices/{price}"), format!("/products/{product}")] {
        api.send(
            Method::PATCH,
            &path,
            Some(Body::Json(&json!({ "status": "archived" }))),
        )
        .await;
    }
}
