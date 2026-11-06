//! Queued sending: one `notify.send` job per channel, delivered by a worker.

use std::sync::Arc;

use navree_contracts::{Broadcaster, Dispatcher};
use navree_notify::{Channel, Notifier, NotifyOptions, SendNotificationJob};
use navree_testing::TestApp;
use serde_json::Value;

use crate::support::{self, DealWon, RecordingBroadcaster, RecordingDispatcher, TestUser};

fn queued() -> NotifyOptions {
    NotifyOptions {
        queue: true,
        ..NotifyOptions::default()
    }
}

/// A queueing app with a recording dispatcher and broadcaster.
async fn queued_app() -> (TestApp, Arc<RecordingDispatcher>, Arc<RecordingBroadcaster>) {
    let dispatcher = Arc::new(RecordingDispatcher::default());
    let broadcaster = Arc::new(RecordingBroadcaster::default());
    let app = support::builder(queued())
        .override_value::<dyn Dispatcher>(dispatcher.clone())
        .override_value::<dyn Broadcaster>(broadcaster.clone())
        .build()
        .await;
    (app, dispatcher, broadcaster)
}

const ALL: [Channel; 3] = [Channel::Database, Channel::Mail, Channel::Broadcast];

#[tokio::test]
async fn queued_send_dispatches_job_and_job_delivers() {
    let (app, dispatcher, broadcaster) = queued_app().await;
    let notifier = app.get::<Notifier>().await;
    let user = TestUser::new();

    let id = notifier
        .send(&user, DealWon::via(&ALL, "Agra renewal"))
        .await
        .unwrap();

    assert!(
        support::rows(&app).await.is_empty(),
        "nothing delivered yet"
    );
    assert!(support::sent_mail(&app).await.is_empty());
    assert!(broadcaster.sent().is_empty());
    let jobs = dispatcher.jobs();
    assert_eq!(jobs.len(), 3, "one job per channel");
    let mut via = Vec::new();
    for job in &jobs {
        assert_eq!(job.job_type, "notify.send");
        let payload: SendNotificationJob = serde_json::from_value(job.payload.clone()).unwrap();
        assert_eq!(payload.id, id);
        assert_eq!(payload.notification_type, "deal_won");
        assert_eq!(payload.notifiable_type, "user");
        assert_eq!(payload.notifiable_id, user.id.to_string());
        via.push(payload.delivery.channel());
    }
    assert_eq!(via, ALL);
    assert_eq!(jobs[1].payload["delivery"]["via"], Value::from("mail"));

    for job in &jobs {
        support::run_job(&app, job).await.unwrap();
    }

    let rows = support::rows(&app).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, id);
    let mail = support::sent_mail(&app).await;
    assert_eq!(mail.len(), 1);
    assert_eq!(mail[0].to, vec![user.email.clone().unwrap()]);
    let broadcasts = broadcaster.sent();
    assert_eq!(broadcasts.len(), 1);
    assert_eq!(broadcasts[0].2["id"], Value::from(id.to_string()));
}

#[tokio::test]
async fn a_retried_database_job_stores_the_notification_once() {
    let (app, dispatcher, _broadcaster) = queued_app().await;
    let notifier = app.get::<Notifier>().await;

    notifier
        .send(&TestUser::new(), DealWon::via(&[Channel::Database], "Agra"))
        .await
        .unwrap();
    let job = dispatcher.jobs().remove(0);
    support::run_job(&app, &job).await.unwrap();
    support::run_job(&app, &job).await.unwrap();

    assert_eq!(support::rows(&app).await.len(), 1);
}

#[tokio::test]
async fn send_now_delivers_even_when_queueing() {
    let (app, dispatcher, broadcaster) = queued_app().await;
    let notifier = app.get::<Notifier>().await;

    notifier
        .send_now(&TestUser::new(), DealWon::via(&ALL, "Agra renewal"))
        .await
        .unwrap();

    assert!(dispatcher.jobs().is_empty());
    assert_eq!(support::rows(&app).await.len(), 1);
    assert_eq!(support::sent_mail(&app).await.len(), 1);
    assert_eq!(broadcaster.sent().len(), 1);
}

#[tokio::test]
async fn queueing_without_a_dispatcher_delivers_now() {
    let app = support::builder(queued()).build().await;
    let notifier = app.get::<Notifier>().await;

    notifier
        .send(
            &TestUser::new(),
            DealWon::via(&[Channel::Database, Channel::Mail], "Agra renewal"),
        )
        .await
        .unwrap();

    assert_eq!(support::rows(&app).await.len(), 1);
    assert_eq!(support::sent_mail(&app).await.len(), 1);
}
