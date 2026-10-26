//! The provider conformance suite: the same operations and expectations on
//! the fake, Stripe and Paddle, so the fake agrees with the real drivers.

use std::any::Any;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use http::{HeaderMap, HeaderName, HeaderValue};
use navree_billing::{
    BillableRef, Billing, BillingModule, BillingProvider, PADDLE_SIGNATURE_HEADER, PaddleProvider,
    STRIPE_SIGNATURE_HEADER, StripeProvider, SubscriptionStatus, WebhookAction, WebhookError,
    sign_paddle_webhook, sign_stripe_webhook,
};
use navree_contracts::{Clock, FixedClock, Secret};
use navree_db::DatabaseModule;
use navree_testing::TestApp;
use serde_json::json;
use wiremock::matchers::{body_json, body_string, body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::fixtures::{NOW, TRIAL_END, WORKSPACE, paddle as pfx, stripe as sfx};
use crate::support::{change, checkout_request, client};

/// One provider under test, with what the suite needs to drive it.
struct Case {
    name: &'static str,
    provider: Arc<dyn BillingProvider>,
    customer: String,
    subscription: String,
    base_item: Option<String>,
    base_price: String,
    other_price: String,
    secret: String,
    sign: fn(&str, i64, &[u8]) -> String,
    header: &'static str,
    /// A raw "subscription created" event: trialing, the base price, two
    /// seats, the fixture workspace, the trial ending at `TRIAL_END`.
    created_event: Vec<u8>,
    _keep: Box<dyn Any>,
}

fn at(secs: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(secs, 0).unwrap()
}

async fn fake_case() -> Case {
    let app = TestApp::builder()
        .temp_database()
        .config("billing.provider", "fake")
        .config("billing.routes", false)
        .config("billing.return_url", "https://app.acme.test/settings/billing")
        .config(
            "billing.plans",
            json!({
                "pro": { "prices": { "monthly": "price_pro_m" }, "meters": { "ai_summaries": "price_ai" } },
                "business": { "prices": { "monthly": "price_business_m" } }
            }),
        )
        .module(DatabaseModule::from_config())
        .module(BillingModule::from_config())
        .override_value::<dyn Clock>(Arc::new(FixedClock::new(at(NOW))))
        .build()
        .await;
    let billing = app.get::<Billing>().await;
    let billable = BillableRef {
        billable_type: "workspace".into(),
        billable_id: WORKSPACE,
        name: "Acme".into(),
        email: Some("ada@example.com".into()),
    };
    let checkout = billing
        .checkout(&billable, "pro")
        .quantity(2)
        .trial_days(0)
        .create()
        .await
        .unwrap();
    let fake = billing.fake().unwrap();
    fake.complete_checkout(&checkout.id).await.unwrap();
    let sub = billing.subscription(&billable).await.unwrap().unwrap();
    Case {
        name: "fake",
        provider: fake.clone(),
        customer: sub.provider_customer_id.unwrap(),
        subscription: sub.provider_id.unwrap(),
        base_item: None,
        base_price: "price_pro_m".into(),
        other_price: "price_business_m".into(),
        secret: fake.webhook_secret().to_owned(),
        sign: sign_stripe_webhook,
        header: STRIPE_SIGNATURE_HEADER,
        created_event: serde_json::to_vec(&sfx::event(
            "evt_created",
            "customer.subscription.created",
            sfx::subscription("trialing", "price_pro_m", 2),
        ))
        .unwrap(),
        _keep: Box::new(app),
    }
}

async fn stripe_case() -> Case {
    let server = MockServer::start().await;
    let sub = "/v1/subscriptions/sub_1Q8XyzAbC";
    let ok = |body| ResponseTemplate::new(200).set_body_json(body);
    for (verb, route, body_match, response) in [
        (
            "POST",
            "/v1/checkout/sessions",
            None,
            sfx::checkout_session(),
        ),
        (
            "POST",
            sub,
            Some("items%5B0%5D%5Bprice%5D=price_business_m"),
            sfx::subscription("active", "price_business_m", 3),
        ),
        (
            "POST",
            sub,
            Some("cancel_at_period_end=true"),
            sfx::canceling("price_pro_m"),
        ),
        (
            "POST",
            sub,
            Some("cancel_at_period_end=false"),
            sfx::subscription("active", "price_pro_m", 2),
        ),
        (
            "POST",
            sub,
            Some("pause_collection%5Bbehavior%5D=void"),
            sfx::paused("price_pro_m"),
        ),
        ("DELETE", sub, None, sfx::canceled("price_pro_m")),
        (
            "GET",
            "/v1/subscriptions",
            None,
            sfx::list(vec![sfx::subscription("active", "price_pro_m", 2)]),
        ),
        (
            "GET",
            "/v1/invoices",
            None,
            sfx::list(vec![sfx::invoice("paid", 5800)]),
        ),
        (
            "POST",
            "/v1/billing_portal/sessions",
            None,
            sfx::portal_session(),
        ),
        ("GET", "/v1/prices/price_pro_m", None, sfx::metered_price()),
        (
            "POST",
            "/v1/webhook_endpoints",
            None,
            sfx::webhook_endpoint(),
        ),
    ] {
        let mut mock = Mock::given(method(verb)).and(path(route));
        if let Some(body) = body_match {
            mock = mock.and(body_string_contains(body));
        }
        mock.respond_with(ok(response)).mount(&server).await;
    }
    Mock::given(method("POST"))
        .and(path(sub))
        .and(body_string("pause_collection="))
        .respond_with(ok(sfx::subscription("active", "price_pro_m", 2)))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/prices/price_nope"))
        .respond_with(ResponseTemplate::new(404).set_body_json(sfx::error("No such price")))
        .mount(&server)
        .await;
    let provider = StripeProvider::new(
        client(),
        Secret::new("sk_test_51Q8".to_owned()),
        Some(Secret::new("whsec_conformance".to_owned())),
        server.uri(),
    );
    Case {
        name: "stripe",
        provider: Arc::new(provider),
        customer: "cus_QX1aBcDeF".into(),
        subscription: "sub_1Q8XyzAbC".into(),
        base_item: Some("si_QX1base".into()),
        base_price: "price_pro_m".into(),
        other_price: "price_business_m".into(),
        secret: "whsec_conformance".into(),
        sign: sign_stripe_webhook,
        header: STRIPE_SIGNATURE_HEADER,
        created_event: serde_json::to_vec(&sfx::event(
            "evt_created",
            "customer.subscription.created",
            sfx::subscription("trialing", "price_pro_m", 2),
        ))
        .unwrap(),
        _keep: Box::new(server),
    }
}

async fn paddle_case() -> Case {
    let server = MockServer::start().await;
    let sub = "/subscriptions/sub_01j8acme";
    let ok = |data| ResponseTemplate::new(200).set_body_json(pfx::envelope(data));
    Mock::given(method("POST"))
        .and(path("/transactions"))
        .respond_with(ok(pfx::checkout_transaction()))
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path(sub))
        .and(body_json(json!({ "scheduled_change": null })))
        .respond_with(ok(pfx::subscription("active", "pri_pro_m", 2)))
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path(sub))
        .respond_with(ok(pfx::subscription("active", "pri_business_m", 3)))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{sub}/cancel")))
        .and(body_json(
            json!({ "effective_from": "next_billing_period" }),
        ))
        .respond_with(ok(pfx::canceling("pri_pro_m")))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{sub}/cancel")))
        .and(body_json(json!({ "effective_from": "immediately" })))
        .respond_with(ok(pfx::canceled("pri_pro_m")))
        .mount(&server)
        .await;
    for (route, response) in [
        (format!("{sub}/pause"), pfx::paused("pri_pro_m")),
        (
            format!("{sub}/resume"),
            pfx::subscription("active", "pri_pro_m", 2),
        ),
        (
            "/customers/ctm_01j8acme/portal-sessions".to_owned(),
            pfx::portal_session(),
        ),
        (
            "/notification-settings".to_owned(),
            pfx::notification_setting(),
        ),
    ] {
        Mock::given(method("POST"))
            .and(path(route))
            .respond_with(ok(response))
            .mount(&server)
            .await;
    }
    for (route, response) in [
        (
            "/subscriptions",
            json!([pfx::subscription("active", "pri_pro_m", 2)]),
        ),
        (
            "/transactions",
            json!([pfx::transaction("completed", "5800")]),
        ),
        ("/prices/pri_pro_m", pfx::price()),
    ] {
        Mock::given(method("GET"))
            .and(path(route))
            .respond_with(ok(response))
            .mount(&server)
            .await;
    }
    Mock::given(method("GET"))
        .and(path("/prices/pri_nope"))
        .respond_with(
            ResponseTemplate::new(404).set_body_json(pfx::error("Entity pri_nope not found")),
        )
        .mount(&server)
        .await;
    let provider = PaddleProvider::new(
        client(),
        Secret::new("pdl_sdbx_apikey_01j8".to_owned()),
        Some(Secret::new("pdl_ntfset_conformance".to_owned())),
        server.uri(),
    );
    Case {
        name: "paddle",
        provider: Arc::new(provider),
        customer: "ctm_01j8acme".into(),
        subscription: "sub_01j8acme".into(),
        base_item: None,
        base_price: "pri_pro_m".into(),
        other_price: "pri_business_m".into(),
        secret: "pdl_ntfset_conformance".into(),
        sign: sign_paddle_webhook,
        header: PADDLE_SIGNATURE_HEADER,
        created_event: serde_json::to_vec(&pfx::event(
            "evt_created",
            "subscription.created",
            pfx::subscription("trialing", "pri_pro_m", 2),
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

async fn conformance(case: Case) {
    let name = case.name;
    let provider = &case.provider;
    let id = case.subscription.as_str();

    let checkout = provider
        .create_checkout(&checkout_request(Some(&case.customer), &case.base_price))
        .await
        .unwrap();
    assert!(!checkout.id.is_empty(), "{name}");
    assert!(
        checkout.url.starts_with("https://"),
        "{name}: {}",
        checkout.url
    );

    let changed = provider
        .update_subscription(id, &change(case.base_item.as_deref(), &case.other_price, 3))
        .await
        .unwrap();
    assert_eq!(changed.id, case.subscription, "{name}");
    assert_eq!(changed.customer_id, case.customer, "{name}");
    assert_eq!(changed.items[0].price_id, case.other_price, "{name}");
    assert_eq!(changed.items[0].quantity, Some(3), "{name}");

    let canceling = provider.cancel_subscription(id, true).await.unwrap();
    assert!(canceling.ends_at.is_some(), "{name}: cancel at period end");
    assert_ne!(canceling.status, SubscriptionStatus::Canceled, "{name}");
    let resumed = provider.resume_subscription(id).await.unwrap();
    assert_eq!(resumed.ends_at, None, "{name}: resumed");
    let paused = provider.pause_subscription(id).await.unwrap();
    assert_eq!(paused.status, SubscriptionStatus::Paused, "{name}");
    let unpaused = provider.unpause_subscription(id).await.unwrap();
    assert_eq!(unpaused.status, SubscriptionStatus::Active, "{name}");
    let canceled = provider.cancel_subscription(id, false).await.unwrap();
    assert_eq!(canceled.status, SubscriptionStatus::Canceled, "{name}");
    assert!(canceled.ends_at.is_some(), "{name}: ended");

    let subs = provider.subscriptions(&case.customer).await.unwrap();
    assert!(subs.iter().any(|sub| sub.id == case.subscription), "{name}");
    let invoices = provider.invoices(&case.customer).await.unwrap();
    assert!(!invoices.is_empty(), "{name}: invoices");
    assert!(
        invoices.iter().all(|invoice| !invoice.id.is_empty()),
        "{name}"
    );
    let portal = provider
        .portal_url(
            &case.customer,
            "https://app.acme.test/settings/billing",
            std::slice::from_ref(&case.subscription),
        )
        .await
        .unwrap();
    assert!(portal.starts_with("https://"), "{name}: {portal}");
    assert!(
        provider.price_exists(&case.base_price).await.unwrap(),
        "{name}"
    );
    let missing = if name == "paddle" {
        "pri_nope"
    } else {
        "price_nope"
    };
    assert!(!provider.price_exists(missing).await.unwrap(), "{name}");
    let endpoint = provider
        .create_webhook_endpoint("https://api.acme.test/api/billing/webhook")
        .await
        .unwrap();
    assert!(
        endpoint.secret.is_some() && !endpoint.events.is_empty(),
        "{name}"
    );

    // Webhooks: signed by the provider's scheme, parsed to the same values.
    let tolerance = Duration::from_secs(300);
    let body = &case.created_event;
    provider
        .verify_webhook(&signed(&case, body), body, at(NOW), tolerance)
        .unwrap();
    assert_eq!(
        provider.verify_webhook(&signed(&case, b"{}"), body, at(NOW), tolerance),
        Err(WebhookError::SignatureMismatch),
        "{name}"
    );
    assert_eq!(
        provider.verify_webhook(&signed(&case, body), body, at(NOW + 301), tolerance),
        Err(WebhookError::Expired),
        "{name}"
    );
    let event = provider.parse_webhook(body).unwrap();
    assert_eq!(event.id, "evt_created", "{name}");
    assert_eq!(event.occurred_at, at(NOW), "{name}");
    let [WebhookAction::Subscription(sub)] = event.actions.as_slice() else {
        panic!("{name}: {:?}", event.actions);
    };
    assert_eq!(sub.status, SubscriptionStatus::Trialing, "{name}");
    assert_eq!(sub.trial_ends_at, Some(at(TRIAL_END)), "{name}");
    assert_eq!(sub.current_period_start, Some(at(NOW)), "{name}");
    assert_eq!(sub.current_period_end, Some(at(TRIAL_END)), "{name}");
    assert_eq!(sub.items[0].price_id, case.base_price, "{name}");
    assert_eq!(sub.items[0].quantity, Some(2), "{name}");
    assert_eq!(sub.ends_at, None, "{name}");
    assert_eq!(
        sub.billable(),
        Some(("workspace".to_owned(), WORKSPACE)),
        "{name}"
    );
    assert_eq!(
        sub.metadata.get("plan").map(String::as_str),
        Some("pro"),
        "{name}"
    );
}

#[tokio::test]
async fn the_fake_conforms() {
    conformance(fake_case().await).await;
}

#[tokio::test]
async fn stripe_conforms() {
    conformance(stripe_case().await).await;
}

#[tokio::test]
async fn paddle_conforms() {
    conformance(paddle_case().await).await;
}
