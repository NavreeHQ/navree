//! The mail routes: tracking, provider webhooks and the admin routes.

use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, Utc};
use http::Method;
use navree_contracts::{Clock, FixedClock};
use navree_db::PageQuery;
use navree_http_client::{FakeResponse, FakeServer, HttpClient};
use navree_mail::{
    Deliveries, DeliveryFilter, DeliveryStatus, MANAGE_PERMISSION, Message, Outbox, Suppressions,
    TemplateMail,
};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::harness::{MailApp, mail_app, principal};

fn promo(to: &str) -> Message {
    Message::new().to(to).subject("Spring offer").html(
        "<html><body><p><a href=\"https://acme.test/promo?a=1&amp;b=2\">See the offer</a></p>\
             <p><a href=\"mailto:help@acme.test\">Help</a></p></body></html>",
    )
}

/// The first `attribute="..."` value in `html` (unescaped).
fn attribute(html: &str, attribute: &str) -> String {
    let start = html.find(&format!("{attribute}=\"")).expect("attribute") + attribute.len() + 2;
    let end = start + html[start..].find('"').expect("closing quote");
    html[start..end].replace("&amp;", "&")
}

async fn tracked() -> MailApp {
    mail_app(|b| {
        b.config("mail.tracking.opens", true)
            .config("mail.tracking.clicks", true)
            .config("mail.tracking.base_url", "https://links.acme.test/")
    })
    .await
}

// ---------------------------------------------------------------------
// Tracking
// ---------------------------------------------------------------------

#[tokio::test]
async fn clicks_go_through_a_signed_redirect() {
    let mail = tracked().await;
    let report = mail
        .mailer()
        .await
        .deliver(&promo("anu@example.com"))
        .await
        .unwrap();
    let id = report.delivery_id.unwrap();
    let html = mail.fake.sent()[0].html.clone().unwrap();

    let link = attribute(&html, "href");
    assert!(
        link.starts_with(&format!(
            "https://links.acme.test/mail/t/c/{}?u=https%3A%2F%2Fagra.test%2Fpromo%3Fa%3D1%26b%3D2&expires=",
            id.simple()
        )),
        "{link}"
    );
    assert!(
        html.contains("href=\"mailto:help@acme.test\""),
        "other links stay"
    );
    let path = link.strip_prefix("https://links.acme.test").unwrap();

    let response = mail.app.client().get(path).send().await;

    response.assert_status(302);
    let delivery = mail.get::<Deliveries>().await.find(id).await.unwrap();
    assert!(delivery.clicked_at.is_some());
    assert!(delivery.opened_at.is_some());
    let events = mail.get::<Deliveries>().await.events(id).await.unwrap();
    assert_eq!(events[0].provider, "tracking");
    assert_eq!(
        events[0].url.as_deref(),
        Some("https://acme.test/promo?a=1&b=2")
    );
}

#[tokio::test]
async fn the_redirect_goes_to_the_original_link() {
    let mail = tracked().await;
    mail.mailer()
        .await
        .send_message(promo("anu@example.com"))
        .await
        .unwrap();
    let html = mail.fake.sent()[0].html.clone().unwrap();
    let path = attribute(&html, "href").replace("https://links.acme.test", "");

    let response = mail.app.client().get(&path).send().await;

    assert_eq!(
        response.header("location"),
        Some("https://acme.test/promo?a=1&b=2")
    );
}

#[tokio::test]
async fn the_open_image_records_an_open() {
    let mail = tracked().await;
    let report = mail
        .mailer()
        .await
        .deliver(&promo("anu@example.com"))
        .await
        .unwrap();
    let html = mail.fake.sent()[0].html.clone().unwrap();
    let image = &html[html.find("<img").unwrap()..];
    assert!(
        html.find("<img").unwrap() < html.find("</body>").unwrap(),
        "{html}"
    );
    let path = attribute(image, "src").replace("https://links.acme.test", "");

    let response = mail.app.client().get(&path).send().await;

    response.assert_status(200);
    let delivery = mail
        .get::<Deliveries>()
        .await
        .find(report.delivery_id.unwrap())
        .await
        .unwrap();
    assert!(delivery.opened_at.is_some());
    assert_eq!(delivery.clicked_at, None);
}

#[tokio::test]
async fn tampered_tracking_links_are_not_found() {
    let mail = tracked().await;
    mail.mailer()
        .await
        .send_message(promo("anu@example.com"))
        .await
        .unwrap();
    let html = mail.fake.sent()[0].html.clone().unwrap();
    let path = attribute(&html, "href").replace("https://links.acme.test", "");

    let other_target = path.replace("acme.test%2Fpromo", "evil.test%2Fpromo");
    mail.app
        .client()
        .get(&other_target)
        .send()
        .await
        .assert_status(404);
    // Any other hex digit: the last one may already be a 0.
    let last = if path.ends_with('0') { '1' } else { '0' };
    let bad_signature = format!("{}{last}", &path[..path.len() - 1]);
    mail.app
        .client()
        .get(&bad_signature)
        .send()
        .await
        .assert_status(404);
    let unsigned = path[..path.find("&expires").unwrap()].to_owned();
    mail.app
        .client()
        .get(&unsigned)
        .send()
        .await
        .assert_status(404);
}

#[tokio::test]
async fn tracking_needs_a_base_url() {
    let Err(error) = navree_testing::TestApp::builder()
        .temp_database()
        .config("mail.delivery.enabled", true)
        .config("mail.tracking.opens", true)
        .module(navree_db::DatabaseModule::from_config())
        .module(crate::support::view_module())
        .module(navree_mail::MailModule::from_config())
        .try_build()
        .await
    else {
        panic!("the boot should fail");
    };
    assert!(error.to_string().contains("base_url"), "{error}");
}

#[tokio::test]
async fn tracking_needs_the_delivery_log() {
    let Err(error) = navree_testing::TestApp::builder()
        .config("mail.tracking.clicks", true)
        .module(crate::support::view_module())
        .module(navree_mail::MailModule::from_config())
        .try_build()
        .await
    else {
        panic!("the boot should fail");
    };
    assert!(error.to_string().contains("delivery.enabled"), "{error}");
}

// ---------------------------------------------------------------------
// Provider webhooks
// ---------------------------------------------------------------------

async fn sent_to_ada(mail: &MailApp) -> Uuid {
    mail.mailer()
        .await
        .deliver(
            &TemplateMail::new("anu@example.com", "Welcome", "mail/welcome")
                .context(json!({ "name": "Anu" })),
        )
        .await
        .unwrap()
        .delivery_id
        .unwrap()
}

async fn status(mail: &MailApp, id: Uuid) -> DeliveryStatus {
    mail.get::<Deliveries>()
        .await
        .find(id)
        .await
        .unwrap()
        .status
}

fn mailgun_body(key: &str, timestamp: i64) -> Value {
    let token = "6c0b5ab7f42d";
    let signature = hex::encode(navree_crypto::hmac_sha256(
        key.as_bytes(),
        format!("{timestamp}{token}").as_bytes(),
    ));
    json!({
        "signature": { "timestamp": timestamp.to_string(), "token": token, "signature": signature },
        "event-data": {
            "event": "failed",
            "severity": "permanent",
            "recipient": "anu@example.com",
            "timestamp": 1_793_088_000.5,
            "message": { "headers": { "message-id": "fake-1" } },
            "delivery-status": { "description": "No such user" },
        },
    })
}

#[tokio::test]
async fn mailgun_events_are_verified_and_recorded() {
    let mail = mail_app(|b| b.config("mail.webhooks.mailgun.signing_key", "key-test")).await;
    let id = sent_to_ada(&mail).await;
    let client = mail.app.client();

    client
        .post("/api/mail/webhooks/mailgun")
        .json(&mailgun_body("wrong-key", Utc::now().timestamp()))
        .send()
        .await
        .assert_error_code(401, "invalid_signature");
    client
        .post("/api/mail/webhooks/mailgun")
        .json(&mailgun_body("key-test", Utc::now().timestamp() - 3600))
        .send()
        .await
        .assert_status(401);
    assert_eq!(status(&mail, id).await, DeliveryStatus::Sent);

    client
        .post("/api/mail/webhooks/mailgun")
        .json(&mailgun_body("key-test", Utc::now().timestamp()))
        .send()
        .await
        .assert_status(204);

    assert_eq!(status(&mail, id).await, DeliveryStatus::Bounced);
    let suppressions = mail.get::<Suppressions>().await;
    assert!(
        suppressions
            .is_suppressed(None, "anu@example.com")
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn resend_events_are_verified_with_their_svix_signature() {
    let key = b"a-resend-signing-key";
    let secret = format!("whsec_{}", STANDARD.encode(key));
    let mail = mail_app(|b| b.config("mail.webhooks.resend.secret", secret.as_str())).await;
    let id = sent_to_ada(&mail).await;
    let body = json!({
        "type": "email.complained",
        "created_at": "2026-10-27T10:00:00.000Z",
        "data": { "email_id": "fake-1", "to": ["anu@example.com"] },
    })
    .to_string();
    let timestamp = Utc::now().timestamp().to_string();
    let signature = STANDARD.encode(navree_crypto::hmac_sha256(
        key,
        format!("msg_1.{timestamp}.{body}").as_bytes(),
    ));
    let post = |signature: String| {
        mail.app
            .client()
            .post("/api/mail/webhooks/resend")
            .header("content-type", "application/json")
            .header("svix-id", "msg_1")
            .header("svix-timestamp", timestamp.as_str())
            .header("svix-signature", signature)
            .body(body.clone())
    };

    post("v1,bm90IGl0".to_owned())
        .send()
        .await
        .assert_status(401);
    post(format!("v1,bm90IGl0 v1,{signature}"))
        .send()
        .await
        .assert_status(204);

    assert_eq!(status(&mail, id).await, DeliveryStatus::Complained);
}

#[tokio::test]
async fn postmark_events_need_the_token() {
    let mail = mail_app(|b| b.config("mail.webhooks.postmark.token", "pm-token")).await;
    let id = sent_to_ada(&mail).await;
    let body = json!({
        "RecordType": "Delivery",
        "MessageID": "fake-1",
        "Recipient": "anu@example.com",
        "DeliveredAt": "2026-10-27T10:00:00Z",
    });
    let basic =
        |password: &str| format!("Basic {}", STANDARD.encode(format!("postmark:{password}")));

    mail.app
        .client()
        .post("/api/mail/webhooks/postmark")
        .header("authorization", basic("wrong"))
        .json(&body)
        .send()
        .await
        .assert_status(401);
    mail.app
        .client()
        .post("/api/mail/webhooks/postmark")
        .header("authorization", basic("pm-token"))
        .json(&body)
        .send()
        .await
        .assert_status(204);
    assert_eq!(status(&mail, id).await, DeliveryStatus::Delivered);

    mail.app
        .client()
        .post("/api/mail/webhooks/postmark?token=pm-token")
        .json(
            &json!({ "RecordType": "Open", "MessageID": "fake-1", "Recipient": "anu@example.com" }),
        )
        .send()
        .await
        .assert_status(204);
    let delivery = mail.get::<Deliveries>().await.find(id).await.unwrap();
    assert!(delivery.opened_at.is_some());
}

const SES_TOPIC: &str = "arn:aws:sns:eu-west-1:123456789012:ses";
const SNS_CERT_URL: &str = "https://sns.eu-west-1.amazonaws.com/SimpleNotificationService-01d088a6f77103d0fe307c0069e40ed6.pem";

/// A file of `tests/fixtures/sns`: the test signing certificate, and SNS
/// messages signed by OpenSSL with its key (see `generate.py` there).
fn sns_fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/sns/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

/// A signed message of `sns-messages.json`.
fn sns_message(name: &str) -> Value {
    let messages: Value = serde_json::from_str(&sns_fixture("sns-messages.json")).unwrap();
    assert!(messages[name].is_object(), "no SNS message {name}");
    messages[name].clone()
}

/// 2026-10-27 at `time`, UTC (the bounce was published at 10:00:02).
fn on_the_27th(time: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&format!("2026-10-27T{time}Z"))
        .unwrap()
        .with_timezone(&Utc)
}

/// The SES events route on topic `SES_TOPIC`, a fake client that serves
/// the test certificate as `SNS_CERT_URL` (with `certificate`'s status)
/// and confirms subscriptions, and a clock a minute after the bounce.
async fn ses_app(certificate: u16) -> (MailApp, FakeServer, Arc<FixedClock>) {
    let (client, server) = HttpClient::fake();
    server.on(
        Method::GET,
        SNS_CERT_URL,
        FakeResponse::new(certificate).text(sns_fixture("signer-cert.pem")),
    );
    server.on(
        Method::GET,
        "https://sns.eu-west-1.amazonaws.com/?Action=ConfirmSubscription*",
        FakeResponse::new(200),
    );
    let clock = Arc::new(FixedClock::new(on_the_27th("10:01:02")));
    let app_clock: Arc<dyn Clock> = clock.clone();
    let mail = mail_app(|b| {
        b.config("mail.webhooks.ses.token", "ses-token")
            .config("mail.webhooks.ses.topic_arns", json!([SES_TOPIC]))
            .override_value(Arc::new(client))
            .override_value::<dyn Clock>(app_clock)
    })
    .await;
    (mail, server, clock)
}

/// Posts `message` to the SES route the way SNS does, with `token`.
async fn post_ses(mail: &MailApp, token: &str, message: &Value) -> navree_testing::TestResponse {
    mail.app
        .client()
        .post(&format!("/api/mail/webhooks/ses?token={token}"))
        .header("content-type", "text/plain; charset=UTF-8")
        .header(
            "x-amz-sns-message-type",
            message["Type"].as_str().unwrap_or_default(),
        )
        .body(serde_json::to_vec(message).unwrap())
        .send()
        .await
}

#[tokio::test]
async fn ses_events_arrive_through_signed_sns_messages() {
    let (mail, server, _clock) = ses_app(200).await;
    let id = sent_to_ada(&mail).await;

    post_ses(&mail, "ses-token", &sns_message("ses_bounce_v1"))
        .await
        .assert_status(204);

    assert_eq!(status(&mail, id).await, DeliveryStatus::Bounced);
    let events = mail.get::<Deliveries>().await.events(id).await.unwrap();
    assert_eq!(
        events[0].detail.as_deref(),
        Some("smtp; 550 5.1.1 user unknown")
    );
    // The certificate was fetched from the SNS host.
    server.assert_sent(|request| request.url().as_str() == SNS_CERT_URL);
}

#[tokio::test]
async fn ses_confirmations_of_both_signature_versions_are_checked() {
    let (mail, server, _clock) = ses_app(200).await;

    // Version 1 (SHA-1): the subscription is confirmed at its SNS URL.
    post_ses(
        &mail,
        "ses-token",
        &sns_message("ses_subscription_confirmation_v1"),
    )
    .await
    .assert_status(204);
    server.assert_sent(|request| {
        request
            .url()
            .as_str()
            .starts_with("https://sns.eu-west-1.amazonaws.com/?Action=ConfirmSubscription")
    });
    // Version 2 (SHA-256): unsubscribing is acknowledged.
    post_ses(
        &mail,
        "ses-token",
        &sns_message("ses_unsubscribe_confirmation_v2"),
    )
    .await
    .assert_status(204);

    // A forged confirmation never makes the app fetch its URL.
    let mut forged = sns_message("ses_subscription_confirmation_v1");
    forged["SubscribeURL"] =
        json!("https://sns.eu-west-1.amazonaws.com/?Action=ConfirmSubscription&TopicArn=forged");
    post_ses(&mail, "ses-token", &forged)
        .await
        .assert_error_code(401, "invalid_signature");
    server.assert_not_sent(|request| request.url().as_str().contains("forged"));
}

#[tokio::test]
async fn ses_messages_that_cannot_be_trusted_record_nothing() {
    let (mail, server, clock) = ses_app(200).await;
    let id = sent_to_ada(&mail).await;
    let bounce = sns_message("ses_bounce_v2");

    // The token is still checked, on top of the signature.
    post_ses(&mail, "wrong", &bounce)
        .await
        .assert_error_code(401, "invalid_signature");
    // An unsigned envelope, with the right token and topic.
    post_ses(
        &mail,
        "ses-token",
        &json!({
            "Type": "Notification",
            "MessageId": "b6a0d6b0",
            "TopicArn": SES_TOPIC,
            "Message": bounce["Message"],
        }),
    )
    .await
    .assert_error_code(401, "invalid_signature");
    // Tampered with: the bounce is no longer what SNS signed.
    let mut tampered = bounce.clone();
    tampered["Message"] = json!(
        bounce["Message"]
            .as_str()
            .unwrap()
            .replace("anu@example.com", "gauri@example.com")
    );
    post_ses(&mail, "ses-token", &tampered)
        .await
        .assert_error_code(401, "invalid_signature");
    // Signed by another key.
    post_ses(&mail, "ses-token", &sns_message("ses_bounce_other_key_v2"))
        .await
        .assert_error_code(401, "invalid_signature");
    // A certificate on a host that is not SNS.
    let mut foreign = bounce.clone();
    foreign["SigningCertURL"] = json!("https://sns.eu-west-1.amazonaws.com.evil.example/cert.pem");
    post_ses(&mail, "ses-token", &foreign)
        .await
        .assert_error_code(401, "invalid_signature");
    server.assert_not_sent(|request| request.url().as_str().contains("evil.example"));
    // A signature version SNS does not use.
    let mut version = bounce.clone();
    version["SignatureVersion"] = json!("3");
    post_ses(&mail, "ses-token", &version)
        .await
        .assert_error_code(401, "invalid_signature");
    // Another topic.
    let mut topic = bounce.clone();
    topic["TopicArn"] = json!("arn:aws:sns:eu-west-1:123456789012:other");
    post_ses(&mail, "ses-token", &topic)
        .await
        .assert_error_code(401, "invalid_signature");
    // Stale: published more than an hour ago.
    clock.set(on_the_27th("11:00:03"));
    post_ses(&mail, "ses-token", &bounce)
        .await
        .assert_error_code(401, "invalid_signature");

    let events = mail.get::<Deliveries>().await.events(id).await.unwrap();
    assert!(events.is_empty(), "{events:?}");
    assert_ne!(status(&mail, id).await, DeliveryStatus::Bounced);

    // Within the hour, the same message is recorded.
    clock.set(on_the_27th("10:30:00"));
    post_ses(&mail, "ses-token", &bounce)
        .await
        .assert_status(204);
    assert_eq!(status(&mail, id).await, DeliveryStatus::Bounced);
}

#[tokio::test]
async fn an_unreachable_signing_certificate_is_a_503_that_sns_retries() {
    let (mail, _server, _clock) = ses_app(503).await;
    let id = sent_to_ada(&mail).await;

    post_ses(&mail, "ses-token", &sns_message("ses_bounce_v1"))
        .await
        .assert_status(503);
    assert_ne!(status(&mail, id).await, DeliveryStatus::Bounced);
}

#[tokio::test]
async fn webhook_routes_mount_only_with_their_secret() {
    let mail = mail_app(|b| b).await;
    for provider in ["ses", "mailgun", "postmark", "resend"] {
        mail.app
            .client()
            .post(&format!("/api/mail/webhooks/{provider}"))
            .json(&json!({}))
            .send()
            .await
            .assert_status(404);
    }
}

// ---------------------------------------------------------------------
// Admin routes
// ---------------------------------------------------------------------

async fn admin() -> (MailApp, Uuid) {
    let mail = mail_app(|b| b).await;
    let user = Uuid::now_v7();
    mail.authorizer.grant(user, MANAGE_PERMISSION);
    (mail, user)
}

#[tokio::test]
async fn the_delivery_log_is_listed_and_shown() {
    let (mail, user) = admin().await;
    let id = sent_to_ada(&mail).await;
    mail.fake
        .bounce(mail.app.ctx(), "anu@example.com")
        .await
        .unwrap();
    let client = mail.app.client().acting_as(principal(user));

    let page = client
        .get("/api/mail/deliveries?status=bounced")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(page["data"][0]["id"], json!(id));
    assert_eq!(page["data"][0]["status"], "bounced");
    assert_eq!(page["data"][0]["template"], "mail/welcome");

    let shown = client
        .get(&format!("/api/mail/deliveries/{id}"))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(shown["provider_id"], "fake-1");
    assert_eq!(shown["events"][0]["kind"], "bounced");
    assert_eq!(shown["events"][0]["bounce_type"], "hard");

    client
        .get(&format!("/api/mail/deliveries/{}", Uuid::now_v7()))
        .send()
        .await
        .assert_status(404);
    client
        .get("/api/mail/deliveries?status=lost")
        .send()
        .await
        .assert_status(400);
}

#[tokio::test]
async fn suppressions_are_managed() {
    let (mail, user) = admin().await;
    let client = mail.app.client().acting_as(principal(user));
    let workspace = Uuid::now_v7();

    let created = client
        .post("/api/mail/suppressions")
        .json(&json!({ "email": "Eve <EVE@example.com>", "workspace_id": workspace, "detail": "asked by phone" }))
        .send()
        .await
        .assert_status(201)
        .json_value();
    assert_eq!(created["email"], "eve@example.com");
    assert_eq!(created["reason"], "manual");
    client
        .post("/api/mail/suppressions")
        .json(&json!({ "email": "not an address" }))
        .send()
        .await
        .assert_status(400);

    let page = client
        .get(&format!("/api/mail/suppressions?workspace_id={workspace}"))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(page["data"].as_array().unwrap().len(), 1);

    let id = created["id"].as_str().unwrap();
    client
        .delete(&format!("/api/mail/suppressions/{id}"))
        .send()
        .await
        .assert_status(204);
    let suppressions = mail.get::<Suppressions>().await;
    assert!(
        !suppressions
            .is_suppressed(Some(workspace), "eve@example.com")
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn held_mail_is_released_or_discarded_by_an_admin() {
    let (mail, user) = admin().await;
    let mailer = mail.mailer().await;
    for subject in ["First", "Second"] {
        mailer
            .send(&navree_mail::Mailable::hold(
                Message::new()
                    .to("all@acme.test")
                    .subject(subject)
                    .text("Hi"),
                "Needs approval",
            ))
            .await
            .unwrap();
    }
    let client = mail.app.client().acting_as(principal(user));

    let page = client
        .get("/api/mail/outbox")
        .send()
        .await
        .assert_status(200)
        .json_value();
    let held = page["data"].as_array().unwrap().clone();
    assert_eq!(held.len(), 2);
    let (second, first) = (
        held[0]["id"].as_str().unwrap(),
        held[1]["id"].as_str().unwrap(),
    );

    let released = client
        .post(&format!("/api/mail/outbox/{first}/release"))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(released["status"], "sent");
    assert_eq!(released["transport"], "fake");
    client
        .post(&format!("/api/mail/outbox/{first}/release"))
        .send()
        .await
        .assert_status(409);
    let discarded = client
        .post(&format!("/api/mail/outbox/{second}/discard"))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(discarded["status"], "discarded");
    assert_eq!(discarded["decided_by"], json!(user));

    mail.fake.assert_sent(|m| m.subject == "First");
    assert_eq!(mail.fake.sent().len(), 1);
    let all = client
        .get("/api/mail/outbox?status=all")
        .send()
        .await
        .json_value();
    assert_eq!(all["data"].as_array().unwrap().len(), 2);
    let outbox = mail.get::<Outbox>().await;
    assert!(
        outbox
            .list(
                Some(navree_mail::OutboxStatus::Held),
                None,
                PageQuery::new(1, 10)
            )
            .await
            .unwrap()
            .data
            .is_empty()
    );
}

#[tokio::test]
async fn transport_settings_are_checked_before_a_tenant_saves_them() {
    let (mail, user) = admin().await;
    let client = mail.app.client().acting_as(principal(user));

    client
        .post("/api/mail/transports/usable")
        .json(&json!({ "type": "array" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("usable", json!(true))
        .assert_json_path("kind", json!("array"));
    client
        .post("/api/mail/transports/usable")
        .json(&json!({ "type": "smtp", "url": "smtp://u:p@127.0.0.1:2525" }))
        .send()
        .await
        .assert_error_code(422, "mail_transport_unusable");
}

#[tokio::test]
async fn admin_routes_need_the_manage_permission() {
    let (mail, _) = admin().await;
    let stranger = mail.app.client().acting_as(principal(Uuid::now_v7()));

    stranger
        .get("/api/mail/deliveries")
        .send()
        .await
        .assert_status(403);
    stranger
        .get("/api/mail/outbox")
        .send()
        .await
        .assert_status(403);
    mail.app
        .client()
        .get("/api/mail/suppressions")
        .send()
        .await
        .assert_status(401);
}

#[tokio::test]
async fn admin_routes_mount_only_with_the_delivery_pipeline() {
    let mail = mail_app(|b| b.config("mail.delivery.routes", false)).await;
    let user = Uuid::now_v7();
    mail.authorizer.grant(user, MANAGE_PERMISSION);

    mail.app
        .client()
        .acting_as(principal(user))
        .get("/api/mail/deliveries")
        .send()
        .await
        .assert_status(404);
    let page = mail
        .get::<Deliveries>()
        .await
        .list(&DeliveryFilter::default(), PageQuery::new(1, 1))
        .await
        .unwrap();
    assert!(page.data.is_empty());
}
