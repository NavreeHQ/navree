//! Native apps' push devices end with the sign-in that registered them
//! (feature `notify`): browsers signed in on the session cookie register
//! through navree-notify's `POST /api/notifications/push-devices`, and the
//! push channel asks navree-auth which of their sessions are live.

use std::sync::Arc;
use std::time::Duration;

use navree_auth::AuthService;
use navree_contracts::{Clock, FixedClock};
use navree_notify::{
    Channel, Notifiable, Notification, NotifyModule, NotifyOptions, PushDevices, PushDriver,
    PushFake, PushMessage,
};
use navree_session::{SessionModule, SessionOptions};
use navree_testing::{TestApp, TestClient};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::support::{self, PASSWORD, create_user};

const IPHONE: &str = "740f4707bebcf74f9b7c25d48e3358945f6aa01da5ddb387462c7eaf61bb78ad";
const PIXEL: &str = "dQw4w9WgXcQ:APA91bHun4MxP5egoKMwt2KZFBaFUH-1RYqx-w0rk3l2";
const IPAD: &str = "8a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f9";

/// The user as navree-notify addresses them.
struct Member(Uuid);

impl Notifiable for Member {
    fn notifiable_type(&self) -> &'static str {
        navree_notify::USER_NOTIFIABLE_TYPE
    }

    fn notifiable_id(&self) -> String {
        self.0.to_string()
    }

    fn route_mail(&self) -> Option<String> {
        None
    }
}

/// A shipment notice, over push only.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct OrderShipped {
    order: String,
}

impl Notification for OrderShipped {
    const TYPE: &'static str = "order_shipped";
    const CATEGORY: &'static str = "orders";

    fn via(&self, _n: &dyn Notifiable) -> Vec<Channel> {
        vec![Channel::Push]
    }

    fn to_push(&self, _n: &dyn Notifiable) -> Option<PushMessage> {
        Some(
            PushMessage::new("Your order has shipped")
                .body(format!("Order {} is on its way to Amritsar", self.order))
                .href(format!("/orders/{}", self.order)),
        )
    }
}

/// An app with sessions, notifications and the push fake; navree-auth's own
/// `LiveSessions` (nothing overridden).
async fn app(clock: &Arc<FixedClock>, push: &PushFake) -> TestApp {
    support::mail_builder()
        .override_value::<dyn Clock>(clock.clone())
        .config("notify.push.apns.bundle_ids", json!(["com.example.orders"]))
        .config("notify.push.fcm.packages", json!(["com.example.orders"]))
        .override_value::<dyn PushDriver>(push.driver())
        .module(SessionModule::for_root(SessionOptions::default()))
        .module(NotifyModule::for_root(NotifyOptions::default()))
        .build()
        .await
}

/// A phone signed in as `email` on its session cookie.
async fn phone(app: &TestApp, email: &str) -> TestClient {
    let client = app.client();
    client.csrf_cookie().await;
    client
        .post("/api/auth/session/login")
        .xsrf()
        .json(&json!({ "email": email, "password": PASSWORD }))
        .send()
        .await
        .assert_status(200);
    client
}

/// Registers the phone's push token, as the app does after the system hands
/// it one.
async fn register(client: &TestClient, platform: &str, token: &str) {
    client
        .post("/api/notifications/push-devices")
        .xsrf()
        .json(&json!({ "platform": platform, "token": token, "app_id": "com.example.orders" }))
        .send()
        .await
        .assert_status(201);
}

/// The tokens of `user`'s saved devices.
async fn saved(app: &TestApp, user: Uuid) -> Vec<String> {
    let mut tokens: Vec<String> = app
        .get::<PushDevices>()
        .await
        .of(navree_notify::USER_NOTIFIABLE_TYPE, &user.to_string())
        .await
        .unwrap()
        .into_iter()
        .map(|device| device.token)
        .collect();
    tokens.sort();
    tokens
}

async fn ship(app: &TestApp, user: Uuid) {
    app.get::<navree_notify::Notifier>()
        .await
        .send(
            &Member(user),
            OrderShipped {
                order: "A-1042".to_owned(),
            },
        )
        .await
        .unwrap();
}

/// The id of the session `client` is signed in with.
async fn current_session(client: &TestClient) -> String {
    let sessions = client
        .get("/api/auth/sessions")
        .send()
        .await
        .assert_status(200)
        .json_value();
    sessions["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|session| session["current"] == json!(true))
        .and_then(|session| session["id"].as_str())
        .map(str::to_owned)
        .expect("the current session")
}

#[tokio::test]
async fn a_signed_out_phone_gets_no_notifications() {
    let (clock, push) = (support::clock(), PushFake::new());
    let app = app(&clock, &push).await;
    let user = create_user(&app, "simran@example.com").await.id;
    let iphone = phone(&app, "simran@example.com").await;
    let pixel = phone(&app, "simran@example.com").await;
    register(&iphone, "ios", IPHONE).await;
    register(&pixel, "android", PIXEL).await;
    ship(&app, user).await;
    assert_eq!(push.sent_to(IPHONE).len(), 1);
    assert_eq!(push.sent_to(PIXEL).len(), 1);

    // Signing out on the iPhone ends its device at once.
    iphone
        .post("/api/auth/logout")
        .xsrf()
        .send()
        .await
        .assert_status(204);
    assert_eq!(saved(&app, user).await, [PIXEL]);

    ship(&app, user).await;
    assert_eq!(push.sent_to(IPHONE).len(), 1, "nothing after signing out");
    assert_eq!(push.sent_to(PIXEL).len(), 2);
}

#[tokio::test]
async fn a_session_ended_elsewhere_revoked_or_expired_gets_nothing() {
    let (clock, push) = (support::clock(), PushFake::new());
    let app = app(&clock, &push).await;
    let user = create_user(&app, "simran@example.com").await.id;
    let iphone = phone(&app, "simran@example.com").await;
    let ipad = phone(&app, "simran@example.com").await;
    let pixel = phone(&app, "simran@example.com").await;
    let laptop = phone(&app, "simran@example.com").await;
    register(&iphone, "ios", IPHONE).await;
    register(&ipad, "ios", IPAD).await;
    register(&pixel, "android", PIXEL).await;

    // Ended from the laptop: deleted at once.
    let session = current_session(&iphone).await;
    laptop
        .delete(&format!("/api/auth/sessions/{session}"))
        .xsrf()
        .send()
        .await
        .assert_status(204);
    let mut left = vec![IPAD.to_owned(), PIXEL.to_owned()];
    left.sort();
    assert_eq!(saved(&app, user).await, left);

    // Revoked without a hook (every refresh token of the user, as a
    // password reset does): the push channel finds out before it sends.
    let ipad_session: Uuid = current_session(&ipad).await.parse().unwrap();
    let pixel_session: Uuid = current_session(&pixel).await.parse().unwrap();
    let auth = app.get::<AuthService>().await;
    assert_eq!(
        auth.live_sessions(user, &[ipad_session, pixel_session])
            .await
            .unwrap()
            .len(),
        2
    );
    auth.revoke_all_refresh_tokens(user).await.unwrap();
    assert!(
        auth.live_sessions(user, &[ipad_session, pixel_session])
            .await
            .unwrap()
            .is_empty()
    );
    ship(&app, user).await;
    for token in [IPHONE, IPAD, PIXEL] {
        assert!(push.sent_to(token).is_empty(), "{token}");
    }
    assert!(saved(&app, user).await.is_empty());

    // Expired: a new sign-in whose day has passed.
    let tablet = phone(&app, "simran@example.com").await;
    register(&tablet, "android", PIXEL).await;
    ship(&app, user).await;
    assert_eq!(push.sent_to(PIXEL).len(), 1);
    clock.advance(Duration::from_secs(25 * 3600));
    ship(&app, user).await;
    assert_eq!(
        push.sent_to(PIXEL).len(),
        1,
        "nothing after the sign-in expired"
    );
    assert!(saved(&app, user).await.is_empty());
}

#[tokio::test]
async fn logging_out_everywhere_ends_every_device() {
    let (clock, push) = (support::clock(), PushFake::new());
    let app = app(&clock, &push).await;
    let user = create_user(&app, "simran@example.com").await.id;
    register(&phone(&app, "simran@example.com").await, "ios", IPHONE).await;
    register(&phone(&app, "simran@example.com").await, "android", PIXEL).await;
    app.get::<AuthService>()
        .await
        .logout(None, user, true)
        .await
        .unwrap();
    assert!(saved(&app, user).await.is_empty());
}
