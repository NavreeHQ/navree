//! Apps wired to the real drivers (`billing.provider = "stripe"` and
//! `"paddle"`), pointed at a local mock server: checkout, signed webhooks
//! through the route and the queue, changes, usage and invoices.

use std::sync::Arc;

use chrono::DateTime;
use navree_billing::{
    BillableRef, Billing, BillingModule, PADDLE_SIGNATURE_HEADER, STRIPE_SIGNATURE_HEADER,
    SubscriptionStatus, sign_paddle_webhook, sign_stripe_webhook,
};
use navree_contracts::{Clock, FixedClock};
use navree_db::DatabaseModule;
use navree_events::EventsModule;
use navree_http_client::HttpClientModule;
use navree_queue::{Queue, QueueModule};
use navree_testing::{TestApp, TestAppBuilder};
use serde_json::json;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::fixtures::{NOW, WORKSPACE, paddle as pfx, stripe as sfx};

fn agra() -> BillableRef {
    BillableRef {
        billable_type: "workspace".into(),
        billable_id: WORKSPACE,
        name: "Agra".into(),
        email: Some("anu@example.com".into()),
    }
}

fn app(server: &MockServer) -> TestAppBuilder {
    crate::servers::database(TestApp::builder())
        .config("queue.driver", "memory")
        .config("http_client.ssrf.allow_hosts", json!(["127.0.0.1"]))
        .config("http_client.retries", 0)
        .config("billing.routes", false)
        .config(
            "billing.return_url",
            "https://app.acme.test/settings/billing",
        )
        .config("billing.stripe.api_base", server.uri())
        .config("billing.paddle.api_base", server.uri())
        .module(DatabaseModule::from_config())
        .module(EventsModule::for_root())
        .module(QueueModule::from_config())
        .module(HttpClientModule::from_config())
        .module(BillingModule::from_config())
        .override_value::<dyn Clock>(Arc::new(FixedClock::new(
            DateTime::from_timestamp(NOW, 0).unwrap(),
        )))
}

#[tokio::test]
async fn a_stripe_app_checks_out_receives_webhooks_and_swaps() {
    let server = MockServer::start().await;
    let ok = |body| ResponseTemplate::new(200).set_body_json(body);
    Mock::given(method("POST"))
        .and(path("/v1/customers"))
        .respond_with(ok(sfx::customer()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/checkout/sessions"))
        .respond_with(ok(sfx::checkout_session()))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/subscriptions/sub_1Q8XyzAbC"))
        .and(body_string_contains("price_business_m"))
        .respond_with(ok(sfx::subscription("active", "price_business_m", 2)))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/invoices"))
        .respond_with(ok(sfx::list(vec![sfx::invoice("paid", 5800)])))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/prices/price_ai"))
        .respond_with(ok(sfx::metered_price()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/billing/meters/mtr_test_61Q8AiSummaries"))
        .respond_with(ok(sfx::meter()))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/billing/meter_events"))
        .and(body_string_contains("payload%5Bvalue%5D=5"))
        .respond_with(ok(sfx::meter_event()))
        .expect(1)
        .mount(&server)
        .await;

    let app = app(&server)
        .config("billing.provider", "stripe")
        .config("billing.stripe.secret_key", "sk_test_51Q8")
        .config("billing.stripe.webhook_secret", "whsec_app")
        .config(
            "billing.plans",
            json!({
                "pro": {
                    "prices": { "monthly": "price_pro_m" },
                    "trial_days": 14,
                    "meters": { "ai_summaries": "price_ai" }
                },
                "business": { "prices": { "monthly": "price_business_m" } }
            }),
        )
        .build()
        .await;
    let billing = app.get::<Billing>().await;
    assert!(billing.fake().is_none());
    assert_eq!(billing.provider().name(), "stripe");

    let checkout = billing
        .checkout(&agra(), "pro")
        .quantity(2)
        .create()
        .await
        .unwrap();
    assert_eq!(
        checkout.url,
        "https://checkout.stripe.com/c/pay/cs_test_a1B2c3D4"
    );
    assert_eq!(
        billing
            .customer(&agra())
            .await
            .unwrap()
            .unwrap()
            .provider_customer_id
            .as_deref(),
        Some("cus_QX1aBcDeF")
    );

    let client = app.client();
    for event in [
        sfx::event(
            "evt_1Q8Completed",
            "checkout.session.completed",
            json!({ "id": "cs_test_a1B2c3D4", "object": "checkout.session", "mode": "subscription",
                    "customer": "cus_QX1aBcDeF", "subscription": "sub_1Q8XyzAbC",
                    "metadata": { "billable_type": "workspace", "billable_id": WORKSPACE.to_string() } }),
        ),
        sfx::event(
            "evt_1Q8Created",
            "customer.subscription.created",
            sfx::subscription("trialing", "price_pro_m", 2),
        ),
        sfx::event("evt_1Q8Paid", "invoice.paid", sfx::invoice("paid", 0)),
    ] {
        let body = event.to_string();
        client
            .post("/api/billing/webhook")
            .header(
                STRIPE_SIGNATURE_HEADER,
                sign_stripe_webhook("whsec_app", NOW, body.as_bytes()).as_str(),
            )
            .header("content-type", "application/json")
            .body(body)
            .send()
            .await
            .assert_status(200);
    }
    app.get::<Queue>().await.run_pending().await.unwrap();
    let sub = billing.subscription(&agra()).await.unwrap().unwrap();
    assert!(sub.on_trial() && sub.on_plan("pro"));
    assert_eq!(sub.quantity, 2);
    assert_eq!(sub.provider_id.as_deref(), Some("sub_1Q8XyzAbC"));

    billing
        .report_usage(&agra(), "ai_summaries", 5)
        .await
        .unwrap();
    let summary = billing.report_pending_usage().await.unwrap();
    assert_eq!((summary.reported, summary.units), (1, 5));

    let swapped = billing.swap(&agra(), "business").await.unwrap();
    assert!(swapped.on_plan("business"));
    assert_eq!(swapped.status, SubscriptionStatus::Active);
    let sent = server.received_requests().await.unwrap();
    let swap = sent
        .iter()
        .find(|r| r.url.path() == "/v1/subscriptions/sub_1Q8XyzAbC")
        .unwrap();
    let form = crate::support::form(swap);
    assert_eq!(
        crate::support::field(&form, "items[0][id]"),
        Some("si_QX1base")
    );
    assert_eq!(
        crate::support::field(&form, "items[1][id]"),
        Some("si_QX1meter")
    );
    assert_eq!(
        crate::support::field(&form, "items[1][deleted]"),
        Some("true")
    );

    let invoices = billing.invoices(&agra()).await.unwrap();
    assert_eq!(invoices[0].total_cents, 5800);
    // Cached: the mock expects one call.
    billing.invoices(&agra()).await.unwrap();

    // A forged webhook is refused.
    client
        .post("/api/billing/webhook")
        .header(
            STRIPE_SIGNATURE_HEADER,
            sign_stripe_webhook("whsec_forged", NOW, b"{}").as_str(),
        )
        .body("{}")
        .send()
        .await
        .assert_error(400, "Invalid webhook signature.");
}

#[tokio::test]
async fn a_paddle_app_links_the_customer_from_the_transaction() {
    let server = MockServer::start().await;
    let ok = |data| ResponseTemplate::new(200).set_body_json(pfx::envelope(data));
    Mock::given(method("POST"))
        .and(path("/transactions"))
        .respond_with(ok(pfx::checkout_transaction()))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/subscriptions/sub_01j8agra/cancel"))
        .respond_with(ok(pfx::canceling("pri_pro_m")))
        .mount(&server)
        .await;

    let app = app(&server)
        .config("billing.provider", "paddle")
        .config("billing.paddle.api_key", "pdl_sdbx_apikey_01j8")
        .config("billing.paddle.webhook_secret", "pdl_ntfset_app")
        .config(
            "billing.plans",
            json!({ "pro": { "prices": { "monthly": "pri_pro_m" } } }),
        )
        .build()
        .await;
    let billing = app.get::<Billing>().await;
    // A billable without an email: Paddle's checkout collects it.
    let anonymous = BillableRef {
        email: None,
        ..agra()
    };
    let checkout = billing.checkout(&anonymous, "pro").create().await.unwrap();
    assert_eq!(checkout.url, "https://acme.test/pay?_ptxn=txn_01j8checkout");
    assert!(billing.customer(&anonymous).await.unwrap().is_none());

    let client = app.client();
    for event in [
        pfx::event(
            "evt_01j8completed",
            "transaction.completed",
            pfx::transaction("completed", "0"),
        ),
        pfx::event(
            "evt_01j8created",
            "subscription.created",
            pfx::subscription("trialing", "pri_pro_m", 1),
        ),
    ] {
        let body = event.to_string();
        client
            .post("/api/billing/webhook")
            .header(
                PADDLE_SIGNATURE_HEADER,
                sign_paddle_webhook("pdl_ntfset_app", NOW, body.as_bytes()).as_str(),
            )
            .body(body)
            .send()
            .await
            .assert_status(200);
    }
    app.get::<Queue>().await.run_pending().await.unwrap();
    let customer = billing.customer(&anonymous).await.unwrap().unwrap();
    assert_eq!(customer.provider.as_deref(), Some("paddle"));
    assert_eq!(
        customer.provider_customer_id.as_deref(),
        Some("ctm_01j8agra")
    );
    let sub = billing.subscription(&anonymous).await.unwrap().unwrap();
    assert!(sub.on_trial() && sub.on_plan("pro"));

    let canceled = billing.cancel(&anonymous).await.unwrap();
    assert!(canceled.on_grace_period());
}

#[tokio::test]
async fn a_misconfigured_real_provider_fails_the_boot_naming_the_key() {
    let server = MockServer::start().await;
    let error = app(&server)
        .config("billing.provider", "stripe")
        .config("billing.stripe.webhook_secret", "whsec_app")
        .try_build()
        .await
        .map(|_| ())
        .expect_err("a missing secret key fails the boot")
        .to_string();
    assert!(error.contains("billing.stripe.secret_key"), "{error}");
    assert!(!error.contains("whsec_app"), "{error}");
}
