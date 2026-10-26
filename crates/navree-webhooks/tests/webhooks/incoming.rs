//! Incoming webhooks: `VerifiedWebhook<S>` with the built-in sources, a
//! Standard Webhooks source of the app's own and a configured HMAC source.

use std::sync::Arc;

use http::Method;
use navree_contracts::{Clock, FixedClock};
use navree_db::{DatabaseModule, Uow};
use navree_http::{AppError, Controller, Json, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_testing::{TestApp, TestAppBuilder};
use navree_webhooks::{
    GitHub, Hmac, Paddle, SourceOptions, StandardWebhooks, Stripe, VerifiedWebhook, WebhookSource,
    WebhooksModule,
};
use serde_json::{Value, json};

use crate::support::start;

/// Delivery events from a mail provider (Standard Webhooks).
struct MailEvents;

impl WebhookSource for MailEvents {
    const NAME: &'static str = "mail_events";
    type Verifier = StandardWebhooks;
}

/// Calendly's `t=..,v1=..` header (a configured HMAC source).
struct Calendly;

impl WebhookSource for Calendly {
    const NAME: &'static str = "calendly";
    type Verifier = Hmac;
}

/// A source the tests never configure.
struct Unconfigured;

impl WebhookSource for Unconfigured {
    const NAME: &'static str = "unconfigured";
    type Verifier = StandardWebhooks;
}

/// What every test route answers: what the extractor saw.
fn seen<S: WebhookSource>(hook: &VerifiedWebhook<S>, first: bool) -> Result<Json<Value>, AppError> {
    let body: Value = hook.json()?;
    Ok(Json(json!({
        "source": hook.source(),
        "first": first,
        "id": hook.id(),
        "event": hook.event_type(),
        "timestamp": hook.timestamp().map(|t| t.timestamp()),
        "body": body,
    })))
}

async fn github(uow: Uow, hook: VerifiedWebhook<GitHub>) -> Result<Json<Value>, AppError> {
    let first = hook.mark_processed(&uow).await?;
    seen(&hook, first)
}

async fn stripe(uow: Uow, hook: VerifiedWebhook<Stripe>) -> Result<Json<Value>, AppError> {
    let first = hook.mark_processed(&uow).await?;
    seen(&hook, first)
}

async fn paddle(uow: Uow, hook: VerifiedWebhook<Paddle>) -> Result<Json<Value>, AppError> {
    let first = hook.mark_processed(&uow).await?;
    seen(&hook, first)
}

async fn mail(uow: Uow, hook: VerifiedWebhook<MailEvents>) -> Result<Json<Value>, AppError> {
    let first = hook.mark_processed(&uow).await?;
    if hook.event_type() == Some("explode") {
        // The handler fails after marking: the receipt rolls back with it.
        return Err(AppError::internal("boom"));
    }
    seen(&hook, first)
}

async fn calendly(hook: VerifiedWebhook<Calendly>) -> Result<Json<Value>, AppError> {
    seen(&hook, true)
}

async fn unconfigured(hook: VerifiedWebhook<Unconfigured>) -> Result<Json<Value>, AppError> {
    seen(&hook, true)
}

struct HooksController;

impl Injectable for HooksController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(HooksController) })
    }
}

impl Controller for HooksController {
    fn prefix(&self) -> &'static str {
        "/hooks"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::POST, "/github", github).public(),
            RouteDef::on(Method::POST, "/stripe", stripe).public(),
            RouteDef::on(Method::POST, "/paddle", paddle).public(),
            RouteDef::on(Method::POST, "/mail", mail).public(),
            RouteDef::on(Method::POST, "/calendly", calendly).public(),
            RouteDef::on(Method::POST, "/unconfigured", unconfigured).public(),
        ]
    }
}

struct HooksModule;

impl Module for HooksModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<HooksController>();
    }
}

const MAIL_SECRET: &str = "whsec_MfKQ9r8GKYqrTwjUPD8ILPZIo2LaLaSw";

fn incoming_app() -> TestAppBuilder {
    incoming_app_with(Arc::new(FixedClock::new(start())))
}

fn incoming_app_with(clock: Arc<FixedClock>) -> TestAppBuilder {
    TestApp::builder()
        .temp_database()
        .config("webhooks.routes", false)
        .config("webhooks.incoming.max_body_bytes", 2048)
        .config("webhooks.incoming.github.secret", "test-secret")
        .config("webhooks.incoming.stripe.secret", "whsec_stripe")
        .config("webhooks.incoming.paddle.secret", "pdl_secret")
        .config("webhooks.incoming.mail_events.secret", MAIL_SECRET)
        .config("webhooks.incoming.mail_events.tolerance_secs", 60)
        .config(
            "webhooks.incoming.calendly",
            json!({
                "secret": "calendly-key",
                "header": "Calendly-Webhook-Signature",
                "algorithm": "sha256",
                "encoding": "hex",
                "format": "t={timestamp},v1={signature}",
                "signed": "{timestamp}.{body}"
            }),
        )
        .module(DatabaseModule::from_config())
        .module(WebhooksModule::from_config())
        .module(HooksModule)
        .override_value::<dyn Clock>(clock)
}

fn now() -> i64 {
    start().timestamp()
}

#[tokio::test]
async fn github_webhooks_are_verified_and_marked_once() {
    let app = incoming_app().build().await;
    let body = serde_json::to_vec(&json!({
        "action": "opened",
        "issue": { "number": 7, "title": "Export fails" },
    }))
    .unwrap();
    let send = |signature: String| {
        app.client()
            .post("/api/hooks/github")
            .header("X-GitHub-Event", "issues")
            .header("X-GitHub-Delivery", "72d3162e-cc78-11e3-81ab-4c9367dc0958")
            .header("X-Hub-Signature-256", signature)
            .header("Content-Type", "application/json")
            .body(body.clone())
            .send()
    };

    send(GitHub::sign("test-secret", &body))
        .await
        .assert_status(200)
        .assert_json_path("first", json!(true))
        .assert_json_path("id", json!("72d3162e-cc78-11e3-81ab-4c9367dc0958"))
        .assert_json_path("event", json!("issues"))
        .assert_json_path("timestamp", Value::Null)
        .assert_json_path("body.issue.number", json!(7));
    send(GitHub::sign("test-secret", &body))
        .await
        .assert_status(200)
        .assert_json_path("first", json!(false));
    send(GitHub::sign("wrong-secret", &body))
        .await
        .assert_error(400, "Invalid webhook signature");
    app.client()
        .post("/api/hooks/github")
        .body(body.clone())
        .send()
        .await
        .assert_error(400, "Invalid webhook signature");
}

#[tokio::test]
async fn stripe_and_paddle_webhooks_check_their_timestamps() {
    let app = incoming_app().build().await;
    let body = br#"{"id":"evt_1","type":"invoice.paid"}"#.to_vec();

    app.client()
        .post("/api/hooks/stripe")
        .header(
            "Stripe-Signature",
            Stripe::sign("whsec_stripe", now() - 10, &body),
        )
        .body(body.clone())
        .send()
        .await
        .assert_status(200)
        .assert_json_path("id", json!("evt_1"))
        .assert_json_path("event", json!("invoice.paid"))
        .assert_json_path("timestamp", json!(now() - 10));
    app.client()
        .post("/api/hooks/stripe")
        .header(
            "Stripe-Signature",
            Stripe::sign("whsec_stripe", now() - 301, &body),
        )
        .body(body.clone())
        .send()
        .await
        .assert_error(400, "Invalid webhook signature");

    let paddle = br#"{"event_id":"evt_01","event_type":"subscription.created"}"#.to_vec();
    app.client()
        .post("/api/hooks/paddle")
        .header(
            "Paddle-Signature",
            Paddle::sign("pdl_secret", now(), &paddle),
        )
        .body(paddle.clone())
        .send()
        .await
        .assert_status(200)
        .assert_json_path("id", json!("evt_01"))
        .assert_json_path("event", json!("subscription.created"));
    app.client()
        .post("/api/hooks/paddle")
        .header(
            "Paddle-Signature",
            Paddle::sign("pdl_secret", now() + 600, &paddle),
        )
        .body(paddle)
        .send()
        .await
        .assert_error(400, "Invalid webhook signature");
}

#[tokio::test]
async fn an_app_source_uses_its_own_secret_and_tolerance() {
    let app = incoming_app().build().await;
    let body = br#"{"type":"email.bounced","data":{"to":"ada@example.com"}}"#.to_vec();
    let send = |id: &'static str, timestamp: i64, body: Vec<u8>| {
        let signature = StandardWebhooks::sign(MAIL_SECRET, id, timestamp, &body);
        app.client()
            .post("/api/hooks/mail")
            .header("webhook-id", id)
            .header("webhook-timestamp", timestamp.to_string())
            .header("webhook-signature", signature)
            .body(body)
            .send()
    };

    send("msg_1", now() - 30, body.clone())
        .await
        .assert_status(200)
        .assert_json_path("source", json!("mail_events"))
        .assert_json_path("id", json!("msg_1"))
        .assert_json_path("event", json!("email.bounced"))
        .assert_json_path("first", json!(true));
    // The source's own tolerance (60 s), not the default 300 s.
    send("msg_2", now() - 61, body.clone())
        .await
        .assert_error(400, "Invalid webhook signature");

    // A handler that fails rolls its receipt back: the redelivery is new.
    send("msg_3", now(), br#"{"type":"explode"}"#.to_vec())
        .await
        .assert_status(500);
    send("msg_3", now(), br#"{"type":"again"}"#.to_vec())
        .await
        .assert_status(200)
        .assert_json_path("first", json!(true));
    send("msg_3", now(), br#"{"type":"again"}"#.to_vec())
        .await
        .assert_status(200)
        .assert_json_path("first", json!(false));
    // A tampered body fails.
    let signature = StandardWebhooks::sign(MAIL_SECRET, "msg_4", now(), &body);
    app.client()
        .post("/api/hooks/mail")
        .header("webhook-id", "msg_4")
        .header("webhook-timestamp", now().to_string())
        .header("webhook-signature", signature)
        .body(br#"{"type":"email.delivered"}"#.to_vec())
        .send()
        .await
        .assert_error(400, "Invalid webhook signature");
}

#[tokio::test]
async fn configured_hmac_sources_verify_with_their_options() {
    let app = incoming_app().build().await;
    let source = SourceOptions {
        header: Some("Calendly-Webhook-Signature".to_owned()),
        format: Some("t={timestamp},v1={signature}".to_owned()),
        signed: Some("{timestamp}.{body}".to_owned()),
        ..SourceOptions::new("calendly-key")
    };
    let body = br#"{"event":"invitee.created"}"#.to_vec();
    let id_of_body = {
        use sha2::Digest;
        hex::encode(sha2::Sha256::digest(&body))
    };

    app.client()
        .post("/api/hooks/calendly")
        .header(
            "Calendly-Webhook-Signature",
            Hmac::sign(&source, Some(now()), None, &body).unwrap(),
        )
        .body(body.clone())
        .send()
        .await
        .assert_status(200)
        .assert_json_path("timestamp", json!(now()))
        .assert_json_path("id", json!(id_of_body));
    app.client()
        .post("/api/hooks/calendly")
        .header("Calendly-Webhook-Signature", format!("t={},v1=00", now()))
        .body(body)
        .send()
        .await
        .assert_error(400, "Invalid webhook signature");
}

#[tokio::test]
async fn receipts_are_pruned_after_receipt_days() {
    let clock = Arc::new(FixedClock::new(start()));
    let app = incoming_app_with(clock.clone()).build().await;
    let body = br#"{"action":"opened"}"#.to_vec();
    let send = || {
        app.client()
            .post("/api/hooks/github")
            .header("X-GitHub-Delivery", "delivery-1")
            .header("X-Hub-Signature-256", GitHub::sign("test-secret", &body))
            .body(body.clone())
            .send()
    };
    send().await.assert_json_path("first", json!(true));

    clock.advance(std::time::Duration::from_secs(29 * 86_400));
    app.run_command("webhooks:prune", &[]).await.unwrap();
    send().await.assert_json_path("first", json!(false));

    clock.advance(std::time::Duration::from_secs(2 * 86_400));
    app.run_command("webhooks:prune", &[]).await.unwrap();
    send().await.assert_json_path("first", json!(true));
}

#[tokio::test]
async fn bodies_over_max_body_bytes_are_refused() {
    let app = incoming_app().build().await;
    let body = vec![b'x'; 4096];
    app.client()
        .post("/api/hooks/github")
        .header("X-Hub-Signature-256", GitHub::sign("test-secret", &body))
        .body(body)
        .send()
        .await
        .assert_status(413);
}

#[tokio::test]
async fn a_source_without_a_secret_is_a_server_error() {
    let app = incoming_app().build().await;
    app.client()
        .post("/api/hooks/unconfigured")
        .body("{}")
        .send()
        .await
        .assert_error(500, None);
}

#[tokio::test]
async fn a_configured_source_with_an_empty_secret_fails_the_boot() {
    let error = incoming_app()
        .config("webhooks.incoming.github.secret", "")
        .try_build()
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("webhooks.incoming.github.secret"), "{error}");
}

#[tokio::test]
async fn secrets_come_from_the_environment() {
    let app = TestApp::builder()
        .sources(navree_config::ConfigSources::new().env_vars(
            "APP_",
            [
                ("APP_WEBHOOKS__INCOMING__GITHUB__SECRET", "from-env"),
                ("APP_WEBHOOKS__INCOMING__TOLERANCE_SECS", "120"),
            ],
        ))
        // Verifying needs no database, queue or access module: with the
        // defaults (routes on) this app still boots.
        .module(WebhooksModule::from_config())
        .build()
        .await;
    let options = app.get::<navree_webhooks::WebhooksOptions>().await;
    assert_eq!(
        options.incoming.source("github").unwrap().secret,
        "from-env"
    );
    assert_eq!(options.incoming.tolerance_secs, 120);
}
