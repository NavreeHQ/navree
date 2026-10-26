//! How notifications render per channel, and how failures are reported.

use navis_db::DatabaseModule;
use navis_mail::TemplateMail;
use navis_notify::{Channel, Notifiable, Notification, Notifier, NotifyError, NotifyModule};
use navis_testing::TestApp;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::support::{self, DealWon, TestUser};

/// A weekly digest: mailed to a fixed address, stored with the default
/// database data (the notification itself).
#[derive(Clone, Debug, Serialize, Deserialize)]
struct WeeklyDigest {
    deals: u32,
}

impl Notification for WeeklyDigest {
    const TYPE: &'static str = "weekly_digest";

    fn via(&self, _notifiable: &dyn Notifiable) -> Vec<Channel> {
        vec![Channel::Database, Channel::Mail]
    }

    fn to_mail(&self, _notifiable: &dyn Notifiable) -> Option<TemplateMail> {
        Some(
            TemplateMail::new("sales@navis.test", "Weekly digest", "mail/deal-won")
                .context(json!({ "title": format!("{} deals", self.deals) })),
        )
    }
}

#[tokio::test]
async fn repeated_channels_deliver_once() {
    let app = support::app().await;
    let notifier = app.get::<Notifier>().await;

    notifier
        .send(
            &TestUser::new(),
            DealWon::via(
                &[
                    Channel::Database,
                    Channel::Mail,
                    Channel::Database,
                    Channel::Mail,
                ],
                "Acme",
            ),
        )
        .await
        .unwrap();

    assert_eq!(support::rows(&app).await.len(), 1);
    assert_eq!(support::sent_mail(&app).await.len(), 1);
}

#[tokio::test]
async fn a_mail_with_recipients_is_not_routed_to_the_notifiable() {
    let app = support::app().await;
    let notifier = app.get::<Notifier>().await;

    notifier
        .send(&TestUser::new(), WeeklyDigest { deals: 3 })
        .await
        .unwrap();

    let sent = support::sent_mail(&app).await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].to, vec!["sales@navis.test".to_owned()]);
}

#[tokio::test]
async fn database_data_defaults_to_the_serialized_notification() {
    let app = support::app().await;
    let notifier = app.get::<Notifier>().await;

    notifier
        .send(&TestUser::new(), WeeklyDigest { deals: 3 })
        .await
        .unwrap();

    let row = support::rows(&app).await.remove(0);
    assert_eq!(row.notification_type, "weekly_digest");
    assert_eq!(row.data, json!({ "deals": 3 }));
}

#[tokio::test]
async fn every_failed_channel_is_reported_and_the_others_still_deliver() {
    let app = TestApp::builder()
        .temp_database()
        .module(DatabaseModule)
        .module(NotifyModule)
        .build()
        .await;
    let notifier = app.get::<Notifier>().await;

    let error = notifier
        .send(
            &TestUser::new(),
            DealWon::via(
                &[Channel::Mail, Channel::Database, Channel::Broadcast],
                "Acme",
            ),
        )
        .await
        .unwrap_err();

    let NotifyError::Several(errors) = &error else {
        panic!("expected several errors, got {error:?}");
    };
    let channels: Vec<_> = errors
        .iter()
        .map(|error| match error {
            NotifyError::Unavailable { channel, .. } => *channel,
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    assert_eq!(channels, [Some(Channel::Mail), Some(Channel::Broadcast)]);
    assert!(
        error
            .to_string()
            .starts_with("2 notification channels failed: 1) the mail channel is not available"),
        "{error}"
    );
    assert_eq!(support::rows(&app).await.len(), 1);
}
