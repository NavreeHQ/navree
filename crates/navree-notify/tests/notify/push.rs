//! Push subscriptions: the signed-in user's routes, the service, and the
//! push key.

use navree_crypto::webpush::VapidKey;
use navree_notify::{Notifier, NotifyOptions, PushSubscriptions};
use navree_testing::{TestApp, principal};
use serde_json::json;

use crate::external::subscription;
use crate::support::{self, Member, TestUser};

/// An app with a VAPID key pair.
async fn app_with_vapid() -> (TestApp, VapidKey) {
    let key = VapidKey::generate();
    let mut options = NotifyOptions::default();
    options.web_push.public_key = Some(key.application_server_key());
    options.web_push.private_key = key.private_key().into();
    options.web_push.subject = "mailto:ops@acme.test".to_owned();
    let app = support::builder(options).build().await;
    (app, key)
}

#[tokio::test]
async fn the_push_key_is_the_application_server_key() {
    let (app, key) = app_with_vapid().await;
    let user = TestUser::new();

    app.acting_as(principal(user.id))
        .get("/api/notifications/push-key")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("public_key", json!(key.application_server_key()));
    assert_eq!(
        app.get::<Notifier>().await.application_server_key(),
        Some(key.application_server_key())
    );
    app.client()
        .get("/api/notifications/push-key")
        .send()
        .await
        .assert_error(401, None);
}

#[tokio::test]
async fn without_a_key_pair_there_is_no_push_key_route() {
    let app = support::app().await;
    let user = TestUser::new();
    app.acting_as(principal(user.id))
        .get("/api/notifications/push-key")
        .send()
        .await
        .assert_status(404);
}

#[tokio::test]
async fn a_user_subscribes_and_unsubscribes_their_browser() {
    let (app, _key) = app_with_vapid().await;
    let user = TestUser::new();
    let as_user = app.acting_as(principal(user.id));
    let browser = subscription("https://fcm.googleapis.com/fcm/send/abc:def");

    let created = as_user
        .post("/api/notifications/push-subscriptions")
        .header("user-agent", "Mozilla/5.0 (Android)")
        .json(&browser)
        .send()
        .await
        .assert_status(201);
    let body = created.json_value();
    assert_eq!(body["endpoint"], browser.endpoint);
    assert_eq!(body["notifiable_type"], "user");
    assert_eq!(body["notifiable_id"], user.id.to_string());
    assert_eq!(body["user_agent"], "Mozilla/5.0 (Android)");
    assert!(body.get("auth").is_none(), "the keys are never returned");
    assert!(body.get("p256dh").is_none());
    let push = app.get::<PushSubscriptions>().await;
    let saved = push.of("user", &user.id.to_string()).await.unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].auth, browser.keys.auth);

    // Subscribing again keeps one row.
    as_user
        .post("/api/notifications/push-subscriptions")
        .json(&browser)
        .send()
        .await
        .assert_status(201);
    assert_eq!(
        push.of("user", &user.id.to_string()).await.unwrap().len(),
        1
    );

    // Someone else cannot remove it.
    let other = TestUser::new();
    app.acting_as(principal(other.id))
        .delete("/api/notifications/push-subscriptions")
        .json(&json!({ "endpoint": browser.endpoint }))
        .send()
        .await
        .assert_error(404, None);

    as_user
        .delete("/api/notifications/push-subscriptions")
        .json(&json!({ "endpoint": browser.endpoint }))
        .send()
        .await
        .assert_status(204);
    assert!(
        push.of("user", &user.id.to_string())
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn invalid_subscriptions_are_refused_by_field() {
    let (app, _key) = app_with_vapid().await;
    let as_user = app.acting_as(principal(TestUser::new().id));

    let mut insecure = subscription("http://push.example.com/abc");
    as_user
        .post("/api/notifications/push-subscriptions")
        .json(&insecure)
        .send()
        .await
        .assert_validation_error("endpoint");
    insecure.endpoint = "https://push.example.com/abc".to_owned();
    insecure.keys.p256dh = "BAAA".to_owned();
    as_user
        .post("/api/notifications/push-subscriptions")
        .json(&insecure)
        .send()
        .await
        .assert_validation_error("keys.p256dh");
    as_user
        .post("/api/notifications/push-subscriptions")
        .json(&json!({ "endpoint": "https://push.example.com/abc" }))
        .send()
        .await
        .assert_status(400);
}

#[tokio::test]
async fn a_browser_moves_to_whoever_subscribes_it_last() {
    let (app, _key) = app_with_vapid().await;
    let push = app.get::<PushSubscriptions>().await;
    let (first, second) = (Member::new(), Member::new());
    let browser = subscription("https://web.push.apple.com/QGuA");

    push.subscribe(&first, browser.clone(), None).await.unwrap();
    let moved = push
        .subscribe(&second, browser.clone(), None)
        .await
        .unwrap();

    assert_eq!(moved.notifiable_id, second.id.to_string());
    assert!(
        push.of("user", &first.id.to_string())
            .await
            .unwrap()
            .is_empty()
    );
    assert!(push.delete(moved.id).await.unwrap());
    assert!(!push.delete(moved.id).await.unwrap());
}

#[tokio::test]
async fn the_routes_follow_notify_routes() {
    let mut options = NotifyOptions {
        routes: false,
        ..NotifyOptions::default()
    };
    let key = VapidKey::generate();
    options.web_push.public_key = Some(key.application_server_key());
    options.web_push.private_key = key.private_key().into();
    options.web_push.subject = "mailto:ops@acme.test".to_owned();
    let app = support::builder(options).build().await;

    app.acting_as(principal(TestUser::new().id))
        .post("/api/notifications/push-subscriptions")
        .json(&subscription("https://fcm.googleapis.com/fcm/send/x"))
        .send()
        .await
        .assert_status(404);
}
