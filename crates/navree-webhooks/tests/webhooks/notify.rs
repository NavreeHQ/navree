//! The notification callbacks (feature `notify`): Twilio, Vonage and Meta
//! requests verified as incoming webhooks and applied to navree-notify's
//! delivery log and opt-outs.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::Utc;
use navree_contracts::BoxFuture;
use navree_db::DatabaseModule;
use navree_notify::{
    Channel, ChannelError, DeliveryLog, DeliveryStatus, Notifiable, Notification, Notifier,
    NotifyModule, NotifyOptions, OptOuts, SentMessage, SmsDriver, SmsMessage, WhatsAppDriver,
    WhatsAppMessage,
};
use navree_testing::{TestApp, TestAppBuilder};
use navree_webhooks::{Meta, Twilio, Vonage, WebhooksModule};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

const TWILIO_TOKEN: &str = "twilio-auth-token";
const VONAGE_SECRET: &str = "vonage-signature-secret";
const META_SECRET: &str = "meta-app-secret";
const PHONE: &str = "+14155550100";

/// A driver that answers like a provider: ids `{prefix}1`, `{prefix}2`...
struct ProviderLike {
    provider: &'static str,
    prefix: &'static str,
    sent: AtomicU64,
}

impl ProviderLike {
    fn new(provider: &'static str, prefix: &'static str) -> Arc<Self> {
        Arc::new(Self {
            provider,
            prefix,
            sent: AtomicU64::new(0),
        })
    }

    fn next(&self) -> Result<SentMessage, ChannelError> {
        let n = self.sent.fetch_add(1, Ordering::Relaxed) + 1;
        Ok(SentMessage::new(self.provider).id(format!("{}{n}", self.prefix)))
    }
}

impl SmsDriver for ProviderLike {
    fn send<'a>(
        &'a self,
        _to: &'a str,
        _message: &'a SmsMessage,
    ) -> BoxFuture<'a, Result<SentMessage, ChannelError>> {
        let result = self.next();
        Box::pin(async move { result })
    }
}

impl WhatsAppDriver for ProviderLike {
    fn send<'a>(
        &'a self,
        _to: &'a str,
        _message: &'a WhatsAppMessage,
    ) -> BoxFuture<'a, Result<SentMessage, ChannelError>> {
        let result = self.next();
        Box::pin(async move { result })
    }
}

struct Person(Uuid);

impl Notifiable for Person {
    fn notifiable_type(&self) -> &'static str {
        "user"
    }

    fn notifiable_id(&self) -> String {
        self.0.to_string()
    }

    fn route_mail(&self) -> Option<String> {
        None
    }

    fn route_sms(&self) -> Option<String> {
        Some(PHONE.to_owned())
    }

    fn route_whatsapp(&self) -> Option<String> {
        Some(PHONE.to_owned())
    }
}

#[derive(Serialize, Deserialize)]
struct Reminder(Vec<Channel>);

impl Notification for Reminder {
    const TYPE: &'static str = "reminder";

    fn via(&self, _n: &dyn Notifiable) -> Vec<Channel> {
        self.0.clone()
    }

    fn to_sms(&self, _n: &dyn Notifiable) -> Option<SmsMessage> {
        Some(SmsMessage::new("Training due today"))
    }

    fn to_whatsapp(&self, _n: &dyn Notifiable) -> Option<WhatsAppMessage> {
        Some(WhatsAppMessage::text("Training due today"))
    }
}

/// Whether to skip a test that applies provider callbacks to navree-notify's
/// delivery log: with navree-db's own units of work (`NAVREE_TEST_NATIVE_UNITS_OF_WORK=1`)
/// it fails, because navree-notify still takes SeaORM's transaction
/// (`Uow::tx`) until its own port (step 6 of the data-layer port).
fn skip_until_notify_is_ported() -> bool {
    let skip = crate::servers::native_units_of_work();
    if skip {
        eprintln!("skipped: navree-notify is not ported off SeaORM's transaction yet");
    }
    skip
}

/// Webhooks with the three sources and notifications through `sms` and
/// `whatsapp`.
fn app(sms: Arc<dyn SmsDriver>, whatsapp: Arc<dyn WhatsAppDriver>) -> TestAppBuilder {
    crate::servers::database(TestApp::builder())
        .config("webhooks.routes", false)
        .config("webhooks.incoming.twilio.secret", TWILIO_TOKEN)
        .config("webhooks.incoming.vonage.secret", VONAGE_SECRET)
        .config(
            "webhooks.incoming.meta",
            json!({ "secret": META_SECRET, "verify_token": "meta-verify-token" }),
        )
        .module(DatabaseModule::from_config())
        .module(WebhooksModule::from_config())
        .module(NotifyModule::for_root(NotifyOptions {
            routes: false,
            ..NotifyOptions::default()
        }))
        .override_value::<dyn SmsDriver>(sms)
        .override_value::<dyn WhatsAppDriver>(whatsapp)
}

/// Sends one reminder over `channels`; returns the provider's message id.
async fn remind(app: &TestApp, channels: &[Channel]) -> String {
    let id = app
        .get::<Notifier>()
        .await
        .send(&Person(Uuid::now_v7()), Reminder(channels.to_vec()))
        .await
        .unwrap();
    let deliveries = app
        .get::<DeliveryLog>()
        .await
        .for_notification(id)
        .await
        .unwrap();
    deliveries[0].provider_message_id.clone().unwrap()
}

async fn status_of(app: &TestApp, provider: &str, id: &str) -> Option<DeliveryStatus> {
    app.get::<DeliveryLog>()
        .await
        .find_by_message(provider, id)
        .await
        .unwrap()
        .and_then(|d| d.delivery_status())
}

/// Posts Twilio's signed form to the callback route.
async fn post_twilio(
    app: &TestApp,
    params: &[(&str, &str)],
    token: &str,
) -> navree_testing::TestResponse {
    let url = "https://api.example.com/api/webhooks/notify/twilio";
    app.client()
        .post("/api/webhooks/notify/twilio")
        .header("host", "api.example.com")
        .header("x-forwarded-proto", "https")
        .header("content-type", "application/x-www-form-urlencoded")
        .header("x-twilio-signature", Twilio::sign(token, url, params))
        .body(serde_urlencoded::to_string(params).unwrap())
        .send()
        .await
}

#[tokio::test]
async fn twilio_statuses_and_stop_replies_are_applied() {
    if skip_until_notify_is_ported() {
        return;
    }
    let twilio = ProviderLike::new("twilio", "SM");
    let app = app(twilio.clone(), twilio.clone()).build().await;
    let sid = remind(&app, &[Channel::Sms]).await;

    let response = post_twilio(
        &app,
        &[
            ("MessageSid", &sid),
            ("MessageStatus", "delivered"),
            ("To", PHONE),
        ],
        TWILIO_TOKEN,
    )
    .await
    .assert_status(200);
    assert_eq!(response.header("content-type"), Some("text/xml"));
    assert!(response.text().contains("<Response></Response>"));
    assert_eq!(
        status_of(&app, "twilio", &sid).await,
        Some(DeliveryStatus::Delivered)
    );

    post_twilio(
        &app,
        &[
            ("MessageSid", "SMinbound"),
            ("From", PHONE),
            ("To", "+15550001111"),
            ("Body", "STOP"),
        ],
        TWILIO_TOKEN,
    )
    .await
    .assert_status(200);
    let opt_outs = app.get::<OptOuts>().await;
    assert!(opt_outs.is_opted_out(Channel::Sms, PHONE).await.unwrap());

    post_twilio(
        &app,
        &[("MessageSid", &sid), ("MessageStatus", "read")],
        "wrong-token",
    )
    .await
    .assert_error(400, "Invalid webhook signature");
    assert_eq!(
        status_of(&app, "twilio", &sid).await,
        Some(DeliveryStatus::Delivered)
    );
}

#[tokio::test]
async fn vonage_statuses_and_replies_are_applied() {
    if skip_until_notify_is_ported() {
        return;
    }
    let vonage = ProviderLike::new("vonage", "aaaaaaaa-bbbb-cccc-dddd-00000000000");
    let app = app(vonage.clone(), vonage.clone()).build().await;
    let id = remind(&app, &[Channel::Sms]).await;
    let send = |body: serde_json::Value, secret: &'static str| {
        let body = body.to_string();
        let authorization = Vonage::sign(secret, body.as_bytes(), Utc::now().timestamp(), "jti");
        app.client()
            .post("/api/webhooks/notify/vonage")
            .header("authorization", authorization)
            .header("content-type", "application/json")
            .body(body)
            .send()
    };

    send(
        json!({ "message_uuid": id, "to": "14155550100", "from": "Acme", "status": "delivered", "channel": "sms" }),
        VONAGE_SECRET,
    )
    .await
    .assert_status(200);
    assert_eq!(
        status_of(&app, "vonage", &id).await,
        Some(DeliveryStatus::Delivered)
    );

    send(
        json!({ "channel": "sms", "message_uuid": "in-1", "to": "15550001111", "from": "14155550100", "text": "stop", "message_type": "text" }),
        VONAGE_SECRET,
    )
    .await
    .assert_status(200);
    assert!(
        app.get::<OptOuts>()
            .await
            .is_opted_out(Channel::Sms, PHONE)
            .await
            .unwrap()
    );

    send(
        json!({ "message_uuid": id, "status": "read" }),
        "not-the-secret",
    )
    .await
    .assert_error(400, "Invalid webhook signature");
}

#[tokio::test]
async fn meta_answers_the_handshake_and_applies_statuses_and_replies() {
    if skip_until_notify_is_ported() {
        return;
    }
    let meta = ProviderLike::new("meta", "wamid.");
    let app = app(meta.clone(), meta.clone()).build().await;

    let handshake = app
        .client()
        .get("/api/webhooks/notify/meta?hub.mode=subscribe&hub.verify_token=meta-verify-token&hub.challenge=1158201444")
        .send()
        .await
        .assert_status(200);
    assert_eq!(handshake.text(), "1158201444");
    app.client()
        .get("/api/webhooks/notify/meta?hub.mode=subscribe&hub.verify_token=guess&hub.challenge=1")
        .send()
        .await
        .assert_error(400, "Invalid webhook signature");

    let wamid = remind(&app, &[Channel::WhatsApp]).await;
    let body = json!({
        "object": "whatsapp_business_account",
        "entry": [{ "id": "WABA", "changes": [{ "field": "messages", "value": {
            "messaging_product": "whatsapp",
            "metadata": { "display_phone_number": "15550001111", "phone_number_id": "1055" },
            "statuses": [{ "id": wamid, "status": "read", "timestamp": "1700000000", "recipient_id": "14155550100" }],
            "messages": [{ "from": "14155550100", "id": "wamid.in", "timestamp": "1700000000", "type": "text", "text": { "body": "STOP" } }]
        } }] }]
    })
    .to_string();
    app.client()
        .post("/api/webhooks/notify/meta")
        .header(
            "x-hub-signature-256",
            Meta::sign(META_SECRET, body.as_bytes()),
        )
        .header("content-type", "application/json")
        .body(body.clone())
        .send()
        .await
        .assert_status(200);

    assert_eq!(
        status_of(&app, "meta", &wamid).await,
        Some(DeliveryStatus::Read)
    );
    let opt_outs = app.get::<OptOuts>().await;
    assert!(
        opt_outs
            .is_opted_out(Channel::WhatsApp, PHONE)
            .await
            .unwrap()
    );
    assert!(!opt_outs.is_opted_out(Channel::Sms, PHONE).await.unwrap());

    app.client()
        .post("/api/webhooks/notify/meta")
        .header("x-hub-signature-256", Meta::sign("other", body.as_bytes()))
        .body(body)
        .send()
        .await
        .assert_error(400, "Invalid webhook signature");
}

#[tokio::test]
async fn only_configured_sources_get_a_route() {
    let app = crate::servers::database(TestApp::builder())
        .config("webhooks.routes", false)
        .config("webhooks.incoming.twilio.secret", TWILIO_TOKEN)
        .module(DatabaseModule::from_config())
        .module(WebhooksModule::from_config())
        .module(NotifyModule::for_root(NotifyOptions {
            routes: false,
            ..NotifyOptions::default()
        }))
        .build()
        .await;

    app.client()
        .post("/api/webhooks/notify/vonage")
        .body("{}")
        .send()
        .await
        .assert_status(404);
    app.client()
        .get("/api/webhooks/notify/meta")
        .send()
        .await
        .assert_status(404);
    app.client()
        .post("/api/webhooks/notify/twilio")
        .body("")
        .send()
        .await
        .assert_error(400, "Invalid webhook signature");
}

#[tokio::test]
async fn without_notify_module_nothing_is_mounted() {
    let app = crate::servers::database(TestApp::builder())
        .config("webhooks.routes", false)
        .config("webhooks.incoming.twilio.secret", TWILIO_TOKEN)
        .module(DatabaseModule::from_config())
        .module(WebhooksModule::from_config())
        .build()
        .await;

    app.client()
        .post("/api/webhooks/notify/twilio")
        .body("")
        .send()
        .await
        .assert_status(404);
}
