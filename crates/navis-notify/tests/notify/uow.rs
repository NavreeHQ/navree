//! `Notifier::send_in`: notifying inside a unit of work.

use std::sync::Arc;
use std::time::Duration;

use navis_contracts::{Broadcaster, Dispatcher};
use navis_db::Db;
use navis_notify::{Channel, Notifier, NotifyOptions, SendNotificationJob};
use sea_orm::ConnectionTrait;

use crate::support::{self, DealWon, RecordingBroadcaster, RecordingDispatcher, TestUser};

const ALL: [Channel; 3] = [Channel::Database, Channel::Mail, Channel::Broadcast];

#[tokio::test]
async fn send_in_stores_with_the_transaction_and_delivers_after_commit() {
    let (app, broadcaster) = support::app_with_broadcaster().await;
    let notifier = app.get::<Notifier>().await;
    let db = app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    // The business change: the transaction now holds SQLite's write lock.
    let tx = uow.tx().await.unwrap();
    tx.execute_unprepared("SELECT 1").await.unwrap();
    drop(tx);

    let id = notifier
        .send_in(&uow, &TestUser::new(), DealWon::via(&ALL, "Acme renewal"))
        .await
        .unwrap();

    assert!(support::rows(&app).await.is_empty(), "not committed yet");
    assert!(support::sent_mail(&app).await.is_empty());
    assert!(broadcaster.sent().is_empty());

    uow.commit().await.unwrap();

    let rows = support::rows(&app).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, id);
    assert_eq!(support::sent_mail(&app).await.len(), 1);
    assert_eq!(broadcaster.sent().len(), 1);
}

#[tokio::test]
async fn send_in_a_rolled_back_unit_of_work_sends_nothing() {
    let (app, broadcaster) = support::app_with_broadcaster().await;
    let notifier = app.get::<Notifier>().await;
    let uow = app.get::<Db>().await.uow(None, None).await;

    notifier
        .send_in(&uow, &TestUser::new(), DealWon::via(&ALL, "Acme renewal"))
        .await
        .unwrap();
    uow.rollback().await.unwrap();

    assert!(support::rows(&app).await.is_empty());
    assert!(support::sent_mail(&app).await.is_empty());
    assert!(broadcaster.sent().is_empty());
}

#[tokio::test]
async fn send_in_queues_the_other_channels_after_commit() {
    let dispatcher = Arc::new(RecordingDispatcher::default());
    let app = support::builder(NotifyOptions {
        queue: true,
        ..NotifyOptions::default()
    })
    .override_value::<dyn Dispatcher>(dispatcher.clone())
    .override_value::<dyn Broadcaster>(Arc::new(RecordingBroadcaster::default()))
    .build()
    .await;
    let notifier = app.get::<Notifier>().await;
    let uow = app.get::<Db>().await.uow(None, None).await;

    notifier
        .send_in(&uow, &TestUser::new(), DealWon::via(&ALL, "Acme renewal"))
        .await
        .unwrap();
    assert!(dispatcher.jobs().is_empty(), "nothing queued before commit");
    uow.commit().await.unwrap();

    assert_eq!(
        support::rows(&app).await.len(),
        1,
        "stored in the transaction"
    );
    let via: Vec<Channel> = dispatcher
        .jobs()
        .into_iter()
        .map(|job| {
            serde_json::from_value::<SendNotificationJob>(job.payload)
                .unwrap()
                .delivery
                .channel()
        })
        .collect();
    assert_eq!(via, [Channel::Mail, Channel::Broadcast]);
}

/// `send` and `send_now` inside a unit of work's scope (a request handler,
/// `Db::transaction`) join it like `send_in`: on SQLite its transaction
/// holds the write lock, which a row written through another connection
/// would wait for until `busy_timeout`.
#[tokio::test]
async fn send_inside_a_unit_of_work_scope_joins_it() {
    let (app, broadcaster) = support::app_with_broadcaster().await;
    let notifier = app.get::<Notifier>().await;
    let db = app.get::<Db>().await;
    let quick = Duration::from_secs(3);

    let uow = db.uow(None, None).await;
    // BEGIN IMMEDIATE: the unit of work holds SQLite's write lock.
    drop(uow.tx().await.unwrap());
    let sent = tokio::time::timeout(
        quick,
        uow.scope(notifier.send(&TestUser::new(), DealWon::via(&ALL, "Acme renewal"))),
    )
    .await
    .expect("does not wait for the write lock")
    .unwrap();
    let sent_now = tokio::time::timeout(
        quick,
        uow.scope(notifier.send_now(&TestUser::new(), DealWon::via(&ALL, "Globex"))),
    )
    .await
    .expect("does not wait for the write lock")
    .unwrap();
    assert!(support::rows(&app).await.is_empty(), "not committed yet");
    assert!(support::sent_mail(&app).await.is_empty());
    assert!(broadcaster.sent().is_empty());

    uow.commit().await.unwrap();
    let mut ids: Vec<_> = support::rows(&app).await.iter().map(|row| row.id).collect();
    ids.sort();
    assert_eq!(ids, [sent, sent_now]);
    assert_eq!(support::sent_mail(&app).await.len(), 2);
    assert_eq!(broadcaster.sent().len(), 2);

    let rolled_back = db.uow(None, None).await;
    rolled_back
        .scope(notifier.send(&TestUser::new(), DealWon::via(&ALL, "Initech")))
        .await
        .unwrap();
    rolled_back.rollback().await.unwrap();
    assert_eq!(support::rows(&app).await.len(), 2);
    assert_eq!(support::sent_mail(&app).await.len(), 2);
    assert_eq!(broadcaster.sent().len(), 2);
}
