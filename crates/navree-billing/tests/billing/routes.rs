//! The `/api/billing` routes, the webhook route and the `Subscribed`
//! extractor.

use std::sync::Arc;

use http::Method;
use navree_billing::{STRIPE_SIGNATURE_HEADER, Subscribed, sign_stripe_webhook};
use navree_http::{Controller, Json, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use serde_json::{Value, json};

use crate::support::{UserBilling, WorkspaceBilling, World, builder, platform, start};

/// `GET /api/premium`: needs a subscription.
struct PremiumController;

impl Injectable for PremiumController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(PremiumController) })
    }
}

impl Controller for PremiumController {
    fn prefix(&self) -> &'static str {
        "/premium"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let show = |subscription: Subscribed| async move {
            Json(json!({
                "plan": subscription.plan,
                "on_trial": subscription.on_trial(),
                "status": subscription.status,
            }))
        };
        vec![RouteDef::on(Method::GET, "/", show).authenticated()]
    }
}

struct PremiumModule;

impl Module for PremiumModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<PremiumController>();
    }
}

#[tokio::test]
async fn the_overview_and_the_plans() {
    let world = World::new().await;
    let ada = world.user("Ada", "ada@example.com").await;
    world.workspace(ada.id, "Acme").await;
    let client = world.as_user(ada.id);

    let overview = client
        .get("/api/billing")
        .header("X-Workspace", "acme")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("subscription", Value::Null)
        .assert_json_path("trial.on_trial", json!(false))
        .assert_json_path("usage", json!({}))
        .json_value();
    let plans = overview["plans"].as_array().unwrap();
    assert_eq!(plans.len(), 3);
    assert_eq!(plans[1]["key"], "pro");
    assert_eq!(plans[1]["intervals"], json!(["monthly", "yearly"]));
    assert_eq!(plans[1]["meters"], json!(["ai_summaries"]));
    assert_eq!(plans[1]["prices"]["yearly"], "price_pro_y");

    client
        .get("/api/billing/plans")
        .header("X-Workspace", "acme")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.2.key", json!("starter"))
        .assert_json_path("data.2.intervals", json!(["monthly"]));
}

#[tokio::test]
async fn checkout_swap_cancel_resume_invoices_and_portal() {
    let world = World::new().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let acme = world.workspace(ada.id, "Acme").await;
    let client = world.as_user(ada.id);
    let post =
        |path: &str, body: Value| client.post(path).header("X-Workspace", "acme").json(&body);

    let url = post("/api/billing/checkout", json!({ "plan": "pro" }))
        .send()
        .await
        .assert_status(200)
        .json_value()["url"]
        .as_str()
        .unwrap()
        .to_owned();
    let fake = world.fake().await;
    let checkout = fake.assert_checkout(|c| c.url == url);
    assert_eq!(
        checkout.customer_email.as_deref(),
        Some("ada@example.com"),
        "prefilled with the signed-in user's email"
    );
    post("/api/billing/checkout", json!({ "plan": "gold" }))
        .send()
        .await
        .assert_validation_error("plan");
    post(
        "/api/billing/checkout",
        json!({ "plan": "starter", "interval": "yearly" }),
    )
    .send()
    .await
    .assert_validation_error("interval");

    fake.complete_checkout(&checkout.id).await.unwrap();
    world.run_queue().await;
    post("/api/billing/checkout", json!({ "plan": "pro" }))
        .send()
        .await
        .assert_error(
            409,
            "Already subscribed; change the plan with swap instead.",
        );

    client
        .get("/api/billing")
        .header("X-Workspace", "acme")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("subscription.__type", json!("Subscription"))
        .assert_json_path("subscription.plan", json!("pro"))
        .assert_json_path("subscription.on_trial", json!(true))
        .assert_json_path("trial.on_trial", json!(true))
        .assert_json_path("usage.ai_summaries", json!(0));

    post(
        "/api/billing/swap",
        json!({ "plan": "business", "interval": "yearly" }),
    )
    .send()
    .await
    .assert_status(200)
    .assert_json_path("plan", json!("business"))
    .assert_json_path("interval", json!("yearly"));
    post("/api/billing/cancel", json!({}))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("canceled", json!(true))
        .assert_json_path("on_grace_period", json!(true))
        .assert_json_path("valid", json!(true));
    post("/api/billing/resume", json!({}))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("canceled", json!(false));

    client
        .get("/api/billing/invoices")
        .header("X-Workspace", "acme")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.0.status", json!("paid"))
        .assert_json_path("data.0.total_cents", json!(0));

    let portal = post("/api/billing/portal", json!({}))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert!(
        portal["url"]
            .as_str()
            .unwrap()
            .ends_with("return_url=https://app.acme.test/settings/billing")
    );
    post(
        "/api/billing/portal",
        json!({ "return_url": "https://app.acme.test/elsewhere" }),
    )
    .send()
    .await
    .assert_status(200);
    post(
        "/api/billing/portal",
        json!({ "return_url": "https://evil.test/phish" }),
    )
    .send()
    .await
    .assert_validation_error("return_url");
    let _ = acme;
}

#[tokio::test]
async fn the_routes_need_billing_manage_in_the_workspace() {
    let world = World::new().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let grace = world.user("Grace", "grace@example.com").await;
    let alan = world.user("Alan", "alan@example.com").await;
    let acme = world.workspace(ada.id, "Acme").await;
    world.join(acme.id, grace.id, "member").await;
    world.join(acme.id, alan.id, "admin").await;

    world
        .as_user(grace.id)
        .get("/api/billing")
        .header("X-Workspace", "acme")
        .send()
        .await
        .assert_error(403, None);
    world
        .as_user(alan.id)
        .get("/api/billing")
        .header("X-Workspace", "acme")
        .send()
        .await
        .assert_status(200);
    world
        .as_user(ada.id)
        .get("/api/billing")
        .send()
        .await
        .assert_error(400, "The request names no workspace.");
    world
        .app
        .client()
        .get("/api/billing")
        .header("X-Workspace", "acme")
        .send()
        .await
        .assert_error(401, None);
}

#[tokio::test]
async fn the_webhook_route_is_public_and_verified() {
    let world = World::new().await;
    let fake = world.fake().await;
    let body = json!({
        "id": "evt_route", "object": "event", "type": "customer.created",
        "created": start().timestamp(), "data": { "object": { "id": "cus_x" } }
    })
    .to_string();
    let signature =
        sign_stripe_webhook(fake.webhook_secret(), start().timestamp(), body.as_bytes());
    let client = world.app.client();
    client
        .post("/api/billing/webhook")
        .header(STRIPE_SIGNATURE_HEADER, signature.as_str())
        .header("content-type", "application/json")
        .body(body.clone())
        .send()
        .await
        .assert_status(200)
        .assert_json_path("received", json!(true))
        .assert_json_path("duplicate", json!(false));
    client
        .post("/api/billing/webhook")
        .header(STRIPE_SIGNATURE_HEADER, signature.as_str())
        .body(body.clone())
        .send()
        .await
        .assert_status(200)
        .assert_json_path("duplicate", json!(true));
    let forged = sign_stripe_webhook("whsec_forged", start().timestamp(), body.as_bytes());
    client
        .post("/api/billing/webhook")
        .header(STRIPE_SIGNATURE_HEADER, forged.as_str())
        .body(body.clone())
        .send()
        .await
        .assert_error(400, "Invalid webhook signature.");
    client
        .post("/api/billing/webhook")
        .body(body)
        .send()
        .await
        .assert_error(400, "Invalid webhook signature.");
}

#[tokio::test]
async fn subscribed_answers_402_without_a_subscription() {
    let world = World::with(builder().module(PremiumModule)).await;
    let ada = world.user("Ada", "ada@example.com").await;
    let acme = world.workspace(ada.id, "Acme").await;
    let premium = || {
        world
            .as_user(ada.id)
            .get("/api/premium")
            .header("X-Workspace", "acme")
    };
    premium()
        .send()
        .await
        .assert_error(402, "This workspace needs an active subscription");

    world.billing().await.start_trial(&acme, 3).await.unwrap();
    premium()
        .send()
        .await
        .assert_status(200)
        .assert_json_path("plan", json!("business"))
        .assert_json_path("on_trial", json!(true));

    world.advance_days(3);
    premium().send().await.assert_error(402, None);
    world.subscribe(&acme, "starter").await;
    premium()
        .send()
        .await
        .assert_status(200)
        .assert_json_path("plan", json!("starter"))
        .assert_json_path("status", json!("active"));
}

#[tokio::test]
async fn users_can_be_the_route_billable() {
    let world = World::with(platform().module(UserBilling).module(PremiumModule)).await;
    let ada = world.user("Ada", "ada@example.com").await;
    let client = world.as_user(ada.id);
    client
        .get("/api/premium")
        .send()
        .await
        .assert_error(402, "This user needs an active subscription");
    let url = client
        .post("/api/billing/checkout")
        .json(&json!({ "plan": "starter" }))
        .send()
        .await
        .assert_status(200)
        .json_value()["url"]
        .as_str()
        .unwrap()
        .to_owned();
    let fake = world.fake().await;
    let checkout = fake.assert_checkout(|c| c.url == url);
    assert_eq!(checkout.billable_type, "user");
    assert_eq!(checkout.billable_id, ada.id);
    fake.complete_checkout(&checkout.id).await.unwrap();
    world.run_queue().await;
    client.get("/api/premium").send().await.assert_status(200);
    client
        .get("/api/billing")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("subscription.plan", json!("starter"));
}

#[tokio::test]
async fn routes_can_be_turned_off_and_two_billables_need_a_choice() {
    let world = World::with(builder().config("billing.routes", false)).await;
    let ada = world.user("Ada", "ada@example.com").await;
    world.workspace(ada.id, "Acme").await;
    world
        .as_user(ada.id)
        .get("/api/billing")
        .header("X-Workspace", "acme")
        .send()
        .await
        .assert_status(404);
    let fake = world.fake().await;
    let body = json!({ "id": "evt_x", "type": "x", "created": start().timestamp(), "data": { "object": {} } })
        .to_string();
    world
        .app
        .client()
        .post("/api/billing/webhook")
        .header(
            STRIPE_SIGNATURE_HEADER,
            sign_stripe_webhook(fake.webhook_secret(), start().timestamp(), body.as_bytes())
                .as_str(),
        )
        .body(body)
        .send()
        .await
        .assert_status(200);

    let world = World::with(builder().config("billing.webhook_route", false)).await;
    world
        .app
        .client()
        .post("/api/billing/webhook")
        .body("{}")
        .send()
        .await
        .assert_status(404);

    let error = platform()
        .module(WorkspaceBilling)
        .module(UserBilling)
        .try_build()
        .await
        .map(|_| ())
        .expect_err("two billables without billing.routes_billable fail the boot")
        .to_string();
    assert!(error.contains("billing.routes_billable"), "{error}");
    platform()
        .module(WorkspaceBilling)
        .module(UserBilling)
        .config("billing.routes_billable", "user")
        .try_build()
        .await
        .unwrap();

    let error = platform()
        .try_build()
        .await
        .map(|_| ())
        .expect_err("routes without a billable fail the boot")
        .to_string();
    assert!(error.contains("for_feature"), "{error}");
    platform()
        .config("billing.routes", false)
        .try_build()
        .await
        .unwrap();
}
