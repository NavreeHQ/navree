//! Native app push devices: the service, the signed-in session's routes,
//! and the push channel over them (live sessions, gone devices,
//! preferences, quiet hours, the delivery log, links inside the app).

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use navree_contracts::{BoxError, BoxFuture, CredentialRevoker, Principal};
use navree_notify::{
    Channel, ChannelError, Delivery, LiveSessions, NewPushDevice, Notifiable, Notification,
    Notifier, NotifyError, NotifyOptions, PushDevices, PushDriver, PushEnvironment, PushFake,
    PushMessage, PushPlatform,
};
use navree_testing::{TestApp, TestAppBuilder, principal};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::people::{Person, at, held, prefer, timed};
use crate::support::{self, RecordingDispatcher, TestUser};

const IPHONE: &str = "740f4707bebcf74f9b7c25d48e3358945f6aa01da5ddb387462c7eaf61bb78ad";
const NEW_IPHONE: &str = "8a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f9";
const PIXEL: &str = "dQw4w9WgXcQ:APA91bHun4MxP5egoKMwt2KZFBaFUH-1RYqx-w0rk3l2";

/// The sessions that have ended, as navree-auth would answer.
#[derive(Default)]
struct Sessions {
    ended: Mutex<HashSet<String>>,
}

impl Sessions {
    fn end(&self, session: &str) {
        self.ended.lock().unwrap().insert(session.to_owned());
    }
}

impl LiveSessions for Sessions {
    fn live<'a>(
        &'a self,
        _user_id: Uuid,
        sessions: &'a [String],
    ) -> BoxFuture<'a, Result<Vec<String>, BoxError>> {
        let ended = self.ended.lock().unwrap().clone();
        let live = sessions
            .iter()
            .filter(|session| !ended.contains(*session))
            .cloned()
            .collect();
        Box::pin(async move { Ok(live) })
    }
}

/// A shipment notice over the push channel (and the database).
#[derive(Clone, Debug, Serialize, Deserialize)]
struct OrderShipped {
    order: String,
    href: String,
}

impl OrderShipped {
    fn new(order: &str) -> Self {
        Self {
            order: order.to_owned(),
            href: format!("/orders/{order}"),
        }
    }
}

impl Notification for OrderShipped {
    const TYPE: &'static str = "order_shipped";
    const CATEGORY: &'static str = "orders";

    fn via(&self, _n: &dyn Notifiable) -> Vec<Channel> {
        vec![Channel::Database, Channel::Push]
    }

    fn to_push(&self, _n: &dyn Notifiable) -> Option<PushMessage> {
        Some(
            PushMessage::new("Your order has shipped")
                .body(format!("Order {} is on its way to Ludhiana", self.order))
                .href(self.href.clone())
                .collapse(format!("order-{}", self.order)),
        )
    }
}

fn device(platform: PushPlatform, token: &str) -> NewPushDevice {
    NewPushDevice {
        platform,
        token: token.to_owned(),
        app_id: "com.acme.crm".to_owned(),
        environment: PushEnvironment::Production,
    }
}

/// The signed-in user `id` in session `session`.
fn signed_in(id: Uuid, session: &str) -> Principal {
    Principal {
        session_id: Some(session.to_owned()),
        ..principal(id)
    }
}

/// `builder` with the push fake, the sessions and the apps of
/// `notify.push`.
fn with_push(builder: TestAppBuilder, push: &PushFake, sessions: &Arc<Sessions>) -> TestAppBuilder {
    builder
        .config("notify.push.apns.bundle_ids", json!(["com.acme.crm"]))
        .config("notify.push.fcm.packages", json!(["com.acme.crm"]))
        .override_value::<dyn PushDriver>(push.driver())
        .override_value::<dyn LiveSessions>(sessions.clone())
}

async fn push_app() -> (TestApp, PushFake, Arc<Sessions>) {
    let push = PushFake::new();
    let sessions = Arc::new(Sessions::default());
    let app = with_push(support::builder(NotifyOptions::default()), &push, &sessions)
        .build()
        .await;
    (app, push, sessions)
}

// ---------------------------------------------------------------------
// The service
// ---------------------------------------------------------------------

#[tokio::test]
async fn a_device_registers_once_per_token() {
    let (app, _push, _sessions) = push_app().await;
    let devices = app.get::<PushDevices>().await;
    let user = TestUser::new();

    let first = devices
        .register(&user, "session-1", device(PushPlatform::Ios, IPHONE))
        .await
        .unwrap();
    let again = devices
        .register(&user, "session-1", device(PushPlatform::Ios, IPHONE))
        .await
        .unwrap();

    assert_eq!(first.id, again.id);
    assert_eq!(again.platform, "ios");
    assert_eq!(again.environment, "production");
    assert_eq!(again.app_id, "com.acme.crm");
    assert!(again.last_seen_at >= first.last_seen_at);
    let saved = devices.of("user", &user.id.to_string()).await.unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].token, IPHONE);
    assert_eq!(saved[0].session_id, "session-1");
}

#[tokio::test]
async fn a_token_moves_to_whoever_registers_it_last() {
    let (app, _push, _sessions) = push_app().await;
    let devices = app.get::<PushDevices>().await;
    let (simran, harpreet) = (TestUser::new(), TestUser::new());

    devices
        .register(
            &simran,
            "session-simran",
            device(PushPlatform::Android, PIXEL),
        )
        .await
        .unwrap();
    let moved = devices
        .register(
            &harpreet,
            "session-harpreet",
            device(PushPlatform::Android, PIXEL),
        )
        .await
        .unwrap();

    assert_eq!(moved.notifiable_id, harpreet.id.to_string());
    assert_eq!(moved.session_id, "session-harpreet");
    assert!(
        devices
            .of("user", &simran.id.to_string())
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn a_session_keeps_one_device_per_app() {
    let (app, _push, _sessions) = push_app().await;
    let devices = app.get::<PushDevices>().await;
    let user = TestUser::new();

    devices
        .register(&user, "session-1", device(PushPlatform::Ios, IPHONE))
        .await
        .unwrap();
    // The system gave the app a new token.
    devices
        .register(&user, "session-1", device(PushPlatform::Ios, NEW_IPHONE))
        .await
        .unwrap();
    // Another phone of the same person keeps its own.
    devices
        .register(&user, "session-2", device(PushPlatform::Android, PIXEL))
        .await
        .unwrap();

    let tokens: Vec<String> = devices
        .of("user", &user.id.to_string())
        .await
        .unwrap()
        .into_iter()
        .map(|d| d.token)
        .collect();
    assert_eq!(tokens, [NEW_IPHONE, PIXEL]);
}

#[tokio::test]
async fn ending_a_session_removes_its_devices() {
    let (app, _push, _sessions) = push_app().await;
    let devices = app.get::<PushDevices>().await;
    let user = TestUser::new();
    devices
        .register(&user, "session-1", device(PushPlatform::Ios, IPHONE))
        .await
        .unwrap();
    devices
        .register(&user, "session-2", device(PushPlatform::Android, PIXEL))
        .await
        .unwrap();

    assert_eq!(devices.end_session("session-1").await.unwrap(), 1);

    let left = devices.of("user", &user.id.to_string()).await.unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].token, PIXEL);
    assert!(devices.unregister(&user, PIXEL).await.unwrap());
    assert!(!devices.unregister(&user, PIXEL).await.unwrap());
}

#[tokio::test]
async fn offboarding_removes_devices() {
    let (app, _push, _sessions) = push_app().await;
    let devices = app.get::<PushDevices>().await;
    let (leaver, stayer) = (TestUser::new(), TestUser::new());
    devices
        .register(&leaver, "session-1", device(PushPlatform::Ios, IPHONE))
        .await
        .unwrap();
    devices
        .register(&stayer, "session-2", device(PushPlatform::Android, PIXEL))
        .await
        .unwrap();

    let revokers = app.ctx().get_all::<dyn CredentialRevoker>().await.unwrap();
    let notify = revokers
        .iter()
        .find(|revoker| revoker.name() == "notify")
        .expect("the push devices revoker");
    let revoked = notify.revoke_for_users(&[leaver.id]).await.unwrap();

    assert_eq!(revoked.get("push_devices"), Some(&1));
    assert!(
        devices
            .of("user", &leaver.id.to_string())
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        devices
            .of("user", &stayer.id.to_string())
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn invalid_devices_and_other_apps_are_refused() {
    let (app, _push, _sessions) = push_app().await;
    let devices = app.get::<PushDevices>().await;
    let user = TestUser::new();

    let mut other_app = device(PushPlatform::Ios, IPHONE);
    other_app.app_id = "com.evil.app".to_owned();
    assert!(matches!(
        devices.register(&user, "session-1", other_app).await,
        Err(NotifyError::InvalidRecipient(_))
    ));
    assert!(matches!(
        devices
            .register(&user, "session-1", device(PushPlatform::Ios, "not-hex"))
            .await,
        Err(NotifyError::InvalidRecipient(_))
    ));
    assert!(matches!(
        devices
            .register(&user, "", device(PushPlatform::Ios, IPHONE))
            .await,
        Err(NotifyError::InvalidRecipient(_))
    ));
}

// ---------------------------------------------------------------------
// The routes
// ---------------------------------------------------------------------

#[tokio::test]
async fn a_signed_in_session_registers_and_removes_its_device() {
    let (app, _push, _sessions) = push_app().await;
    let user = TestUser::new();
    let phone = app.acting_as(signed_in(user.id, "session-1"));

    let created = phone
        .post("/api/notifications/push-devices")
        .json(&json!({
            "platform": "ios",
            "token": IPHONE,
            "app_id": "com.acme.crm",
            "environment": "sandbox",
        }))
        .send()
        .await
        .assert_status(201);
    let body = created.json_value();
    assert_eq!(body["platform"], "ios");
    assert_eq!(body["environment"], "sandbox");
    assert_eq!(body["app_id"], "com.acme.crm");
    assert_eq!(body["notifiable_id"], user.id.to_string());
    assert!(body.get("token").is_none(), "the token is never returned");
    assert!(body.get("token_hash").is_none());
    assert!(body.get("session_id").is_none());
    let devices = app.get::<PushDevices>().await;
    let saved = devices.of("user", &user.id.to_string()).await.unwrap();
    assert_eq!(saved[0].session_id, "session-1");

    // Someone else cannot remove it.
    app.acting_as(signed_in(TestUser::new().id, "session-9"))
        .delete("/api/notifications/push-devices")
        .json(&json!({ "token": IPHONE }))
        .send()
        .await
        .assert_error(404, None);

    phone
        .delete("/api/notifications/push-devices")
        .json(&json!({ "token": IPHONE }))
        .send()
        .await
        .assert_status(204);
    assert!(
        devices
            .of("user", &user.id.to_string())
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn registering_needs_a_signed_in_session() {
    let (app, _push, _sessions) = push_app().await;
    let body = json!({ "platform": "android", "token": PIXEL, "app_id": "com.acme.crm" });

    app.client()
        .post("/api/notifications/push-devices")
        .json(&body)
        .send()
        .await
        .assert_error(401, None);
    // An API token without a session (a personal access token) cannot.
    app.acting_as(principal(TestUser::new().id))
        .post("/api/notifications/push-devices")
        .json(&body)
        .send()
        .await
        .assert_status(403);
}

#[tokio::test]
async fn invalid_devices_are_refused_by_field() {
    let (app, _push, _sessions) = push_app().await;
    let phone = app.acting_as(signed_in(TestUser::new().id, "session-1"));

    for (body, field) in [
        (
            json!({ "platform": "ios", "token": PIXEL, "app_id": "com.acme.crm" }),
            "token",
        ),
        (
            json!({ "platform": "android", "token": PIXEL, "app_id": "com.evil.app" }),
            "app_id",
        ),
        (
            json!({ "platform": "android", "token": PIXEL, "app_id": "com..acme" }),
            "app_id",
        ),
        (
            json!({ "platform": "android", "token": PIXEL, "app_id": "com.acme.crm", "environment": "sandbox" }),
            "environment",
        ),
    ] {
        phone
            .post("/api/notifications/push-devices")
            .json(&body)
            .send()
            .await
            .assert_validation_error(field);
    }
    phone
        .post("/api/notifications/push-devices")
        .json(&json!({ "platform": "windows", "token": PIXEL, "app_id": "com.acme.crm" }))
        .send()
        .await
        .assert_status(400);
}

#[tokio::test]
async fn registration_is_throttled() {
    let (app, _push, _sessions) = push_app().await;
    let phone = app.acting_as(signed_in(TestUser::new().id, "session-1"));
    let body = json!({ "platform": "android", "token": PIXEL, "app_id": "com.acme.crm" });

    // The limit refills over the minute, so a slow run may get a request or
    // two more through; a burst is stopped all the same.
    let mut accepted = 0;
    let mut throttled = false;
    for _ in 0..45 {
        let response = phone
            .post("/api/notifications/push-devices")
            .json(&body)
            .send()
            .await;
        match response.status().as_u16() {
            201 => accepted += 1,
            429 => {
                throttled = true;
                break;
            }
            other => panic!("unexpected status {other}"),
        }
    }
    assert!(throttled, "45 registrations in a burst are throttled");
    assert!(accepted >= 30, "the first 30 go through, {accepted} did");
}

// ---------------------------------------------------------------------
// The channel
// ---------------------------------------------------------------------

#[tokio::test]
async fn push_goes_to_every_device_and_is_logged_per_device() {
    let (app, push, _sessions) = push_app().await;
    let devices = app.get::<PushDevices>().await;
    let user = TestUser::new();
    let iphone = devices
        .register(&user, "session-1", device(PushPlatform::Ios, IPHONE))
        .await
        .unwrap();
    let pixel = devices
        .register(&user, "session-2", device(PushPlatform::Android, PIXEL))
        .await
        .unwrap();

    let id = app
        .get::<Notifier>()
        .await
        .send(&user, OrderShipped::new("A-1042"))
        .await
        .unwrap();

    for token in [IPHONE, PIXEL] {
        let sent = push.sent_to(token);
        assert_eq!(sent.len(), 1, "{token}");
        assert_eq!(sent[0].href.as_deref(), Some("/orders/A-1042"));
        assert_eq!(sent[0].collapse.as_deref(), Some("order-A-1042"));
    }
    let platforms: Vec<PushPlatform> = push.sent().iter().map(|s| s.to.platform).collect();
    assert_eq!(platforms, [PushPlatform::Ios, PushPlatform::Android]);
    let logged = support::deliveries(&app).await;
    let mut recipients: Vec<String> = logged
        .iter()
        .filter(|d| d.notification_id == id && d.channel == "push")
        .map(|d| {
            assert_eq!(d.status, "sent");
            assert_eq!(d.provider.as_deref(), Some("fake"));
            d.recipient.clone().unwrap()
        })
        .collect();
    recipients.sort();
    let mut expected = vec![iphone.id.to_string(), pixel.id.to_string()];
    expected.sort();
    assert_eq!(recipients, expected);
    let touched = devices.of("user", &user.id.to_string()).await.unwrap();
    assert!(touched.iter().all(|d| d.last_used_at.is_some()));
}

#[tokio::test]
async fn a_gone_device_is_deleted() {
    let (app, push, _sessions) = push_app().await;
    let devices = app.get::<PushDevices>().await;
    let user = TestUser::new();
    devices
        .register(&user, "session-1", device(PushPlatform::Ios, IPHONE))
        .await
        .unwrap();
    devices
        .register(&user, "session-2", device(PushPlatform::Android, PIXEL))
        .await
        .unwrap();
    push.expire(IPHONE);

    app.get::<Notifier>()
        .await
        .send(&user, OrderShipped::new("A-1042"))
        .await
        .unwrap();

    assert_eq!(push.sent_to(PIXEL).len(), 1);
    let left = devices.of("user", &user.id.to_string()).await.unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].token, PIXEL);
    let failed: Vec<_> = support::deliveries(&app)
        .await
        .into_iter()
        .filter(|d| d.status == "failed")
        .collect();
    assert_eq!(failed.len(), 1);
    assert_eq!(
        failed[0].error.as_deref(),
        Some(ChannelError::Gone.to_string().as_str())
    );
}

#[tokio::test]
async fn a_signed_out_session_gets_nothing() {
    let (app, push, sessions) = push_app().await;
    let user = TestUser::new();
    // Two phones sign in and register through the routes.
    for (session, body) in [
        (
            "session-iphone",
            json!({ "platform": "ios", "token": IPHONE, "app_id": "com.acme.crm" }),
        ),
        (
            "session-pixel",
            json!({ "platform": "android", "token": PIXEL, "app_id": "com.acme.crm" }),
        ),
    ] {
        app.acting_as(signed_in(user.id, session))
            .post("/api/notifications/push-devices")
            .json(&body)
            .send()
            .await
            .assert_status(201);
    }

    // The iPhone signs out (or is ended from the other phone).
    sessions.end("session-iphone");
    app.get::<Notifier>()
        .await
        .send(&user, OrderShipped::new("A-1042"))
        .await
        .unwrap();

    assert!(
        push.sent_to(IPHONE).is_empty(),
        "a signed-out phone gets nothing"
    );
    assert_eq!(push.sent_to(PIXEL).len(), 1);
    let left = app
        .get::<PushDevices>()
        .await
        .of("user", &user.id.to_string())
        .await
        .unwrap();
    assert_eq!(left.len(), 1, "the signed-out phone's device is deleted");
    assert_eq!(left[0].token, PIXEL);
}

#[tokio::test]
async fn preferences_turn_push_off() {
    let (app, push, _sessions) = push_app().await;
    let user = TestUser::new();
    app.get::<PushDevices>()
        .await
        .register(&user, "session-1", device(PushPlatform::Ios, IPHONE))
        .await
        .unwrap();
    app.acting_as(signed_in(user.id, "session-1"))
        .patch("/api/notifications/preferences")
        .json(&json!({ "categories": { "orders": { "channels": { "push": false } } } }))
        .send()
        .await
        .assert_status(200);

    app.get::<Notifier>()
        .await
        .send(&user, OrderShipped::new("A-1042"))
        .await
        .unwrap();

    push.assert_nothing_sent();
    assert_eq!(
        support::rows(&app).await.len(),
        1,
        "the in-app row still is"
    );
}

#[tokio::test]
async fn quiet_hours_hold_push() {
    // 14:00 UTC is 23:00 in Tokyo.
    let push = PushFake::new();
    let sessions = Arc::new(Sessions::default());
    let (builder, clock, _fakes) = timed(NotifyOptions::default(), "2026-10-28T14:00:00Z");
    let app = with_push(builder, &push, &sessions).build().await;
    let tokyo = Person::in_zone(Some("Asia/Tokyo"));
    prefer(
        &app,
        &tokyo.owner(),
        json!({"quiet_hours": {"start": "22:00", "end": "07:00"}}),
    )
    .await;
    app.get::<PushDevices>()
        .await
        .register(&tokyo, "session-1", device(PushPlatform::Ios, IPHONE))
        .await
        .unwrap();
    let notifier = app.get::<Notifier>().await;

    notifier
        .send(&tokyo, OrderShipped::new("A-1042"))
        .await
        .unwrap();

    push.assert_nothing_sent();
    let rows = held(&app).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].channel, "push");
    assert_eq!(rows[0].release_at, at("2026-10-28T22:00:00Z"));

    clock.set(at("2026-10-28T22:00:00Z"));
    notifier.release_due().await.unwrap();
    assert_eq!(push.sent_to(IPHONE).len(), 1);
}

#[tokio::test]
async fn a_retried_job_skips_reached_devices() {
    let push = PushFake::new();
    let sessions = Arc::new(Sessions::default());
    let dispatcher = Arc::new(RecordingDispatcher::default());
    let app = with_push(
        support::builder(NotifyOptions {
            queue: true,
            ..NotifyOptions::default()
        }),
        &push,
        &sessions,
    )
    .override_value::<dyn navree_contracts::Dispatcher>(dispatcher.clone())
    .build()
    .await;
    let user = TestUser::new();
    let devices = app.get::<PushDevices>().await;
    devices
        .register(&user, "session-1", device(PushPlatform::Ios, IPHONE))
        .await
        .unwrap();
    app.get::<Notifier>()
        .await
        .send(&user, OrderShipped::new("A-1042"))
        .await
        .unwrap();
    let job = dispatcher
        .jobs()
        .into_iter()
        .find(|job| job.payload["delivery"]["via"] == "push")
        .expect("a push job");

    support::run_job(&app, &job).await.unwrap();
    // A second phone signs in before the retry.
    devices
        .register(&user, "session-2", device(PushPlatform::Android, PIXEL))
        .await
        .unwrap();
    support::run_job(&app, &job).await.unwrap();

    assert_eq!(push.sent_to(IPHONE).len(), 1, "reached once");
    assert_eq!(push.sent_to(PIXEL).len(), 1);
}

#[tokio::test]
async fn a_refusal_fails_the_delivery_so_the_queue_retries_it() {
    let (app, push, _sessions) = push_app().await;
    let user = TestUser::new();
    app.get::<PushDevices>()
        .await
        .register(&user, "session-1", device(PushPlatform::Ios, IPHONE))
        .await
        .unwrap();
    push.refuse("the service is busy");

    let error = app
        .get::<Notifier>()
        .await
        .send(&user, OrderShipped::new("A-1042"))
        .await
        .unwrap_err();

    assert!(
        matches!(
            &error,
            NotifyError::Channel {
                channel: Channel::Push,
                ..
            }
        ),
        "{error:?}"
    );
    assert_eq!(
        app.get::<PushDevices>()
            .await
            .of("user", &user.id.to_string())
            .await
            .unwrap()
            .len(),
        1,
        "a refusal keeps the device"
    );
}

/// A notification whose link leaves the app.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Phishing;

impl Notification for Phishing {
    const TYPE: &'static str = "phishing";

    fn via(&self, _n: &dyn Notifiable) -> Vec<Channel> {
        vec![Channel::Push]
    }

    fn to_push(&self, _n: &dyn Notifiable) -> Option<PushMessage> {
        Some(PushMessage::new("Verify your account").href("https://evil.example/login"))
    }
}

#[tokio::test]
async fn an_href_outside_the_app_is_refused_before_sending() {
    let (app, push, _sessions) = push_app().await;
    let user = TestUser::new();
    app.get::<PushDevices>()
        .await
        .register(&user, "session-1", device(PushPlatform::Ios, IPHONE))
        .await
        .unwrap();

    let error = app
        .get::<Notifier>()
        .await
        .send(&user, Phishing)
        .await
        .unwrap_err();

    assert!(
        matches!(
            &error,
            NotifyError::Channel { channel: Channel::Push, source: ChannelError::InvalidMessage(m) } if m.starts_with("href")
        ),
        "{error:?}"
    );
    push.assert_nothing_sent();
}

#[tokio::test]
async fn the_push_job_carries_the_rendered_message() {
    let delivery = Delivery::Push {
        message: PushMessage::new("Hi").href("/inbox"),
    };
    assert_eq!(delivery.channel(), Channel::Push);
    let json = serde_json::to_value(&delivery).unwrap();
    assert_eq!(json["via"], "push");
    assert_eq!(json["message"]["href"], "/inbox");
    let back: Delivery = serde_json::from_value(json).unwrap();
    assert_eq!(back, delivery);
}

#[tokio::test]
async fn without_a_driver_the_channel_is_unavailable() {
    let app = support::app().await;
    let user = TestUser::new();
    let error = app
        .get::<Notifier>()
        .await
        .send(&user, OrderShipped::new("A-1042"))
        .await
        .unwrap_err();
    assert!(
        matches!(
            &error,
            NotifyError::Unavailable {
                channel: Some(Channel::Push),
                ..
            }
        ),
        "{error:?}"
    );
}

/// navree-auth's own sessions decide: a session signed out with
/// `AuthService::logout` ends its phone's device, the other phone's stays.
#[cfg(feature = "auth")]
#[tokio::test]
async fn signing_out_of_navree_auth_ends_the_device() {
    use navree_auth::{AuthModule, AuthService, ClientMeta};
    use navree_crypto::{CryptoModule, generate_key};

    let push = PushFake::new();
    let app = support::builder(NotifyOptions::default())
        .config("app.key", generate_key())
        .config("hashing.memory_kib", 1024)
        .config("hashing.iterations", 1)
        .config("notify.push.apns.bundle_ids", json!(["com.acme.crm"]))
        .config("notify.push.fcm.packages", json!(["com.acme.crm"]))
        .module(CryptoModule::from_config())
        .module(AuthModule::from_config())
        .override_value::<dyn PushDriver>(push.driver())
        .build()
        .await;
    let auth = app.get::<AuthService>().await;
    let user = auth
        .create_user("Navjot Kaur", "navjot@acme.test", "correct horse battery")
        .await
        .unwrap();
    let iphone = auth
        .issue_tokens(&user, ClientMeta::default())
        .await
        .unwrap();
    let iphone_session = auth.sessions(user.id, None).await.unwrap()[0].id;
    auth.issue_tokens(&user, ClientMeta::default())
        .await
        .unwrap();
    let pixel_session = auth
        .sessions(user.id, None)
        .await
        .unwrap()
        .into_iter()
        .map(|session| session.id)
        .find(|id| *id != iphone_session)
        .unwrap();
    let devices = app.get::<PushDevices>().await;
    let recipient = TestUser {
        id: user.id,
        email: None,
    };
    devices
        .register(
            &recipient,
            &iphone_session.to_string(),
            device(PushPlatform::Ios, IPHONE),
        )
        .await
        .unwrap();
    devices
        .register(
            &recipient,
            &pixel_session.to_string(),
            device(PushPlatform::Android, PIXEL),
        )
        .await
        .unwrap();

    auth.logout(Some(&iphone.refresh_token), user.id, false)
        .await
        .unwrap();
    app.get::<Notifier>()
        .await
        .send(&recipient, OrderShipped::new("A-1042"))
        .await
        .unwrap();

    assert!(
        push.sent_to(IPHONE).is_empty(),
        "the signed-out iPhone gets nothing"
    );
    assert_eq!(push.sent_to(PIXEL).len(), 1);
    let left = devices.of("user", &user.id.to_string()).await.unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].token, PIXEL);
}
