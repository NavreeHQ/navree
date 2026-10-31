//! The WebSocket handshake and frames against a client writing bytes by hand.

use std::net::SocketAddr;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use http::Method;
use navree_http::routing::Router;
use navree_http::ws::{
    DEFAULT_MAX_FRAME_BYTES, DEFAULT_MAX_MESSAGE_BYTES, Message, WebSocket, WebSocketError,
    WebSocketUpgrade,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_tungstenite::connect_async_with_config;
use tokio_tungstenite::tungstenite::Message as ClientMessage;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

use crate::support::{PATIENCE, Served, connect, no_limits, read_response, start};

const KEY: &str = "dGhlIHNhbXBsZSBub25jZQ==";

/// Answers each message with its length.
async fn lengths(mut socket: WebSocket) {
    while let Some(Ok(message)) = socket.recv().await {
        let length = match &message {
            Message::Text(text) => text.len(),
            Message::Binary(bytes) => bytes.len(),
            _ => continue,
        };
        if socket
            .send(Message::text(length.to_string()))
            .await
            .is_err()
        {
            return;
        }
    }
}

async fn plain() -> Served {
    let handler = |upgrade: WebSocketUpgrade| async move { upgrade.on_upgrade(lengths) };
    let app = Router::new()
        .route(Method::GET, "/ws", handler)
        .route(Method::POST, "/ws", handler)
        .route(Method::PUT, "/ws", handler)
        .route(Method::CONNECT, "/ws", handler);
    start(app, no_limits()).await
}

/// A server whose socket handler reports how the connection ended.
async fn reporting(
    configure: fn(WebSocketUpgrade) -> WebSocketUpgrade,
) -> (
    Served,
    mpsc::UnboundedReceiver<Result<Message, WebSocketError>>,
) {
    let (sender, receiver) = mpsc::unbounded_channel();
    let handler = move |upgrade: WebSocketUpgrade| {
        let sender = sender.clone();
        async move {
            configure(upgrade).on_upgrade(move |mut socket| async move {
                while let Some(message) = socket.recv().await {
                    let failed = message.is_err();
                    let _ = sender.send(message);
                    if failed {
                        return;
                    }
                }
            })
        }
    };
    let app = Router::new().route(Method::GET, "/ws", handler);
    (start(app, no_limits()).await, receiver)
}

fn handshake(method: &str, version: &str, lines: &[&str]) -> String {
    let lines: String = lines.iter().map(|line| format!("{line}\r\n")).collect();
    format!("{method} /ws {version}\r\nHost: test\r\n{lines}\r\n")
}

fn standard<'a>(key: &'a str, version: &'a str) -> Vec<String> {
    vec![
        "Connection: Upgrade".to_owned(),
        "Upgrade: websocket".to_owned(),
        format!("Sec-WebSocket-Version: {version}"),
        format!("Sec-WebSocket-Key: {key}"),
    ]
}

async fn answer(addr: SocketAddr, request: &str) -> String {
    let mut stream = connect(addr).await;
    stream.write_all(request.as_bytes()).await.unwrap();
    read_response(&mut stream).await
}

async fn upgraded(addr: SocketAddr) -> TcpStream {
    let mut stream = connect(addr).await;
    let lines = standard(KEY, "13");
    let lines: Vec<_> = lines.iter().map(String::as_str).collect();
    stream
        .write_all(handshake("GET", "HTTP/1.1", &lines).as_bytes())
        .await
        .unwrap();
    let response = read_response(&mut stream).await;
    assert!(response.starts_with("HTTP/1.1 101"), "{response}");
    stream
}

/// One masked client frame. The mask is all zeroes, so the payload goes as it
/// is.
fn frame(fin: bool, opcode: u8, payload: &[u8]) -> Vec<u8> {
    let mut frame = declared(fin, opcode, payload.len() as u64);
    frame.extend_from_slice(payload);
    frame
}

/// The header of a masked frame declaring `length` bytes.
fn declared(fin: bool, opcode: u8, length: u64) -> Vec<u8> {
    let mut frame = vec![if fin { 0x80 } else { 0 } | opcode];
    if length < 126 {
        frame.push(0x80 | length as u8);
    } else if length <= u64::from(u16::MAX) {
        frame.push(0x80 | 126);
        frame.extend_from_slice(&(length as u16).to_be_bytes());
    } else {
        frame.push(0x80 | 127);
        frame.extend_from_slice(&length.to_be_bytes());
    }
    frame.extend_from_slice(&[0; 4]);
    frame
}

const TEXT: u8 = 1;
const BINARY: u8 = 2;
const CONTINUATION: u8 = 0;

async fn closes(stream: &mut TcpStream) {
    timeout(PATIENCE, async {
        let mut sink = [0_u8; 4096];
        while let Ok(read) = stream.read(&mut sink).await {
            if read == 0 {
                return;
            }
        }
    })
    .await
    .expect("the server closes the connection");
}

#[tokio::test]
async fn the_accept_key_is_the_one_of_rfc_6455() {
    let server = plain().await;
    let lines = standard(KEY, "13");
    let lines: Vec<_> = lines.iter().map(String::as_str).collect();
    let response = answer(server.addr, &handshake("GET", "HTTP/1.1", &lines))
        .await
        .to_ascii_lowercase();
    assert!(response.starts_with("http/1.1 101"), "{response}");
    assert!(response.contains("connection: upgrade"), "{response}");
    assert!(response.contains("upgrade: websocket"), "{response}");
    assert!(
        response.contains("sec-websocket-accept: s3pplmbitxaq9kygzzhzrbk+xoo="),
        "{response}"
    );
    assert!(!response.contains("sec-websocket-protocol"), "{response}");
}

#[tokio::test]
async fn a_bad_key_is_refused() {
    let server = plain().await;
    for key in [
        "",
        "short",
        "dGhlIHNhbXBsZSBub25jZQ",
        "dGhlIHNhbXBsZSBub25jZQ===",
        "dGhlIHNhbXBsZSBub25jZQ=A",
        "dGhlIHNhbXBsZSBub25jZR==",
        "dGhlIHNhbXBsZSBub25jZ!==",
        "dGhlIHNhbXBsZSBub25jZSBib2R5IGFkZGVk",
        "not base64 at all, sorry!",
    ] {
        let lines = standard(key, "13");
        let lines: Vec<_> = lines.iter().map(String::as_str).collect();
        let response = answer(server.addr, &handshake("GET", "HTTP/1.1", &lines)).await;
        assert!(response.starts_with("HTTP/1.1 400"), "{key}: {response}");
    }
}

#[tokio::test]
async fn a_missing_or_repeated_key_is_refused() {
    let server = plain().await;
    let lines = [
        "Connection: Upgrade",
        "Upgrade: websocket",
        "Sec-WebSocket-Version: 13",
    ];
    let response = answer(server.addr, &handshake("GET", "HTTP/1.1", &lines)).await;
    assert!(response.starts_with("HTTP/1.1 400"), "{response}");

    let lines = [
        "Connection: Upgrade",
        "Upgrade: websocket",
        "Sec-WebSocket-Version: 13",
        "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==",
        "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==",
    ];
    let response = answer(server.addr, &handshake("GET", "HTTP/1.1", &lines)).await;
    assert!(response.starts_with("HTTP/1.1 400"), "{response}");
}

#[tokio::test]
async fn a_wrong_version_is_refused_and_names_the_right_one() {
    let server = plain().await;
    for version in ["", "12", "14", "8", "013", "13, 8", "thirteen"] {
        let lines = standard(KEY, version);
        let lines: Vec<_> = lines.iter().map(String::as_str).collect();
        let response = answer(server.addr, &handshake("GET", "HTTP/1.1", &lines)).await;
        assert!(
            response.starts_with("HTTP/1.1 400"),
            "{version}: {response}"
        );
        assert!(
            response
                .to_ascii_lowercase()
                .contains("sec-websocket-version: 13"),
            "{version}: {response}"
        );
    }
    let lines = [
        "Connection: Upgrade",
        "Upgrade: websocket",
        "Sec-WebSocket-Version: 13",
        "Sec-WebSocket-Version: 8",
        "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==",
    ];
    let response = answer(server.addr, &handshake("GET", "HTTP/1.1", &lines)).await;
    assert!(response.starts_with("HTTP/1.1 400"), "{response}");
}

#[tokio::test]
async fn an_http1_upgrade_must_be_a_get() {
    let server = plain().await;
    let lines = standard(KEY, "13");
    let lines: Vec<_> = lines.iter().map(String::as_str).collect();
    for method in ["POST", "PUT"] {
        let response = answer(server.addr, &handshake(method, "HTTP/1.1", &lines)).await;
        assert!(response.starts_with("HTTP/1.1 405"), "{method}: {response}");
    }
}

#[tokio::test]
async fn an_http10_request_cannot_upgrade() {
    let server = plain().await;
    let lines = standard(KEY, "13");
    let lines: Vec<_> = lines.iter().map(String::as_str).collect();
    let response = answer(server.addr, &handshake("GET", "HTTP/1.0", &lines)).await;
    assert!(response.contains(" 400 "), "{response}");
}

#[tokio::test]
async fn connection_and_upgrade_are_read_as_token_lists() {
    let server = plain().await;
    let version = "Sec-WebSocket-Version: 13";
    let key = "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==";
    for lines in [
        ["Connection: keep-alive, Upgrade", "Upgrade: websocket"],
        ["Connection: UPGRADE", "Upgrade: WebSocket"],
    ] {
        let all = [lines[0], lines[1], version, key];
        let response = answer(server.addr, &handshake("GET", "HTTP/1.1", &all)).await;
        assert!(
            response.starts_with("HTTP/1.1 101"),
            "{lines:?}: {response}"
        );
    }
    let all = [
        "Connection: keep-alive",
        "Connection: Upgrade",
        "Upgrade: websocket",
        version,
        key,
    ];
    let response = answer(server.addr, &handshake("GET", "HTTP/1.1", &all)).await;
    assert!(response.starts_with("HTTP/1.1 101"), "{response}");

    for lines in [
        ["Connection: not-upgrade", "Upgrade: websocket"],
        ["Connection: upgrade-insecure", "Upgrade: websocket"],
        ["Connection: close", "Upgrade: websocket"],
        ["Connection: Upgrade", "Upgrade: websockets"],
        ["Connection: Upgrade", "Upgrade: h2c"],
        ["Connection: Upgrade", "X-Other: 1"],
    ] {
        let all = [lines[0], lines[1], version, key];
        let response = answer(server.addr, &handshake("GET", "HTTP/1.1", &all)).await;
        assert!(
            response.starts_with("HTTP/1.1 400"),
            "{lines:?}: {response}"
        );
    }
}

#[tokio::test]
async fn only_a_protocol_the_server_names_is_chosen() {
    let handler = |upgrade: WebSocketUpgrade| async move {
        upgrade
            .protocols(["graphql-transport-ws", "graphql-ws"])
            .on_upgrade(lengths)
    };
    let server = start(
        Router::new().route(Method::GET, "/ws", handler),
        no_limits(),
    )
    .await;
    let selected = |offer: &'static str| {
        let addr = server.addr;
        async move {
            let mut lines = standard(KEY, "13");
            lines.push(offer.to_owned());
            let lines: Vec<_> = lines.iter().map(String::as_str).collect();
            let response = answer(addr, &handshake("GET", "HTTP/1.1", &lines)).await;
            assert!(response.starts_with("HTTP/1.1 101"), "{response}");
            response
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("sec-websocket-protocol: ")
                        .map(str::to_owned)
                })
                .map(|protocol| protocol.trim().to_owned())
        }
    };
    assert_eq!(selected("X-None: 1").await, None);
    assert_eq!(
        selected("Sec-WebSocket-Protocol: graphql-ws")
            .await
            .as_deref(),
        Some("graphql-ws")
    );
    // The server's preference, not the client's order.
    assert_eq!(
        selected("Sec-WebSocket-Protocol: graphql-ws, graphql-transport-ws")
            .await
            .as_deref(),
        Some("graphql-transport-ws")
    );
    assert_eq!(selected("Sec-WebSocket-Protocol: evil, , ,").await, None);
    assert_eq!(selected("Sec-WebSocket-Protocol: GRAPHQL-WS").await, None);
    assert_eq!(
        selected("Sec-WebSocket-Protocol: graphql-ws\r\nSec-WebSocket-Protocol: other")
            .await
            .as_deref(),
        Some("graphql-ws")
    );
    // A list is not one protocol that happens to contain the name.
    assert_eq!(selected("Sec-WebSocket-Protocol: xgraphql-ws").await, None);
}

#[test]
fn the_default_limits_are_the_documented_ones() {
    assert_eq!(DEFAULT_MAX_FRAME_BYTES, 1024 * 1024);
    assert_eq!(DEFAULT_MAX_MESSAGE_BYTES, 1024 * 1024);
}

async fn client_without_limits(
    addr: SocketAddr,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>> {
    let config = WebSocketConfig::default()
        .max_message_size(None)
        .max_frame_size(None);
    let (socket, _) = connect_async_with_config(format!("ws://{addr}/ws"), Some(config), false)
        .await
        .unwrap();
    socket
}

#[tokio::test]
async fn a_message_of_the_default_limit_passes() {
    let server = plain().await;
    let mut socket = client_without_limits(server.addr).await;
    socket
        .send(ClientMessage::binary(vec![7; DEFAULT_MAX_MESSAGE_BYTES]))
        .await
        .unwrap();
    match timeout(PATIENCE, socket.next()).await.unwrap() {
        Some(Ok(ClientMessage::Text(text))) => {
            assert_eq!(text.as_str(), DEFAULT_MAX_MESSAGE_BYTES.to_string());
        }
        other => panic!("{other:?}"),
    }
}

/// The header alone is enough: the server neither waits for the payload nor
/// makes room for it.
#[tokio::test]
async fn an_oversized_frame_is_refused_from_its_header() {
    let (_server, mut reports) = {
        let (server, reports) = reporting(|upgrade| upgrade).await;
        let mut stream = upgraded(server.addr).await;
        stream
            .write_all(&declared(true, BINARY, DEFAULT_MAX_FRAME_BYTES as u64 + 1))
            .await
            .unwrap();
        closes(&mut stream).await;
        (server, reports)
    };
    let report = timeout(PATIENCE, reports.recv()).await.unwrap().unwrap();
    let error = report.unwrap_err();
    assert!(error.is_too_large(), "{error}");
}

#[tokio::test]
async fn a_frame_may_declare_the_largest_length_there_is() {
    let (server, mut reports) = reporting(|upgrade| upgrade).await;
    let mut stream = upgraded(server.addr).await;
    stream
        .write_all(&declared(true, BINARY, u64::from(u32::MAX) * 8))
        .await
        .unwrap();
    closes(&mut stream).await;
    let error = timeout(PATIENCE, reports.recv())
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert!(error.is_too_large(), "{error}");
}

#[tokio::test]
async fn frame_and_message_limits_are_options() {
    let (server, mut reports) =
        reporting(|upgrade| upgrade.max_frame_size(16).max_message_size(24)).await;
    let mut stream = upgraded(server.addr).await;
    stream
        .write_all(&frame(true, BINARY, &[1; 16]))
        .await
        .unwrap();
    let report = timeout(PATIENCE, reports.recv()).await.unwrap().unwrap();
    assert_eq!(report.unwrap(), Message::binary(vec![1; 16]));
    stream
        .write_all(&frame(true, BINARY, &[1; 17]))
        .await
        .unwrap();
    closes(&mut stream).await;
    let error = timeout(PATIENCE, reports.recv())
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert!(error.is_too_large(), "{error}");
}

/// Each frame fits; together they do not, however they are cut.
#[tokio::test]
async fn a_fragmented_message_answers_to_the_message_limit() {
    let (server, mut reports) =
        reporting(|upgrade| upgrade.max_frame_size(16).max_message_size(24)).await;
    let mut stream = upgraded(server.addr).await;
    stream
        .write_all(&frame(false, BINARY, &[1; 16]))
        .await
        .unwrap();
    stream
        .write_all(&frame(false, CONTINUATION, &[1; 8]))
        .await
        .unwrap();
    stream
        .write_all(&frame(true, CONTINUATION, &[1; 1]))
        .await
        .unwrap();
    closes(&mut stream).await;
    let error = timeout(PATIENCE, reports.recv())
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert!(error.is_too_large(), "{error}");
}

/// RFC 6455: a server closes on a client frame that is not masked.
#[tokio::test]
async fn an_unmasked_frame_ends_the_connection() {
    let (server, mut reports) = reporting(|upgrade| upgrade).await;
    let mut stream = upgraded(server.addr).await;
    stream
        .write_all(&[0x80 | TEXT, 2, b'h', b'i'])
        .await
        .unwrap();
    closes(&mut stream).await;
    let report = timeout(PATIENCE, reports.recv()).await.unwrap().unwrap();
    assert!(report.is_err(), "{report:?}");
}

#[tokio::test]
async fn text_that_is_not_utf8_ends_the_connection() {
    let (server, mut reports) = reporting(|upgrade| upgrade).await;
    let mut stream = upgraded(server.addr).await;
    stream
        .write_all(&frame(true, TEXT, b"\xff\xfe"))
        .await
        .unwrap();
    closes(&mut stream).await;
    let report = timeout(PATIENCE, reports.recv()).await.unwrap().unwrap();
    assert!(report.is_err(), "{report:?}");
}

/// A client that only sends pings gets its pongs, and nothing else happens:
/// the handler sees them, and they cost no more than the frames they are.
#[tokio::test]
async fn pings_are_answered_for_the_handler() {
    let (server, mut reports) = reporting(|upgrade| upgrade).await;
    let mut stream = upgraded(server.addr).await;
    stream
        .write_all(&frame(true, 9, b"are you there"))
        .await
        .unwrap();
    let report = timeout(PATIENCE, reports.recv()).await.unwrap().unwrap();
    assert_eq!(report.unwrap(), Message::Ping(b"are you there"[..].into()));
    let mut pong = [0_u8; 15];
    timeout(Duration::from_secs(5), stream.read_exact(&mut pong))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&pong[..2], &[0x80 | 10, 13]);
    assert_eq!(&pong[2..], b"are you there");
}
