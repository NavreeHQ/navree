//! `StripeProvider` against a local mock server.

use chrono::{DateTime, Utc};
use http::{HeaderMap, HeaderValue};
use navree_billing::{
    BillingError, BillingProvider, InvoiceStatus, ProviderError, STRIPE_API_VERSION, STRIPE_EVENTS,
    StripeProvider, SubscriptionStatus, UsageReport, WebhookAction, WebhookError,
    sign_stripe_webhook,
};
use navree_contracts::Secret;
use std::time::Duration;
use wiremock::matchers::{body_string, body_string_contains, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::fixtures::{NOW, PERIOD_END, TRIAL_END, WORKSPACE, stripe as fx};
use crate::support::{change, checkout_request, client, field, form};

const SUB: &str = "/v1/subscriptions/sub_1Q8XyzAbC";

async fn stripe() -> (MockServer, StripeProvider) {
    let server = MockServer::start().await;
    let provider = StripeProvider::new(
        client(),
        Secret::new("sk_test_51Q8".to_owned()),
        Some(Secret::new("whsec_test_Jx2".to_owned())),
        server.uri(),
    );
    (server, provider)
}

fn ok(body: serde_json::Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(body)
}

fn at(secs: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(secs, 0).unwrap()
}

#[tokio::test]
async fn customers_and_checkouts_are_authenticated_form_posts_on_the_pinned_version() {
    let (server, stripe) = stripe().await;
    Mock::given(method("POST"))
        .and(path("/v1/customers"))
        .and(header("authorization", "Bearer sk_test_51Q8"))
        .and(header("stripe-version", STRIPE_API_VERSION))
        .and(header("content-type", "application/x-www-form-urlencoded"))
        .respond_with(ok(fx::customer()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/checkout/sessions"))
        .and(header("stripe-version", STRIPE_API_VERSION))
        .respond_with(ok(fx::checkout_session()))
        .expect(1)
        .mount(&server)
        .await;

    let customer = stripe
        .create_customer(&navree_billing::NewCustomer {
            billable_type: "workspace".into(),
            billable_id: WORKSPACE,
            name: "Acme".into(),
            email: Some("ada@example.com".into()),
        })
        .await
        .unwrap();
    assert_eq!(customer.as_deref(), Some("cus_QX1aBcDeF"));
    let checkout = stripe
        .create_checkout(&checkout_request(customer.as_deref(), "price_pro_m"))
        .await
        .unwrap();
    assert_eq!(checkout.id, "cs_test_a1B2c3D4");
    assert_eq!(
        checkout.url,
        "https://checkout.stripe.com/c/pay/cs_test_a1B2c3D4"
    );

    let requests = server.received_requests().await.unwrap();
    let created = form(&requests[0]);
    assert_eq!(field(&created, "name"), Some("Acme"));
    assert_eq!(field(&created, "email"), Some("ada@example.com"));
    assert_eq!(
        field(&created, "metadata[billable_type]"),
        Some("workspace")
    );
    assert_eq!(
        field(&created, "metadata[billable_id]"),
        Some(WORKSPACE.to_string().as_str())
    );
    let session = form(&requests[1]);
    assert_eq!(field(&session, "mode"), Some("subscription"));
    assert_eq!(field(&session, "customer"), Some("cus_QX1aBcDeF"));
    assert_eq!(field(&session, "line_items[0][price]"), Some("price_pro_m"));
    assert_eq!(field(&session, "line_items[0][quantity]"), Some("2"));
    assert_eq!(
        field(&session, "subscription_data[trial_period_days]"),
        Some("14")
    );
    assert_eq!(
        field(&session, "subscription_data[metadata][plan]"),
        Some("pro")
    );
    assert_eq!(
        field(&session, "success_url"),
        Some("https://app.acme.test/settings/billing?upgraded=1")
    );
}

#[tokio::test]
async fn subscription_changes_return_the_mapped_subscription() {
    let (server, stripe) = stripe().await;
    Mock::given(method("POST"))
        .and(path(SUB))
        .and(body_string_contains(
            "items%5B0%5D%5Bprice%5D=price_business_m",
        ))
        .respond_with(ok(fx::subscription("active", "price_business_m", 3)))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(SUB))
        .and(body_string("cancel_at_period_end=true"))
        .respond_with(ok(fx::canceling("price_pro_m")))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(SUB))
        .and(body_string("cancel_at_period_end=false"))
        .respond_with(ok(fx::subscription("active", "price_pro_m", 2)))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(SUB))
        .and(body_string("pause_collection%5Bbehavior%5D=void"))
        .respond_with(ok(fx::paused("price_pro_m")))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(SUB))
        .and(body_string("pause_collection="))
        .respond_with(ok(fx::subscription("active", "price_pro_m", 2)))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(SUB))
        .respond_with(ok(fx::canceled("price_pro_m")))
        .mount(&server)
        .await;

    let changed = stripe
        .update_subscription(
            "sub_1Q8XyzAbC",
            &change(Some("si_QX1base"), "price_business_m", 3),
        )
        .await
        .unwrap();
    assert_eq!(changed.status, SubscriptionStatus::Active);
    assert_eq!(changed.items[0].price_id, "price_business_m");
    assert_eq!(changed.items[0].quantity, Some(3));
    assert_eq!(changed.items[1].quantity, None, "metered");
    assert_eq!(changed.current_period_end, Some(at(PERIOD_END)));
    let sent = form(&server.received_requests().await.unwrap()[0]);
    assert_eq!(field(&sent, "items[0][id]"), Some("si_QX1base"));
    assert_eq!(
        field(&sent, "proration_behavior"),
        Some("create_prorations")
    );

    let canceling = stripe
        .cancel_subscription("sub_1Q8XyzAbC", true)
        .await
        .unwrap();
    assert_eq!(canceling.status, SubscriptionStatus::Active);
    assert_eq!(canceling.ends_at, Some(at(PERIOD_END)));
    let resumed = stripe.resume_subscription("sub_1Q8XyzAbC").await.unwrap();
    assert_eq!(resumed.ends_at, None);
    let paused = stripe.pause_subscription("sub_1Q8XyzAbC").await.unwrap();
    assert_eq!(paused.status, SubscriptionStatus::Paused);
    let unpaused = stripe.unpause_subscription("sub_1Q8XyzAbC").await.unwrap();
    assert_eq!(unpaused.status, SubscriptionStatus::Active);
    let canceled = stripe
        .cancel_subscription("sub_1Q8XyzAbC", false)
        .await
        .unwrap();
    assert_eq!(canceled.status, SubscriptionStatus::Canceled);
    assert_eq!(canceled.ends_at, Some(at(NOW)));
}

#[tokio::test]
async fn lists_invoices_the_portal_prices_and_webhook_endpoints() {
    let (server, stripe) = stripe().await;
    Mock::given(method("GET"))
        .and(path("/v1/subscriptions"))
        .and(query_param("customer", "cus_QX1aBcDeF"))
        .and(query_param("status", "all"))
        .respond_with(ok(fx::list(vec![fx::subscription(
            "trialing",
            "price_pro_m",
            2,
        )])))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/invoices"))
        .and(query_param("customer", "cus_QX1aBcDeF"))
        .respond_with(ok(fx::list(vec![fx::invoice("paid", 5800)])))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/billing_portal/sessions"))
        .respond_with(ok(fx::portal_session()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/prices/price_pro_m"))
        .respond_with(ok(fx::metered_price()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/prices/price_gone"))
        .respond_with(
            ResponseTemplate::new(404).set_body_json(fx::error("No such price: 'price_gone'")),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/webhook_endpoints"))
        .respond_with(ok(fx::webhook_endpoint()))
        .mount(&server)
        .await;

    let subs = stripe.subscriptions("cus_QX1aBcDeF").await.unwrap();
    assert_eq!(subs.len(), 1);
    assert_eq!(subs[0].status, SubscriptionStatus::Trialing);
    assert_eq!(subs[0].trial_ends_at, Some(at(TRIAL_END)));
    assert_eq!(
        subs[0].billable(),
        Some(("workspace".to_owned(), WORKSPACE))
    );

    let invoices = stripe.invoices("cus_QX1aBcDeF").await.unwrap();
    assert_eq!(invoices.len(), 1);
    let invoice = &invoices[0];
    assert_eq!(invoice.number, "A1B2C3D4-0001");
    assert_eq!(invoice.status, InvoiceStatus::Paid);
    assert_eq!(invoice.total_cents, 5800);
    assert_eq!(invoice.currency, "usd");
    assert!(invoice.pdf_url.as_deref().unwrap().ends_with("/pdf"));
    assert!(invoice.hosted_url.is_some());
    assert_eq!(invoice.created_at, at(NOW));

    let url = stripe
        .portal_url(
            "cus_QX1aBcDeF",
            "https://app.acme.test/settings/billing",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(url, "https://billing.stripe.com/p/session/test_YWNjdA");
    assert!(stripe.price_exists("price_pro_m").await.unwrap());
    assert!(!stripe.price_exists("price_gone").await.unwrap());

    let endpoint = stripe
        .create_webhook_endpoint("https://api.acme.test/api/billing/webhook")
        .await
        .unwrap();
    assert_eq!(endpoint.id, "we_1Q8Endpoint");
    assert_eq!(endpoint.secret.as_deref(), Some("whsec_created_by_api"));
    let requests = server.received_requests().await.unwrap();
    let created = form(requests.last().unwrap());
    assert_eq!(field(&created, "api_version"), Some(STRIPE_API_VERSION));
    for (index, event) in STRIPE_EVENTS.iter().enumerate() {
        assert_eq!(
            field(&created, &format!("enabled_events[{index}]")),
            Some(*event)
        );
    }
    let portal = form(&requests[2]);
    assert_eq!(field(&portal, "customer"), Some("cus_QX1aBcDeF"));
    assert_eq!(
        field(&portal, "return_url"),
        Some("https://app.acme.test/settings/billing")
    );
}

#[tokio::test]
async fn usage_becomes_meter_events_after_resolving_the_meter_once() {
    let (server, stripe) = stripe().await;
    Mock::given(method("GET"))
        .and(path("/v1/prices/price_ai"))
        .respond_with(ok(fx::metered_price()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/billing/meters/mtr_test_61Q8AiSummaries"))
        .respond_with(ok(fx::meter()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/billing/meter_events"))
        .respond_with(ok(fx::meter_event()))
        .expect(2)
        .mount(&server)
        .await;
    let report = UsageReport {
        customer_id: "cus_QX1aBcDeF".into(),
        subscription_id: "sub_1Q8XyzAbC".into(),
        meter: "ai_summaries".into(),
        price_id: "price_ai".into(),
        quantity: 3,
        idempotency_key: "navree-usage-abc".into(),
        timestamp: at(NOW),
    };
    stripe.report_usage(&report).await.unwrap();
    stripe.report_usage(&report).await.unwrap();
    let requests = server.received_requests().await.unwrap();
    let event = form(requests.last().unwrap());
    assert_eq!(field(&event, "event_name"), Some("ai_summary_generated"));
    assert_eq!(
        field(&event, "payload[stripe_customer_id]"),
        Some("cus_QX1aBcDeF")
    );
    assert_eq!(field(&event, "payload[value]"), Some("3"));
    assert_eq!(field(&event, "identifier"), Some("navree-usage-abc"));
    assert_eq!(field(&event, "timestamp"), Some(NOW.to_string().as_str()));
}

#[tokio::test]
async fn errors_carry_the_status_and_message_but_no_secrets() {
    let (server, stripe) = stripe().await;
    Mock::given(method("POST"))
        .and(path("/v1/checkout/sessions"))
        .respond_with(
            ResponseTemplate::new(400).set_body_json(fx::error("No such price: 'price_x'")),
        )
        .mount(&server)
        .await;
    let error = stripe
        .create_checkout(&checkout_request(None, "price_x"))
        .await
        .unwrap_err();
    assert!(matches!(
        &error,
        ProviderError::Api { provider: "stripe", status: 400, message } if message == "No such price: 'price_x'"
    ));
    let response = BillingError::from(error).to_app_error();
    assert_eq!(response.status.as_u16(), 502);
    assert_eq!(
        response.body()["message"],
        "The payment provider refused the request: No such price: 'price_x'"
    );

    // An unreachable API is a transport error.
    let unreachable = StripeProvider::new(
        client(),
        Secret::new("sk_test_51Q8".to_owned()),
        None,
        "http://127.0.0.1:9",
    );
    let error = unreachable.price_exists("price_x").await.unwrap_err();
    assert!(matches!(error, ProviderError::Transport { .. }), "{error}");
    let message = BillingError::from(error).to_app_error().body()["message"].clone();
    assert!(!message.as_str().unwrap().contains("sk_test"));
}

#[tokio::test]
async fn webhooks_are_verified_and_parsed() {
    let (_server, stripe) = stripe().await;
    let body = serde_json::to_vec(&fx::event(
        "evt_1Q8Created",
        "customer.subscription.created",
        fx::subscription("trialing", "price_pro_m", 2),
    ))
    .unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(
        "stripe-signature",
        HeaderValue::from_str(&sign_stripe_webhook("whsec_test_Jx2", NOW, &body)).unwrap(),
    );
    let tolerance = Duration::from_secs(300);
    stripe
        .verify_webhook(&headers, &body, at(NOW + 10), tolerance)
        .unwrap();
    assert_eq!(
        stripe.verify_webhook(&headers, b"{}", at(NOW), tolerance),
        Err(WebhookError::SignatureMismatch)
    );
    let event = stripe.parse_webhook(&body).unwrap();
    assert_eq!(event.id, "evt_1Q8Created");
    assert_eq!(event.occurred_at, at(NOW));
    let [WebhookAction::Subscription(sub)] = event.actions.as_slice() else {
        panic!("{:?}", event.actions);
    };
    assert_eq!(sub.id, "sub_1Q8XyzAbC");
    assert_eq!(sub.customer_id, "cus_QX1aBcDeF");
    assert_eq!(sub.trial_ends_at, Some(at(TRIAL_END)));

    let without_secret = StripeProvider::new(
        client(),
        Secret::new("sk".to_owned()),
        None,
        "https://api.stripe.com",
    );
    assert_eq!(
        without_secret.verify_webhook(&headers, &body, at(NOW), tolerance),
        Err(WebhookError::NoSecret)
    );
}
