//! Subscribing to public and private channels, and broadcasting.

use std::time::Duration;

use keel_contracts::Broadcaster;
use keel_realtime::RealtimeError;
use serde_json::json;

use crate::support::{Server, signature};

const QUIET: Duration = Duration::from_millis(200);

#[tokio::test]
async fn public_subscribe_and_receive_broadcast() {
    let server = Server::start().await;
    let mut subscriber = server.connect().await;
    let mut bystander = server.connect().await;

    subscriber.subscribe("news", None, None).await;
    let succeeded = subscriber
        .expect("pusher_internal:subscription_succeeded")
        .await;
    assert_eq!(succeeded.channel.as_deref(), Some("news"));
    assert_eq!(server.realtime.subscribers("news"), 1);

    let delivered = server
        .realtime
        .broadcast(
            ["news"],
            "story.published",
            json!({"id": 1, "title": "Hello"}),
        )
        .unwrap();
    assert_eq!(delivered, 1);
    let event = subscriber.expect("story.published").await;
    assert_eq!(event.channel.as_deref(), Some("news"));
    assert_eq!(event.data, json!({"id": 1, "title": "Hello"}));

    bystander.expect_silence(QUIET).await;
}

#[tokio::test]
async fn broadcast_reaches_every_listed_channel_and_skips_empty_ones() {
    let server = Server::start().await;
    let mut a = server.connect().await;
    let mut b = server.connect().await;
    a.subscribe("one", None, None).await;
    a.expect("pusher_internal:subscription_succeeded").await;
    b.subscribe("two", None, None).await;
    b.expect("pusher_internal:subscription_succeeded").await;

    let delivered = server
        .realtime
        .broadcast(
            vec!["one".to_owned(), "two".to_owned(), "nobody".to_owned()],
            "ping",
            "hi",
        )
        .unwrap();
    assert_eq!(delivered, 2);
    let event = a.expect("ping").await;
    assert_eq!(
        (event.channel.as_deref(), event.data),
        (Some("one"), json!("hi"))
    );
    let event = b.expect("ping").await;
    assert_eq!(
        (event.channel.as_deref(), event.data),
        (Some("two"), json!("hi"))
    );
}

#[tokio::test]
async fn unsubscribe_stops_delivery() {
    let server = Server::start().await;
    let mut client = server.connect().await;
    client.subscribe("news", None, None).await;
    client
        .expect("pusher_internal:subscription_succeeded")
        .await;

    client
        .send("pusher:unsubscribe", json!({"channel": "news"}))
        .await;
    client.assert_alive().await; // the unsubscribe has been processed
    assert_eq!(server.realtime.subscribers("news"), 0);
    let delivered = server
        .realtime
        .broadcast(["news"], "story", json!({}))
        .unwrap();
    assert_eq!(delivered, 0);
    client.expect_silence(QUIET).await;
}

#[tokio::test]
async fn private_subscribe_requires_valid_auth_signature() {
    let server = Server::start().await;
    let mut client = server.connect().await;
    let mut other = server.connect().await;
    let channel = "private-crm.deals";

    // No signature, a garbage one, one for another socket, one for another channel.
    let wrong = [
        None,
        Some("test-key:00".to_owned()),
        Some(signature(&other.socket_id, channel, None)),
        Some(signature(&client.socket_id, "private-crm.other", None)),
    ];
    for auth in wrong {
        client.subscribe(channel, auth.as_deref(), None).await;
        client.expect_error(4009).await;
    }
    assert_eq!(server.realtime.subscribers(channel), 0);
    assert_eq!(
        server
            .realtime
            .broadcast([channel], "deal.updated", json!({}))
            .unwrap(),
        0
    );

    // The right signature: subscribed and receiving.
    let auth = signature(&client.socket_id, channel, None);
    client.subscribe(channel, Some(&auth), None).await;
    let succeeded = client
        .expect("pusher_internal:subscription_succeeded")
        .await;
    assert_eq!(succeeded.channel.as_deref(), Some(channel));
    server
        .realtime
        .broadcast([channel], "deal.updated", json!({"deal": {"id": 7}}))
        .unwrap();
    let event = client.expect("deal.updated").await;
    assert_eq!(event.data["deal"]["id"], 7);

    // A signature is bound to its socket: replaying it elsewhere fails.
    other.subscribe(channel, Some(&auth), None).await;
    other.expect_error(4009).await;
}

#[tokio::test]
async fn presence_subscribe_requires_signed_channel_data() {
    let server = Server::start().await;
    let mut client = server.connect().await;
    let channel = "presence-room.1";
    let data = r#"{"user_id":"u1","user_info":{"name":"Ada"}}"#;

    // Signed without the channel data, or for other channel data.
    client
        .subscribe(
            channel,
            Some(&signature(&client.socket_id, channel, None)),
            Some(data),
        )
        .await;
    client.expect_error(4009).await;
    let forged = r#"{"user_id":"admin"}"#;
    client
        .subscribe(
            channel,
            Some(&signature(&client.socket_id, channel, Some(data))),
            Some(forged),
        )
        .await;
    client.expect_error(4009).await;
    // No channel data at all.
    client
        .subscribe(
            channel,
            Some(&signature(&client.socket_id, channel, None)),
            None,
        )
        .await;
    client.expect_error(4009).await;

    let auth = signature(&client.socket_id, channel, Some(data));
    client.subscribe(channel, Some(&auth), Some(data)).await;
    let succeeded = client
        .expect("pusher_internal:subscription_succeeded")
        .await;
    assert_eq!(
        succeeded.data,
        json!({"presence": {"ids": ["u1"], "hash": {"u1": {"name": "Ada"}}, "count": 1}})
    );
}

#[tokio::test]
async fn invalid_channel_names_and_too_many_subscriptions_are_refused() {
    let server = Server::start().await;
    let mut client = server.connect().await;

    client.subscribe("bad name", None, None).await;
    let error = client.expect("pusher:error").await;
    assert!(
        error.data["message"]
            .as_str()
            .unwrap()
            .contains("channel name")
    );
    client.send("pusher:subscribe", json!({})).await;
    client.expect("pusher:error").await;

    for i in 0..100 {
        client.subscribe(&format!("c{i}"), None, None).await;
        client
            .expect("pusher_internal:subscription_succeeded")
            .await;
    }
    client.subscribe("c100", None, None).await;
    let error = client.expect("pusher:error").await;
    assert!(
        error.data["message"]
            .as_str()
            .unwrap()
            .contains("at most 100"),
        "{error:?}"
    );
    // Re-subscribing to a channel it already has is fine.
    client.subscribe("c5", None, None).await;
    client
        .expect("pusher_internal:subscription_succeeded")
        .await;
    assert_eq!(server.realtime.subscribers("c5"), 1);
}

#[tokio::test]
async fn realtime_is_the_registered_broadcaster() {
    let server = Server::start().await;
    let mut client = server.connect().await;
    client.subscribe("news", None, None).await;
    client
        .expect("pusher_internal:subscription_succeeded")
        .await;

    let broadcaster = server
        .app
        .ctx()
        .get::<dyn Broadcaster>()
        .await
        .expect("dyn Broadcaster is registered");
    broadcaster
        .broadcast(
            vec!["news".to_owned()],
            "story".to_owned(),
            json!({"id": 2}),
        )
        .await
        .unwrap();
    let event = client.expect("story").await;
    assert_eq!(event.data, json!({"id": 2}));

    let error = broadcaster
        .broadcast(
            vec!["news".to_owned()],
            "pusher:error".to_owned(),
            json!({}),
        )
        .await
        .expect_err("reserved event names are refused");
    assert!(error.to_string().contains("pusher:error"), "{error}");
}

#[tokio::test]
async fn broadcast_rejects_reserved_and_malformed_event_names() {
    let server = Server::start().await;
    for event in [
        "",
        "pusher:pong",
        "pusher_internal:member_added",
        &"e".repeat(201),
    ] {
        let error = server
            .realtime
            .broadcast(["news"], event, json!({}))
            .unwrap_err();
        assert!(
            matches!(error, RealtimeError::InvalidEvent(_)),
            "{event}: {error}"
        );
    }
}
