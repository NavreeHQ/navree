//! Disconnecting users, graceful shutdown and module wiring.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use navis_config::{ConfigModule, ConfigSources};
use navis_contracts::Principal;
use navis_http::{Authenticator, HttpModule, HttpOptions};
use navis_kernel::{AppContext, BoxError, BoxFuture, Module, ModuleBuilder, Navis};
use navis_realtime::{ChannelDecision, ModuleBuilderRealtimeExt, RealtimeModule, RealtimeOptions};
use serde_json::json;
use tokio::time::Instant;
use uuid::Uuid;

use crate::support::{ChannelsModule, HeaderAuthenticator, KEY, ORIGIN, Server, app, options};

#[tokio::test]
async fn disconnect_user_closes_sockets() {
    let server = Server::start().await;
    let (ada, bob) = (Uuid::now_v7(), Uuid::now_v7());

    // Ada's two connections: one signed into a private channel through the
    // auth endpoint, one that joined a presence channel.
    let mut first = server.connect().await;
    server.join(&mut first, ada, "private-crm.deals").await;
    let mut second = server.connect().await;
    server.join(&mut second, ada, "presence-room.1").await;
    // Bob's connection, in the same presence room.
    let mut other = server.connect().await;
    server.join(&mut other, bob, "presence-room.1").await;
    second.expect("pusher_internal:member_added").await;
    // An anonymous connection.
    let mut anonymous = server.connect().await;

    assert_eq!(server.realtime.disconnect_user(ada), 2);
    for client in [&mut first, &mut second] {
        let message = client.expect_error(4009).await;
        assert!(message.contains("disconnected"), "{message}");
        assert_eq!(client.expect_close().await, 4009);
    }
    // Bob sees Ada leave the room; he and the anonymous client stay connected.
    let removed = other.expect("pusher_internal:member_removed").await;
    assert_eq!(removed.data["user_id"], ada.to_string());
    other.assert_alive().await;
    anonymous.assert_alive().await;
    assert_eq!(server.realtime.connections(), 2);
    assert_eq!(server.realtime.subscribers("private-crm.deals"), 0);

    assert_eq!(server.realtime.disconnect_user(Uuid::now_v7()), 0);
}

#[tokio::test]
async fn an_authenticated_handshake_belongs_to_its_user() {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let server = Server::start().await;
    let user = Uuid::now_v7();
    let mut request = server.url_for(KEY).into_client_request().unwrap();
    request
        .headers_mut()
        .insert("x-user-id", user.to_string().parse().unwrap());
    let (ws, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    let mut client = crate::support::Client {
        ws,
        socket_id: String::new(),
    };
    client.expect("pusher:connection_established").await;

    assert_eq!(server.realtime.disconnect_user(user), 1);
    client.expect_error(4009).await;
}

#[tokio::test]
async fn graceful_shutdown_closes_sockets_with_reconnect_code() {
    let server = Server::start().await;
    let mut client = server.connect().await;
    client.subscribe("news", None, None).await;
    client
        .expect("pusher_internal:subscription_succeeded")
        .await;
    let closing = tokio::spawn(async move {
        let message = client.expect_error(4200).await;
        (message, client.expect_close().await)
    });

    let Server { app, realtime, .. } = server;
    let started = Instant::now();
    tokio::time::timeout(Duration::from_secs(5), app.shutdown())
        .await
        .expect("shutdown in time")
        .expect("clean shutdown");
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(
        realtime.connections(),
        0,
        "shutdown waited for the connection"
    );

    let (message, code) = closing.await.unwrap();
    assert!(message.contains("shutting down"), "{message}");
    assert_eq!(code, 4200, "clients reconnect right away");
}

fn allow(
    _principal: &Principal,
    _params: &HashMap<String, String>,
    _ctx: &AppContext,
) -> BoxFuture<'static, Result<ChannelDecision, BoxError>> {
    Box::pin(async { Ok(ChannelDecision::Allow) })
}

/// Registers `crm.deals` again, and a malformed pattern.
struct ConflictingModule;

impl Module for ConflictingModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.channel("crm.deals", allow).channel("users.{id", allow);
    }
}

#[tokio::test]
async fn duplicate_and_malformed_channel_patterns_fail_the_boot() {
    let error = app(options())
        .module(ConflictingModule)
        .boot()
        .await
        .expect_err("boot must fail")
        .to_string();
    assert!(error.contains("`crm.deals` is registered twice"), "{error}");
    assert!(
        error.contains("ChannelsModule") && error.contains("ConflictingModule"),
        "{error}"
    );
    assert!(error.contains("users.{id"), "{error}");
}

#[tokio::test]
async fn invalid_options_fail_the_boot() {
    let error = app(RealtimeOptions {
        key: "has space".to_owned(),
        activity_timeout_secs: 0,
        ..options()
    })
    .boot()
    .await
    .expect_err("boot must fail")
    .to_string();
    assert!(error.contains("realtime.key"), "{error}");
    assert!(error.contains("realtime.activity_timeout_secs"), "{error}");
}

#[tokio::test]
async fn from_config_reads_the_realtime_section() {
    let sources = ConfigSources::new()
        .set(
            "realtime",
            json!({"key": "configured-key", "secret": "configured-secret", "activity_timeout_secs": 45}),
        )
        .env_vars("APP_", [("APP_REALTIME__ALLOWED_ORIGINS", ORIGIN)]);
    let server = Server::serve(
        Navis::app()
            .module(ConfigModule::for_root(sources))
            .module(HttpModule::for_root(HttpOptions {
                per_ip_rate_limit: None,
                ..HttpOptions::default()
            }))
            .module(RealtimeModule::from_config())
            .module(ChannelsModule)
            .override_value::<dyn Authenticator>(Arc::new(HeaderAuthenticator)),
    )
    .await;
    assert_eq!(server.realtime.options().key, "configured-key");

    let mut client = server.open("configured-key", Some(ORIGIN)).await;
    let established = client.expect("pusher:connection_established").await;
    assert_eq!(established.data["activity_timeout"], 45);
    let socket_id = established.data["socket_id"].as_str().unwrap().to_owned();
    let mut evil = server
        .open("configured-key", Some("http://evil.example"))
        .await;
    evil.expect_error(4009).await;

    // Signatures use the configured key and secret.
    let message = format!("{socket_id}:private-crm.deals");
    let mac = navis_crypto::hmac_sha256(b"configured-secret", message.as_bytes());
    let expected = format!("configured-key:{}", hex::encode(mac));
    assert_eq!(
        server.realtime.sign(&socket_id, "private-crm.deals", None),
        expected
    );
    client
        .subscribe("private-crm.deals", Some(&expected), None)
        .await;
    client
        .expect("pusher_internal:subscription_succeeded")
        .await;
}

#[tokio::test]
async fn without_a_secret_signatures_still_round_trip() {
    let server = Server::start_with(RealtimeOptions {
        secret: Default::default(),
        ..options()
    })
    .await;
    let user = Uuid::now_v7();
    let mut client = server.connect().await;
    server.join(&mut client, user, "private-crm.deals").await;
    assert_ne!(
        server
            .realtime
            .sign(&client.socket_id, "private-crm.deals", None),
        crate::support::signature(&client.socket_id, "private-crm.deals", None),
        "a random secret, not an empty one"
    );
}
