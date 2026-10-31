//! A hostile client against a real socket: attacks that hold, kept so they
//! keep holding. A test marked `#[ignore]` is a measurement, run by hand.
//!
//! These use a multi-threaded runtime: the server's tasks must not share a
//! thread with the client that is flooding them.

use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use bytes::Bytes;
use http::{Method, Request, Response, StatusCode};
use navree_http::ServerOptions;
use navree_http::body::{Body, read_body};
use navree_http::routing::Router;
use navree_http::ws::{Message, WebSocket, WebSocketUpgrade};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio::net::{TcpSocket, TcpStream};
use tokio::sync::Notify;
use tokio::time::{sleep, timeout};

use crate::support::{
    Dribble, GET, PATIENCE, answer, connect, dribble, echo_app, exchange, no_limits, read_response,
    read_until_closed, start, stays_open,
};

const LIMIT_MS: u64 = 300;
const LIMIT: Duration = Duration::from_millis(LIMIT_MS);
/// Far past every limit: a test that waits this long has shown a limit does
/// not apply.
const WELL_PAST: Duration = Duration::from_secs(6);

// Frames.

const PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";
const DATA: u8 = 0;
const HEADERS: u8 = 1;
const RST_STREAM: u8 = 3;
const SETTINGS: u8 = 4;
const PING: u8 = 6;
const GOAWAY: u8 = 7;
const WINDOW_UPDATE: u8 = 8;
const CONTINUATION: u8 = 9;
const END_STREAM: u8 = 1;
const END_HEADERS: u8 = 4;
const ACK: u8 = 1;

fn frame(kind: u8, flags: u8, stream: u32, payload: &[u8]) -> Vec<u8> {
    let mut frame = (payload.len() as u32).to_be_bytes()[1..].to_vec();
    frame.push(kind);
    frame.push(flags);
    frame.extend_from_slice(&stream.to_be_bytes());
    frame.extend_from_slice(payload);
    frame
}

/// `GET /` in HPACK's static table.
const GET_ROOT: &[u8] = &[
    0x82, // :method GET
    0x86, // :scheme http
    0x84, // :path /
    0x01, 0x04, b't', b'e', b's', b't', // :authority test
];

/// `POST /echo`, from the static table and literals.
fn post_echo() -> Vec<u8> {
    let mut block = vec![0x83, 0x86]; // :method POST, :scheme http
    block.extend_from_slice(&[0x04, 0x05]); // :path, literal
    block.extend_from_slice(b"/echo");
    block.extend_from_slice(&[0x01, 0x04]);
    block.extend_from_slice(b"test");
    block
}

async fn raw(addr: SocketAddr) -> TcpStream {
    let mut stream = connect(addr).await;
    stream.write_all(PREFACE).await.unwrap();
    stream.write_all(&frame(SETTINGS, 0, 0, &[])).await.unwrap();
    stream
}

async fn next_frame(stream: &mut TcpStream) -> Option<(u8, u32, Vec<u8>)> {
    let mut head = [0_u8; 9];
    stream.read_exact(&mut head).await.ok()?;
    let length = u32::from_be_bytes([0, head[0], head[1], head[2]]) as usize;
    let id = u32::from_be_bytes([head[5], head[6], head[7], head[8]]);
    let mut payload = vec![0_u8; length];
    stream.read_exact(&mut payload).await.ok()?;
    Some((head[3], id, payload))
}

/// Frames until the connection ends, or `None` once `limit` passes.
async fn frames_until_closed(stream: &mut TcpStream, limit: Duration) -> Option<Vec<u8>> {
    timeout(limit, async {
        let mut kinds = Vec::new();
        while let Some((kind, _, _)) = next_frame(stream).await {
            kinds.push(kind);
        }
        kinds
    })
    .await
    .ok()
}

/// The server ended the connection over an attack, with the client's burst
/// still unread on its side, so the close is a reset, and a reset can take
/// whatever was on its way with it, the GOAWAY included. So the frames only
/// have to show a GOAWAY when the socket closed cleanly. `None` once `limit`
/// passes.
async fn ended_by_the_server(stream: &mut TcpStream, limit: Duration) -> Option<()> {
    timeout(limit, async {
        let mut kinds = Vec::new();
        let ended = loop {
            let mut head = [0_u8; 9];
            if let Err(error) = stream.read_exact(&mut head).await {
                break error.kind();
            }
            let length = u32::from_be_bytes([0, head[0], head[1], head[2]]) as usize;
            let mut payload = vec![0_u8; length];
            if let Err(error) = stream.read_exact(&mut payload).await {
                break error.kind();
            }
            kinds.push(head[3]);
        };
        println!(
            "attack ended by {ended:?}, GOAWAY seen: {}",
            kinds.contains(&GOAWAY)
        );
        assert!(
            kinds.contains(&GOAWAY) || ended != std::io::ErrorKind::UnexpectedEof,
            "a clean close with no GOAWAY: {kinds:?}"
        );
    })
    .await
    .ok()
}

/// A flood of `frames` until the server closes the connection or `limit`
/// passes, reading whatever comes back so the server's writes never stop it.
/// How long the connection lasted, and the bytes sent.
async fn flood(stream: TcpStream, frames: Vec<u8>, limit: Duration) -> (Duration, u64) {
    let started = Instant::now();
    let (mut reader, mut writer) = stream.into_split();
    let sent = Arc::new(AtomicU64::new(0));
    let burst: Vec<u8> = std::iter::repeat_n(frames, 8192 / 17 + 1)
        .flatten()
        .collect();
    let writing = {
        let sent = sent.clone();
        tokio::spawn(async move {
            while writer.write_all(&burst).await.is_ok() {
                sent.fetch_add(burst.len() as u64, Ordering::Relaxed);
            }
        })
    };
    let mut chunk = vec![0_u8; 64 * 1024];
    let closed = timeout(limit, async {
        loop {
            match reader.read(&mut chunk).await {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
        }
    })
    .await;
    writing.abort();
    let lasted = match closed {
        Ok(()) => started.elapsed(),
        Err(_) => limit,
    };
    (lasted, sent.load(Ordering::Relaxed))
}

// Timers under a flood.

/// tokio's cooperative budget: a task that spends it on socket reads gets
/// `Pending` from its next timer too, even one that has fired. The connection
/// polls hyper first, so a client that keeps the socket readable must not be
/// able to keep the clock from expiring.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_ping_flood_does_not_outlive_the_header_timeout() {
    let server = start(
        echo_app(),
        ServerOptions {
            header_read_timeout_ms: Some(LIMIT_MS),
            ..no_limits()
        },
    )
    .await;
    let client = raw(server.addr).await;
    let (lasted, sent) = flood(client, frame(PING, 0, 0, &[0; 8]), WELL_PAST).await;
    println!("ping flood: lasted {lasted:?}, {} MiB sent", sent >> 20);
    assert!(lasted < WELL_PAST, "still open after {lasted:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_ping_flood_does_not_outlive_the_idle_timeout() {
    let server = start(
        echo_app(),
        ServerOptions {
            idle_timeout_ms: Some(LIMIT_MS),
            ..no_limits()
        },
    )
    .await;
    let mut client = raw(server.addr).await;
    client
        .write_all(&frame(HEADERS, END_STREAM | END_HEADERS, 1, GET_ROOT))
        .await
        .unwrap();
    let (lasted, sent) = flood(client, frame(PING, 0, 0, &[0; 8]), WELL_PAST).await;
    println!("ping flood: lasted {lasted:?}, {} MiB sent", sent >> 20);
    assert!(lasted < WELL_PAST, "still open after {lasted:?}");
}

/// The server keeps serving someone else through that flood.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_ping_flood_does_not_stop_other_clients() {
    let server = start(echo_app(), ServerOptions::default()).await;
    let addr = server.addr;
    let flooding = tokio::spawn(async move {
        let client = raw(addr).await;
        flood(client, frame(PING, 0, 0, &[0; 8]), Duration::from_secs(3)).await
    });
    sleep(Duration::from_millis(200)).await;
    let mut slowest = Duration::ZERO;
    while !flooding.is_finished() {
        let started = Instant::now();
        let mut client = connect(addr).await;
        let response = exchange(&mut client, GET).await;
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        slowest = slowest.max(started.elapsed());
        sleep(Duration::from_millis(50)).await;
    }
    println!("slowest request during the flood: {slowest:?}");
    assert!(slowest < Duration::from_secs(2), "{slowest:?}");
}

// The connection cap.

/// One keep-alive request per slot, repeated inside the idle timeout: the
/// client finishes every request, breaks no limit, and at the defaults would
/// cost 134 requests a second to hold 10 000 connections. An idle connection
/// is closed for a client that waits, so this one loses slots, and getting
/// them back means queueing behind the clients already waiting. That is about
/// holding slots between requests: a client holding them with requests in
/// flight is `max_connections_per_ip`'s to limit.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn keep_alive_requests_cannot_keep_every_slot() {
    const SLOTS: usize = 8;
    let server = start(
        echo_app(),
        ServerOptions {
            header_read_timeout_ms: Some(LIMIT_MS),
            idle_timeout_ms: Some(LIMIT_MS),
            body_read_timeout_ms: Some(LIMIT_MS),
            max_connections: Some(SLOTS),
            ..no_limits()
        },
    )
    .await;
    let mut holders = Vec::new();
    for _ in 0..SLOTS {
        let addr = server.addr;
        holders.push(tokio::spawn(async move {
            let mut client = connect(addr).await;
            loop {
                let sent = client.write_all(GET).await;
                let response = match sent {
                    Ok(()) => read_response(&mut client).await,
                    Err(_) => String::new(),
                };
                if response.is_empty() {
                    client = connect(addr).await;
                    continue;
                }
                assert!(response.starts_with("HTTP/1.1 200"), "{response}");
                sleep(LIMIT / 3).await;
            }
        }));
    }
    sleep(LIMIT).await;
    let mut honest = connect(server.addr).await;
    honest.write_all(GET).await.unwrap();
    let served = timeout(WELL_PAST, honest.read_u8()).await;
    for holder in &holders {
        assert!(!holder.is_finished(), "a holder gave up");
        holder.abort();
    }
    assert!(served.is_ok(), "an honest client waited {WELL_PAST:?}");
}

async fn silent_socket(mut socket: WebSocket) {
    while socket.recv().await.is_some() {}
}

fn websocket_app() -> Router {
    let socket = |upgrade: WebSocketUpgrade| async move { upgrade.on_upgrade(silent_socket) };
    Router::new()
        .route(Method::GET, "/ws", socket)
        .route(Method::CONNECT, "/ws", socket)
        .route(Method::GET, "/", || async { "hello" })
}

/// A WebSocket that says nothing keeps its slot through every limit of HTTP.
/// `upgraded_idle_timeout_ms` is the one that bounds it, off by default
/// because a quiet WebSocket can be a healthy one, so by default they can
/// fill a server: `by_default_a_quiet_websocket_stays` pins that.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn with_an_upgraded_idle_timeout_silent_websockets_give_their_slots_back() {
    let server = start(
        websocket_app(),
        ServerOptions {
            header_read_timeout_ms: Some(LIMIT_MS),
            idle_timeout_ms: Some(LIMIT_MS),
            body_read_timeout_ms: Some(LIMIT_MS),
            upgraded_idle_timeout_ms: Some(LIMIT_MS),
            max_connections: Some(2),
            ..no_limits()
        },
    )
    .await;
    let url = format!("ws://{}/ws", server.addr);
    let (_first, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    let (_second, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    let mut honest = connect(server.addr).await;
    honest.write_all(GET).await.unwrap();
    let served = timeout(WELL_PAST, honest.read_u8()).await;
    assert!(served.is_ok(), "an honest client waited {WELL_PAST:?}");
}

/// On HTTP/2 a WebSocket is one stream of the connection: when it ends there
/// is nothing in flight, and the idle timeout runs as for any other stream.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_http2_connection_goes_idle_after_its_websocket_ends() {
    let server = start(
        websocket_app(),
        ServerOptions {
            idle_timeout_ms: Some(LIMIT_MS),
            ..no_limits()
        },
    )
    .await;
    let mut client = raw(server.addr).await;
    let mut block = Vec::new();
    for (name, value) in [
        (":method", "CONNECT"),
        (":protocol", "websocket"),
        (":scheme", "http"),
        (":path", "/ws"),
        (":authority", "test"),
        ("sec-websocket-version", "13"),
    ] {
        block.push(0);
        block.push(name.len() as u8);
        block.extend_from_slice(name.as_bytes());
        block.push(value.len() as u8);
        block.extend_from_slice(value.as_bytes());
    }
    client
        .write_all(&frame(HEADERS, END_HEADERS, 1, &block))
        .await
        .unwrap();
    loop {
        let (kind, _, _) = timeout(PATIENCE, next_frame(&mut client))
            .await
            .expect("the server answers")
            .expect("the connection stays");
        if kind == HEADERS {
            break;
        }
    }
    // The client closes the stream: no WebSocket, no request.
    client
        .write_all(&frame(RST_STREAM, 0, 1, &8_u32.to_be_bytes()))
        .await
        .unwrap();
    let ended = frames_until_closed(&mut client, WELL_PAST).await;
    assert!(ended.is_some(), "an idle connection outlived {LIMIT:?}");
}

/// A `CONNECT` answered 2xx that nobody upgrades, on HTTP/1: the socket goes
/// with the request, so the stopped clock cannot leave a slot behind.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_http1_connect_nobody_upgrades_gives_its_slot_back() {
    let app = answer(|_request| async { Response::new(Body::empty()) });
    let server = start(
        app,
        ServerOptions {
            max_connections: Some(1),
            ..no_limits()
        },
    )
    .await;
    let mut client = connect(server.addr).await;
    client
        .write_all(b"CONNECT test:80 HTTP/1.1\r\nHost: test:80\r\n\r\n")
        .await
        .unwrap();
    let response = read_until_closed(&mut client).await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    let mut next = connect(server.addr).await;
    let response = exchange(&mut next, GET).await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
}

/// Connections reset the moment they connect: no slot is lost and the
/// accept loop stays up.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_burst_of_resets_loses_no_slot() {
    const SLOTS: usize = 4;
    let server = start(
        echo_app(),
        ServerOptions {
            header_read_timeout_ms: Some(10_000),
            max_connections: Some(SLOTS),
            ..no_limits()
        },
    )
    .await;
    for _ in 0..1500 {
        let client = connect(server.addr).await;
        client.set_zero_linger().unwrap();
        drop(client);
    }
    let mut held = Vec::new();
    for _ in 0..SLOTS {
        let mut client = connect(server.addr).await;
        let response = exchange(&mut client, GET).await;
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        held.push(client);
    }
}

/// Every way a connection can end, one slot at a time: none keeps it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn no_way_of_ending_a_connection_keeps_its_slot() {
    let app = answer(|request: Request<Body>| async move {
        match request.uri().path() {
            "/panic" => panic!("a handler panics"),
            "/panic-in-body" => {
                let chunks = futures_util::stream::once(async {
                    if true {
                        panic!("a response body panics");
                    }
                    Ok::<_, std::convert::Infallible>(Bytes::new())
                });
                Response::new(Body::from_stream(chunks))
            }
            "/echo" => {
                let status = match read_body(request.into_body(), 1024).await {
                    Ok(_) => StatusCode::OK,
                    Err(error) => error.status(),
                };
                Response::builder()
                    .status(status)
                    .body(Body::empty())
                    .unwrap()
            }
            _ => Response::new(Body::from("hello")),
        }
    });
    let server = start(
        app,
        ServerOptions {
            header_read_timeout_ms: Some(LIMIT_MS),
            body_read_timeout_ms: Some(LIMIT_MS),
            max_connections: Some(1),
            ..no_limits()
        },
    )
    .await;
    let addr = server.addr;
    let served = || async move {
        let mut client = connect(addr).await;
        let response = exchange(&mut client, GET).await;
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    };

    let requests: [&[u8]; 8] = [
        // Silence, half a head, half a preface.
        b"",
        b"GET / HT",
        b"PRI * HTTP/2.0\r\n",
        // A body that stops.
        b"POST /echo HTTP/1.1\r\nHost: t\r\nContent-Length: 9\r\n\r\nhalf",
        b"POST /echo HTTP/1.1\r\nHost: t\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nhalf\r\n",
        // A malformed request, then two panics.
        b"NOT HTTP\r\n\r\n",
        b"GET /panic HTTP/1.1\r\nHost: t\r\n\r\n",
        b"GET /panic-in-body HTTP/1.1\r\nHost: t\r\n\r\n",
    ];
    for request in requests {
        let mut client = connect(addr).await;
        client.write_all(request).await.unwrap();
        read_until_closed(&mut client).await;
        timeout(PATIENCE, served())
            .await
            .unwrap_or_else(|_| panic!("the slot of {request:?} is gone"));
    }

    // The client walks away with a handler running, on both protocols.
    for request in [&b"POST /echo HTTP/1.1\r\nHost: t\r\nContent-Length: 9\r\n\r\n"[..]] {
        let mut client = connect(addr).await;
        client.write_all(request).await.unwrap();
        sleep(Duration::from_millis(50)).await;
        client.set_zero_linger().unwrap();
        drop(client);
        timeout(PATIENCE, served()).await.expect("the slot returns");
    }

    // HTTP/2: a panicking stream, a panicking stream body, a stalled body,
    // then the connection itself goes.
    let mut client = raw(addr).await;
    for (stream, path) in [(1_u32, "/panic"), (3, "/panic-in-body")] {
        let mut block = vec![0x82, 0x86, 0x04, path.len() as u8];
        block.extend_from_slice(path.as_bytes());
        block.extend_from_slice(&[0x01, 0x04]);
        block.extend_from_slice(b"test");
        client
            .write_all(&frame(HEADERS, END_STREAM | END_HEADERS, stream, &block))
            .await
            .unwrap();
    }
    client
        .write_all(&frame(HEADERS, END_HEADERS, 5, &post_echo()))
        .await
        .unwrap();
    sleep(Duration::from_millis(100)).await;
    drop(client);
    timeout(PATIENCE, served()).await.expect("the slot returns");
}

// HTTP/2 streams.

/// How many streams of a connection a handler sees at once, however early
/// they are opened: before the client has acknowledged any setting.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_stream_limit_holds_before_the_settings_are_acknowledged() {
    const LIMIT: usize = 16;
    let running = Arc::new(AtomicUsize::new(0));
    let most = Arc::new(AtomicUsize::new(0));
    let app = {
        let (running, most) = (running.clone(), most.clone());
        answer(move |_request| {
            let (running, most) = (running.clone(), most.clone());
            async move {
                let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                most.fetch_max(now, Ordering::SeqCst);
                sleep(Duration::from_millis(500)).await;
                running.fetch_sub(1, Ordering::SeqCst);
                Response::new(Body::empty())
            }
        })
    };
    let server = start(
        app,
        ServerOptions {
            http2_max_concurrent_streams: LIMIT as u32,
            ..no_limits()
        },
    )
    .await;
    let mut client = connect(server.addr).await;
    let mut burst = PREFACE.to_vec();
    burst.extend(frame(SETTINGS, 0, 0, &[]));
    for stream in 0..200 {
        burst.extend(frame(
            HEADERS,
            END_STREAM | END_HEADERS,
            stream * 2 + 1,
            GET_ROOT,
        ));
    }
    client.write_all(&burst).await.unwrap();
    sleep(Duration::from_millis(300)).await;
    assert!(most.load(Ordering::SeqCst) <= LIMIT, "{most:?}");
}

/// Rapid reset in one burst: h2 counts streams reset before they are accepted
/// and ends the connection.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_rapid_reset_burst_ends_the_connection() {
    let started = Arc::new(AtomicUsize::new(0));
    let app = {
        let started = started.clone();
        answer(move |_request| {
            started.fetch_add(1, Ordering::SeqCst);
            async {
                std::future::pending::<()>().await;
                Response::new(Body::empty())
            }
        })
    };
    let server = start(app, ServerOptions::default()).await;
    let mut client = raw(server.addr).await;
    let mut burst = Vec::new();
    for stream in 0..4096_u32 {
        let id = stream * 2 + 1;
        burst.extend(frame(HEADERS, END_STREAM | END_HEADERS, id, GET_ROOT));
        burst.extend(frame(RST_STREAM, 0, id, &8_u32.to_be_bytes()));
    }
    let _ = client.write_all(&burst).await;
    ended_by_the_server(&mut client, PATIENCE)
        .await
        .expect("the server ends the connection");
    println!("rapid reset burst: {started:?} handlers started of 4096");
}

/// Counts the handlers alive, whatever way they end.
struct Running(Arc<AtomicUsize>);

impl Running {
    fn new(running: Arc<AtomicUsize>) -> Self {
        running.fetch_add(1, Ordering::SeqCst);
        Self(running)
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// A CONTINUATION flood: h2 stops it long before the limit on a head, with
/// nothing having reached the app and none of the flood answered. The client
/// reads as it writes, so the GOAWAY has every chance to arrive, and only a
/// reset may take its place: with the flood unread the server's close is one,
/// and a reset can overtake what was on its way, under load even for a client
/// that reads. A close that is not a reset must show why it happened.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_continuation_flood_ends_the_connection() {
    let reached = Arc::new(AtomicUsize::new(0));
    let app = {
        let reached = reached.clone();
        answer(move |_request| {
            reached.fetch_add(1, Ordering::SeqCst);
            async { Response::new(Body::empty()) }
        })
    };
    let server = start(app, ServerOptions::default()).await;
    let client = raw(server.addr).await;
    let (mut reader, mut writer) = client.into_split();
    let mut burst = frame(HEADERS, 0, 1, GET_ROOT);
    let filler = frame(CONTINUATION, 0, 1, &[0x40, 1, b'a', 1, b'b']);
    for _ in 0..50_000 {
        burst.extend_from_slice(&filler);
    }
    let writing = tokio::spawn(async move {
        let _ = writer.write_all(&burst).await;
        writer
    });
    let (frames, ended) = timeout(PATIENCE, all_frames(&mut reader))
        .await
        .expect("the server ends the connection");
    drop(writing);
    let kinds: Vec<u8> = frames.iter().map(|(kind, _)| *kind).collect();
    assert!(!kinds.contains(&HEADERS), "a request was served: {kinds:?}");
    assert_eq!(reached.load(Ordering::SeqCst), 0, "it reached the app");
    let goaway = frames
        .iter()
        .find(|(kind, _)| *kind == GOAWAY)
        .map(|(_, payload)| u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]));
    println!("continuation flood ended by {ended:?}, GOAWAY code {goaway:?}");
    match goaway {
        Some(code) => assert_eq!(code, ENHANCE_YOUR_CALM),
        None => assert_ne!(ended, std::io::ErrorKind::UnexpectedEof, "a clean close"),
    }
}

/// `SETTINGS` needs an answer, and h2 answers one before it reads the next,
/// so a client that floods them and reads the answers is a frame flood like
/// any other: the deadline does not care which.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_settings_flood_does_not_outlive_the_header_timeout() {
    let server = start(
        echo_app(),
        ServerOptions {
            header_read_timeout_ms: Some(LIMIT_MS),
            ..no_limits()
        },
    )
    .await;
    let client = raw(server.addr).await;
    let (lasted, sent) = flood(client, frame(SETTINGS, 0, 0, &[]), WELL_PAST).await;
    println!("settings flood: lasted {lasted:?}, {} MiB sent", sent >> 20);
    assert!(lasted < WELL_PAST, "still open after {lasted:?}");
}

/// Floods `SETTINGS` and reads nothing: whether the server ended it in
/// `patience`.
async fn a_settings_flood_that_reads_nothing_ends(
    server: ServerOptions,
    patience: Duration,
) -> bool {
    let server = start(echo_app(), server).await;
    let mut client = raw(server.addr).await;
    let burst: Vec<u8> = std::iter::repeat_n(frame(SETTINGS, 0, 0, &[]), 1024)
        .flatten()
        .collect();
    let flooding = async { while client.write_all(&burst).await.is_ok() {} };
    timeout(patience, flooding).await.is_ok()
}

/// A client that floods them and reads nothing has every answer waiting on
/// the server. h2 does not read on until the last one is written, so nothing
/// piles up there, and the write stall limit ends the connection.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_settings_flood_that_reads_nothing_ends_at_the_write_stall() {
    let limited = ServerOptions {
        write_stall_timeout_ms: Some(LIMIT_MS),
        ..no_limits()
    };
    assert!(a_settings_flood_that_reads_nothing_ends(limited, WELL_PAST).await);
}

/// The control: that limit is what ends it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn with_no_limit_a_settings_flood_that_reads_nothing_stays() {
    assert!(!a_settings_flood_that_reads_nothing_ends(no_limits(), LIMIT * 3).await);
}

/// A slow body on one stream: the stream ends with a 408 at its own limit,
/// the other stream of the connection is not touched, and the connection
/// stays.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stalled_stream_times_out_alone() {
    let server = start(
        echo_app(),
        ServerOptions {
            body_read_timeout_ms: Some(LIMIT_MS),
            ..no_limits()
        },
    )
    .await;
    let mut client = raw(server.addr).await;
    client
        .write_all(&frame(HEADERS, END_HEADERS, 1, &post_echo()))
        .await
        .unwrap();
    client
        .write_all(&frame(HEADERS, END_HEADERS, 3, &post_echo()))
        .await
        .unwrap();
    // Stream 3 keeps moving past stream 1's limit.
    for _ in 0..8 {
        sleep(LIMIT / 4).await;
        client
            .write_all(&frame(DATA, 0, 3, b"still here"))
            .await
            .unwrap();
    }
    let mut answered = Vec::new();
    timeout(PATIENCE, async {
        while !answered.contains(&1) {
            let (kind, stream, _) = next_frame(&mut client).await.expect("a frame");
            if kind == HEADERS {
                answered.push(stream);
            }
        }
    })
    .await
    .expect("stream 1 is answered");
    assert_eq!(answered, [1], "stream 3 is still being read");
}

/// Budget bypasses: bytes that are not body bytes earn nothing, whether they
/// are chunk extensions, trailers, or a client told to continue.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bytes_that_are_not_a_body_earn_no_time() {
    const CASES: [(&[u8], &[u8]); 3] = [
        (
            b"POST /echo HTTP/1.1\r\nHost: t\r\nTransfer-Encoding: chunked\r\n\r\n",
            b"4;name=value-value-value-value-value-value-value-value-value-value",
        ),
        (
            b"POST /echo HTTP/1.1\r\nHost: t\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n",
            b"x-trailer: value-value-value-value-value-value-value-value-value-value",
        ),
        (
            b"POST /echo HTTP/1.1\r\nHost: t\r\nContent-Length: 9\r\nExpect: 100-continue\r\n\r\n",
            b"",
        ),
    ];
    let server = start(
        echo_app(),
        ServerOptions {
            body_read_timeout_ms: Some(LIMIT_MS),
            ..no_limits()
        },
    )
    .await;
    for (head, drip) in CASES {
        let mut client = connect(server.addr).await;
        client.write_all(head).await.unwrap();
        let started = Instant::now();
        let ended = dribble(&mut client, drip, Duration::from_millis(50)).await;
        let response = match ended {
            Dribble::Sent => read_until_closed(&mut client).await,
            Dribble::Closed(seen) => seen,
        };
        assert!(
            started.elapsed() < LIMIT * 3,
            "{:?}: {:?}",
            String::from_utf8_lossy(head),
            started.elapsed()
        );
        assert!(
            response.is_empty() || response.contains(" 408 "),
            "{response}"
        );
    }
}

// Shutting down.

/// A client that ignores the GOAWAY, ignores its ping, and keeps a request
/// open cannot hold the server past its grace period.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_client_ignoring_the_goaway_cannot_outlast_the_grace_period() {
    let app = answer(|_request| async {
        std::future::pending::<()>().await;
        Response::new(Body::empty())
    });
    let mut server = crate::support::start_with(
        app,
        navree_http::HttpOptions {
            server: no_limits(),
            shutdown_grace_secs: 1,
            ..navree_http::HttpOptions::default()
        },
    )
    .await;
    let mut client = raw(server.addr).await;
    client
        .write_all(&frame(HEADERS, END_STREAM | END_HEADERS, 1, GET_ROOT))
        .await
        .unwrap();
    sleep(Duration::from_millis(100)).await;
    server.stop();
    let streams = tokio::spawn(async move {
        for stream in 1..u32::MAX {
            let opened = frame(HEADERS, END_STREAM | END_HEADERS, stream * 2 + 1, GET_ROOT);
            if client.write_all(&opened).await.is_err() {
                return;
            }
            sleep(Duration::from_millis(20)).await;
        }
    });
    let started = Instant::now();
    timeout(PATIENCE, server.stopped())
        .await
        .expect("the server ends");
    streams.abort();
    assert!(started.elapsed() < Duration::from_secs(5), "{started:?}");
}

/// The same on HTTP/1: sixteen connections each holding a handler that
/// blocks reading a body that never arrives, the body timeout off. Graceful
/// shutdown force-closes them at the grace period rather than waiting on the
/// clients.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn slow_clients_cannot_outlast_the_grace_period() {
    let mut server = crate::support::start_with(
        crate::support::echo_app(),
        navree_http::HttpOptions {
            server: ServerOptions {
                max_connections: Some(64),
                ..no_limits()
            },
            shutdown_grace_secs: 1,
            ..navree_http::HttpOptions::default()
        },
    )
    .await;
    let mut clients = Vec::new();
    for _ in 0..16 {
        let mut client = connect(server.addr).await;
        client
            .write_all(b"POST /echo HTTP/1.1\r\nHost: t\r\nContent-Length: 999\r\n\r\nx")
            .await
            .unwrap();
        clients.push(client);
    }
    sleep(Duration::from_millis(50)).await;
    server.stop();
    let started = Instant::now();
    timeout(Duration::from_secs(10), server.stopped())
        .await
        .expect("the server ends at its grace period");
    println!("grace period ended it after {:?}", started.elapsed());
    assert!(started.elapsed() < Duration::from_secs(5), "{started:?}");
}

// Headers.

/// A thousand empty headers on HTTP/1: `max_headers` turns them away.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_http1_request_cannot_carry_a_thousand_headers() {
    let server = start(echo_app(), ServerOptions::default()).await;
    let mut request = b"GET / HTTP/1.1\r\nHost: t\r\n".to_vec();
    for _ in 0..900 {
        request.extend_from_slice(b"a:\r\n");
    }
    request.extend_from_slice(b"\r\n");
    let mut client = connect(server.addr).await;
    let response = exchange(&mut client, &request).await;
    assert!(response.starts_with("HTTP/1.1 431"), "{response}");
}

/// `count` headers of a hundred bytes each, for `GET /`.
fn long_headers(count: usize) -> Vec<u8> {
    let mut block = GET_ROOT.to_vec();
    for _ in 0..count {
        block.extend_from_slice(&[0, 1, b'a', 100]);
        block.extend_from_slice(&[b'v'; 100]);
    }
    block
}

/// `max_header_bytes` is what the server announces, and it holds from the
/// first stream of a client that has not acknowledged anything.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn max_header_bytes_is_the_http2_header_list_limit() {
    const MAX: usize = 8 * 1024;
    let reached = Arc::new(AtomicUsize::new(0));
    let app = {
        let reached = reached.clone();
        answer(move |_request| {
            reached.fetch_add(1, Ordering::SeqCst);
            async { Response::new(Body::empty()) }
        })
    };
    let server = start(
        app,
        ServerOptions {
            max_header_bytes: MAX,
            max_headers: 10_000,
            ..no_limits()
        },
    )
    .await;
    let mut client = raw(server.addr).await;
    let (kind, _, settings) = timeout(PATIENCE, next_frame(&mut client))
        .await
        .expect("a frame")
        .expect("the server's settings");
    assert_eq!(kind, SETTINGS);
    let mut announced = vec![0, 6];
    announced.extend_from_slice(&(MAX as u32).to_be_bytes());
    assert!(
        settings.chunks(6).any(|setting| setting == announced),
        "SETTINGS_MAX_HEADER_LIST_SIZE: {settings:?}"
    );

    // Each header counts 133 bytes: 133 * 100 is over, 133 * 40 is not.
    let over = frame(HEADERS, END_STREAM | END_HEADERS, 1, &long_headers(100));
    client.write_all(&over).await.unwrap();
    let fits = frame(HEADERS, END_STREAM | END_HEADERS, 3, &long_headers(40));
    client.write_all(&fits).await.unwrap();
    timeout(PATIENCE, async {
        while reached.load(Ordering::SeqCst) == 0 {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the request that fits reaches the app");
    sleep(LIMIT).await;
    assert_eq!(reached.load(Ordering::SeqCst), 1, "the other one did too");
}

// Limits with one turned off.

/// With the header timeout off, the first byte after a response has nothing
/// to replace the idle deadline with, so that deadline stays.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_idle_timeout_holds_with_the_header_timeout_off() {
    let server = start(
        echo_app(),
        ServerOptions {
            idle_timeout_ms: Some(LIMIT_MS),
            ..no_limits()
        },
    )
    .await;
    let mut client = connect(server.addr).await;
    assert!(exchange(&mut client, GET).await.starts_with("HTTP/1.1 200"));
    client.write_all(b"G").await.unwrap();
    let closed = timeout(WELL_PAST, client.read_u8()).await;
    assert!(closed.is_ok(), "still open {WELL_PAST:?} later");
}

/// The rest of a pipelined head stays in hyper's buffer, so no read ever shows
/// its first byte. After a request without a body the head still has only the
/// header timeout.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_pipelined_head_is_held_to_the_header_timeout() {
    let server = start(
        echo_app(),
        ServerOptions {
            header_read_timeout_ms: Some(LIMIT_MS),
            idle_timeout_ms: Some(30_000),
            ..no_limits()
        },
    )
    .await;
    let mut client = connect(server.addr).await;
    let mut request = GET.to_vec();
    request.extend_from_slice(b"GET / HT");
    client.write_all(&request).await.unwrap();
    let first = read_response(&mut client).await;
    assert!(first.starts_with("HTTP/1.1 200"), "{first}");
    let closed = timeout(WELL_PAST, client.read_u8()).await;
    assert!(closed.is_ok(), "still open {WELL_PAST:?} later");
}

/// After a request with a body the server cannot see where the body ended, so
/// the start of a pipelined head has the idle timeout. That is the bound on
/// it: the connection does not stay.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_head_pipelined_after_a_body_is_held_to_the_idle_timeout() {
    const IDLE: Duration = Duration::from_millis(2000);
    let server = start(
        echo_app(),
        ServerOptions {
            header_read_timeout_ms: Some(LIMIT_MS),
            idle_timeout_ms: Some(IDLE.as_millis() as u64),
            ..no_limits()
        },
    )
    .await;
    let mut client = connect(server.addr).await;
    client
        .write_all(b"POST /echo HTTP/1.1\r\nHost: t\r\nContent-Length: 4\r\n\r\nbodyGET / HT")
        .await
        .unwrap();
    let first = read_response(&mut client).await;
    assert!(first.starts_with("HTTP/1.1 200"), "{first}");
    let started = Instant::now();
    assert_eq!(read_until_closed(&mut client).await, "");
    assert!(started.elapsed() >= IDLE / 2, "{:?}", started.elapsed());
}

/// A body that streams without end, for a client that never takes it.
fn endless_app() -> impl tower::Service<
    Request<Body>,
    Response = Response<Body>,
    Error = std::convert::Infallible,
    Future: Send + 'static,
> + Clone
+ Send
+ 'static {
    answer(|_request| async {
        let chunks = futures_util::stream::repeat_with(|| {
            Ok::<_, std::convert::Infallible>(Bytes::from(vec![7_u8; 16 * 1024]))
        });
        Response::new(Body::from_stream(chunks))
    })
}

const SETTINGS_INITIAL_WINDOW_SIZE: [u8; 2] = [0, 4];

/// `SETTINGS` opening every stream with a window of `size`.
fn window_settings(size: u32) -> Vec<u8> {
    let mut payload = SETTINGS_INITIAL_WINDOW_SIZE.to_vec();
    payload.extend_from_slice(&size.to_be_bytes());
    frame(SETTINGS, 0, 0, &payload)
}

/// A stream with no window: the socket is fine, the response just never gets
/// to move. Only that stream ends, and the connection goes on.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stream_held_back_by_flow_control_is_reset_alone() {
    let server = start(
        endless_app(),
        ServerOptions {
            write_stall_timeout_ms: Some(LIMIT_MS),
            ..no_limits()
        },
    )
    .await;
    let mut client = connect(server.addr).await;
    client.write_all(PREFACE).await.unwrap();
    client.write_all(&window_settings(0)).await.unwrap();
    client
        .write_all(&frame(HEADERS, END_STREAM | END_HEADERS, 1, GET_ROOT))
        .await
        .unwrap();
    let started = Instant::now();
    timeout(PATIENCE, async {
        loop {
            let (kind, stream, _) = next_frame(&mut client).await.expect("the connection stays");
            if kind == RST_STREAM && stream == 1 {
                return;
            }
        }
    })
    .await
    .expect("the stream is reset");
    assert!(started.elapsed() >= LIMIT, "{:?}", started.elapsed());

    client
        .write_all(&frame(HEADERS, END_STREAM | END_HEADERS, 3, GET_ROOT))
        .await
        .unwrap();
    timeout(PATIENCE, async {
        loop {
            let (kind, stream, _) = next_frame(&mut client).await.expect("the connection stays");
            if kind == HEADERS && stream == 3 {
                return;
            }
        }
    })
    .await
    .expect("the next stream is answered");
}

/// The same with no limit: the stream waits for as long as the client likes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn with_no_limit_a_stream_held_back_by_flow_control_stays() {
    let server = start(endless_app(), no_limits()).await;
    let mut client = connect(server.addr).await;
    client.write_all(PREFACE).await.unwrap();
    client.write_all(&window_settings(0)).await.unwrap();
    client
        .write_all(&frame(HEADERS, END_STREAM | END_HEADERS, 1, GET_ROOT))
        .await
        .unwrap();
    let reset = timeout(LIMIT * 3, async {
        loop {
            let (kind, _, _) = next_frame(&mut client).await.expect("the connection stays");
            if kind == RST_STREAM {
                return;
            }
        }
    });
    assert!(reset.await.is_err(), "the stream was reset");
}

/// An HTTP/2 client that stops reading the socket altogether, with a window
/// wide open: the connection has stopped moving, so the connection goes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_http2_client_that_stops_reading_loses_its_connection() {
    let app = answer(|request: Request<Body>| async move {
        if request.uri().path() == "/hello" {
            return Response::new(Body::from("hello"));
        }
        let chunks = futures_util::stream::repeat_with(|| {
            Ok::<_, std::convert::Infallible>(Bytes::from(vec![7_u8; 16 * 1024]))
        });
        Response::new(Body::from_stream(chunks))
    });
    let server = start(
        app,
        ServerOptions {
            write_stall_timeout_ms: Some(LIMIT_MS),
            max_connections: Some(1),
            ..no_limits()
        },
    )
    .await;
    let mut stopped = connect(server.addr).await;
    stopped.write_all(PREFACE).await.unwrap();
    stopped
        .write_all(&window_settings(i32::MAX as u32))
        .await
        .unwrap();
    stopped
        .write_all(&frame(WINDOW_UPDATE, 0, 0, &0x7fff_0000_u32.to_be_bytes()))
        .await
        .unwrap();
    stopped
        .write_all(&frame(HEADERS, END_STREAM | END_HEADERS, 1, GET_ROOT))
        .await
        .unwrap();
    let started = Instant::now();
    let mut next = connect(server.addr).await;
    let response = exchange(&mut next, b"GET /hello HTTP/1.1\r\nHost: t\r\n\r\n").await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(started.elapsed() >= LIMIT, "{:?}", started.elapsed());
}

/// A 2xx to an HTTP/2 `CONNECT` with nobody to upgrade: the stream is over
/// with its response, and the connection is idle like any other.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_http2_connect_nobody_upgrades_leaves_the_connection_idle() {
    let app = answer(|_request| async { Response::new(Body::empty()) });
    let server = start(
        app,
        ServerOptions {
            idle_timeout_ms: Some(LIMIT_MS),
            ..no_limits()
        },
    )
    .await;
    let mut client = raw(server.addr).await;
    let mut block = Vec::new();
    for (name, value) in [
        (":method", "CONNECT"),
        (":protocol", "websocket"),
        (":scheme", "http"),
        (":path", "/"),
        (":authority", "test"),
    ] {
        block.push(0);
        block.push(name.len() as u8);
        block.extend_from_slice(name.as_bytes());
        block.push(value.len() as u8);
        block.extend_from_slice(value.as_bytes());
    }
    client
        .write_all(&frame(HEADERS, END_HEADERS, 1, &block))
        .await
        .unwrap();
    let ended = frames_until_closed(&mut client, PATIENCE).await;
    assert!(ended.is_some(), "an idle connection stayed open");
}

/// A stalled body on HTTP/2 ends its stream: the client keeps its connection
/// and opens the next one.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_timed_out_stream_leaves_its_connection_open() {
    let server = start(
        echo_app(),
        ServerOptions {
            body_read_timeout_ms: Some(LIMIT_MS),
            ..no_limits()
        },
    )
    .await;
    let mut client = raw(server.addr).await;
    for stream in [1_u32, 3, 5] {
        client
            .write_all(&frame(HEADERS, END_HEADERS, stream, &post_echo()))
            .await
            .unwrap();
        timeout(PATIENCE, async {
            loop {
                let (kind, id, _) = next_frame(&mut client).await.expect("the connection stays");
                if kind == HEADERS && id == stream {
                    return;
                }
                if kind == PING {
                    let pong = frame(PING, ACK, 0, &[0; 8]);
                    client.write_all(&pong).await.unwrap();
                }
            }
        })
        .await
        .expect("the stream is answered");
    }
}

/// `Method` is in the imports for tests of an app with routes; a route that
/// is not `CONNECT` answers one with a 405 and the clock keeps running.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_router_answers_connect_with_a_405_and_the_clock_runs() {
    let app = navree_http::routing::Router::new().route(Method::GET, "/", || async { "hello" });
    let server = start(
        app,
        ServerOptions {
            idle_timeout_ms: Some(LIMIT_MS),
            ..no_limits()
        },
    )
    .await;
    let mut client = raw(server.addr).await;
    let mut block = Vec::new();
    for (name, value) in [
        (":method", "CONNECT"),
        (":protocol", "websocket"),
        (":scheme", "http"),
        (":path", "/"),
        (":authority", "test"),
    ] {
        block.push(0);
        block.push(name.len() as u8);
        block.extend_from_slice(name.as_bytes());
        block.push(value.len() as u8);
        block.extend_from_slice(value.as_bytes());
    }
    client
        .write_all(&frame(HEADERS, END_HEADERS | END_STREAM, 1, &block))
        .await
        .unwrap();
    frames_until_closed(&mut client, PATIENCE)
        .await
        .expect("the idle timeout ends it");
}

// The reclaim of idle connections.

fn capped(connections: usize) -> ServerOptions {
    ServerOptions {
        max_connections: Some(connections),
        ..no_limits()
    }
}

/// Whether `client` still gets an answer.
async fn still_served(client: &mut TcpStream) -> bool {
    client.write_all(GET).await.is_ok() && read_response(client).await.starts_with("HTTP/1.1 200")
}

/// Two idle connections fill the server, the oldest first.
async fn two_idle(addr: SocketAddr) -> (TcpStream, TcpStream) {
    let mut oldest = connect(addr).await;
    assert!(still_served(&mut oldest).await);
    sleep(Duration::from_millis(20)).await;
    let mut newest = connect(addr).await;
    assert!(still_served(&mut newest).await);
    (oldest, newest)
}

/// Having connected says nothing about wanting a slot. A client that has
/// already gone must not cost an honest connection, closed for nobody.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_client_that_has_already_left_evicts_nobody() {
    let server = start(echo_app(), capped(2)).await;
    let (mut oldest, _newest) = two_idle(server.addr).await;
    drop(connect(server.addr).await);
    sleep(LIMIT).await;
    assert!(still_served(&mut oldest).await, "evicted for nobody");
}

/// A client that sends nothing is not known to want anything, and must not
/// take the place of a connection that has finished its requests.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_silent_client_evicts_nobody() {
    let server = start(
        echo_app(),
        ServerOptions {
            header_read_timeout_ms: Some(30_000),
            ..capped(2)
        },
    )
    .await;
    let (mut oldest, _newest) = two_idle(server.addr).await;
    let _silent = connect(server.addr).await;
    sleep(LIMIT).await;
    assert!(still_served(&mut oldest).await, "evicted for silence");
}

/// Nor may it stand in the way: a client that does ask, behind it in the
/// backlog, gets an idle connection's slot as usual.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_silent_client_does_not_hold_up_the_client_behind_it() {
    let server = start(
        echo_app(),
        ServerOptions {
            header_read_timeout_ms: Some(30_000),
            ..capped(2)
        },
    )
    .await;
    let (_oldest, _newest) = two_idle(server.addr).await;
    let _silent = connect(server.addr).await;
    sleep(LIMIT).await;
    let mut asking = connect(server.addr).await;
    asking.write_all(GET).await.unwrap();
    let served = timeout(WELL_PAST, asking.read_u8()).await;
    assert!(served.is_ok(), "it waited {WELL_PAST:?} behind silence");
}

/// More silent clients than a full server keeps at its door: they give way,
/// and a client behind all of them gets in.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_crowd_of_silent_clients_does_not_hold_up_an_honest_one() {
    let server = start(
        echo_app(),
        ServerOptions {
            header_read_timeout_ms: Some(30_000),
            ..capped(2)
        },
    )
    .await;
    let (_oldest, _newest) = two_idle(server.addr).await;
    let mut crowd = Vec::new();
    for _ in 0..150 {
        crowd.push(connect(server.addr).await);
    }
    let mut asking = connect(server.addr).await;
    asking.write_all(GET).await.unwrap();
    let served = timeout(WELL_PAST, asking.read_u8()).await;
    assert!(served.is_ok(), "it waited {WELL_PAST:?} behind a crowd");
}

/// A client that says nothing at the door of a full server has the header
/// timeout, as it would with a slot.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_client_silent_at_the_door_is_closed_at_the_header_timeout() {
    let server = start(
        echo_app(),
        ServerOptions {
            header_read_timeout_ms: Some(LIMIT_MS),
            ..capped(1)
        },
    )
    .await;
    let mut held = connect(server.addr).await;
    assert!(still_served(&mut held).await);
    let mut silent = connect(server.addr).await;
    let closed = timeout(WELL_PAST, silent.read_u8()).await;
    assert!(closed.is_ok(), "still at the door {WELL_PAST:?} later");
    assert!(still_served(&mut held).await, "and it cost nobody anything");
}

/// How much of an 8 MiB download arrives when the server finishes writing it
/// while the client is behind, with the tail in hyper's buffer: `waiter` says
/// whether a client then comes for the slot.
async fn tail_of_a_download(waiter: bool) -> usize {
    const CHUNK: usize = 8 * 1024;
    const CHUNKS: usize = 1024;
    let finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let app = {
        let finished = finished.clone();
        answer(move |request: Request<Body>| {
            let finished = finished.clone();
            async move {
                if request.uri().path() == "/hello" {
                    return Response::new(Body::from("hello"));
                }
                let chunks = futures_util::stream::unfold(0, move |sent| {
                    let finished = finished.clone();
                    async move {
                        if sent == CHUNKS {
                            return None;
                        }
                        // hyper stops asking once it has the content length.
                        if sent + 1 == CHUNKS {
                            finished.store(true, Ordering::SeqCst);
                        }
                        let chunk = Bytes::from(vec![7_u8; CHUNK]);
                        Some((Ok::<_, std::convert::Infallible>(chunk), sent + 1))
                    }
                });
                Response::builder()
                    .header("content-length", CHUNK * CHUNKS)
                    .body(Body::from_stream(chunks))
                    .unwrap()
            }
        })
    };
    let server = start(app, capped(1)).await;
    let mut client = connect(server.addr).await;
    client.write_all(GET).await.unwrap();
    let mut seen = Vec::new();
    let mut chunk = vec![0_u8; 64 * 1024];
    while !finished.load(Ordering::SeqCst) {
        let read = timeout(PATIENCE, client.read(&mut chunk))
            .await
            .expect("the download moves")
            .unwrap();
        seen.extend_from_slice(&chunk[..read]);
        sleep(Duration::from_millis(1)).await;
    }
    // The last chunk reaches hyper's buffer and the body ends.
    sleep(Duration::from_millis(50)).await;
    if waiter {
        // Served only if the connection counted as idle: under load the tail
        // may still be moving, and then nothing is reclaimed.
        let mut next = connect(server.addr).await;
        let hello = b"GET /hello HTTP/1.1\r\nHost: t\r\n\r\n";
        let served = timeout(Duration::from_secs(2), exchange(&mut next, hello)).await;
        println!("the waiter was served: {}", served.is_ok());
    }
    let head_ends = seen.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    let rest = timeout(Duration::from_secs(5), async {
        while seen.len() - head_ends < CHUNK * CHUNKS {
            match client.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(read) => seen.extend_from_slice(&chunk[..read]),
            }
        }
    });
    let _ = rest.await;
    seen.len() - head_ends
}

/// The control: nobody comes for the slot, and every byte arrives.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_download_finishes_when_nobody_waits_for_its_slot() {
    assert_eq!(tail_of_a_download(false).await, 8 * 1024 * 1024);
}

/// The response body ending is not the response arriving: a tail left in
/// hyper's buffer must not be lost to a client that waits. How much is left
/// depends on the socket's buffers, so
/// `a_reclaim_does_not_cut_an_http1_response_left_in_hyper_buffer` is the one
/// that always leaves a tail.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_reclaim_does_not_cut_a_response_still_on_its_way() {
    let received = tail_of_a_download(true).await;
    println!(
        "reclaimed download: {received} of {} bytes",
        8 * 1024 * 1024
    );
    assert_eq!(received, 8 * 1024 * 1024);
}

/// Connections that are in-memory pipes, whose size is the same on every
/// system: a socket's buffers take as much as the system likes, and may take
/// a whole response at once.
struct Pipes(tokio::sync::mpsc::UnboundedReceiver<DuplexStream>);

impl navree_http::serve::Listener for Pipes {
    type Stream = DuplexStream;
    type Io = DuplexStream;
    type Addr = ();

    async fn accept(&mut self) -> (DuplexStream, ()) {
        match self.0.recv().await {
            Some(pipe) => (pipe, ()),
            None => std::future::pending().await,
        }
    }

    fn handshake(
        &self,
        stream: DuplexStream,
    ) -> impl Future<Output = std::io::Result<DuplexStream>> + Send + 'static {
        std::future::ready(Ok(stream))
    }
}

const PIPE: usize = 64 * 1024;
const LARGE_BODY: usize = 4 * 1024 * 1024;

/// A server on [`Pipes`] answering with [`LARGE_BODY`] in one piece, or
/// `hello` at `/hello`: the sender of its connections.
fn serve_pipes(server: ServerOptions) -> tokio::sync::mpsc::UnboundedSender<DuplexStream> {
    let app = answer(|request: Request<Body>| async move {
        match request.uri().path() {
            "/hello" => Response::new(Body::from("hello")),
            _ => Response::new(Body::from(vec![7_u8; LARGE_BODY])),
        }
    });
    let (connections, accepted) = tokio::sync::mpsc::unbounded_channel();
    let options = navree_http::HttpOptions {
        server,
        ..navree_http::HttpOptions::default()
    };
    tokio::spawn(async move {
        navree_http::serve(Pipes(accepted), app)
            .options(&options)
            .await
    });
    connections
}

/// Reads a response of [`LARGE_BODY`] until it is all there or the connection
/// closes: the bytes of its body.
async fn read_large_body(client: &mut DuplexStream) -> usize {
    let mut seen = Vec::new();
    let mut chunk = vec![0_u8; 256 * 1024];
    let read = timeout(PATIENCE, async {
        loop {
            match client.read(&mut chunk).await {
                Ok(0) | Err(_) => return,
                Ok(read) => seen.extend_from_slice(&chunk[..read]),
            }
            let head = seen.windows(4).position(|w| w == b"\r\n\r\n");
            if head.is_some_and(|head| seen.len() >= head + 4 + LARGE_BODY) {
                return;
            }
        }
    });
    read.await.expect("the response moves");
    let head = seen.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    seen.len() - head - 4
}

/// A client that reads nothing fills the pipe, and the response body ends
/// with the rest of the response in hyper's write buffer. The connection is
/// not idle: the waiter must wait while the client takes every byte, and is
/// then really served.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_reclaim_does_not_cut_an_http1_response_left_in_hyper_buffer() {
    let connections = serve_pipes(capped(1));
    let (mut client, pipe) = tokio::io::duplex(PIPE);
    connections.send(pipe).unwrap();
    client.write_all(GET).await.unwrap();
    sleep(LIMIT).await;

    let (mut next, pipe) = tokio::io::duplex(PIPE);
    connections.send(pipe).unwrap();
    next.write_all(b"GET /hello HTTP/1.1\r\nHost: t\r\n\r\n")
        .await
        .unwrap();
    let mut chunk = vec![0_u8; 4096];
    assert!(
        timeout(Duration::from_millis(1500), next.read(&mut chunk))
            .await
            .is_err(),
        "the waiter took the slot of a response still on its way"
    );

    let received = read_large_body(&mut client).await;
    println!("download with a waiter: {received} of {LARGE_BODY} bytes");
    assert_eq!(received, LARGE_BODY, "the response was cut");

    let served = timeout(PATIENCE, async {
        let mut answer = Vec::new();
        while !answer.ends_with(b"hello") {
            let read = next.read(&mut chunk).await.unwrap();
            assert_ne!(read, 0, "closed before it was served");
            answer.extend_from_slice(&chunk[..read]);
        }
    });
    served.await.expect("the waiter is served");
}

/// The idle timeout is for a connection with nothing going on: a client still
/// taking a response is not idle because hyper has the last of it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_idle_timeout_does_not_cut_a_response_left_in_hyper_buffer() {
    let connections = serve_pipes(ServerOptions {
        idle_timeout_ms: Some(LIMIT_MS),
        ..no_limits()
    });
    let (mut client, pipe) = tokio::io::duplex(PIPE);
    connections.send(pipe).unwrap();
    client.write_all(GET).await.unwrap();
    sleep(LIMIT * 3).await;
    let received = read_large_body(&mut client).await;
    println!("download past the idle timeout: {received} of {LARGE_BODY} bytes");
    assert_eq!(received, LARGE_BODY, "the response was cut");
}

/// The same on HTTP/2, with a client's window holding the response back.
/// `body` makes the response, one chunk smaller than a frame: its last, so
/// hyper has nothing more to wait for once h2 has it.
async fn a_response_behind_flow_control_is_not_cut(body: fn(usize) -> Body) {
    const BODY: usize = 8 * 1024;
    const WINDOW: u32 = 1024;
    let app = answer(move |request: Request<Body>| async move {
        match request.version() {
            http::Version::HTTP_2 => Response::new(body(BODY)),
            _ => Response::new(Body::from("hello")),
        }
    });
    let server = start(app, capped(1)).await;
    let mut client = connect(server.addr).await;
    client.write_all(PREFACE).await.unwrap();
    client.write_all(&window_settings(WINDOW)).await.unwrap();
    client
        .write_all(&frame(HEADERS, END_STREAM | END_HEADERS, 1, GET_ROOT))
        .await
        .unwrap();
    let mut received = 0;
    while received < WINDOW as usize {
        let (kind, _, payload) = timeout(PATIENCE, next_frame(&mut client))
            .await
            .expect("a frame")
            .expect("the connection stays");
        if kind == DATA {
            received += payload.len();
        }
    }

    // Longer than a reclaimed connection lingers, or a reclaim would not show.
    let mut next = connect(server.addr).await;
    next.write_all(GET).await.unwrap();
    assert!(
        stays_open(&mut next, Duration::from_millis(1500)).await,
        "the waiter took the slot of a response still on its way"
    );

    let update = (BODY as u32).to_be_bytes();
    let _ = client.write_all(&frame(WINDOW_UPDATE, 0, 1, &update)).await;
    let _ = client.write_all(&frame(WINDOW_UPDATE, 0, 0, &update)).await;
    let rest = timeout(PATIENCE, async {
        while received < BODY {
            match next_frame(&mut client).await {
                Some((DATA, _, payload)) => received += payload.len(),
                Some(_) => {}
                None => return,
            }
        }
    });
    rest.await.expect("the rest of the response arrives");
    println!("HTTP/2 response behind flow control: {received} of {BODY} bytes");
    assert_eq!(received, BODY);

    let served = read_response(&mut next).await;
    assert!(served.starts_with("HTTP/1.1 200"), "{served}");
}

/// A body known to end with its last chunk: hyper would otherwise end the
/// stream with most of the chunk queued in h2.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_reclaim_does_not_cut_an_http2_response_still_on_its_way() {
    a_response_behind_flow_control_is_not_cut(|bytes| Body::from(vec![7_u8; bytes])).await;
}

/// A stream ends when it says so, after its last chunk has gone to h2.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_reclaim_does_not_cut_a_streamed_http2_response_still_on_its_way() {
    a_response_behind_flow_control_is_not_cut(|bytes| {
        Body::from_stream(futures_util::stream::once(async move {
            Ok::<_, std::convert::Infallible>(Bytes::from(vec![7_u8; bytes]))
        }))
    })
    .await;
}

/// Holding a response back until the client has room must not ask for room
/// it never needed: a window of exactly the body's size is enough, however
/// the body ends, and END_STREAM arrives with no more updates.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_window_of_exactly_the_body_finishes_the_response() {
    const BODY: usize = 8 * 1024;
    let app = answer(|request: Request<Body>| async move {
        if request.uri().path() == "/" {
            return Response::new(Body::from(vec![7_u8; BODY]));
        }
        let chunk = futures_util::stream::once(async {
            Ok::<_, std::convert::Infallible>(Bytes::from(vec![7_u8; BODY]))
        });
        Response::new(Body::from_stream(chunk))
    });
    let server = start(app, no_limits()).await;
    for path in ["/", "/streamed"] {
        let mut client = connect(server.addr).await;
        client.write_all(PREFACE).await.unwrap();
        client
            .write_all(&window_settings(BODY as u32))
            .await
            .unwrap();
        let mut block = vec![0x82, 0x86, 0x04, path.len() as u8];
        block.extend_from_slice(path.as_bytes());
        block.extend_from_slice(&[0x01, 0x04]);
        block.extend_from_slice(b"test");
        client
            .write_all(&frame(HEADERS, END_STREAM | END_HEADERS, 1, &block))
            .await
            .unwrap();
        let mut received = 0;
        let ended = timeout(PATIENCE, async {
            loop {
                let mut head = [0_u8; 9];
                client.read_exact(&mut head).await.unwrap();
                let length = u32::from_be_bytes([0, head[0], head[1], head[2]]) as usize;
                let mut payload = vec![0_u8; length];
                client.read_exact(&mut payload).await.unwrap();
                if head[3] == DATA {
                    received += length;
                    if head[4] & END_STREAM != 0 {
                        return;
                    }
                }
            }
        });
        ended.await.expect("the stream ends on its own");
        assert_eq!(received, BODY, "{path}");
    }
}

/// An HTTP/2 connection takes the linger to close, and the server looks for
/// an idle connection every 100 ms while a client waits. Until it has gone,
/// that one client must not cost any more connections.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_waiting_client_evicts_one_http2_connection() {
    const IDLE: usize = 6;
    let server = start(echo_app(), capped(IDLE)).await;
    let mut clients = Vec::new();
    for _ in 0..IDLE {
        let mut client = raw(server.addr).await;
        client
            .write_all(&frame(HEADERS, END_STREAM | END_HEADERS, 1, GET_ROOT))
            .await
            .unwrap();
        loop {
            let (kind, _, _) = timeout(PATIENCE, next_frame(&mut client))
                .await
                .expect("a frame")
                .expect("the connection stays");
            if kind == DATA {
                break;
            }
        }
        clients.push(client);
    }
    let mut waiting = connect(server.addr).await;
    let served = exchange(&mut waiting, GET).await;
    assert!(served.starts_with("HTTP/1.1 200"), "{served}");
    let mut evicted = 0;
    for client in &mut clients {
        let told = timeout(Duration::from_millis(100), async {
            while let Some((kind, _, _)) = next_frame(client).await {
                if kind == GOAWAY {
                    return true;
                }
            }
            true
        });
        evicted += usize::from(told.await.unwrap_or(false));
    }
    println!("one waiting client evicted {evicted} of {IDLE} HTTP/2 connections");
    assert_eq!(evicted, 1);
}

/// A connection from one local address of many.
async fn connect_from(ip: &str, addr: SocketAddr) -> TcpStream {
    let socket = TcpSocket::new_v4().unwrap();
    socket
        .bind(format!("{ip}:0").parse().unwrap())
        .expect("a loopback source address");
    socket.connect(addr).await.expect("connects")
}

/// Sending the first byte of the next head with each request keeps a
/// connection from ever looking idle. A few addresses doing that, each under
/// `max_connections_per_ip`, must not fill the server: a head that has only
/// started has not earned its slot.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_few_addresses_cannot_keep_every_slot_by_starting_the_next_head() {
    const ADDRESSES: u8 = 4;
    const PER_ADDRESS: usize = 2;
    let server = start(
        echo_app(),
        ServerOptions {
            header_read_timeout_ms: Some(LIMIT_MS),
            idle_timeout_ms: Some(LIMIT_MS),
            max_connections: Some(ADDRESSES as usize * PER_ADDRESS),
            max_connections_per_ip: Some(PER_ADDRESS),
            ..no_limits()
        },
    )
    .await;
    let mut holders = Vec::new();
    for address in 0..ADDRESSES {
        for _ in 0..PER_ADDRESS {
            let addr = server.addr;
            let ip = format!("127.0.0.{}", 2 + address);
            holders.push(tokio::spawn(async move {
                let mut client = connect_from(&ip, addr).await;
                client
                    .write_all(b"GET / HTTP/1.1\r\nHost: t\r\n\r\nG")
                    .await
                    .unwrap();
                // Until the server takes the slot back.
                while read_response(&mut client).await.starts_with("HTTP/1.1 200") {
                    sleep(LIMIT / 4).await;
                    let next = b"ET / HTTP/1.1\r\nHost: t\r\n\r\nG";
                    if client.write_all(next).await.is_err() {
                        return;
                    }
                }
            }));
        }
    }
    sleep(LIMIT).await;
    let holding_at_first = holders
        .iter()
        .filter(|holder| !holder.is_finished())
        .count();
    assert_eq!(
        holding_at_first,
        holders.len(),
        "the holders stay on their own"
    );
    let mut honest = connect_from("127.0.0.9", server.addr).await;
    honest.write_all(GET).await.unwrap();
    let served = timeout(WELL_PAST, honest.read_u8()).await;
    let holding = holders
        .iter()
        .filter(|holder| !holder.is_finished())
        .count();
    for holder in &holders {
        holder.abort();
    }
    assert!(
        served.is_ok(),
        "an honest client waited {WELL_PAST:?} with {holding} holders left"
    );
}

// The address limit.

/// A storm of connections over the limit costs the address nothing later:
/// no count stays behind, and another address is served throughout.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_storm_over_the_address_limit_leaves_no_count_behind() {
    let server = start(
        echo_app(),
        ServerOptions {
            max_connections_per_ip: Some(2),
            ..no_limits()
        },
    )
    .await;
    let mut first = connect(server.addr).await;
    let mut second = connect(server.addr).await;
    assert!(still_served(&mut first).await && still_served(&mut second).await);
    let addr = server.addr;
    let storm = tokio::spawn(async move {
        for _ in 0..1500 {
            drop(connect(addr).await);
        }
    });
    let mut other = connect_from("127.0.0.2", addr).await;
    let mut slowest = Duration::ZERO;
    while !storm.is_finished() {
        let started = Instant::now();
        assert!(still_served(&mut other).await, "another address is served");
        slowest = slowest.max(started.elapsed());
    }
    storm.await.unwrap();
    println!("slowest request of another address in the storm: {slowest:?}");
    assert!(still_served(&mut first).await, "the storm cost it nothing");
    drop(first);
    drop(second);
    timeout(PATIENCE, async {
        loop {
            sleep(Duration::from_millis(10)).await;
            let mut one = connect(server.addr).await;
            let mut two = connect(server.addr).await;
            if still_served(&mut one).await && still_served(&mut two).await {
                return;
            }
        }
    })
    .await
    .expect("the address has all of its limit back");
}

/// A panic and an upgrade both give the address back, the upgrade only when
/// its socket closes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_address_count_follows_the_socket() {
    let app = websocket_app().route(Method::GET, "/panic", || async {
        if true {
            panic!("a handler panics");
        }
    });
    let server = start(
        app,
        ServerOptions {
            max_connections_per_ip: Some(1),
            ..no_limits()
        },
    )
    .await;
    let mut client = connect(server.addr).await;
    client
        .write_all(b"GET /panic HTTP/1.1\r\nHost: t\r\n\r\n")
        .await
        .unwrap();
    read_until_closed(&mut client).await;

    let url = format!("ws://{}/ws", server.addr);
    let opened = timeout(PATIENCE, async {
        loop {
            sleep(Duration::from_millis(10)).await;
            if let Ok(socket) = tokio_tungstenite::connect_async(&url).await {
                return socket;
            }
        }
    });
    let (socket, _) = opened.await.expect("the panic gave the address back");
    let mut over = connect(server.addr).await;
    assert_eq!(read_until_closed(&mut over).await, "", "the socket counts");
    drop(socket);
    timeout(PATIENCE, async {
        loop {
            sleep(Duration::from_millis(10)).await;
            let mut next = connect(server.addr).await;
            if still_served(&mut next).await {
                return;
            }
        }
    })
    .await
    .expect("the closed socket gave the address back");
}

// The write stall.

/// Grants too small to move a chunk do not count as progress.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_trickle_of_window_updates_does_not_put_off_the_stall() {
    let server = start(
        endless_app(),
        ServerOptions {
            write_stall_timeout_ms: Some(LIMIT_MS),
            ..no_limits()
        },
    )
    .await;
    let mut client = connect(server.addr).await;
    client.write_all(PREFACE).await.unwrap();
    client.write_all(&window_settings(0)).await.unwrap();
    client
        .write_all(&frame(HEADERS, END_STREAM | END_HEADERS, 1, GET_ROOT))
        .await
        .unwrap();
    let (mut reader, mut writer) = client.into_split();
    let trickle = tokio::spawn(async move {
        let one = frame(WINDOW_UPDATE, 0, 1, &1_u32.to_be_bytes());
        while writer.write_all(&one).await.is_ok() {
            sleep(Duration::from_millis(50)).await;
        }
    });
    let reset = timeout(WELL_PAST, async {
        loop {
            let mut head = [0_u8; 9];
            reader.read_exact(&mut head).await.expect("a frame");
            let length = u32::from_be_bytes([0, head[0], head[1], head[2]]) as usize;
            let mut payload = vec![0_u8; length];
            reader.read_exact(&mut payload).await.expect("a frame");
            if head[3] == RST_STREAM {
                return;
            }
        }
    })
    .await;
    trickle.abort();
    assert!(reset.is_ok(), "the trickle put the stall off");
}

/// A WebSocket that keeps writing.
async fn writing_socket(mut socket: WebSocket) {
    let message = Bytes::from(vec![7_u8; 1024]);
    while socket.send(Message::Binary(message.clone())).await.is_ok() {}
}

/// A `CONNECT` for `path`.
fn connect_block(path: &str) -> Vec<u8> {
    let mut block = Vec::new();
    for (name, value) in [
        (":method", "CONNECT"),
        (":protocol", "websocket"),
        (":scheme", "http"),
        (":path", path),
        (":authority", "test"),
        ("sec-websocket-version", "13"),
    ] {
        block.push(0);
        block.push(name.len() as u8);
        block.extend_from_slice(name.as_bytes());
        block.push(value.len() as u8);
        block.extend_from_slice(value.as_bytes());
    }
    block
}

/// An HTTP/2 WebSocket of a handler that keeps writing, held back by a client
/// that gives it no window, on a server of one slot: a client waiting for
/// that slot.
async fn a_client_behind_a_websocket_held_back_by_flow_control(
    server: ServerOptions,
) -> (crate::support::Served, TcpStream, TcpStream) {
    let socket = |upgrade: WebSocketUpgrade| async move { upgrade.on_upgrade(writing_socket) };
    let app = Router::new()
        .route(Method::GET, "/ws", socket)
        .route(Method::CONNECT, "/ws", socket)
        .route(Method::GET, "/", || async { "hello" });
    let server = start(
        app,
        ServerOptions {
            max_connections: Some(1),
            ..server
        },
    )
    .await;
    let mut client = connect(server.addr).await;
    client.write_all(PREFACE).await.unwrap();
    client.write_all(&window_settings(0)).await.unwrap();
    client
        .write_all(&frame(HEADERS, END_HEADERS, 1, &connect_block("/ws")))
        .await
        .unwrap();
    loop {
        let (kind, _, _) = timeout(PATIENCE, next_frame(&mut client))
            .await
            .expect("the server answers")
            .expect("the connection stays");
        if kind == HEADERS {
            break;
        }
    }
    let mut honest = connect(server.addr).await;
    honest.write_all(GET).await.unwrap();
    (server, client, honest)
}

/// What bounds that client: it has gone silent, and that ends the socket.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_http2_websocket_held_back_by_flow_control_gives_up_its_slot_when_silent() {
    let (_server, _client, mut honest) =
        a_client_behind_a_websocket_held_back_by_flow_control(ServerOptions {
            upgraded_idle_timeout_ms: Some(LIMIT_MS),
            ..no_limits()
        })
        .await;
    let served = timeout(WELL_PAST, honest.read_u8()).await;
    assert!(served.is_ok(), "an honest client waited {WELL_PAST:?}");
}

/// The stall limit covers a handler writing to an HTTP/1 socket, and a stream
/// of an HTTP/2 response. It does not reach an HTTP/2 WebSocket, and this
/// pins that: hyper keeps the stream's send half to itself, so a write waiting
/// for the client's window and a socket with nothing to say look the same,
/// and a limit here would close healthy quiet sockets.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_write_stall_limit_does_not_reach_an_http2_websocket() {
    let (_server, _client, mut honest) =
        a_client_behind_a_websocket_held_back_by_flow_control(ServerOptions {
            write_stall_timeout_ms: Some(LIMIT_MS),
            ..no_limits()
        })
        .await;
    assert!(stays_open(&mut honest, LIMIT * 3).await);
}

// The upgraded idle timeout.

/// A WebSocket whose server pings, as GraphQL's does every 15 s.
async fn pinging_socket(mut socket: WebSocket) {
    loop {
        tokio::select! {
            message = socket.recv() => {
                if !matches!(message, Some(Ok(_))) {
                    return;
                }
            }
            () = sleep(LIMIT / 3) => {
                if socket.send(Message::Ping(Bytes::new())).await.is_err() {
                    return;
                }
            }
        }
    }
}

/// The timeout is set above the protocol's heartbeat, as the documentation
/// says, and the client says nothing, not even a pong. The server's own pings
/// are the only bytes, and they show nothing about the client.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_server_heartbeat_does_not_keep_a_silent_websocket_alive() {
    let socket = |upgrade: WebSocketUpgrade| async move { upgrade.on_upgrade(pinging_socket) };
    let app = Router::new()
        .route(Method::GET, "/ws", socket)
        .route(Method::CONNECT, "/ws", socket)
        .route(Method::GET, "/", || async { "hello" });
    let server = start(
        app,
        ServerOptions {
            upgraded_idle_timeout_ms: Some(LIMIT_MS),
            max_connections: Some(1),
            ..no_limits()
        },
    )
    .await;
    // Never polled again: no pong goes out.
    let (_socket, _) = tokio_tungstenite::connect_async(format!("ws://{}/ws", server.addr))
        .await
        .unwrap();
    let mut honest = connect(server.addr).await;
    honest.write_all(GET).await.unwrap();
    let served = timeout(WELL_PAST, honest.read_u8()).await;
    assert!(served.is_ok(), "an honest client waited {WELL_PAST:?}");
}

/// A client that talks in masked frames of 17 bytes, the second mask byte
/// being `mask`, on an HTTP/1 WebSocket: whether it is still open after five
/// timeouts of steady talk.
async fn a_17_byte_talker_stays(mask: u8) -> bool {
    let server = start(
        websocket_app(),
        ServerOptions {
            upgraded_idle_timeout_ms: Some(LIMIT_MS),
            ..no_limits()
        },
    )
    .await;
    let mut client = connect(server.addr).await;
    let response = exchange(
        &mut client,
        b"GET /ws HTTP/1.1\r\nHost: t\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\
          Sec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n",
    )
    .await;
    assert!(response.starts_with("HTTP/1.1 101"), "{response}");
    let mut message = vec![0x82, 0x80 | 11, 1, mask, 3, 4];
    message.extend_from_slice(&[0; 11]);
    assert_eq!(message.len(), 17);
    let mut byte = [0_u8; 1];
    for _ in 0..15 {
        if client.write_all(&message).await.is_err() {
            return false;
        }
        if timeout(LIMIT / 3, client.read(&mut byte)).await.is_ok() {
            return false;
        }
    }
    true
}

/// The control: any other 17 bytes are traffic.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_websocket_talking_in_17_byte_messages_stays() {
    assert!(a_17_byte_talker_stays(7).await);
}

/// On HTTP/1 the bytes are WebSocket frames: one of 17 bytes with a 6 in its
/// fourth has the shape of an HTTP/2 PING, and is traffic like any other.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_websocket_message_is_never_taken_for_an_http2_ping() {
    assert!(a_17_byte_talker_stays(6).await);
}

/// Two PINGs in one write say no more than one does.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pings_never_keep_a_quiet_http2_websocket_open() {
    let server = start(
        websocket_app(),
        ServerOptions {
            upgraded_idle_timeout_ms: Some(LIMIT_MS),
            ..no_limits()
        },
    )
    .await;
    let mut client = raw(server.addr).await;
    client
        .write_all(&frame(HEADERS, END_HEADERS, 1, &connect_block("/ws")))
        .await
        .unwrap();
    loop {
        let (kind, _, _) = timeout(PATIENCE, next_frame(&mut client))
            .await
            .expect("the server answers")
            .expect("the connection stays");
        if kind == HEADERS {
            break;
        }
    }
    let mut pings = frame(PING, 0, 0, &[0; 8]);
    pings.extend(frame(PING, 0, 0, &[1; 8]));
    let started = Instant::now();
    let mut chunk = [0_u8; 256];
    let closed = loop {
        if started.elapsed() >= WELL_PAST {
            break false;
        }
        if client.write_all(&pings).await.is_err() {
            break true;
        }
        if let Ok(Ok(0) | Err(_)) = timeout(LIMIT / 3, client.read(&mut chunk)).await {
            break true;
        }
    };
    assert!(closed, "open {WELL_PAST:?} past a limit of {LIMIT:?}");
}

// Floods and bursts, measured.

/// Other frames than PING: the deadline does not care which.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_window_update_flood_does_not_outlive_the_idle_timeout() {
    let server = start(
        echo_app(),
        ServerOptions {
            idle_timeout_ms: Some(LIMIT_MS),
            ..no_limits()
        },
    )
    .await;
    let mut client = raw(server.addr).await;
    client
        .write_all(&frame(HEADERS, END_STREAM | END_HEADERS, 1, GET_ROOT))
        .await
        .unwrap();
    let update = frame(WINDOW_UPDATE, 0, 0, &1_u32.to_be_bytes());
    let (lasted, sent) = flood(client, update, WELL_PAST).await;
    println!(
        "window update flood: lasted {lasted:?}, {} MiB sent",
        sent >> 20
    );
    assert!(lasted < WELL_PAST, "still open after {lasted:?}");
}

/// A flood with a request in flight is not cut, as the module says. One per
/// worker thread, measured from another client.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn floods_with_requests_in_flight_leave_other_clients_served() {
    let app = answer(|request: Request<Body>| async move {
        if request.uri().path() == "/" {
            std::future::pending::<()>().await;
        }
        Response::new(Body::from("hello"))
    });
    let server = start(app, ServerOptions::default()).await;
    let addr = server.addr;
    let floods: Vec<_> = (0..4)
        .map(|_| {
            tokio::spawn(async move {
                let mut client = raw(addr).await;
                client
                    .write_all(&frame(HEADERS, END_STREAM | END_HEADERS, 1, GET_ROOT))
                    .await
                    .unwrap();
                flood(client, frame(PING, 0, 0, &[0; 8]), Duration::from_secs(4)).await
            })
        })
        .collect();
    sleep(Duration::from_millis(200)).await;
    let mut slowest = Duration::ZERO;
    let mut served = 0;
    while floods.iter().any(|flood| !flood.is_finished()) {
        let started = Instant::now();
        let mut client = connect(addr).await;
        let response = exchange(&mut client, b"GET /x HTTP/1.1\r\nHost: t\r\n\r\n").await;
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        slowest = slowest.max(started.elapsed());
        served += 1;
        sleep(Duration::from_millis(50)).await;
    }
    for flood in floods {
        let (lasted, sent) = flood.await.unwrap();
        println!(
            "flood in flight: lasted {lasted:?}, {} MiB sent",
            sent >> 20
        );
    }
    println!("{served} honest requests, slowest {slowest:?}");
    assert!(slowest < Duration::from_secs(2), "{slowest:?}");
}

/// Every frame of a connection until it ends, and how it ended.
async fn all_frames(
    reader: &mut (impl tokio::io::AsyncRead + Unpin),
) -> (Vec<(u8, Vec<u8>)>, std::io::ErrorKind) {
    let mut frames = Vec::new();
    loop {
        let mut head = [0_u8; 9];
        if let Err(error) = reader.read_exact(&mut head).await {
            return (frames, error.kind());
        }
        let length = u32::from_be_bytes([0, head[0], head[1], head[2]]) as usize;
        let mut payload = vec![0_u8; length];
        if let Err(error) = reader.read_exact(&mut payload).await {
            return (frames, error.kind());
        }
        frames.push((head[3], payload));
    }
}

/// A burst of a `HEADERS` and `second(stream)` per stream, from a client that
/// reads as it writes so the GOAWAY has every chance to arrive: how many
/// handlers ran, and the GOAWAY's error code.
async fn burst_read_as_it_goes(second: impl Fn(u32) -> Vec<u8>) -> (usize, Option<u32>) {
    let started = Arc::new(AtomicUsize::new(0));
    let app = {
        let started = started.clone();
        answer(move |_request| {
            started.fetch_add(1, Ordering::SeqCst);
            async {
                std::future::pending::<()>().await;
                Response::new(Body::empty())
            }
        })
    };
    let server = start(app, ServerOptions::default()).await;
    let client = raw(server.addr).await;
    let (mut reader, mut writer) = client.into_split();
    let mut burst = Vec::new();
    for stream in 0..4096_u32 {
        let id = stream * 2 + 1;
        burst.extend(frame(HEADERS, END_STREAM | END_HEADERS, id, GET_ROOT));
        burst.extend(second(id));
    }
    let writing = tokio::spawn(async move {
        let _ = writer.write_all(&burst).await;
        writer
    });
    let (frames, ended) = timeout(PATIENCE, all_frames(&mut reader))
        .await
        .expect("the server ends the connection");
    drop(writing);
    let goaway = frames
        .iter()
        .find(|(kind, _)| *kind == GOAWAY)
        .map(|(_, payload)| u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]));
    let started = started.load(Ordering::SeqCst);
    println!("{started} handlers of 4096, GOAWAY code {goaway:?}, ended by {ended:?}");
    (started, goaway)
}

const PROTOCOL_ERROR: u32 = 1;
const ENHANCE_YOUR_CALM: u32 = 11;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_rapid_reset_burst_is_stopped_by_the_limit_on_resets() {
    let reset = |id| frame(RST_STREAM, 0, id, &8_u32.to_be_bytes());
    let (started, goaway) = burst_read_as_it_goes(reset).await;
    assert!(started <= 256, "{started} handlers ran");
    assert!(
        matches!(goaway, None | Some(ENHANCE_YOUR_CALM)),
        "{goaway:?}"
    );
}

/// MadeYouReset: frames that make the server reset streams itself. An update
/// that overflows a stream's window is a stream error, which the server
/// answers with a reset of its own, and h2 counts those. An update of zero
/// would not do: it is a connection error at the first frame and reaches no
/// limit at all.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_made_you_reset_burst_is_stopped_by_the_limit_on_resets() {
    let provoke = |id| frame(WINDOW_UPDATE, 0, id, &0x7fff_ffff_u32.to_be_bytes());
    let (started, goaway) = burst_read_as_it_goes(provoke).await;
    assert!(started <= 2048, "{started} handlers ran");
    // Under load a reset can overtake the GOAWAY even for a client that reads
    // as it writes. A protocol error would mean the limit was never reached.
    assert_ne!(goaway, Some(PROTOCOL_ERROR));
}

/// A rapid reset paced enough for hyper to accept each stream, so that h2's
/// counter never fills. It buys nothing but ordinary requests: every handler
/// is cancelled and nothing piles up.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_rapid_reset_slow_enough_to_be_accepted_leaves_nothing_running() {
    let started = Arc::new(AtomicUsize::new(0));
    let running = Arc::new(AtomicUsize::new(0));
    let app = {
        let (started, running) = (started.clone(), running.clone());
        answer(move |_request| {
            started.fetch_add(1, Ordering::SeqCst);
            let guard = Running::new(running.clone());
            async move {
                let _guard = guard;
                std::future::pending::<()>().await;
                Response::new(Body::empty())
            }
        })
    };
    let server = start(app, ServerOptions::default()).await;
    let mut client = raw(server.addr).await;
    let begun = Instant::now();
    let mut id = 1_u32;
    let mut sent = 0;
    while begun.elapsed() < Duration::from_secs(3) {
        let mut pair = frame(HEADERS, END_STREAM | END_HEADERS, id, GET_ROOT);
        pair.extend(frame(RST_STREAM, 0, id, &8_u32.to_be_bytes()));
        if client.write_all(&pair).await.is_err() {
            break;
        }
        sent += 1;
        id += 2;
        sleep(Duration::from_millis(1)).await;
    }
    sleep(LIMIT).await;
    println!("slow rapid reset: {sent} pairs, {started:?} handlers, {running:?} running");
    assert!(started.load(Ordering::SeqCst) > 0, "measured nothing");
    assert_eq!(running.load(Ordering::SeqCst), 0);
}

/// The memory of this process, by asking the operating system: the workspace
/// allows no counting allocator.
#[cfg(windows)]
fn working_set() -> u64 {
    let script = format!("(Get-Process -Id {}).WorkingSet64", std::process::id());
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .output()
        .expect("powershell runs");
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .expect("a number of bytes")
}

/// One byte on the wire per header, from HPACK's dynamic table: one entry in
/// a `HeaderMap`, as many as `max_headers` lets through. The default limits,
/// requests held by their handlers, eight connections.
#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "a measurement: fails when a connection's requests hold more than the bound"]
async fn hpack_does_not_multiply_a_clients_bytes_into_the_servers_memory() {
    // What a request costs with nothing in it comes first, and goes on what
    // a stream costs whatever a client sends.
    let (_, control) = memory_of_requests(1).await;
    let list_headers = ServerOptions::default().max_headers - 1;
    let (sent, held) = memory_of_requests(list_headers).await;
    let per_connection = held / MEASURED_CONNECTIONS as u64;
    println!(
        "hpack: {} KiB sent, {} MiB held ({} MiB with one header), {} MiB per connection, \
         {} GiB at 10 000",
        sent >> 10,
        held >> 20,
        control >> 20,
        per_connection >> 20,
        (per_connection * 10_000) >> 30
    );
    assert!(per_connection < 4 * 1024 * 1024, "{per_connection} bytes");
}

#[cfg(windows)]
const MEASURED_CONNECTIONS: usize = 8;

/// A full set of streams on each of [`MEASURED_CONNECTIONS`], `headers` to a
/// request, held by their handlers: the bytes sent and the memory held.
#[cfg(windows)]
async fn memory_of_requests(headers: usize) -> (u64, u64) {
    const CONNECTIONS: usize = MEASURED_CONNECTIONS;
    const STREAMS: u32 = 200;
    let arrived = Arc::new(AtomicUsize::new(0));
    let release = Arc::new(Notify::new());
    let app = {
        let (arrived, release) = (arrived.clone(), release.clone());
        answer(move |request: Request<Body>| {
            let (arrived, release) = (arrived.clone(), release.clone());
            async move {
                arrived.fetch_add(1, Ordering::SeqCst);
                release.notified().await;
                drop(request);
                Response::new(Body::empty())
            }
        })
    };
    let server = start(app, ServerOptions::default()).await;
    let before = working_set();
    let mut sent = 0_u64;
    let mut clients = Vec::new();
    for _ in 0..CONNECTIONS {
        let mut client = raw(server.addr).await;
        let mut first = post_echo();
        first.extend_from_slice(&[0x40, 0x01, b'a', 0x00]);
        first.extend(std::iter::repeat_n(0xBE, headers));
        let mut rest = post_echo();
        rest.extend(std::iter::repeat_n(0xBE, headers));
        let mut burst = frame(HEADERS, END_HEADERS, 1, &first);
        for stream in 1..STREAMS {
            burst.extend(frame(HEADERS, END_HEADERS, stream * 2 + 1, &rest));
        }
        client.write_all(&burst).await.unwrap();
        sent += burst.len() as u64;
        clients.push(client);
    }
    timeout(PATIENCE, async {
        while arrived.load(Ordering::SeqCst) < CONNECTIONS * STREAMS as usize {
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("every request reaches its handler");
    let held = working_set().saturating_sub(before);
    release.notify_waiters();
    (sent, held)
}

// Failing: every test below this line fails today, and passes once what it
// shows is fixed.

/// Told to go, stays: h2 takes new streams until a client acknowledges the
/// PING that comes with a graceful GOAWAY, and this one never does. It opens
/// a stream `every` so often, inside the linger, until the connection ends.
/// `answered` counts the responses it gets after the GOAWAY.
async fn stays_after_the_goaway(client: TcpStream, every: Duration, answered: Arc<AtomicU64>) {
    let (mut reader, mut writer) = client.into_split();
    let told = Arc::new(Notify::new());
    let reading = {
        let told = told.clone();
        tokio::spawn(async move {
            let mut gone = false;
            loop {
                let mut head = [0_u8; 9];
                if reader.read_exact(&mut head).await.is_err() {
                    return;
                }
                let length = u32::from_be_bytes([0, head[0], head[1], head[2]]) as usize;
                let mut payload = vec![0_u8; length];
                if reader.read_exact(&mut payload).await.is_err() {
                    return;
                }
                if head[3] == GOAWAY {
                    gone = true;
                    told.notify_one();
                }
                if gone && head[3] == HEADERS {
                    answered.fetch_add(1, Ordering::SeqCst);
                }
            }
        })
    };
    told.notified().await;
    let mut stream = 3_u32;
    while !reading.is_finished() {
        let opened = frame(HEADERS, END_STREAM | END_HEADERS, stream, GET_ROOT);
        if writer.write_all(&opened).await.is_err() {
            return;
        }
        stream += 2;
        sleep(every).await;
    }
}

/// An HTTP/2 connection with a finished request, the kind a reclaim closes.
async fn finished_a_request(addr: SocketAddr) -> TcpStream {
    let mut client = raw(addr).await;
    client
        .write_all(&frame(HEADERS, END_STREAM | END_HEADERS, 1, GET_ROOT))
        .await
        .unwrap();
    loop {
        let (kind, _, _) = timeout(PATIENCE, next_frame(&mut client))
            .await
            .expect("a frame")
            .expect("the connection stays");
        if kind == DATA {
            return client;
        }
    }
}

/// The reclaim of an idle connection, on HTTP/2. Every limit is on, and none
/// of them ends this client: a request every 100 ms is neither idle nor a dead
/// peer, and each one takes the linger's deadline away. The slot was given to
/// the honest client, and must reach it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_http2_client_ignoring_the_goaway_gives_a_reclaimed_slot_back() {
    let server = start(
        echo_app(),
        ServerOptions {
            header_read_timeout_ms: Some(LIMIT_MS),
            idle_timeout_ms: Some(LIMIT_MS),
            body_read_timeout_ms: Some(LIMIT_MS),
            write_stall_timeout_ms: Some(LIMIT_MS),
            http2_keep_alive_interval_ms: Some(LIMIT_MS),
            http2_keep_alive_timeout_ms: LIMIT_MS,
            max_connections: Some(1),
            ..no_limits()
        },
    )
    .await;
    let client = finished_a_request(server.addr).await;
    let answered = Arc::new(AtomicU64::new(0));
    let staying = tokio::spawn(stays_after_the_goaway(
        client,
        LIMIT / 3,
        answered.clone(),
    ));
    let mut honest = connect(server.addr).await;
    honest.write_all(GET).await.unwrap();
    let served = timeout(WELL_PAST, honest.read_u8()).await;
    staying.abort();
    assert!(
        served.is_ok(),
        "an honest client waited {WELL_PAST:?}, and the client told to go was answered \
         {answered:?} more times"
    );
}

/// The same with an idle connection right there. The honest client has had
/// its one reclaim, spent on a connection that stays, so it waits on with a
/// slot it could have had standing idle.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_http2_client_ignoring_the_goaway_leaves_nobody_waiting_behind_an_idle_slot() {
    let server = start(echo_app(), capped(2)).await;
    let oldest = finished_a_request(server.addr).await;
    let answered = Arc::new(AtomicU64::new(0));
    let staying = tokio::spawn(stays_after_the_goaway(
        oldest,
        LIMIT / 3,
        answered.clone(),
    ));
    sleep(Duration::from_millis(20)).await;
    let mut idle = finished_a_request(server.addr).await;
    let mut honest = connect(server.addr).await;
    honest.write_all(GET).await.unwrap();
    let served = timeout(WELL_PAST, honest.read_u8()).await;
    staying.abort();
    let told_to_go = timeout(LIMIT, async {
        while let Some((kind, _, _)) = next_frame(&mut idle).await {
            if kind == GOAWAY {
                return true;
            }
        }
        true
    });
    let told_to_go = told_to_go.await.unwrap_or(false);
    assert!(
        served.is_ok(),
        "an honest client waited {WELL_PAST:?}, the idle connection told to go: {told_to_go}, \
         the client told to go answered {answered:?} more times"
    );
}

/// Nor does a client with a reclaim still to come get it: none happens while
/// the last connection reclaimed is lingering, and between two requests this
/// one always is. One connection and two requests a second turn the reclaim
/// off for the whole server. Requests this far apart leave it in flight for
/// too little of the time for a look to land on one.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_http2_client_ignoring_the_goaway_holds_up_no_other_reclaim() {
    let server = start(
        echo_app(),
        ServerOptions {
            header_read_timeout_ms: Some(30_000),
            ..capped(2)
        },
    )
    .await;
    let oldest = finished_a_request(server.addr).await;
    let answered = Arc::new(AtomicU64::new(0));
    let staying = tokio::spawn(stays_after_the_goaway(
        oldest,
        Duration::from_millis(500),
        answered.clone(),
    ));
    sleep(Duration::from_millis(20)).await;
    let _idle = finished_a_request(server.addr).await;
    // Both at the door before either says anything, so the second is seen.
    let mut first = connect(server.addr).await;
    let mut second = connect(server.addr).await;
    sleep(LIMIT).await;
    first.write_all(GET).await.unwrap();
    second.write_all(GET).await.unwrap();
    let served = timeout(WELL_PAST, async {
        tokio::select! {
            _ = first.read_u8() => {}
            _ = second.read_u8() => {}
        }
    })
    .await;
    staying.abort();
    assert!(
        served.is_ok(),
        "two clients waited {WELL_PAST:?} with a connection idle all along, and the client \
         told to go was answered {answered:?} more times"
    );
}

/// Not committed: the control, a client that stops once told to go.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn control_a_client_that_leaves_gives_the_slot_back() {
    let server = start(echo_app(), capped(1)).await;
    let client = finished_a_request(server.addr).await;
    let answered = Arc::new(AtomicU64::new(0));
    let staying = tokio::spawn(stays_after_the_goaway(
        client,
        Duration::from_secs(60),
        answered.clone(),
    ));
    let mut honest = connect(server.addr).await;
    honest.write_all(GET).await.unwrap();
    let started = Instant::now();
    let served = timeout(WELL_PAST, honest.read_u8()).await;
    staying.abort();
    println!("control: {:?} after {:?}", served.is_ok(), started.elapsed());
    assert!(served.is_ok());
}
