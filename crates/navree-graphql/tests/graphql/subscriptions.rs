//! Subscriptions over a real WebSocket: `graphql-transport-ws` and
//! `graphql-ws`, authentication in `connection_init` or on the upgrade
//! request, events from the bus, keep-alive pings and the close codes; and
//! the endpoint's 401 for invalid credentials, over real HTTP.

use std::future::IntoFuture;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_graphql::futures_util::{SinkExt, Stream, StreamExt, stream};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use http::request::Parts;
use navree_config::{ConfigModule, ConfigSources};
use navree_contracts::{AuthVia, BoxFuture, Principal};
use navree_events::{EventBus, EventsModule};
use navree_graphql::{
    Context, ContextExt, EventStream, GraphqlModule, GraphqlOptions, ModuleBuilderGraphqlExt,
    Result,
};
use navree_http::{AppError, Authenticator, HttpModule, HttpOptions, build_router};
use navree_kernel::{App, ModuleBuilder, Navree};
use navree_macros::{injectable, module, resolver};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use uuid::Uuid;

use crate::support::PostCreated;

const WAIT: Duration = Duration::from_secs(5);

#[injectable]
struct LiveResolver;

#[resolver]
impl LiveResolver {
    #[query]
    #[public]
    async fn ping(&self) -> &'static str {
        "pong"
    }

    /// New posts.
    #[subscription]
    #[authenticated]
    async fn post_created(&self, ctx: &Context<'_>) -> Result<impl Stream<Item = PostCreated>> {
        EventStream::<PostCreated>::new(ctx)
    }

    /// The viewer's id, once.
    #[subscription]
    #[authenticated]
    async fn viewer(&self, ctx: &Context<'_>) -> Result<impl Stream<Item = Uuid>> {
        let me = ctx.principal()?.user_id;
        Ok(stream::once(async move { me }))
    }
}

#[module(register = Self::wire)]
struct LiveModule;

impl LiveModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.resolver::<LiveResolver>();
    }
}

/// Accepts JWT-shaped bearer tokens and trusts their `sub` (the signature
/// is navree-auth's business, not these tests'), and a `session=<user id>`
/// cookie as a session.
struct TokenAuthenticator;

impl Authenticator for TokenAuthenticator {
    fn authenticate<'a>(
        &'a self,
        parts: &'a mut Parts,
    ) -> BoxFuture<'a, std::result::Result<Option<Principal>, AppError>> {
        Box::pin(async move {
            let invalid = || AppError::unauthenticated().with_detail("Invalid token");
            let Some(header) = parts.headers.get("authorization") else {
                let session = parts
                    .headers
                    .get("cookie")
                    .and_then(|value| value.to_str().ok())
                    .and_then(|cookies| {
                        cookies
                            .split(';')
                            .find_map(|cookie| cookie.trim().strip_prefix("session="))
                    });
                return match session {
                    Some(id) => Ok(Some(Principal {
                        user_id: Uuid::parse_str(id).map_err(|_| invalid())?,
                        via: AuthVia::Session,
                        token_abilities: None,
                        session_id: None,
                    })),
                    None => Ok(None),
                };
            };
            let token = header
                .to_str()
                .ok()
                .and_then(|value| value.strip_prefix("Bearer "))
                .ok_or_else(invalid)?;
            let payload = token.split('.').nth(1).ok_or_else(invalid)?;
            let claims: Value = URL_SAFE_NO_PAD
                .decode(payload)
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .ok_or_else(invalid)?;
            let user_id = claims["sub"]
                .as_str()
                .and_then(|sub| Uuid::parse_str(sub).ok())
                .ok_or_else(invalid)?;
            Ok(Some(Principal {
                user_id,
                via: AuthVia::Token,
                token_abilities: None,
                session_id: None,
            }))
        })
    }
}

fn token(user: Uuid, expires_in: Duration) -> String {
    let exp = (SystemTime::now() + expires_in)
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
    let payload = URL_SAFE_NO_PAD.encode(json!({ "sub": user, "exp": exp }).to_string());
    format!("{header}.{payload}.x")
}

struct Server {
    app: App,
    addr: SocketAddr,
}

impl Server {
    async fn start(options: GraphqlOptions) -> Server {
        Server::start_with(options, &[]).await
    }

    /// A server whose `http.cors.allowed_origins` are `allowed_origins`.
    async fn start_with(options: GraphqlOptions, allowed_origins: &[&str]) -> Server {
        let mut http = HttpOptions {
            per_ip_rate_limit: None,
            ..HttpOptions::default()
        };
        http.cors.allowed_origins = allowed_origins.iter().map(|o| (*o).to_owned()).collect();
        let app = Navree::app()
            .module(ConfigModule::for_root(ConfigSources::new()))
            .module(HttpModule::for_root(http))
            .module(EventsModule::for_root())
            .module(GraphqlModule::for_root(options))
            .module(LiveModule)
            .override_value::<dyn Authenticator>(Arc::new(TokenAuthenticator))
            .boot()
            .await
            .expect("app boots");
        let router = build_router(app.ctx()).await.expect("router builds");
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let token = app.ctx().shutdown_token();
        tokio::spawn(
            navree_http::serve(listener, router)
                .with_graceful_shutdown(token.cancelled_owned())
                .into_future(),
        );
        Server { app, addr }
    }

    async fn connect(&self, protocol: &str) -> Client {
        self.connect_with(protocol, &[]).await
    }

    /// Connects with extra headers on the upgrade request.
    async fn connect_with(&self, protocol: &str, headers: &[(&'static str, String)]) -> Client {
        let url = format!("ws://{}/graphql/ws", self.addr);
        let mut request = url.into_client_request().unwrap();
        request.headers_mut().insert(
            "sec-websocket-protocol",
            HeaderValue::from_str(protocol).unwrap(),
        );
        for (name, value) in headers {
            request
                .headers_mut()
                .insert(*name, HeaderValue::from_str(value).unwrap());
        }
        let (ws, response) = tokio::time::timeout(WAIT, tokio_tungstenite::connect_async(request))
            .await
            .expect("handshake in time")
            .expect("handshake succeeds");
        assert_eq!(
            response.headers()["sec-websocket-protocol"],
            protocol,
            "the server speaks the protocol"
        );
        Client { ws }
    }

    async fn emit(&self, title: &str) {
        let bus = self.app.ctx().get::<EventBus>().await.unwrap();
        bus.emit(PostCreated {
            id: Uuid::now_v7(),
            title: title.to_owned(),
        })
        .await
        .unwrap();
    }

    /// `POST /graphql` with `authorization` over plain HTTP/1.1: the status.
    async fn post(&self, authorization: &str, body: &Value) -> u16 {
        let body = body.to_string();
        let request = format!(
            "POST /graphql HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\n\
             Authorization: {authorization}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            self.addr,
            body.len()
        );
        let mut stream = TcpStream::connect(self.addr).await.unwrap();
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut response = Vec::new();
        tokio::time::timeout(WAIT, stream.read_to_end(&mut response))
            .await
            .expect("a response in time")
            .unwrap();
        let response = String::from_utf8_lossy(&response);
        response
            .split(' ')
            .nth(1)
            .and_then(|status| status.parse().ok())
            .unwrap_or_else(|| panic!("not an HTTP response: {response}"))
    }

    async fn stop(self) {
        self.app.shutdown().await.unwrap();
    }
}

#[derive(Debug)]
enum Frame {
    Text(Value),
    Close(u16, String),
}

struct Client {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl Client {
    async fn send(&mut self, message: Value) {
        self.ws
            .send(Message::Text(message.to_string().into()))
            .await
            .unwrap();
    }

    async fn next(&mut self) -> Frame {
        loop {
            let message = tokio::time::timeout(WAIT, self.ws.next())
                .await
                .expect("a frame in time")
                .expect("the socket is open")
                .expect("a valid frame");
            match message {
                Message::Text(text) => return Frame::Text(serde_json::from_str(&text).unwrap()),
                Message::Close(frame) => {
                    let (code, reason) = frame
                        .map(|frame| (u16::from(frame.code), frame.reason.to_string()))
                        .unwrap_or((u16::from(CloseCode::Status), String::new()));
                    return Frame::Close(code, reason);
                }
                _ => continue,
            }
        }
    }

    /// The next text frame that is not a keep-alive.
    async fn message(&mut self) -> Value {
        loop {
            match self.next().await {
                Frame::Text(value) if value["type"] == "ping" || value["type"] == "ka" => continue,
                Frame::Text(value) => return value,
                Frame::Close(code, reason) => panic!("closed with {code} {reason}"),
            }
        }
    }

    async fn init(&mut self, payload: Value) {
        self.send(json!({ "type": "connection_init", "payload": payload }))
            .await;
        assert_eq!(self.message().await["type"], "connection_ack");
    }

    /// Subscribes to `viewer` (`graphql-transport-ws`): the payload of the
    /// one `next` message.
    async fn viewer(&mut self) -> Value {
        self.send(json!({
            "type": "subscribe",
            "id": "viewer",
            "payload": { "query": "subscription { viewer }" },
        }))
        .await;
        let next = self.message().await;
        assert_eq!(next["type"], "next", "{next}");
        assert_eq!(self.message().await["type"], "complete");
        next["payload"].clone()
    }
}

fn options() -> GraphqlOptions {
    GraphqlOptions {
        keep_alive_secs: 0,
        ..GraphqlOptions::default()
    }
}

#[tokio::test]
async fn subscriptions_stream_events_to_the_authenticated_client() {
    let server = Server::start(options()).await;
    let me = Uuid::now_v7();
    let mut client = server.connect("graphql-transport-ws").await;
    client
        .init(json!({ "authorization": format!("Bearer {}", token(me, Duration::from_secs(600))) }))
        .await;

    client
        .send(json!({
            "type": "subscribe",
            "id": "viewer",
            "payload": { "query": "subscription { viewer }" },
        }))
        .await;
    let next = client.message().await;
    assert_eq!(next["type"], "next");
    assert_eq!(next["id"], "viewer");
    assert_eq!(next["payload"]["data"]["viewer"], json!(me));
    assert_eq!(client.message().await["type"], "complete");

    client
        .send(json!({
            "type": "subscribe",
            "id": "posts",
            "payload": { "query": "subscription { postCreated { title } }" },
        }))
        .await;
    // The subscription starts asynchronously: emit until it sees one.
    let mut received = None;
    for attempt in 0..50 {
        server.emit(&format!("Post {attempt}")).await;
        if let Ok(frame) = tokio::time::timeout(Duration::from_millis(100), client.message()).await
        {
            received = Some(frame);
            break;
        }
    }
    let next = received.expect("an event reaches the subscriber");
    assert_eq!(next["type"], "next");
    assert_eq!(next["id"], "posts");
    assert!(
        next["payload"]["data"]["postCreated"]["title"]
            .as_str()
            .unwrap()
            .starts_with("Post "),
        "{next}"
    );
    server.stop().await;
}

#[tokio::test]
async fn guests_get_unauthenticated_when_the_subscription_starts() {
    let server = Server::start(options()).await;
    let mut client = server.connect("graphql-transport-ws").await;
    client.init(json!({})).await;
    client
        .send(json!({
            "type": "subscribe",
            "id": "1",
            "payload": { "query": "subscription { viewer }" },
        }))
        .await;
    let next = client.message().await;
    assert_eq!(next["type"], "next");
    assert_eq!(
        next["payload"]["errors"][0]["extensions"]["code"],
        "UNAUTHENTICATED"
    );
    assert_eq!(client.message().await["type"], "complete");
    server.stop().await;
}

#[tokio::test]
async fn without_a_token_in_the_payload_the_upgrade_requests_authorization_counts() {
    let server = Server::start(options()).await;
    let me = Uuid::now_v7();
    let bearer = format!("Bearer {}", token(me, Duration::from_secs(600)));
    let mut client = server
        .connect_with("graphql-transport-ws", &[("authorization", bearer)])
        .await;
    client.init(json!({})).await;
    assert_eq!(client.viewer().await["data"]["viewer"], json!(me));
    server.stop().await;
}

#[tokio::test]
async fn a_session_cookie_counts_only_from_an_allowed_origin() {
    let server = Server::start_with(options(), &["http://app.test"]).await;
    let me = Uuid::now_v7();
    let cookie = format!("session={me}");

    let mut foreign = server
        .connect_with(
            "graphql-transport-ws",
            &[
                ("cookie", cookie.clone()),
                ("origin", "http://evil.test".to_owned()),
            ],
        )
        .await;
    foreign.init(json!({})).await;
    let payload = foreign.viewer().await;
    assert_eq!(
        payload["errors"][0]["extensions"]["code"], "UNAUTHENTICATED",
        "{payload}"
    );

    let mut own = server
        .connect_with(
            "graphql-transport-ws",
            &[("cookie", cookie), ("origin", "http://app.test".to_owned())],
        )
        .await;
    own.init(json!({})).await;
    assert_eq!(own.viewer().await["data"]["viewer"], json!(me));
    server.stop().await;
}

#[tokio::test]
async fn invalid_credentials_are_a_401_before_the_query_runs() {
    let server = Server::start(options()).await;
    let query = json!({ "query": "{ ping }" });
    assert_eq!(server.post("Bearer nope", &query).await, 401);
    let me = Uuid::now_v7();
    let valid = format!("Bearer {}", token(me, Duration::from_secs(600)));
    assert_eq!(server.post(&valid, &query).await, 200);
    server.stop().await;
}

#[tokio::test]
async fn a_refused_token_closes_with_4403() {
    let server = Server::start(options()).await;
    let mut client = server.connect("graphql-transport-ws").await;
    client
        .send(json!({ "type": "connection_init", "payload": { "authorization": "Bearer nope" } }))
        .await;
    match client.next().await {
        Frame::Close(code, reason) => {
            assert_eq!(code, 4403);
            assert_eq!(reason, "Invalid token");
        }
        other => panic!("expected a close, got {other:?}"),
    }
    server.stop().await;
}

#[tokio::test]
async fn a_client_that_never_initializes_is_closed_with_4408() {
    let server = Server::start(GraphqlOptions {
        connection_init_timeout_secs: 1,
        ..options()
    })
    .await;
    let mut client = server.connect("graphql-transport-ws").await;
    match client.next().await {
        Frame::Close(code, _) => assert_eq!(code, 4408),
        other => panic!("expected a close, got {other:?}"),
    }
    server.stop().await;
}

#[tokio::test]
async fn the_socket_closes_with_4401_when_the_token_expires() {
    let server = Server::start(options()).await;
    let mut client = server.connect("graphql-transport-ws").await;
    let me = Uuid::now_v7();
    client
        .init(json!({ "authorization": format!("Bearer {}", token(me, Duration::from_secs(2))) }))
        .await;
    match client.next().await {
        Frame::Close(code, _) => assert_eq!(code, 4401),
        other => panic!("expected a close, got {other:?}"),
    }
    server.stop().await;
}

#[tokio::test]
async fn the_server_pings_idle_sockets() {
    let server = Server::start(GraphqlOptions {
        keep_alive_secs: 1,
        ..options()
    })
    .await;
    let mut client = server.connect("graphql-transport-ws").await;
    client.init(json!({})).await;
    match client.next().await {
        Frame::Text(ping) => assert_eq!(ping, json!({ "type": "ping" })),
        other => panic!("expected a ping, got {other:?}"),
    }
    server.stop().await;
}

#[tokio::test]
async fn the_older_graphql_ws_protocol_works_too() {
    let server = Server::start(options()).await;
    let me = Uuid::now_v7();
    let mut client = server.connect("graphql-ws").await;
    client
        .init(json!({ "Authorization": format!("Bearer {}", token(me, Duration::from_secs(600))) }))
        .await;
    client
        .send(json!({
            "type": "start",
            "id": "1",
            "payload": { "query": "subscription { viewer }" },
        }))
        .await;
    let data = client.message().await;
    assert_eq!(data["type"], "data");
    assert_eq!(data["payload"]["data"]["viewer"], json!(me));
    server.stop().await;
}

#[tokio::test]
async fn subscriptions_can_be_turned_off() {
    let server = Server::start(GraphqlOptions {
        subscriptions: false,
        ..options()
    })
    .await;
    let url = format!("ws://{}/graphql/ws", server.addr);
    let error = tokio_tungstenite::connect_async(url).await.unwrap_err();
    assert!(error.to_string().contains("404"), "{error}");
    server.stop().await;
}
