//! Several hubs on one backplane (two `serve` processes behind a load
//! balancer, here two apps sharing a `MemoryBackplane`): broadcasts,
//! presence and user disconnections reach every process.

use std::sync::Arc;
use std::time::Duration;

use navree_contracts::Broadcaster;
use navree_realtime::{Backplane, BackplaneKind, MemoryBackplane, Realtime, RealtimeOptions};
use serde_json::json;
use uuid::Uuid;

use crate::support::{Server, app, options};

const ROOM: &str = "presence-room.7";
const QUIET: Duration = Duration::from_millis(200);

/// Serves the test app on `backplane`.
async fn node(backplane: &MemoryBackplane) -> Server {
    Server::serve(app(options()).override_value::<dyn Backplane>(Arc::new(backplane.clone()))).await
}

/// Waits until `realtime` lists exactly `expected` on `channel`.
async fn members_become(realtime: &Realtime, channel: &str, expected: &[Uuid]) {
    let mut expected: Vec<String> = expected.iter().map(Uuid::to_string).collect();
    expected.sort();
    for _ in 0..200 {
        if realtime.members(channel) == expected {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(realtime.members(channel), expected, "members of {channel}");
}

#[tokio::test]
async fn broadcasts_reach_the_sockets_of_every_process() {
    let backplane = MemoryBackplane::new();
    let (a, b) = (node(&backplane).await, node(&backplane).await);
    assert!(a.realtime.has_backplane() && b.realtime.has_backplane());
    assert_eq!(backplane.subscribers("realtime:test"), 2);

    let mut on_a = a.connect().await;
    on_a.subscribe("news", None, None).await;
    on_a.expect("pusher_internal:subscription_succeeded").await;
    let mut on_b = b.connect().await;
    on_b.subscribe("news", None, None).await;
    on_b.expect("pusher_internal:subscription_succeeded").await;

    let local = a
        .realtime
        .broadcast(["news"], "posted", json!({ "id": 1 }))
        .unwrap();
    assert_eq!(local, 1, "the count is this process's deliveries");
    for client in [&mut on_a, &mut on_b] {
        let event = client.expect("posted").await;
        assert_eq!(event.channel.as_deref(), Some("news"));
        assert_eq!(event.data, json!({ "id": 1 }));
    }

    // `dyn Broadcaster` (what workers and notifications use) goes through
    // the backplane too, never twice to the same socket.
    let broadcaster: Arc<dyn Broadcaster> = b.realtime.clone();
    broadcaster
        .broadcast(
            vec!["news".to_owned()],
            "edited".to_owned(),
            json!({ "id": 2 }),
        )
        .await
        .unwrap();
    for client in [&mut on_a, &mut on_b] {
        assert_eq!(client.expect("edited").await.data, json!({ "id": 2 }));
        client.expect_silence(QUIET).await;
    }
}

#[tokio::test]
async fn presence_lists_agree_across_processes() {
    let backplane = MemoryBackplane::new();
    let (a, b) = (node(&backplane).await, node(&backplane).await);
    let (ada, bob) = (Uuid::now_v7(), Uuid::now_v7());

    let mut ada_on_a = a.connect().await;
    a.join(&mut ada_on_a, ada, ROOM).await;
    members_become(&b.realtime, ROOM, &[ada]).await;

    // Bob joins on the other process and sees Ada, who sees him arrive.
    let mut bob_on_b = b.connect().await;
    let joined = b.join(&mut bob_on_b, bob, ROOM).await;
    assert_eq!(joined.data["presence"]["count"], 2, "{:?}", joined.data);
    assert_eq!(
        joined.data["presence"]["hash"][ada.to_string()]["name"],
        format!("user {}", &ada.to_string()[..4])
    );
    let added = ada_on_a.expect("pusher_internal:member_added").await;
    assert_eq!(added.data["user_id"], bob.to_string());
    members_become(&a.realtime, ROOM, &[ada, bob]).await;
    assert_eq!(a.realtime.members(ROOM), b.realtime.members(ROOM));

    // A second connection of Ada, on B, is not a new member anywhere.
    let mut ada_on_b = b.connect().await;
    b.join(&mut ada_on_b, ada, ROOM).await;
    bob_on_b.expect_silence(QUIET).await;
    // Her first connection leaving does not remove her: she is still on B.
    drop(ada_on_a);
    bob_on_b.expect_silence(QUIET).await;
    members_become(&a.realtime, ROOM, &[ada, bob]).await;

    // Bob leaving B is announced to Ada's remaining connection.
    drop(bob_on_b);
    let removed = ada_on_b.expect("pusher_internal:member_removed").await;
    assert_eq!(removed.data, json!({ "user_id": bob.to_string() }));
    members_become(&a.realtime, ROOM, &[ada]).await;
}

#[tokio::test]
async fn a_process_that_starts_later_learns_the_members() {
    let backplane = MemoryBackplane::new();
    let a = node(&backplane).await;
    let ada = Uuid::now_v7();
    let mut client = a.connect().await;
    a.join(&mut client, ada, ROOM).await;

    let late = node(&backplane).await;
    members_become(&late.realtime, ROOM, &[ada]).await;
}

#[tokio::test]
async fn a_process_that_shuts_down_takes_its_members_with_it() {
    let backplane = MemoryBackplane::new();
    let (a, b) = (node(&backplane).await, node(&backplane).await);
    let (ada, bob) = (Uuid::now_v7(), Uuid::now_v7());
    let mut ada_on_a = a.connect().await;
    a.join(&mut ada_on_a, ada, ROOM).await;
    let mut bob_on_b = b.connect().await;
    b.join(&mut bob_on_b, bob, ROOM).await;
    ada_on_a.expect("pusher_internal:member_added").await;

    b.app.shutdown().await.unwrap();
    let removed = ada_on_a.expect("pusher_internal:member_removed").await;
    assert_eq!(removed.data, json!({ "user_id": bob.to_string() }));
    members_become(&a.realtime, ROOM, &[ada]).await;
}

#[tokio::test]
async fn disconnecting_a_user_closes_their_sockets_everywhere() {
    let backplane = MemoryBackplane::new();
    let (a, b) = (node(&backplane).await, node(&backplane).await);
    let (ada, bob) = (Uuid::now_v7(), Uuid::now_v7());
    let mut ada_on_b = b.connect().await;
    b.join(&mut ada_on_b, ada, &format!("private-users.{ada}"))
        .await;
    let mut other_on_b = b.connect().await;
    b.join(&mut other_on_b, bob, "private-crm.deals").await;

    assert_eq!(a.realtime.disconnect_user(ada), 0, "no socket of Ada on A");
    ada_on_b.expect_error(4009).await;
    assert_eq!(ada_on_b.expect_close().await, 4009);
    other_on_b.assert_alive().await;

    let mut bob_again = b.connect().await;
    b.join(&mut bob_again, bob, "private-crm.deals").await;
    let broadcaster: Arc<dyn Broadcaster> = a.realtime.clone();
    broadcaster.end_user_subscriptions(bob).await.unwrap();
    for client in [&mut other_on_b, &mut bob_again] {
        client.expect_error(4200).await;
        assert_eq!(client.expect_close().await, 4200);
    }
}

#[tokio::test]
async fn a_backplane_needs_a_shared_secret_and_its_provider() {
    let without_secret = app(RealtimeOptions {
        secret: Default::default(),
        backplane: BackplaneKind::Redis,
        ..options()
    })
    .boot()
    .await
    .expect_err("no secret");
    assert!(
        without_secret.to_string().contains("needs realtime.secret"),
        "{without_secret}"
    );

    let without_provider = app(RealtimeOptions {
        backplane: BackplaneKind::Redis,
        ..options()
    })
    .boot()
    .await
    .expect_err("no provider named redis");
    let message = without_provider.to_string();
    assert!(
        message.contains("realtime.backplane = \"redis\"") && message.contains("RedisModule"),
        "{message}"
    );

    let alone = Server::start().await;
    assert!(!alone.realtime.has_backplane());
}
