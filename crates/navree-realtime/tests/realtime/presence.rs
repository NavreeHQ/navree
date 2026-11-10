//! Presence channels: member lists and member_added / member_removed.

use std::time::Duration;

use serde_json::json;
use uuid::Uuid;

use crate::support::Server;

const ROOM: &str = "presence-room.1";
const QUIET: Duration = Duration::from_millis(200);

fn name(user: Uuid) -> String {
    format!("user {}", &user.to_string()[..4])
}

#[tokio::test]
async fn presence_member_added_and_removed() {
    let server = Server::start().await;
    let (ada, bob) = (Uuid::now_v7(), Uuid::now_v7());
    let mut a = server.connect().await;
    let mut b = server.connect().await;

    let joined = server.join(&mut a, ada, ROOM).await;
    assert_eq!(
        joined.data,
        json!({"presence": {
            "ids": [ada.to_string()],
            "hash": {ada.to_string(): {"name": name(ada)}},
            "count": 1
        }})
    );

    let joined = server.join(&mut b, bob, ROOM).await;
    let presence = &joined.data["presence"];
    assert_eq!(presence["count"], 2);
    let mut ids: Vec<String> = serde_json::from_value(presence["ids"].clone()).unwrap();
    ids.sort();
    let mut expected = vec![ada.to_string(), bob.to_string()];
    expected.sort();
    assert_eq!(ids, expected);
    assert_eq!(presence["hash"][bob.to_string()]["name"], name(bob));

    let added = a.expect("pusher_internal:member_added").await;
    assert_eq!(added.channel.as_deref(), Some(ROOM));
    assert_eq!(
        added.data,
        json!({"user_id": bob.to_string(), "user_info": {"name": name(bob)}})
    );
    b.expect_silence(QUIET).await; // nobody announces bob to himself

    // Leaving by unsubscribing...
    b.send("pusher:unsubscribe", json!({"channel": ROOM})).await;
    let removed = a.expect("pusher_internal:member_removed").await;
    assert_eq!(removed.channel.as_deref(), Some(ROOM));
    assert_eq!(removed.data, json!({"user_id": bob.to_string()}));

    // ...or by disconnecting.
    server.join(&mut b, bob, ROOM).await;
    a.expect("pusher_internal:member_added").await;
    drop(b);
    let removed = a.expect("pusher_internal:member_removed").await;
    assert_eq!(removed.data, json!({"user_id": bob.to_string()}));
}

#[tokio::test]
async fn a_user_with_two_connections_is_one_member() {
    let server = Server::start().await;
    let (ada, bob) = (Uuid::now_v7(), Uuid::now_v7());
    let mut watcher = server.connect().await;
    server.join(&mut watcher, ada, ROOM).await;

    let mut first = server.connect().await;
    server.join(&mut first, bob, ROOM).await;
    watcher.expect("pusher_internal:member_added").await;

    let mut second = server.connect().await;
    let joined = server.join(&mut second, bob, ROOM).await;
    assert_eq!(joined.data["presence"]["count"], 2, "{:?}", joined.data);
    watcher.expect_silence(QUIET).await; // bob was already there

    drop(first);
    watcher.expect_silence(QUIET).await; // bob is still there
    drop(second);
    let removed = watcher.expect("pusher_internal:member_removed").await;
    assert_eq!(removed.data["user_id"], bob.to_string());
}
