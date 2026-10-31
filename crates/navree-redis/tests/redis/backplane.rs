//! The real-time backplane on a real Redis server (`NAVREE_TEST_REDIS_URL`):
//! two apps (two `serve` processes) on one server deliver to each other's
//! sockets, and their presence lists agree.

use std::future::IntoFuture;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use navree_contracts::Secret;
use navree_http::{Authenticator, HttpModule, HttpOptions, build_router};
use navree_kernel::{App, Navree};
use navree_realtime::{
    Backplane, BackplaneHandler, BackplaneKind, Realtime, RealtimeModule, RealtimeOptions,
};
use navree_redis::{RedisBackplane, RedisModule};
use serde_json::{Value, json};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::support::{NoAuthenticator, TestRedis};

const KEY: &str = "test-key";
const ROOM: &str = "presence-room.1";
const WAIT: Duration = Duration::from_secs(5);

#[tokio::test]
async fn messages_reach_every_subscriber_of_the_topic() {
    let Some(t) = TestRedis::connect("messages_reach_every_subscriber_of_the_topic").await else {
        return;
    };
    let shutdown = CancellationToken::new();
    let tracker = TaskTracker::new();
    let backplane = |_: ()| {
        RedisBackplane::new(
            t.redis.connection().clone(),
            &t.prefix,
            shutdown.clone(),
            tracker.clone(),
        )
    };
    let (sender, receiver) = (backplane(()), backplane(()));
    assert_eq!(
        sender.channel("realtime:crm"),
        format!("{}realtime:crm", t.prefix)
    );

    let received = Arc::new(Mutex::new(Vec::new()));
    let sink = received.clone();
    let handler: BackplaneHandler = Arc::new(move |message: Bytes| {
        sink.lock().unwrap().push(message);
    });
    receiver
        .subscribe("realtime:crm", handler, shutdown.clone())
        .await
        .unwrap();
    for i in 0..5 {
        sender.publish("realtime:crm", Bytes::from(format!("m{i}")));
    }
    sender.publish("realtime:other", Bytes::from_static(b"elsewhere"));
    for _ in 0..200 {
        if received.lock().unwrap().len() >= 5 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    let received: Vec<Bytes> = received.lock().unwrap().clone();
    assert_eq!(
        received,
        (0..5)
            .map(|i| Bytes::from(format!("m{i}")))
            .collect::<Vec<_>>(),
        "in order, and only the topic's"
    );
    shutdown.cancel();
    tracker.close();
    tracker.wait().await;
}

/// An app with the realtime module on the Redis backplane, served on
/// 127.0.0.1:0.
struct Node {
    app: App,
    addr: SocketAddr,
    realtime: Arc<Realtime>,
}

async fn node(t: &TestRedis) -> Node {
    let app = Navree::app()
        .module(RedisModule::for_root(t.options()))
        .module(HttpModule::for_root(HttpOptions {
            per_ip_rate_limit: None,
            ..HttpOptions::default()
        }))
        .module(RealtimeModule::for_root(RealtimeOptions {
            app_id: "test".to_owned(),
            key: KEY.to_owned(),
            secret: Secret::new("shared-secret".to_owned()),
            backplane: BackplaneKind::Redis,
            ..RealtimeOptions::default()
        }))
        .override_value::<dyn Authenticator>(Arc::new(NoAuthenticator))
        .boot()
        .await
        .expect("app boots");
    let router = build_router(app.ctx()).await.expect("router builds");
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(
        navree_http::serve(listener, router)
            .with_graceful_shutdown(app.ctx().shutdown_token().cancelled_owned())
            .into_future(),
    );
    let realtime = app.ctx().get::<Realtime>().await.unwrap();
    Node {
        app,
        addr,
        realtime,
    }
}

/// A Pusher client.
struct Client {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
    socket_id: String,
}

impl Client {
    async fn connect(node: &Node) -> Client {
        let url = format!("ws://{}/app/{KEY}?protocol=7", node.addr);
        let (ws, _) = tokio_tungstenite::connect_async(url)
            .await
            .expect("handshake");
        let mut client = Client {
            ws,
            socket_id: String::new(),
        };
        let (_, _, data) = client.expect("pusher:connection_established").await;
        client.socket_id = data["socket_id"].as_str().unwrap().to_owned();
        client
    }

    async fn send(&mut self, event: &str, data: Value) {
        let frame = json!({ "event": event, "data": data }).to_string();
        self.ws.send(Message::text(frame)).await.unwrap();
    }

    /// The next frame: (event, channel, data).
    async fn recv(&mut self) -> (String, Option<String>, Value) {
        let text = tokio::time::timeout(WAIT, async {
            loop {
                match self.ws.next().await {
                    Some(Ok(Message::Text(text))) => return text.to_string(),
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
                    other => panic!("expected a text frame, got {other:?}"),
                }
            }
        })
        .await
        .expect("a frame in time");
        let frame: Value = serde_json::from_str(&text).unwrap();
        let data = match &frame["data"] {
            Value::String(encoded) => serde_json::from_str(encoded).unwrap_or(Value::Null),
            other => other.clone(),
        };
        (
            frame["event"].as_str().unwrap().to_owned(),
            frame["channel"].as_str().map(str::to_owned),
            data,
        )
    }

    async fn expect(&mut self, event: &str) -> (String, Option<String>, Value) {
        let frame = self.recv().await;
        assert_eq!(frame.0, event, "{frame:?}");
        frame
    }

    /// Joins presence channel `ROOM` as `user`, signed by `node`.
    async fn join(&mut self, node: &Node, user: &str) -> Value {
        let channel_data = json!({ "user_id": user, "user_info": { "name": user } }).to_string();
        let auth = node
            .realtime
            .sign(&self.socket_id, ROOM, Some(&channel_data));
        self.send(
            "pusher:subscribe",
            json!({ "channel": ROOM, "auth": auth, "channel_data": channel_data }),
        )
        .await;
        self.expect("pusher_internal:subscription_succeeded")
            .await
            .2
    }
}

async fn members_become(realtime: &Realtime, expected: &[&str]) {
    for _ in 0..300 {
        if realtime.members(ROOM) == expected {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(realtime.members(ROOM), expected);
}

#[tokio::test]
async fn two_processes_share_broadcasts_and_presence() {
    let Some(t) = TestRedis::connect("two_processes_share_broadcasts_and_presence").await else {
        return;
    };
    let (a, b) = (node(&t).await, node(&t).await);

    let mut on_b = Client::connect(&b).await;
    on_b.send("pusher:subscribe", json!({ "channel": "news" }))
        .await;
    on_b.expect("pusher_internal:subscription_succeeded").await;
    assert_eq!(
        a.realtime
            .broadcast(["news"], "posted", json!({ "id": 1 }))
            .unwrap(),
        0
    );
    let (_, channel, data) = on_b.expect("posted").await;
    assert_eq!(
        (channel.as_deref(), data),
        (Some("news"), json!({ "id": 1 }))
    );

    let mut ada = Client::connect(&a).await;
    ada.join(&a, "ada").await;
    members_become(&b.realtime, &["ada"]).await;
    let mut bob = Client::connect(&b).await;
    let joined = bob.join(&b, "bob").await;
    assert_eq!(joined["presence"]["count"], 2, "{joined}");
    let (_, _, added) = ada.expect("pusher_internal:member_added").await;
    assert_eq!(added["user_id"], "bob");
    members_become(&a.realtime, &["ada", "bob"]).await;
    assert_eq!(a.realtime.members(ROOM), b.realtime.members(ROOM));

    drop(bob);
    let (_, _, removed) = ada.expect("pusher_internal:member_removed").await;
    assert_eq!(removed, json!({ "user_id": "bob" }));
    members_become(&a.realtime, &["ada"]).await;

    // A process that shuts down takes its members with it.
    let mut cy = Client::connect(&b).await;
    cy.join(&b, "cy").await;
    ada.expect("pusher_internal:member_added").await;
    b.app.shutdown().await.unwrap();
    let (_, _, removed) = ada.expect("pusher_internal:member_removed").await;
    assert_eq!(removed, json!({ "user_id": "cy" }));
    a.app.shutdown().await.unwrap();
    t.cleanup().await;
}
