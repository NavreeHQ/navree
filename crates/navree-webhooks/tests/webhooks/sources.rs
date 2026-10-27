//! The built-in sources through `VerifiedWebhook<S>`: AWS SNS and Google
//! Pub/Sub with keys served by a fake HTTP client, the Microsoft Graph and
//! Slack handshakes, Twilio's URL signing, Mailgun and Postmark.

use std::sync::Arc;

use http::Method;
use navree_contracts::{Clock, FixedClock};
use navree_crypto::jose::{JwkSet, SigningKey};
use navree_http::{AppError, Controller, Json, ModuleBuilderHttpExt, RouteDef};
use navree_http_client::{FakeResponse, FakeServer, HttpClient};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_testing::{TestApp, TestAppBuilder};
use navree_webhooks::{
    AwsSns, GooglePubSub, GraphNotifications, Mailgun, MicrosoftGraph, Postmark, PubSubPush,
    Slack, SnsMessage, Twilio, VerifiedWebhook, WebhookSource, WebhooksModule,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::support::start;

const TOPIC: &str = "arn:aws:sns:us-east-1:123456789012:ses-events";
const CERT_URL: &str = "https://sns.us-east-1.amazonaws.com/SimpleNotificationService-9c6465fa7f48f5cacd23014631ec1136.pem";
const PUSH_ACCOUNT: &str = "gmail-push@navree-test.iam.gserviceaccount.com";
const PUSH_URL: &str = "https://api.example.com/api/hooks/pubsub";
const GRAPH_STATE: &str = "graph-client-state-7f10";
const TWILIO_TOKEN: &str = "twilio-auth-token";
const SLACK_SECRET: &str = "8f742231b10e8888abcd99yyyzzz85a5";
const MAILGUN_KEY: &str = "key-7f2e5b1c9d8a4e3f6b0a1c2d3e4f5a6b";
const POSTMARK_CREDENTIALS: &str = "postmark:s3cr3t-pa55";

/// A fixture of `tests/fixtures/sources`.
fn fixture(name: &str) -> String {
    let path = format!(
        "{}/tests/fixtures/sources/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn sns_message(name: &str) -> Vec<u8> {
    let messages: Value = serde_json::from_str(&fixture("sns-messages.json")).unwrap();
    serde_json::to_vec(&messages[name]).unwrap()
}

fn now() -> i64 {
    start().timestamp()
}

/// What the routes answer: what the handler saw.
fn seen<S: WebhookSource>(hook: &VerifiedWebhook<S>, extra: Value) -> Json<Value> {
    Json(json!({
        "source": hook.source(),
        "id": hook.id(),
        "event": hook.event_type(),
        "timestamp": hook.timestamp().map(|t| t.timestamp()),
        "extra": extra,
    }))
}

async fn sns(hook: VerifiedWebhook<AwsSns>) -> Result<Json<Value>, AppError> {
    let message: SnsMessage = hook.json()?;
    let event: Value = message.message_json().unwrap_or(Value::Null);
    Ok(seen(&hook, json!({ "eventType": event["eventType"] })))
}

async fn pubsub(hook: VerifiedWebhook<GooglePubSub>) -> Result<Json<Value>, AppError> {
    let push: PubSubPush = hook.json()?;
    let gmail: Value = push.message.data_json().unwrap_or(Value::Null);
    Ok(seen(&hook, gmail))
}

async fn graph(hook: VerifiedWebhook<MicrosoftGraph>) -> Result<Json<Value>, AppError> {
    let batch: GraphNotifications = hook.json()?;
    Ok(seen(&hook, json!({ "resources": batch.value.len() })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct SmsStatus {
    message_sid: String,
    message_status: String,
}

async fn twilio(hook: VerifiedWebhook<Twilio>) -> Result<Json<Value>, AppError> {
    let status: SmsStatus = hook.form()?;
    Ok(seen(
        &hook,
        json!({ "sid": status.message_sid, "status": status.message_status }),
    ))
}

async fn slack(hook: VerifiedWebhook<Slack>) -> Result<Json<Value>, AppError> {
    Ok(seen(&hook, Value::Null))
}

async fn mailgun(hook: VerifiedWebhook<Mailgun>) -> Result<Json<Value>, AppError> {
    Ok(seen(&hook, Value::Null))
}

async fn postmark(hook: VerifiedWebhook<Postmark>) -> Result<Json<Value>, AppError> {
    Ok(seen(&hook, Value::Null))
}

struct SourcesController;

impl Injectable for SourcesController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(SourcesController) })
    }
}

impl Controller for SourcesController {
    fn prefix(&self) -> &'static str {
        "/hooks"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::POST, "/sns", sns).public(),
            RouteDef::on(Method::POST, "/pubsub", pubsub).public(),
            RouteDef::on(Method::POST, "/graph", graph).public(),
            RouteDef::on(Method::POST, "/twilio", twilio).public(),
            RouteDef::on(Method::POST, "/slack", slack).public(),
            RouteDef::on(Method::POST, "/mailgun", mailgun).public(),
            RouteDef::on(Method::POST, "/postmark", postmark).public(),
        ]
    }
}

struct SourcesModule;

impl Module for SourcesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<SourcesController>();
    }
}

/// The Google key that signs the test tokens.
fn google_key() -> SigningKey {
    SigningKey::from_pem(&fixture("signer-key.pem"))
        .unwrap()
        .with_kid("google-test-1")
}

/// A fake client that serves the SNS certificate, confirms subscriptions
/// and serves Google's keys.
fn fake_senders() -> (HttpClient, FakeServer) {
    let (client, server) = HttpClient::fake();
    server.on(
        Method::GET,
        CERT_URL,
        FakeResponse::new(200).text(fixture("signer-cert.pem")),
    );
    server.on(
        Method::GET,
        "https://sns.us-east-1.amazonaws.com/?Action=ConfirmSubscription*",
        FakeResponse::new(200),
    );
    let keys: JwkSet = google_key().public_jwk().into_iter().collect();
    server.on(
        Method::GET,
        GooglePubSub::KEYS_URL,
        FakeResponse::new(200).json(&keys),
    );
    (client, server)
}

/// No database: verifying needs only WebhooksModule (and the HTTP client
/// for the sources that fetch keys).
fn sources_app(client: HttpClient) -> TestAppBuilder {
    TestApp::builder()
        .config("webhooks.routes", false)
        .config("webhooks.incoming.aws_sns.topics", json!([TOPIC]))
        .config(
            "webhooks.incoming.google_pubsub",
            json!({ "service_account": PUSH_ACCOUNT, "url": PUSH_URL }),
        )
        .config("webhooks.incoming.microsoft_graph.secret", GRAPH_STATE)
        .config("webhooks.incoming.twilio.secret", TWILIO_TOKEN)
        .config("webhooks.incoming.slack.secret", SLACK_SECRET)
        .config("webhooks.incoming.mailgun.secret", MAILGUN_KEY)
        .config("webhooks.incoming.postmark.secret", POSTMARK_CREDENTIALS)
        .module(WebhooksModule::from_config())
        .module(SourcesModule)
        .override_value(Arc::new(client))
        .override_value::<dyn Clock>(Arc::new(FixedClock::new(start())))
}

#[tokio::test]
async fn sns_notifications_reach_the_handler_and_confirmations_do_not() {
    let (client, server) = fake_senders();
    let app = sources_app(client).build().await;
    for name in ["notification_v1", "notification_v2"] {
        app.client()
            .post("/api/hooks/sns")
            .header("x-amz-sns-message-type", "Notification")
            .header("content-type", "text/plain; charset=UTF-8")
            .body(sns_message(name))
            .send()
            .await
            .assert_status(200)
            .assert_json_path("source", json!("aws_sns"))
            .assert_json_path("id", json!("da41e39f-ea4d-435a-b922-c6aae3915ebe"))
            .assert_json_path("event", json!("Notification"))
            .assert_json_path("extra.eventType", json!("Bounce"));
    }

    let confirmed = app
        .client()
        .post("/api/hooks/sns")
        .header("x-amz-sns-message-type", "SubscriptionConfirmation")
        .body(sns_message("subscription_confirmation_v1"))
        .send()
        .await
        .assert_status(200);
    assert!(confirmed.text().is_empty(), "{}", confirmed.text());
    server.assert_sent(|request| request.url().as_str().contains("Action=ConfirmSubscription"));

    // A tampered message never reaches the handler.
    let tampered = String::from_utf8(sns_message("notification_v1"))
        .unwrap()
        .replace("Permanent", "Transient");
    app.client()
        .post("/api/hooks/sns")
        .body(tampered)
        .send()
        .await
        .assert_error(400, "Invalid webhook signature");
}

#[tokio::test]
async fn an_unreachable_certificate_is_a_503_the_sender_retries() {
    let (client, server) = HttpClient::fake();
    server.on(Method::GET, CERT_URL, FakeResponse::new(500));
    let app = sources_app(client).build().await;
    app.client()
        .post("/api/hooks/sns")
        .body(sns_message("notification_v2"))
        .send()
        .await
        .assert_status(503);
}

#[tokio::test]
async fn sources_without_their_required_setting_are_a_500() {
    let app = TestApp::builder()
        .config("webhooks.routes", false)
        .config("webhooks.incoming.postmark.secret", "no-colon")
        .module(WebhooksModule::from_config())
        .module(SourcesModule)
        .build()
        .await;
    // No `topics`, no `service_account`, a malformed Postmark secret.
    app.client()
        .post("/api/hooks/sns")
        .body(sns_message("notification_v2"))
        .send()
        .await
        .assert_error(500, None);
    app.client()
        .post("/api/hooks/pubsub")
        .body("{}")
        .send()
        .await
        .assert_error(500, None);
    app.client()
        .post("/api/hooks/postmark")
        .header("authorization", Postmark::authorization("no-colon"))
        .body("{}")
        .send()
        .await
        .assert_error(500, None);
}

fn push_body() -> Vec<u8> {
    use base64::Engine as _;
    let data = base64::engine::general_purpose::STANDARD
        .encode(br#"{"emailAddress":"ada@example.com","historyId":"9876543210"}"#);
    serde_json::to_vec(&json!({
        "message": {
            "data": data,
            "messageId": "2070443601311540",
            "message_id": "2070443601311540",
            "publishTime": "2026-10-26T07:59:58.000Z",
        },
        "subscription": "projects/navree-test/subscriptions/gmail-push",
    }))
    .unwrap()
}

fn push_token(audience: &str, email: &str) -> String {
    google_key()
        .sign_jwt(&json!({
            "aud": audience,
            "email": email,
            "email_verified": true,
            "exp": now() + 3600,
            "iat": now() - 2,
            "iss": "https://accounts.google.com",
            "sub": "113774264463038321964",
        }))
        .unwrap()
}

#[tokio::test]
async fn pubsub_pushes_are_verified_by_their_oidc_token() {
    let (client, server) = fake_senders();
    let app = sources_app(client).build().await;
    let send = |token: String| {
        app.client()
            .post("/api/hooks/pubsub")
            .bearer(&token)
            .json(&serde_json::from_slice::<Value>(&push_body()).unwrap())
            .send()
    };
    send(push_token(PUSH_URL, PUSH_ACCOUNT))
        .await
        .assert_status(200)
        .assert_json_path("id", json!("2070443601311540"))
        .assert_json_path("timestamp", json!(now() - 2))
        .assert_json_path("extra.historyId", json!("9876543210"));
    send(push_token(PUSH_URL, "someone@else.iam.gserviceaccount.com"))
        .await
        .assert_error(400, "Invalid webhook signature");
    send(push_token("https://api.example.com/other", PUSH_ACCOUNT))
        .await
        .assert_error(400, "Invalid webhook signature");
    app.client()
        .post("/api/hooks/pubsub")
        .json(&json!({}))
        .send()
        .await
        .assert_error(400, "Invalid webhook signature");
    // Google's keys were fetched once.
    server.assert_sent_count(1);
}

#[tokio::test]
async fn graph_validation_tokens_are_echoed_and_notifications_checked() {
    let (client, _server) = fake_senders();
    let app = sources_app(client).build().await;
    let validation = app
        .client()
        .post("/api/hooks/graph?validationToken=Validation%3A%20Testing%20client%20application%20reachability")
        .header("content-type", "text/plain")
        .send()
        .await
        .assert_status(200);
    assert_eq!(
        validation.text(),
        "Validation: Testing client application reachability"
    );
    assert!(
        validation
            .header("content-type")
            .is_some_and(|t| t.starts_with("text/plain")),
        "{:?}",
        validation.header("content-type")
    );

    let batch = |state: &str| {
        json!({ "value": [{
            "subscriptionId": "7f105c7d-2dc5-4530-97cd-4e7ae6534c07",
            "clientState": state,
            "changeType": "created",
            "resource": "Users/7a6e/Messages/AAMkAD",
            "tenantId": "84bd8158-6d4d-4958-8b9f-9d6445542f95"
        }]})
    };
    app.client()
        .post("/api/hooks/graph")
        .json(&batch(GRAPH_STATE))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("event", json!("created"))
        .assert_json_path("extra.resources", json!(1));
    app.client()
        .post("/api/hooks/graph")
        .json(&batch("guessed"))
        .send()
        .await
        .assert_error(400, "Invalid webhook signature");
}

#[tokio::test]
async fn twilio_signs_the_public_url_and_the_form() {
    let (client, _server) = fake_senders();
    let app = sources_app(client).build().await;
    let params = [
        ("MessageSid", "SM1f0e8ae6ade43cb3c0ce4525424e404f"),
        ("MessageStatus", "delivered"),
        ("To", "+15558675310"),
    ];
    let body = serde_urlencoded::to_string(params).unwrap();
    // No `url` configured: rebuilt from the proxy headers.
    let url = "https://api.example.com/api/hooks/twilio?tenant=7";
    let send = |signature: String, proto: &'static str| {
        app.client()
            .post("/api/hooks/twilio?tenant=7")
            .header("host", "api.example.com")
            .header("x-forwarded-proto", proto)
            .header("content-type", "application/x-www-form-urlencoded")
            .header("x-twilio-signature", signature)
            .header("i-twilio-idempotency-token", "idem-1")
            .body(body.clone())
            .send()
    };
    send(Twilio::sign(TWILIO_TOKEN, url, &params), "https")
        .await
        .assert_status(200)
        .assert_json_path("id", json!("idem-1"))
        .assert_json_path("extra.status", json!("delivered"));
    // Signed for https, received as http: another URL.
    send(Twilio::sign(TWILIO_TOKEN, url, &params), "http")
        .await
        .assert_error(400, "Invalid webhook signature");
}

#[tokio::test]
async fn twilio_uses_the_configured_url_behind_a_proxy() {
    let (client, _server) = fake_senders();
    let app = sources_app(client)
        .config(
            "webhooks.incoming.twilio.url",
            "https://hooks.example.com/twilio",
        )
        .build()
        .await;
    let params = [("MessageSid", "SM1"), ("MessageStatus", "sent")];
    app.client()
        .post("/api/hooks/twilio")
        .header("host", "internal:3000")
        .header(
            "x-twilio-signature",
            Twilio::sign(TWILIO_TOKEN, "https://hooks.example.com/twilio", &params),
        )
        .body(serde_urlencoded::to_string(params).unwrap())
        .send()
        .await
        .assert_status(200)
        .assert_json_path("extra.status", json!("sent"));
}

#[tokio::test]
async fn slack_challenges_are_answered_and_events_verified() {
    let (client, _server) = fake_senders();
    let app = sources_app(client).build().await;
    let send = |body: Value, timestamp: i64| {
        let bytes = serde_json::to_vec(&body).unwrap();
        app.client()
            .post("/api/hooks/slack")
            .header("x-slack-request-timestamp", timestamp.to_string())
            .header("x-slack-signature", Slack::sign(SLACK_SECRET, timestamp, &bytes))
            .body(bytes)
            .send()
    };
    let challenge = send(
        json!({ "type": "url_verification", "challenge": "3eZbrw1aBm2rZgRNFdxV", "token": "x" }),
        now(),
    )
    .await
    .assert_status(200);
    assert_eq!(challenge.text(), "3eZbrw1aBm2rZgRNFdxV");

    let event = json!({
        "type": "event_callback",
        "event_id": "Ev0PV52K21",
        "event": { "type": "app_mention", "text": "<@U0LAN0Z89> is it everything a river should be?" }
    });
    send(event.clone(), now() - 10)
        .await
        .assert_status(200)
        .assert_json_path("id", json!("Ev0PV52K21"))
        .assert_json_path("event", json!("app_mention"));
    send(event, now() - 301)
        .await
        .assert_error(400, "Invalid webhook signature");
}

#[tokio::test]
async fn mailgun_and_postmark_events_are_verified() {
    let (client, _server) = fake_senders();
    let app = sources_app(client).build().await;
    let token = "a8ce0edb2dd8301dee6c2405235584e45aa91d1e9f979f3de0";
    let event = |signature: String| {
        json!({
            "signature": { "timestamp": now().to_string(), "token": token, "signature": signature },
            "event-data": { "id": "CPgfbmQMTCKtHW6uIWtuVe", "event": "failed", "severity": "permanent" }
        })
    };
    app.client()
        .post("/api/hooks/mailgun")
        .json(&event(Mailgun::sign(MAILGUN_KEY, now(), token)))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("id", json!("CPgfbmQMTCKtHW6uIWtuVe"))
        .assert_json_path("event", json!("failed"))
        .assert_json_path("timestamp", json!(now()));
    app.client()
        .post("/api/hooks/mailgun")
        .json(&event(Mailgun::sign("key-other", now(), token)))
        .send()
        .await
        .assert_error(400, "Invalid webhook signature");

    let bounce = json!({
        "RecordType": "Bounce",
        "MessageID": "883953f4-6105-42a2-a16a-77a8eac79483",
        "Type": "HardBounce",
        "Email": "ada@example.com"
    });
    app.client()
        .post("/api/hooks/postmark")
        .header("authorization", Postmark::authorization(POSTMARK_CREDENTIALS))
        .json(&bounce)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("id", json!("883953f4-6105-42a2-a16a-77a8eac79483"))
        .assert_json_path("event", json!("Bounce"));
    app.client()
        .post("/api/hooks/postmark")
        .header("authorization", Postmark::authorization("postmark:guess"))
        .json(&bounce)
        .send()
        .await
        .assert_error(400, "Invalid webhook signature");
}

#[tokio::test]
async fn keyless_sources_boot_without_a_secret_and_read_topics_from_the_environment() {
    let app = TestApp::builder()
        .sources(navree_config::ConfigSources::new().env_vars(
            "APP_",
            [
                (
                    "APP_WEBHOOKS__INCOMING__AWS_SNS__TOPICS",
                    "arn:aws:sns:us-east-1:1:a,arn:aws:sns:eu-west-1:1:b",
                ),
                (
                    "APP_WEBHOOKS__INCOMING__GOOGLE_PUBSUB__SERVICE_ACCOUNT",
                    PUSH_ACCOUNT,
                ),
            ],
        ))
        .module(WebhooksModule::from_config())
        .build()
        .await;
    let options = app.get::<navree_webhooks::WebhooksOptions>().await;
    assert_eq!(
        options.incoming.source("aws_sns").unwrap().topics,
        ["arn:aws:sns:us-east-1:1:a", "arn:aws:sns:eu-west-1:1:b"]
    );
    assert_eq!(
        options
            .incoming
            .source("google_pubsub")
            .unwrap()
            .service_account
            .as_deref(),
        Some(PUSH_ACCOUNT)
    );
}
