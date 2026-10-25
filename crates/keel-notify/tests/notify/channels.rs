//! Delivery over the database, mail and broadcast channels.

use keel_notify::{Channel, Notifier, NotifyError};
use serde_json::json;

use crate::support::{self, DealWon, TestUser};

#[tokio::test]
async fn database_channel_stores_notification() {
    let app = support::app().await;
    let notifier = app.get::<Notifier>().await;
    let user = TestUser::new();
    let notification = DealWon::via(&[Channel::Database], "Acme renewal");

    notifier.send(&user, notification.clone()).await.unwrap();

    let rows = support::rows(&app).await;
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.notifiable_type, "user");
    assert_eq!(row.notifiable_id, user.id.to_string());
    assert_eq!(row.notification_type, "deal_won");
    assert_eq!(row.data, notification.data());
    assert_eq!(row.read_at, None);
    assert_eq!(row.id.get_version_num(), 7, "ids are UUID v7");
    assert!(support::sent_mail(&app).await.is_empty());
}

#[tokio::test]
async fn mail_channel_sends_via_mailer_array_transport() {
    let app = support::app().await;
    let notifier = app.get::<Notifier>().await;
    let user = TestUser::new();

    notifier
        .send(&user, DealWon::via(&[Channel::Mail], "Acme renewal"))
        .await
        .unwrap();

    let sent = support::sent_mail(&app).await;
    assert_eq!(sent.len(), 1);
    let mail = &sent[0];
    assert_eq!(mail.to, vec![user.email.clone().unwrap()]);
    assert_eq!(mail.from.as_deref(), Some("Keel <noreply@keel.test>"));
    assert_eq!(mail.subject, "Deal won: Acme renewal");
    assert_eq!(mail.html.as_deref(), Some("<p>Acme renewal was won.</p>"));
    assert_eq!(mail.text.as_deref(), Some("Acme renewal was won."));
    assert!(support::rows(&app).await.is_empty(), "mail only");
}

#[tokio::test]
async fn mail_channel_skips_a_notifiable_without_an_address() {
    let app = support::app().await;
    let notifier = app.get::<Notifier>().await;

    notifier
        .send(
            &TestUser::without_email(),
            DealWon::via(&[Channel::Mail, Channel::Database], "Acme renewal"),
        )
        .await
        .unwrap();

    assert!(support::sent_mail(&app).await.is_empty());
    assert_eq!(support::rows(&app).await.len(), 1);
}

#[tokio::test]
async fn broadcast_channel_calls_broadcaster() {
    let (app, broadcaster) = support::app_with_broadcaster().await;
    let notifier = app.get::<Notifier>().await;
    let user = TestUser::new();
    let notification = DealWon::via(&[Channel::Broadcast, Channel::Database], "Acme renewal");

    let id = notifier.send(&user, notification.clone()).await.unwrap();

    let row = support::rows(&app).await.remove(0);
    assert_eq!(row.id, id);
    let sent = broadcaster.sent();
    assert_eq!(sent.len(), 1);
    let (channels, event, payload) = &sent[0];
    assert_eq!(channels, &vec![format!("private-users.{}", user.id)]);
    assert_eq!(event, "notification.created");
    assert_eq!(
        payload,
        &json!({
            "id": id,
            "type": "deal_won",
            "data": notification.data(),
            "read_at": null,
            "created_at": row.created_at,
        })
    );
}

#[tokio::test]
async fn missing_broadcaster_fails_only_the_broadcast_channel() {
    let app = support::app().await;
    let notifier = app.get::<Notifier>().await;

    let error = notifier
        .send(
            &TestUser::new(),
            DealWon::via(&[Channel::Broadcast, Channel::Database], "Acme renewal"),
        )
        .await
        .unwrap_err();

    assert!(
        matches!(
            error,
            NotifyError::Unavailable {
                channel: Some(Channel::Broadcast),
                ..
            }
        ),
        "{error:?}"
    );
    assert_eq!(
        support::rows(&app).await.len(),
        1,
        "the database channel still delivers"
    );
}
