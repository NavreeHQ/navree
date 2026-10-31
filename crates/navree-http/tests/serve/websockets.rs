//! WebSockets: the server hands the connection over and stays out of it.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use http::{Method, Request, StatusCode, Version};
use http_body_util::Empty;
use hyper::client::conn::http2;
use hyper::ext::Protocol;
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use navree_http::ServerOptions;
use navree_http::routing::Router;
use navree_http::ws::{Message, WebSocket, WebSocketUpgrade};
use tokio_tungstenite::tungstenite::Message as ClientMessage;
use tokio_tungstenite::tungstenite::protocol::Role;
use tokio_tungstenite::{WebSocketStream, connect_async};

use crate::support::{GET, PATIENCE, connect, no_limits, read_response, start, stays_open};

async fn echo(mut socket: WebSocket) {
    while let Some(Ok(message)) = socket.recv().await {
        if matches!(message, Message::Text(_)) && socket.send(message).await.is_err() {
            return;
        }
    }
}

fn app() -> Router {
    let socket = |upgrade: WebSocketUpgrade| async move { upgrade.on_upgrade(echo) };
    Router::new()
        .route(Method::GET, "/ws", socket)
        .route(Method::CONNECT, "/ws", socket)
        .route(Method::GET, "/", || async { "hello" })
}

/// Every limit that could cut a quiet connection, short.
fn short_limits() -> ServerOptions {
    ServerOptions {
        header_read_timeout_ms: Some(100),
        idle_timeout_ms: Some(100),
        body_read_timeout_ms: Some(100),
        ..no_limits()
    }
}

async fn echoes<S>(socket: &mut WebSocketStream<S>, text: &str)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    socket
        .send(ClientMessage::text(text.to_owned()))
        .await
        .unwrap();
    match socket.next().await {
        Some(Ok(ClientMessage::Text(echoed))) => assert_eq!(echoed.as_str(), text),
        other => panic!("{other:?}"),
    }
}

fn ws_url(addr: SocketAddr) -> String {
    format!("ws://{addr}/ws")
}

#[tokio::test]
async fn a_websocket_outlives_every_limit_of_the_connection_it_came_from() {
    let server = start(app(), short_limits()).await;
    let (mut socket, response) = connect_async(ws_url(server.addr)).await.unwrap();
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
    echoes(&mut socket, "first").await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    echoes(&mut socket, "still here").await;
}

/// A WebSocket holds a slot under the cap for as long as it is open, and
/// gives it back when it closes.
#[tokio::test]
async fn a_websocket_holds_its_slot_until_it_closes() {
    let server = start(
        app(),
        ServerOptions {
            max_connections: Some(1),
            ..no_limits()
        },
    )
    .await;
    let (mut socket, _) = connect_async(ws_url(server.addr)).await.unwrap();
    echoes(&mut socket, "hello").await;

    let mut waiting = connect(server.addr).await;
    tokio::io::AsyncWriteExt::write_all(&mut waiting, GET)
        .await
        .unwrap();
    assert!(stays_open(&mut waiting, Duration::from_millis(400)).await);

    drop(socket);
    let response = read_response(&mut waiting).await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
}

/// RFC 8441: a WebSocket on a stream of an HTTP/2 connection. The idle
/// timeout leaves the connection alone, and other requests still run on it.
#[tokio::test]
async fn a_websocket_works_over_http2() {
    let server = start(
        app(),
        ServerOptions {
            idle_timeout_ms: Some(100),
            ..no_limits()
        },
    )
    .await;
    let stream = connect(server.addr).await;
    let (mut sender, connection) = http2::Builder::new(TokioExecutor::new())
        .timer(TokioTimer::new())
        .handshake::<_, Empty<bytes::Bytes>>(TokioIo::new(stream))
        .await
        .unwrap();
    tokio::spawn(connection);

    let request = Request::builder()
        .method(Method::CONNECT)
        .version(Version::HTTP_2)
        .uri("http://test/ws")
        .header("sec-websocket-version", "13")
        .extension(Protocol::from_static("websocket"))
        .body(Empty::new())
        .unwrap();
    let response = sender.send_request(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let upgraded = hyper::upgrade::on(response).await.unwrap();
    let mut socket =
        WebSocketStream::from_raw_socket(TokioIo::new(upgraded), Role::Client, None).await;
    echoes(&mut socket, "over two").await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    echoes(&mut socket, "still here").await;

    let other = Request::get("http://test/").body(Empty::new()).unwrap();
    let response = sender.send_request(other).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

const QUIET_MS: u64 = 300;
const QUIET: Duration = Duration::from_millis(QUIET_MS);

fn quiet_limit() -> ServerOptions {
    ServerOptions {
        upgraded_idle_timeout_ms: Some(QUIET_MS),
        ..no_limits()
    }
}

#[tokio::test]
async fn a_quiet_websocket_is_closed_at_the_upgraded_idle_timeout() {
    let server = start(app(), quiet_limit()).await;
    let (mut socket, _) = connect_async(ws_url(server.addr)).await.unwrap();
    let started = Instant::now();
    tokio::time::timeout(PATIENCE, async {
        while let Some(Ok(_)) = socket.next().await {}
    })
    .await
    .expect("the socket is closed");
    assert!(started.elapsed() >= QUIET / 2, "{:?}", started.elapsed());
}

/// The limit is on silence, not on age: a socket that talks stays.
#[tokio::test]
async fn a_websocket_that_talks_outlives_the_upgraded_idle_timeout() {
    let server = start(
        app(),
        ServerOptions {
            upgraded_idle_timeout_ms: Some(2000),
            ..no_limits()
        },
    )
    .await;
    let (mut socket, _) = connect_async(ws_url(server.addr)).await.unwrap();
    let started = Instant::now();
    while started.elapsed() < Duration::from_millis(2600) {
        echoes(&mut socket, "still here").await;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// A WebSocket over HTTP/2, on a connection of its own.
async fn http2_websocket(
    addr: SocketAddr,
) -> (
    http2::SendRequest<Empty<bytes::Bytes>>,
    WebSocketStream<TokioIo<hyper::upgrade::Upgraded>>,
    tokio::task::JoinHandle<()>,
) {
    let stream = connect(addr).await;
    let (mut sender, connection) = http2::Builder::new(TokioExecutor::new())
        .timer(TokioTimer::new())
        .handshake::<_, Empty<bytes::Bytes>>(TokioIo::new(stream))
        .await
        .unwrap();
    let connection = tokio::spawn(async {
        let _ = connection.await;
    });
    let request = Request::builder()
        .method(Method::CONNECT)
        .version(Version::HTTP_2)
        .uri("http://test/ws")
        .header("sec-websocket-version", "13")
        .extension(Protocol::from_static("websocket"))
        .body(Empty::new())
        .unwrap();
    let response = sender.send_request(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let upgraded = hyper::upgrade::on(response).await.unwrap();
    let socket = WebSocketStream::from_raw_socket(TokioIo::new(upgraded), Role::Client, None).await;
    (sender, socket, connection)
}

/// On HTTP/2 the bound is on the connection, once it has nothing else on it,
/// and the keep-alive pings of a connection nobody is using are not traffic.
#[tokio::test]
async fn a_quiet_http2_websocket_is_closed_though_the_connection_keeps_pinging() {
    let server = start(
        app(),
        ServerOptions {
            http2_keep_alive_interval_ms: Some(40),
            ..quiet_limit()
        },
    )
    .await;
    let (_sender, _socket, connection) = http2_websocket(server.addr).await;
    tokio::time::timeout(PATIENCE, connection)
        .await
        .expect("the connection is closed")
        .unwrap();
}

#[tokio::test]
async fn an_http2_websocket_that_talks_outlives_the_upgraded_idle_timeout() {
    let server = start(
        app(),
        ServerOptions {
            upgraded_idle_timeout_ms: Some(2000),
            ..no_limits()
        },
    )
    .await;
    let (_sender, mut socket, _connection) = http2_websocket(server.addr).await;
    let started = Instant::now();
    while started.elapsed() < Duration::from_millis(2600) {
        echoes(&mut socket, "still here").await;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Off by default: a quiet socket stays, on both protocols, however long.
#[tokio::test]
async fn by_default_a_quiet_websocket_stays() {
    let options = ServerOptions {
        http2_keep_alive_interval_ms: Some(40),
        ..no_limits()
    };
    assert_eq!(ServerOptions::default().upgraded_idle_timeout_ms, None);
    let server = start(app(), options).await;
    let (mut first, _) = connect_async(ws_url(server.addr)).await.unwrap();
    let (_sender, mut second, _connection) = http2_websocket(server.addr).await;
    tokio::time::sleep(QUIET * 3).await;
    echoes(&mut first, "still here").await;
    echoes(&mut second, "still here").await;
}
