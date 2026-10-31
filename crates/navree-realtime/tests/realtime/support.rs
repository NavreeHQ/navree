//! Helpers shared by the navree-realtime integration tests.

#![allow(dead_code)]

use std::collections::HashMap;
use std::future::IntoFuture;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use futures_util::{SinkExt, StreamExt};
use http::{Request, StatusCode};
use http_body_util::BodyExt;
use navree_contracts::{AuthVia, Principal, Secret};
use navree_http::body::Body;
use navree_http::{AppError, Authenticator, HttpModule, HttpOptions, build_router};
use navree_kernel::{
    App, AppBuilder, AppContext, BoxError, BoxFuture, Module, ModuleBuilder, Navree,
};
use navree_realtime::{
    ChannelDecision, ModuleBuilderRealtimeExt, Realtime, RealtimeModule, RealtimeOptions,
};
use serde_json::{Value, json};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::{self, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tower::ServiceExt;
use uuid::Uuid;

/// The app key of the test server.
pub const KEY: &str = "test-key";
/// The signing secret of the test server.
pub const SECRET: &str = "test-secret";
/// The origin the test server allows.
pub const ORIGIN: &str = "http://localhost:3000";
/// How long a client waits for an expected frame.
const WAIT: Duration = Duration::from_secs(5);

/// The test server's options.
pub fn options() -> RealtimeOptions {
    RealtimeOptions {
        app_id: "test".to_owned(),
        key: KEY.to_owned(),
        secret: Secret::new(SECRET.to_owned()),
        allowed_origins: vec![ORIGIN.to_owned()],
        activity_timeout_secs: 120,
        ..RealtimeOptions::default()
    }
}

/// `key:hex(hmac_sha256(SECRET, socket_id:channel[:channel_data]))`,
/// computed independently of the crate.
pub fn signature(socket_id: &str, channel: &str, channel_data: Option<&str>) -> String {
    let message = match channel_data {
        Some(data) => format!("{socket_id}:{channel}:{data}"),
        None => format!("{socket_id}:{channel}"),
    };
    let mac = navree_crypto::hmac_sha256(SECRET.as_bytes(), message.as_bytes());
    format!("{KEY}:{}", hex::encode(mac))
}

/// Authenticates requests carrying `x-user-id: <uuid>`.
pub struct HeaderAuthenticator;

impl Authenticator for HeaderAuthenticator {
    fn authenticate<'a>(
        &'a self,
        parts: &'a mut http::request::Parts,
    ) -> BoxFuture<'a, Result<Option<Principal>, AppError>> {
        Box::pin(async move {
            Ok(parts
                .headers
                .get("x-user-id")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| Uuid::parse_str(v).ok())
                .map(|user_id| Principal {
                    user_id,
                    via: AuthVia::Test,
                    token_abilities: None,
                    session_id: None,
                }))
        })
    }
}

/// The channel authorizers of the test app:
/// - `crm.deals`: every signed-in user;
/// - `users.{id}`: only the user `id`;
/// - `room.{id}` (presence): every user, with `{"name": "user <first 4 chars>"}`;
/// - `broken`: the authorizer fails.
pub struct ChannelsModule;

fn deals(
    _principal: &Principal,
    _params: &HashMap<String, String>,
    _ctx: &AppContext,
) -> BoxFuture<'static, Result<ChannelDecision, BoxError>> {
    Box::pin(async { Ok(ChannelDecision::Allow) })
}

fn user(
    principal: &Principal,
    params: &HashMap<String, String>,
    _ctx: &AppContext,
) -> BoxFuture<'static, Result<ChannelDecision, BoxError>> {
    let own = params.get("id") == Some(&principal.user_id.to_string());
    Box::pin(async move {
        Ok(if own {
            ChannelDecision::Allow
        } else {
            ChannelDecision::Deny
        })
    })
}

fn room(
    principal: &Principal,
    _params: &HashMap<String, String>,
    _ctx: &AppContext,
) -> BoxFuture<'static, Result<ChannelDecision, BoxError>> {
    let name = format!("user {}", &principal.user_id.to_string()[..4]);
    Box::pin(async move { Ok(ChannelDecision::Presence(json!({ "name": name }))) })
}

fn broken(
    _principal: &Principal,
    _params: &HashMap<String, String>,
    _ctx: &AppContext,
) -> BoxFuture<'static, Result<ChannelDecision, BoxError>> {
    Box::pin(async { Err("the authorizer failed".into()) })
}

impl Module for ChannelsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.channel("crm.deals", deals)
            .channel("users.{id}", user)
            .channel("room.{id}", room)
            .channel("broken", broken);
    }
}

/// The test app: HTTP, realtime with `options`, the test channels and the
/// header authenticator.
pub fn app(options: RealtimeOptions) -> AppBuilder {
    Navree::app()
        .module(HttpModule::for_root(HttpOptions {
            per_ip_rate_limit: None,
            ..HttpOptions::default()
        }))
        .module(RealtimeModule::for_root(options))
        .module(ChannelsModule)
        .override_value::<dyn Authenticator>(Arc::new(HeaderAuthenticator))
}

/// A booted app served on 127.0.0.1:0.
pub struct Server {
    pub app: App,
    pub addr: SocketAddr,
    pub router: Router,
    pub realtime: Arc<Realtime>,
}

impl Server {
    /// Serves the test app with the default test options.
    pub async fn start() -> Server {
        Server::serve(app(options())).await
    }

    /// Serves the test app with `options`.
    pub async fn start_with(options: RealtimeOptions) -> Server {
        Server::serve(app(options)).await
    }

    /// Boots `builder` and serves it until the app shuts down.
    pub async fn serve(builder: AppBuilder) -> Server {
        let app = builder.boot().await.expect("app boots");
        let router = build_router(app.ctx()).await.expect("router builds");
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let token = app.ctx().shutdown_token();
        tokio::spawn(
            axum::serve(
                listener,
                router
                    .clone()
                    .into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(token.cancelled_owned())
            .into_future(),
        );
        let realtime = app.ctx().get::<Realtime>().await.expect("Realtime");
        Server {
            app,
            addr,
            router,
            realtime,
        }
    }

    /// The WebSocket URL for app key `key`, as pusher-js builds it.
    pub fn url_for(&self, key: &str) -> String {
        format!(
            "ws://{}/app/{key}?protocol=7&client=js&version=8.4.0&flash=false",
            self.addr
        )
    }

    /// Opens a WebSocket to app key `key` from `origin` (no `Origin` header
    /// when `None`), without reading anything.
    pub async fn open(&self, key: &str, origin: Option<&str>) -> Client {
        let mut request = self.url_for(key).into_client_request().unwrap();
        if let Some(origin) = origin {
            request
                .headers_mut()
                .insert("origin", HeaderValue::from_str(origin).unwrap());
        }
        let (ws, response) = tokio::time::timeout(WAIT, tokio_tungstenite::connect_async(request))
            .await
            .expect("handshake in time")
            .expect("handshake succeeds");
        assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
        Client {
            ws,
            socket_id: String::new(),
        }
    }

    /// Opens a WebSocket from the allowed origin and reads
    /// `pusher:connection_established`.
    pub async fn connect(&self) -> Client {
        let mut client = self.open(KEY, Some(ORIGIN)).await;
        let established = client.expect("pusher:connection_established").await;
        client.socket_id = established.data["socket_id"]
            .as_str()
            .expect("socket_id")
            .to_owned();
        client
    }

    /// `POST /broadcasting/auth` as `user` (a guest when `None`) with a form body.
    pub async fn auth(&self, user: Option<Uuid>, socket_id: &str, channel: &str) -> Reply {
        let body = format!("socket_id={socket_id}&channel_name={channel}");
        let mut request = Request::post("/broadcasting/auth")
            .header("content-type", "application/x-www-form-urlencoded");
        if let Some(user) = user {
            request = request.header("x-user-id", user.to_string());
        }
        self.send(request.body(Body::from(body)).unwrap()).await
    }

    /// `POST /broadcasting/auth` as `user` with a JSON body.
    pub async fn auth_json(&self, user: Uuid, body: Value) -> Reply {
        let request = Request::post("/broadcasting/auth")
            .header("content-type", "application/json")
            .header("x-user-id", user.to_string())
            .body(Body::from(body.to_string()))
            .unwrap();
        self.send(request).await
    }

    /// Sends `request` through the app's router.
    pub async fn send(&self, request: Request<Body>) -> Reply {
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json = serde_json::from_slice(&body).unwrap_or(Value::Null);
        Reply { status, json }
    }

    /// Signs `user` into `channel` (private or presence) through the auth
    /// endpoint and subscribes `client`; returns `subscription_succeeded`.
    pub async fn join(&self, client: &mut Client, user: Uuid, channel: &str) -> Event {
        let reply = self.auth(Some(user), &client.socket_id, channel).await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.json);
        client
            .subscribe(
                channel,
                reply.json["auth"].as_str(),
                reply.json["channel_data"].as_str(),
            )
            .await;
        let event = client
            .expect("pusher_internal:subscription_succeeded")
            .await;
        assert_eq!(event.channel.as_deref(), Some(channel));
        event
    }
}

/// A collected HTTP response.
#[derive(Debug)]
pub struct Reply {
    pub status: StatusCode,
    pub json: Value,
}

/// A received Pusher frame, with `data` decoded.
#[derive(Debug)]
pub struct Event {
    pub event: String,
    pub channel: Option<String>,
    pub data: Value,
}

/// A WebSocket client.
pub struct Client {
    pub ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
    pub socket_id: String,
}

impl Client {
    /// Sends `{"event": event, "data": data}`.
    pub async fn send(&mut self, event: &str, data: Value) {
        self.send_text(json!({ "event": event, "data": data }).to_string())
            .await;
    }

    /// Sends a raw text frame.
    pub async fn send_text(&mut self, text: String) {
        self.ws.send(Message::text(text)).await.expect("send");
    }

    /// `pusher:subscribe` to `channel`.
    pub async fn subscribe(
        &mut self,
        channel: &str,
        auth: Option<&str>,
        channel_data: Option<&str>,
    ) {
        let mut data = json!({ "channel": channel });
        if let Some(auth) = auth {
            data["auth"] = json!(auth);
        }
        if let Some(channel_data) = channel_data {
            data["channel_data"] = json!(channel_data);
        }
        self.send("pusher:subscribe", data).await;
    }

    /// The next Pusher frame (WebSocket-level pings and pongs are skipped);
    /// panics on close or after 5 s.
    pub async fn recv(&mut self) -> Event {
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
        .expect("a frame within 5 s");
        let mut frame: Value = serde_json::from_str(&text).expect("frames are JSON");
        let data = match frame["data"].take() {
            Value::String(encoded) => {
                serde_json::from_str(&encoded).unwrap_or(Value::String(encoded))
            }
            other => other,
        };
        Event {
            event: frame["event"].as_str().expect("event").to_owned(),
            channel: frame["channel"].as_str().map(str::to_owned),
            data,
        }
    }

    /// The next frame, which must be `event`.
    pub async fn expect(&mut self, event: &str) -> Event {
        let received = self.recv().await;
        assert_eq!(received.event, event, "{received:?}");
        received
    }

    /// The next frame must be `pusher:error` with `code`; returns its message.
    pub async fn expect_error(&mut self, code: u16) -> String {
        let error = self.expect("pusher:error").await;
        assert_eq!(error.data["code"], code, "{error:?}");
        error.data["message"]
            .as_str()
            .unwrap_or_default()
            .to_owned()
    }

    /// Waits for the server to close the connection; returns the close code.
    pub async fn expect_close(&mut self) -> u16 {
        tokio::time::timeout(WAIT, async {
            loop {
                match self.ws.next().await {
                    Some(Ok(Message::Close(Some(frame)))) => return u16::from(frame.code),
                    Some(Ok(Message::Close(None))) => panic!("close frame without a code"),
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
                    Some(Ok(other)) => panic!("expected a close frame, got {other:?}"),
                    Some(Err(error)) => panic!("expected a close frame, got error {error}"),
                    None => panic!("connection ended without a close frame"),
                }
            }
        })
        .await
        .expect("closed within 5 s")
    }

    /// Asserts that no Pusher frame arrives within `wait`.
    pub async fn expect_silence(&mut self, wait: Duration) {
        let next = tokio::time::timeout(wait, async {
            loop {
                match self.ws.next().await {
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
                    other => return other,
                }
            }
        })
        .await;
        if let Ok(frame) = next {
            panic!("expected silence, got {frame:?}");
        }
    }

    /// `pusher:ping` → `pusher:pong`: proves the connection is still served.
    pub async fn assert_alive(&mut self) {
        self.send("pusher:ping", json!({})).await;
        self.expect("pusher:pong").await;
    }
}

/// Close code type re-exported for tests that inspect frames themselves.
pub type CloseCode = tungstenite::protocol::frame::coding::CloseCode;
