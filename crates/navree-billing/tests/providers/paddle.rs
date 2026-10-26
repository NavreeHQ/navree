//! `PaddleProvider` against a local mock server.

use std::time::Duration;

use chrono::{DateTime, Utc};
use http::{HeaderMap, HeaderValue};
use navree_billing::{
    BillingProvider, InvoiceStatus, NewCustomer, PADDLE_API_VERSION, PADDLE_EVENTS, PaddleProvider,
    ProviderError, ProviderInvoice, SubscriptionStatus, UsageReport, WebhookAction, WebhookError,
    sign_paddle_webhook,
};
use navree_contracts::Secret;
use serde_json::json;
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::fixtures::{NOW, PERIOD_END, TRIAL_END, WORKSPACE, paddle as fx};
use crate::support::{change, checkout_request, client, json};

const SUB: &str = "/subscriptions/sub_01j8acme";

async fn paddle() -> (MockServer, PaddleProvider) {
    let server = MockServer::start().await;
    let provider = PaddleProvider::new(
        client(),
        Secret::new("pdl_sdbx_apikey_01j8".to_owned()),
        Some(Secret::new("pdl_ntfset_01j8_secret".to_owned())),
        server.uri(),
    );
    (server, provider)
}

fn ok(data: serde_json::Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(fx::envelope(data))
}

fn at(secs: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(secs, 0).unwrap()
}

#[tokio::test]
async fn customers_and_checkouts_are_authenticated_json_posts_on_version_1() {
    let (server, paddle) = paddle().await;
    Mock::given(method("POST"))
        .and(path("/customers"))
        .and(header("authorization", "Bearer pdl_sdbx_apikey_01j8"))
        .and(header("paddle-version", PADDLE_API_VERSION))
        .and(header("content-type", "application/json"))
        .respond_with(ok(fx::customer()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/transactions"))
        .respond_with(ok(fx::checkout_transaction()))
        .expect(1)
        .mount(&server)
        .await;

    let customer = paddle
        .create_customer(&NewCustomer {
            billable_type: "workspace".into(),
            billable_id: WORKSPACE,
            name: "Acme".into(),
            email: Some("ada@example.com".into()),
        })
        .await
        .unwrap();
    assert_eq!(customer.as_deref(), Some("ctm_01j8acme"));
    // Without an email Paddle can't create a customer: the checkout will.
    let none = paddle
        .create_customer(&NewCustomer {
            billable_type: "workspace".into(),
            billable_id: WORKSPACE,
            name: "Acme".into(),
            email: None,
        })
        .await
        .unwrap();
    assert_eq!(none, None);

    let checkout = paddle
        .create_checkout(&checkout_request(customer.as_deref(), "pri_pro_m"))
        .await
        .unwrap();
    assert_eq!(checkout.id, "txn_01j8checkout");
    assert_eq!(checkout.url, "https://acme.test/pay?_ptxn=txn_01j8checkout");

    let requests = server.received_requests().await.unwrap();
    let created = json(&requests[0]);
    assert_eq!(created["email"], "ada@example.com");
    assert_eq!(created["name"], "Acme");
    assert_eq!(created["custom_data"]["billable_id"], WORKSPACE.to_string());
    let transaction = json(&requests[1]);
    assert_eq!(
        transaction["items"],
        json!([{ "price_id": "pri_pro_m", "quantity": 2 }])
    );
    assert_eq!(transaction["customer_id"], "ctm_01j8acme");
    assert_eq!(transaction["custom_data"]["plan"], "pro");
    assert_eq!(transaction["collection_mode"], "automatic");
}

#[tokio::test]
async fn a_transaction_without_a_checkout_url_is_explained() {
    let (server, paddle) = paddle().await;
    let mut transaction = fx::checkout_transaction();
    transaction["checkout"] = serde_json::Value::Null;
    Mock::given(method("POST"))
        .and(path("/transactions"))
        .respond_with(ok(transaction))
        .mount(&server)
        .await;
    let error = paddle
        .create_checkout(&checkout_request(None, "pri_pro_m"))
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("default payment link"),
        "{error}"
    );
}

#[tokio::test]
async fn subscription_changes_return_the_mapped_subscription() {
    let (server, paddle) = paddle().await;
    Mock::given(method("PATCH"))
        .and(path(SUB))
        .and(body_json(json!({
            "items": [{ "price_id": "pri_business_m", "quantity": 3 }],
            "proration_billing_mode": "prorated_next_billing_period"
        })))
        .respond_with(ok(fx::subscription("active", "pri_business_m", 3)))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{SUB}/cancel")))
        .and(body_json(
            json!({ "effective_from": "next_billing_period" }),
        ))
        .respond_with(ok(fx::canceling("pri_pro_m")))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{SUB}/cancel")))
        .and(body_json(json!({ "effective_from": "immediately" })))
        .respond_with(ok(fx::canceled("pri_pro_m")))
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path(SUB))
        .and(body_json(json!({ "scheduled_change": null })))
        .respond_with(ok(fx::subscription("active", "pri_pro_m", 2)))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{SUB}/pause")))
        .respond_with(ok(fx::paused("pri_pro_m")))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{SUB}/resume")))
        .respond_with(ok(fx::subscription("active", "pri_pro_m", 2)))
        .mount(&server)
        .await;

    let changed = paddle
        .update_subscription("sub_01j8acme", &change(None, "pri_business_m", 3))
        .await
        .unwrap();
    assert_eq!(changed.status, SubscriptionStatus::Active);
    assert_eq!(changed.items[0].price_id, "pri_business_m");
    assert_eq!(changed.items[0].quantity, Some(3));
    assert_eq!(changed.current_period_end, Some(at(PERIOD_END)));
    let canceling = paddle
        .cancel_subscription("sub_01j8acme", true)
        .await
        .unwrap();
    assert_eq!(canceling.ends_at, Some(at(PERIOD_END)));
    assert_eq!(canceling.status, SubscriptionStatus::Active);
    let resumed = paddle.resume_subscription("sub_01j8acme").await.unwrap();
    assert_eq!(resumed.ends_at, None);
    let paused = paddle.pause_subscription("sub_01j8acme").await.unwrap();
    assert_eq!(paused.status, SubscriptionStatus::Paused);
    let unpaused = paddle.unpause_subscription("sub_01j8acme").await.unwrap();
    assert_eq!(unpaused.status, SubscriptionStatus::Active);
    let canceled = paddle
        .cancel_subscription("sub_01j8acme", false)
        .await
        .unwrap();
    assert_eq!(canceled.status, SubscriptionStatus::Canceled);
    assert_eq!(canceled.ends_at, Some(at(NOW)));
}

#[tokio::test]
async fn lists_invoices_the_portal_prices_charges_and_notification_settings() {
    let (server, paddle) = paddle().await;
    Mock::given(method("GET"))
        .and(path("/subscriptions"))
        .and(query_param("customer_id", "ctm_01j8acme"))
        .respond_with(ok(json!([fx::subscription("trialing", "pri_pro_m", 2)])))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/transactions"))
        .and(query_param("customer_id", "ctm_01j8acme"))
        .respond_with(ok(json!([fx::transaction("completed", "5800")])))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/customers/ctm_01j8acme/portal-sessions"))
        .and(body_json(json!({ "subscription_ids": ["sub_01j8acme"] })))
        .respond_with(ok(fx::portal_session()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/prices/pri_pro_m"))
        .respond_with(ok(fx::price()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/prices/pri_gone"))
        .respond_with(
            ResponseTemplate::new(404).set_body_json(fx::error("Entity pri_gone not found")),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{SUB}/charge")))
        .and(body_json(json!({
            "effective_from": "next_billing_period",
            "items": [{ "price_id": "pri_ai", "quantity": 3 }]
        })))
        .respond_with(ok(fx::subscription("active", "pri_pro_m", 2)))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/notification-settings"))
        .respond_with(ok(fx::notification_setting()))
        .mount(&server)
        .await;

    let subs = paddle.subscriptions("ctm_01j8acme").await.unwrap();
    assert_eq!(subs[0].status, SubscriptionStatus::Trialing);
    assert_eq!(subs[0].trial_ends_at, Some(at(TRIAL_END)));
    assert_eq!(
        subs[0].billable(),
        Some(("workspace".to_owned(), WORKSPACE))
    );

    let invoices = paddle.invoices("ctm_01j8acme").await.unwrap();
    assert_eq!(invoices[0].number, "325-10001");
    assert_eq!(invoices[0].status, InvoiceStatus::Paid);
    assert_eq!(invoices[0].total_cents, 5800);
    assert_eq!(invoices[0].currency, "usd");

    let url = paddle
        .portal_url(
            "ctm_01j8acme",
            "https://ignored.test",
            &["sub_01j8acme".to_owned()],
        )
        .await
        .unwrap();
    assert!(url.starts_with("https://customer-portal.paddle.com/cpl_01j8"));
    assert!(paddle.price_exists("pri_pro_m").await.unwrap());
    assert!(!paddle.price_exists("pri_gone").await.unwrap());

    paddle
        .report_usage(&UsageReport {
            customer_id: "ctm_01j8acme".into(),
            subscription_id: "sub_01j8acme".into(),
            meter: "ai_summaries".into(),
            price_id: "pri_ai".into(),
            quantity: 3,
            idempotency_key: "navree-usage-abc".into(),
            timestamp: at(NOW),
        })
        .await
        .unwrap();

    let endpoint = paddle
        .create_webhook_endpoint("https://api.acme.test/api/billing/webhook")
        .await
        .unwrap();
    assert_eq!(endpoint.id, "ntfset_01j8");
    assert_eq!(
        endpoint.secret.as_deref(),
        Some("pdl_ntfset_01j8_created_by_api")
    );
    let requests = server.received_requests().await.unwrap();
    let created = json(requests.last().unwrap());
    assert_eq!(
        created["destination"],
        "https://api.acme.test/api/billing/webhook"
    );
    assert_eq!(created["subscribed_events"], json!(PADDLE_EVENTS));
}

#[tokio::test]
async fn errors_carry_the_status_and_detail() {
    let (server, paddle) = paddle().await;
    Mock::given(method("PATCH"))
        .and(path(SUB))
        .respond_with(
            ResponseTemplate::new(404).set_body_json(fx::error("Entity sub_01j8acme not found")),
        )
        .mount(&server)
        .await;
    let error = paddle
        .update_subscription("sub_01j8acme", &change(None, "pri_pro_m", 1))
        .await
        .unwrap_err();
    assert!(matches!(
        &error,
        ProviderError::Api { provider: "paddle", status: 404, message } if message == "Entity sub_01j8acme not found"
    ));
}

#[tokio::test]
async fn notifications_are_verified_and_parsed() {
    let (_server, paddle) = paddle().await;
    let tolerance = Duration::from_secs(300);
    let body = serde_json::to_vec(&fx::event(
        "evt_01j8completed",
        "transaction.completed",
        fx::transaction("completed", "5800"),
    ))
    .unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(
        "paddle-signature",
        HeaderValue::from_str(&sign_paddle_webhook("pdl_ntfset_01j8_secret", NOW, &body)).unwrap(),
    );
    paddle
        .verify_webhook(&headers, &body, at(NOW), tolerance)
        .unwrap();
    assert_eq!(
        paddle.verify_webhook(&headers, &body, at(NOW + 301), tolerance),
        Err(WebhookError::Expired)
    );
    let event = paddle.parse_webhook(&body).unwrap();
    assert_eq!(event.id, "evt_01j8completed");
    assert_eq!(event.occurred_at, at(NOW));
    assert_eq!(
        event.actions,
        [
            WebhookAction::CustomerLinked {
                customer_id: "ctm_01j8acme".into(),
                billable: Some(("workspace".into(), WORKSPACE)),
                email: None,
            },
            WebhookAction::InvoicePaid(ProviderInvoice {
                id: "txn_01j8first".into(),
                customer_id: Some("ctm_01j8acme".into()),
                subscription_id: Some("sub_01j8acme".into()),
                amount_cents: 5800,
                currency: "usd".into(),
            }),
        ]
    );
    let failed = serde_json::to_vec(&fx::event(
        "evt_01j8failed",
        "transaction.payment_failed",
        fx::transaction("past_due", "5800"),
    ))
    .unwrap();
    assert!(matches!(
        paddle.parse_webhook(&failed).unwrap().actions.as_slice(),
        [WebhookAction::PaymentFailed(_)]
    ));
    let canceled = serde_json::to_vec(&fx::event(
        "evt_01j8canceled",
        "subscription.canceled",
        fx::canceled("pri_pro_m"),
    ))
    .unwrap();
    let actions = paddle.parse_webhook(&canceled).unwrap().actions;
    let [WebhookAction::Subscription(sub)] = actions.as_slice() else {
        panic!("a subscription action: {actions:?}");
    };
    assert_eq!(sub.status, SubscriptionStatus::Canceled);
}
