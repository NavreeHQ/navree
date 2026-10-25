//! Connecting: the handshake, origin and app key checks, ping/pong.

use std::sync::Arc;
use std::time::Duration;

use keel_http::{Authenticator, HttpModule, HttpOptions};
use keel_kernel::Keel;
use keel_realtime::{RealtimeModule, RealtimeOptions};
use serde_json::json;
use tokio::time::Instant;

use crate::support::{HeaderAuthenticator, KEY, ORIGIN, Server, options};

#[tokio::test]
async fn connect_receives_connection_established() {
    let server = Server::start().await;
    let mut client = server.open(KEY, Some(ORIGIN)).await;

    let established = client.expect("pusher:connection_established").await;
    assert_eq!(established.channel, None);
    let socket_id = established.data["socket_id"].as_str().expect("socket_id");
    let (a, b) = socket_id.split_once('.').expect("<digits>.<digits>");
    assert!(
        !a.is_empty() && !b.is_empty() && (a.to_owned() + b).bytes().all(|c| c.is_ascii_digit()),
        "{socket_id}"
    );
    assert_eq!(established.data["activity_timeout"], 120);

    let other = server.connect().await;
    assert_ne!(other.socket_id, socket_id, "socket ids are unique");
    assert_eq!(server.realtime.connections(), 2);
}

#[tokio::test]
async fn ping_pong() {
    let server = Server::start().await;
    let mut client = server.connect().await;

    client.send("pusher:ping", json!({})).await;
    let pong = client.expect("pusher:pong").await;
    assert_eq!(pong.channel, None);
    assert_eq!(pong.data, json!({}));

    // Another ping on the same connection works too.
    client.assert_alive().await;
}

#[tokio::test]
async fn rejects_disallowed_origin() {
    let server = Server::start().await;

    let mut client = server.open(KEY, Some("http://evil.example")).await;
    let message = client.expect_error(4009).await;
    assert!(message.contains("Origin"), "{message}");
    assert_eq!(client.expect_close().await, 4009);
    assert_eq!(server.realtime.connections(), 0, "never registered");

    // The allowed origin, the server's own origin and non-browser clients
    // (no Origin header) connect.
    let own_origin = format!("http://{}", server.addr);
    for origin in [Some(ORIGIN), Some(own_origin.as_str()), None] {
        let mut client = server.open(KEY, origin).await;
        client.expect("pusher:connection_established").await;
    }
}

#[tokio::test]
async fn allowed_origins_default_to_the_cors_origins() {
    let mut http = HttpOptions {
        per_ip_rate_limit: None,
        ..HttpOptions::default()
    };
    http.cors.allowed_origins = vec!["http://cors.example".to_owned()];
    let server = Server::serve(
        Keel::app()
            .module(HttpModule::for_root(http))
            .module(RealtimeModule::for_root(RealtimeOptions {
                allowed_origins: Vec::new(),
                ..options()
            }))
            .override_value::<dyn Authenticator>(Arc::new(HeaderAuthenticator)),
    )
    .await;

    let mut client = server.open(KEY, Some("http://cors.example")).await;
    client.expect("pusher:connection_established").await;
    let mut client = server.open(KEY, Some(ORIGIN)).await;
    client.expect_error(4009).await;
}

#[tokio::test]
async fn unknown_app_key_is_rejected_with_4001() {
    let server = Server::start().await;
    let mut client = server.open("not-the-key", Some(ORIGIN)).await;
    let message = client.expect_error(4001).await;
    assert!(message.contains("Application does not exist"), "{message}");
    assert_eq!(client.expect_close().await, 4001);
}

#[tokio::test]
async fn malformed_messages_get_an_error_and_keep_the_connection() {
    let server = Server::start().await;
    let mut client = server.connect().await;

    client.send_text("not json".to_owned()).await;
    let error = client.expect("pusher:error").await;
    assert_eq!(error.data["code"], serde_json::Value::Null);

    // Unsupported client events are ignored.
    client
        .send("client-typing", json!({"channel": "news"}))
        .await;
    client.assert_alive().await;
}

#[tokio::test]
async fn idle_connection_is_pinged_then_closed() {
    let server = Server::start_with(RealtimeOptions {
        activity_timeout_secs: 1,
        ..options()
    })
    .await;
    let mut client = server.connect().await;

    // Silent for the activity timeout: the server pings.
    let started = Instant::now();
    client.expect("pusher:ping").await;
    assert!(
        started.elapsed() >= Duration::from_millis(900),
        "{:?}",
        started.elapsed()
    );

    // Answering keeps the connection open past another timeout...
    client.send("pusher:pong", json!({})).await;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    client.expect("pusher:ping").await;

    // ...staying silent gets it closed with 4201 (reconnect).
    client.expect_error(4201).await;
    assert_eq!(client.expect_close().await, 4201);
}
