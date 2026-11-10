//! Broadcasting model changes on real-time channels, after the commit.

use std::sync::{Arc, Mutex};

use navree_contracts::{Broadcaster, Dispatcher, JobContext, JobEnvelope};
use navree_kernel::{BoxError, BoxFuture};
use navree_orm::broadcast::{BroadcastModelEvent, without_broadcasting};
use navree_orm::{Model, ModelLifecycle, without_events};
use serde_json::json;

use super::models::{Ping, Room, create_tables};
use crate::support::epoch;

/// The broadcasts sent, as `channels | event | data`.
#[derive(Default)]
struct FakeBroadcaster(Mutex<Vec<String>>);

impl FakeBroadcaster {
    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}

impl Broadcaster for FakeBroadcaster {
    fn broadcast(
        &self,
        channels: Vec<String>,
        event: String,
        data: serde_json::Value,
    ) -> BoxFuture<'_, Result<(), BoxError>> {
        self.0
            .lock()
            .unwrap()
            .push(format!("{} | {event} | {data}", channels.join(",")));
        Box::pin(async { Ok(()) })
    }
}

/// The jobs dispatched.
#[derive(Default)]
struct FakeDispatcher(Mutex<Vec<JobEnvelope>>);

impl Dispatcher for FakeDispatcher {
    fn dispatch_envelope(&self, envelope: JobEnvelope) -> BoxFuture<'_, Result<String, BoxError>> {
        self.0.lock().unwrap().push(envelope);
        Box::pin(async { Ok("job-1".to_owned()) })
    }
}

async fn broadcasting_app() -> (
    navree_testing::TestApp,
    Arc<navree_db::Db>,
    Arc<FakeBroadcaster>,
    Arc<FakeDispatcher>,
) {
    let broadcaster = Arc::new(FakeBroadcaster::default());
    let dispatcher = Arc::new(FakeDispatcher::default());
    let app = navree_testing::TestApp::builder()
        .temp_database()
        .override_value::<dyn navree_contracts::Clock>(Arc::new(navree_contracts::FixedClock::new(
            epoch(),
        )))
        .override_value::<dyn Broadcaster>(broadcaster.clone())
        .override_value::<dyn Dispatcher>(dispatcher.clone())
        .module(navree_db::DatabaseModule::from_config())
        .build()
        .await;
    let db = app.get::<navree_db::Db>().await;
    create_tables(&db).await;
    (app, db, broadcaster, dispatcher)
}

#[tokio::test]
async fn changes_go_out_on_the_records_channel_after_the_commit() {
    let (_app, db, broadcaster, _dispatcher) = broadcasting_app().await;

    let uow = db.uow(None, None).await;
    let mut room = Room::create(
        &uow,
        Room {
            name: "general".into(),
            ..Room::default()
        },
    )
    .await
    .unwrap();
    assert!(broadcaster.take().is_empty(), "nothing before the commit");
    uow.commit().await.unwrap();
    let sent = broadcaster.take();
    assert_eq!(sent.len(), 1);
    let parts: Vec<&str> = sent[0].splitn(3, " | ").collect();
    assert_eq!(parts[..2], ["private-room.1", "room.created"]);
    // Records serialize their date-times as RFC 3339 in UTC with
    // microseconds (the attribute layer's format).
    let at = epoch().to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(parts[2]).unwrap(),
        json!({"room": {"id": 1, "name": "general", "created_at": at, "updated_at": at, "deleted_at": null}})
    );

    room.name = "random".into();
    room.save(&*db).await.unwrap();
    room.delete(&*db).await.unwrap();
    room.restore(&*db).await.unwrap();
    room.force_delete(&*db).await.unwrap();
    let events: Vec<String> = broadcaster
        .take()
        .iter()
        .map(|line| line.split(" | ").nth(1).unwrap().to_owned())
        .collect();
    assert_eq!(
        events,
        [
            "room.updated",
            "room.trashed",
            "room.deleted",
            "room.updated",
            "room.restored",
            "room.deleted"
        ]
    );
}

#[tokio::test]
async fn broadcasts_can_be_silenced() {
    let (_app, db, broadcaster, _dispatcher) = broadcasting_app().await;
    without_broadcasting(Room::create(
        &*db,
        Room {
            name: "quiet".into(),
            ..Room::default()
        },
    ))
    .await
    .unwrap();
    without_events(Room::create(
        &*db,
        Room {
            name: "quieter".into(),
            ..Room::default()
        },
    ))
    .await
    .unwrap();
    Room::create_quietly(
        &*db,
        Room {
            name: "quietest".into(),
            ..Room::default()
        },
    )
    .await
    .unwrap();
    assert!(broadcaster.take().is_empty());
}

#[tokio::test]
async fn models_choose_channels_names_payloads_and_a_queue() {
    let (app, db, broadcaster, dispatcher) = broadcasting_app().await;
    Ping::create(
        &*db,
        Ping {
            target: "ops".into(),
            ..Ping::default()
        },
    )
    .await
    .unwrap();
    Ping::create(
        &*db,
        Ping {
            target: "nobody".into(),
            ..Ping::default()
        },
    )
    .await
    .unwrap();
    assert!(broadcaster.take().is_empty(), "queued, not sent");
    let jobs = std::mem::take(&mut *dispatcher.0.lock().unwrap());
    assert_eq!(jobs.len(), 1, "no channel, no broadcast");
    let job = &jobs[0];
    assert_eq!(job.job_type, "orm.broadcast");
    assert_eq!(job.options.queue, "broadcasts");
    assert_eq!(
        job.payload,
        json!({"channels": ["private-pings.ops"], "event": "ping.created", "data": {"target": "ops"}})
    );

    // The worker side.
    let payload: BroadcastModelEvent = serde_json::from_value(job.payload.clone()).unwrap();
    navree_contracts::Job::handle(
        payload,
        JobContext {
            app: app.ctx().clone(),
            job_id: "job-1".into(),
            attempt: 1,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        broadcaster.take(),
        [r#"private-pings.ops | ping.created | {"target":"ops"}"#]
    );
}

#[tokio::test]
async fn without_a_broadcaster_nothing_is_sent_and_nothing_fails() {
    let (_app, db, _clock) = crate::support::app(Vec::new()).await;
    create_tables(&db).await;
    let room = Room::create(
        &*db,
        Room {
            name: "alone".into(),
            ..Room::default()
        },
    )
    .await
    .unwrap();
    assert!(room.exists());
}
