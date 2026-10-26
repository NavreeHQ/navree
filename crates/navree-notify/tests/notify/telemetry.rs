//! Telemetry: each channel a notification is delivered over is a
//! `NotificationSent` event.

use std::sync::Arc;

use navree_contracts::Broadcaster;
use navree_db::Db;
use navree_notify::{Channel, Notifier, NotifyOptions};
use navree_telemetry::TelemetryRecorder;

use crate::support::{self, DealWon, RecordingBroadcaster, TestUser};

const ALL: [Channel; 3] = [Channel::Database, Channel::Mail, Channel::Broadcast];

async fn recorded_app() -> (navree_testing::TestApp, TelemetryRecorder) {
    let recorder = TelemetryRecorder::new();
    let app = support::builder(NotifyOptions::default())
        .override_value::<dyn Broadcaster>(Arc::new(RecordingBroadcaster::default()))
        .module(recorder.module())
        .build()
        .await;
    (app, recorder)
}

fn channels(recorder: &TelemetryRecorder) -> Vec<String> {
    let mut channels: Vec<String> = recorder
        .notifications()
        .into_iter()
        .map(|n| n.channel)
        .collect();
    channels.sort();
    channels
}

#[tokio::test]
async fn every_delivered_channel_is_reported() {
    let (app, recorder) = recorded_app().await;
    let user = TestUser::new();
    let id = app
        .get::<Notifier>()
        .await
        .send_now(&user, DealWon::via(&ALL, "Acme renewal"))
        .await
        .unwrap();

    assert_eq!(channels(&recorder), ["broadcast", "database", "mail"]);
    let sent = &recorder.notifications()[0];
    assert_eq!(sent.notification_id, id.to_string());
    assert_eq!(sent.notification_type, "deal_won");
    assert_eq!(sent.notifiable_type, "user");
    assert_eq!(sent.notifiable_id, user.id.to_string());
}

#[tokio::test]
async fn notifications_in_a_unit_of_work_are_reported_after_commit() {
    let (app, recorder) = recorded_app().await;
    let notifier = app.get::<Notifier>().await;
    let uow = app.get::<Db>().await.uow(None, None).await;
    notifier
        .send_in(&uow, &TestUser::new(), DealWon::via(&ALL, "Acme renewal"))
        .await
        .unwrap();
    assert!(recorder.notifications().is_empty(), "nothing before the commit");
    uow.commit().await.unwrap();
    assert_eq!(channels(&recorder), ["broadcast", "database", "mail"]);
}

#[tokio::test]
async fn a_rolled_back_unit_of_work_reports_nothing() {
    let (app, recorder) = recorded_app().await;
    let notifier = app.get::<Notifier>().await;
    let uow = app.get::<Db>().await.uow(None, None).await;
    notifier
        .send_in(&uow, &TestUser::new(), DealWon::via(&ALL, "Acme renewal"))
        .await
        .unwrap();
    uow.rollback().await.unwrap();
    assert!(recorder.notifications().is_empty());
}
