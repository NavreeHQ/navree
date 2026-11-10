//! HTTP/2 with prior knowledge: it works, and its limits hold, against
//! hyper's client and against raw frames for what no client would do.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use bytes::Bytes;
use http::{Request, Response, StatusCode, Version};
use http_body_util::{BodyExt, Full};
use hyper::client::conn::http2::{Builder, SendRequest};
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use navree_http::ServerOptions;
use navree_http::body::Body;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{Notify, Semaphore};
use tokio::task::JoinHandle;
use tokio::time::timeout;

use crate::support::{PATIENCE, answer, connect, echo_app, no_limits, start};

const LIMIT_MS: u64 = 300;
const LIMIT: Duration = Duration::from_millis(LIMIT_MS);

type Sender = SendRequest<Full<Bytes>>;

/// An HTTP/2 client; the task ends when the connection does.
async fn client(addr: SocketAddr, ping_every: Option<Duration>) -> (Sender, JoinHandle<bool>) {
    let stream = connect(addr).await;
    let (sender, connection) = Builder::new(TokioExecutor::new())
        .timer(TokioTimer::new())
        .keep_alive_interval(ping_every)
        .handshake(TokioIo::new(stream))
        .await
        .expect("the HTTP/2 handshake");
    (sender, tokio::spawn(async { connection.await.is_ok() }))
}

fn get(path: &str) -> Request<Full<Bytes>> {
    Request::get(format!("http://test{path}"))
        .body(Full::default())
        .unwrap()
}

/// `(status, version, body)` of one request.
async fn send(sender: &mut Sender, request: Request<Full<Bytes>>) -> (StatusCode, Version, Bytes) {
    let response = sender.send_request(request).await.expect("a response");
    let (parts, body) = response.into_parts();
    let body = body.collect().await.expect("the body").to_bytes();
    (parts.status, parts.version, body)
}

#[tokio::test]
async fn requests_are_served_over_http2() {
    let server = start(echo_app(), ServerOptions::default()).await;
    let (mut sender, _connection) = client(server.addr, None).await;
    let (status, version, body) = send(&mut sender, get("/")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(version, Version::HTTP_2);
    assert_eq!(&body[..], b"hello");

    let upload = Request::post("http://test/echo")
        .body(Full::new(Bytes::from_static(b"over two")))
        .unwrap();
    let (status, _, body) = send(&mut sender, upload).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(&body[..], b"over two");
}

/// One connection, many requests at once: they run concurrently.
#[tokio::test]
async fn streams_of_a_connection_run_at_the_same_time() {
    let arrived = Arc::new(Semaphore::new(0));
    let release = Arc::new(Notify::new());
    let app = {
        let (arrived, release) = (arrived.clone(), release.clone());
        answer(move |_request| {
            let (arrived, release) = (arrived.clone(), release.clone());
            async move {
                arrived.add_permits(1);
                release.notified().await;
                Response::new(Body::from("done"))
            }
        })
    };
    let server = start(app, no_limits()).await;
    let (sender, _connection) = client(server.addr, None).await;
    let requests: Vec<_> = (0..8)
        .map(|_| {
            let mut sender = sender.clone();
            tokio::spawn(async move { send(&mut sender, get("/")).await })
        })
        .collect();
    let all_here = arrived.acquire_many(8).await.unwrap();
    all_here.forget();
    for _ in 0..8 {
        release.notify_one();
    }
    for request in requests {
        assert_eq!(request.await.unwrap().0, StatusCode::OK);
    }
}

#[tokio::test]
async fn an_idle_connection_gets_a_goaway_though_the_client_keeps_pinging() {
    let server = start(
        echo_app(),
        ServerOptions {
            idle_timeout_ms: Some(LIMIT_MS),
            ..no_limits()
        },
    )
    .await;
    let (mut sender, connection) = client(server.addr, Some(Duration::from_millis(20))).await;
    assert_eq!(send(&mut sender, get("/")).await.0, StatusCode::OK);
    let started = Instant::now();
    let graceful = timeout(PATIENCE, connection)
        .await
        .expect("the connection ends")
        .unwrap();
    assert!(graceful, "a GOAWAY, not a dropped socket");
    assert!(started.elapsed() >= LIMIT / 2, "{:?}", started.elapsed());
}

#[tokio::test]
async fn a_request_in_flight_keeps_an_idle_timeout_away() {
    let release = Arc::new(Notify::new());
    let app = {
        let release = release.clone();
        answer(move |_request| {
            let release = release.clone();
            async move {
                release.notified().await;
                Response::new(Body::from("done"))
            }
        })
    };
    let server = start(
        app,
        ServerOptions {
            idle_timeout_ms: Some(LIMIT_MS),
            ..no_limits()
        },
    )
    .await;
    let (mut sender, _connection) = client(server.addr, None).await;
    let request = tokio::spawn(async move { send(&mut sender, get("/")).await });
    tokio::time::sleep(LIMIT * 3).await;
    release.notify_one();
    assert_eq!(request.await.unwrap().0, StatusCode::OK);
}

/// A stream ends on a task of its own: the idle timeout still starts when
/// it does, and until then the connection is busy.
#[tokio::test]
async fn the_idle_timeout_starts_when_a_slow_response_ends() {
    let app = answer(|_request| async {
        let chunks = futures_util::stream::unfold(0, |sent| async move {
            if sent == 3 {
                return None;
            }
            tokio::time::sleep(LIMIT).await;
            Some((Ok::<_, Infallible>(Bytes::from("chunk;")), sent + 1))
        });
        Response::new(Body::from_stream(chunks))
    });
    let server = start(
        app,
        ServerOptions {
            idle_timeout_ms: Some(LIMIT_MS / 3),
            ..no_limits()
        },
    )
    .await;
    let (mut sender, connection) = client(server.addr, None).await;
    let (status, _, body) = send(&mut sender, get("/")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(&body[..], b"chunk;chunk;chunk;");
    timeout(PATIENCE, connection)
        .await
        .expect("the connection ends once it is idle")
        .unwrap();
}

#[tokio::test]
async fn a_new_connection_must_open_a_stream_within_the_header_timeout() {
    let server = start(
        echo_app(),
        ServerOptions {
            header_read_timeout_ms: Some(LIMIT_MS),
            ..no_limits()
        },
    )
    .await;
    let started = Instant::now();
    let (_sender, connection) = client(server.addr, Some(Duration::from_millis(20))).await;
    timeout(PATIENCE, connection)
        .await
        .expect("the connection ends")
        .unwrap();
    assert!(started.elapsed() >= LIMIT, "{:?}", started.elapsed());
}

// Raw frames, for what hyper's client will not do.

const PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";
const HEADERS: u8 = 1;
const SETTINGS: u8 = 4;
const PING: u8 = 6;
const GOAWAY: u8 = 7;
const CONTINUATION: u8 = 9;
const END_STREAM: u8 = 1;
const END_HEADERS: u8 = 4;

fn frame(kind: u8, flags: u8, stream: u32, payload: &[u8]) -> Vec<u8> {
    let mut frame = (payload.len() as u32).to_be_bytes()[1..].to_vec();
    frame.push(kind);
    frame.push(flags);
    frame.extend_from_slice(&stream.to_be_bytes());
    frame.extend_from_slice(payload);
    frame
}

/// The header block of `GET /` in HPACK's static table.
const GET_ROOT: &[u8] = &[
    0x82, // :method GET
    0x86, // :scheme http
    0x84, // :path /
    0x01, 0x04, b't', b'e', b's', b't', // :authority test
];

async fn raw(addr: SocketAddr) -> TcpStream {
    let mut stream = connect(addr).await;
    stream.write_all(PREFACE).await.unwrap();
    stream.write_all(&frame(SETTINGS, 0, 0, &[])).await.unwrap();
    stream
}

/// The kind and payload of the next frame; `None` once the connection ends.
async fn next_frame(stream: &mut TcpStream) -> Option<(u8, Vec<u8>)> {
    let mut head = [0_u8; 9];
    stream.read_exact(&mut head).await.ok()?;
    let length = u32::from_be_bytes([0, head[0], head[1], head[2]]) as usize;
    let mut payload = vec![0_u8; length];
    stream.read_exact(&mut payload).await.ok()?;
    Some((head[3], payload))
}

/// An idle connection hears why it is closing, and the socket stays open
/// long enough for a request that crossed the GOAWAY to be answered. This
/// client never answers the server's ping, which would end the exchange
/// early. The time runs from before the request, so a late client can only
/// measure more of it.
#[tokio::test]
async fn an_idle_timeout_sends_a_goaway_and_lingers_before_closing() {
    let server = start(
        echo_app(),
        ServerOptions {
            idle_timeout_ms: Some(LIMIT_MS),
            ..no_limits()
        },
    )
    .await;
    let started = Instant::now();
    let mut client = raw(server.addr).await;
    client
        .write_all(&frame(HEADERS, END_STREAM | END_HEADERS, 1, GET_ROOT))
        .await
        .unwrap();
    let mut goaway = false;
    while let Some((kind, _)) = timeout(PATIENCE, next_frame(&mut client))
        .await
        .expect("the server closes the connection")
    {
        goaway |= kind == GOAWAY;
    }
    assert!(goaway, "the connection ended without a GOAWAY");
    // The idle timeout, then the second of linger.
    assert!(
        started.elapsed() >= LIMIT + Duration::from_millis(900),
        "{:?}",
        started.elapsed()
    );
}

/// A head that is never finished: the block stops short of `END_HEADERS`,
/// so no request exists and the frames extend nothing.
#[tokio::test]
async fn a_header_block_that_never_ends_is_cut_off() {
    let server = start(
        echo_app(),
        ServerOptions {
            header_read_timeout_ms: Some(LIMIT_MS),
            ..no_limits()
        },
    )
    .await;
    let started = Instant::now();
    let mut client = raw(server.addr).await;
    client
        .write_all(&frame(HEADERS, END_STREAM, 1, GET_ROOT))
        .await
        .unwrap();
    client
        .write_all(&frame(CONTINUATION, 0, 1, &[0x40, 0x01, b'a', 0x01, b'b']))
        .await
        .unwrap();
    while timeout(PATIENCE, next_frame(&mut client))
        .await
        .expect("the server closes the connection")
        .is_some()
    {}
    assert!(started.elapsed() >= LIMIT, "{:?}", started.elapsed());
}

/// The same trick after a first request: the idle timeout still ends it,
/// because half a header block is not a request.
#[tokio::test]
async fn half_a_header_block_does_not_keep_an_idle_connection_open() {
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
    client
        .write_all(&frame(HEADERS, END_STREAM, 3, GET_ROOT))
        .await
        .unwrap();
    while timeout(PATIENCE, next_frame(&mut client))
        .await
        .expect("the server closes the connection")
        .is_some()
    {}
}

/// `http2_keep_alive`: a peer that stops answering pings is dropped, even
/// with a request in flight, where no other limit applies.
#[tokio::test]
async fn a_client_that_stops_answering_pings_is_dropped() {
    let app = answer(|_request| async {
        std::future::pending::<()>().await;
        Response::new(Body::empty())
    });
    let server = start(
        app,
        ServerOptions {
            http2_keep_alive_interval_ms: Some(100),
            http2_keep_alive_timeout_ms: 100,
            ..no_limits()
        },
    )
    .await;
    let mut client = raw(server.addr).await;
    client
        .write_all(&frame(HEADERS, END_STREAM | END_HEADERS, 1, GET_ROOT))
        .await
        .unwrap();
    let mut pinged = false;
    while let Some((kind, _)) = timeout(PATIENCE, next_frame(&mut client))
        .await
        .expect("the server closes the connection")
    {
        pinged |= kind == PING;
    }
    assert!(pinged, "the server asked first");
}

/// The same request with the client answering: it stays connected.
#[tokio::test]
async fn a_client_that_answers_pings_stays() {
    let release = Arc::new(Notify::new());
    let app = {
        let release = release.clone();
        answer(move |_request| {
            let release = release.clone();
            async move {
                release.notified().await;
                Response::new(Body::from("done"))
            }
        })
    };
    let server = start(
        app,
        ServerOptions {
            http2_keep_alive_interval_ms: Some(100),
            http2_keep_alive_timeout_ms: 100,
            ..no_limits()
        },
    )
    .await;
    let (mut sender, _connection) = client(server.addr, None).await;
    let request = tokio::spawn(async move { send(&mut sender, get("/")).await });
    tokio::time::sleep(Duration::from_millis(700)).await;
    release.notify_one();
    assert_eq!(request.await.unwrap().0, StatusCode::OK);
}

#[tokio::test]
async fn a_header_list_over_the_limit_is_refused() {
    let server = start(
        echo_app(),
        ServerOptions {
            max_header_bytes: 8 * 1024,
            ..no_limits()
        },
    )
    .await;
    let (mut sender, _connection) = client(server.addr, None).await;
    let fits = Request::get("http://test/")
        .header("x-fits", "v".repeat(4 * 1024))
        .body(Full::default())
        .unwrap();
    assert_eq!(send(&mut sender, fits).await.0, StatusCode::OK);

    let over = Request::get("http://test/")
        .header("x-over", "v".repeat(16 * 1024))
        .body(Full::default())
        .unwrap();
    match sender.send_request(over).await {
        Ok(response) => assert_eq!(
            response.status(),
            StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE
        ),
        Err(error) => assert!(!error.is_incomplete_message(), "{error}"),
    }
}

/// HPACK makes a header cost a client one byte, so the limit on the size of a
/// list would let a thousand of them through to the app.
#[tokio::test]
async fn a_request_over_max_headers_is_refused_before_the_app() {
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
            max_headers: 8,
            ..no_limits()
        },
    )
    .await;
    let (mut sender, _connection) = client(server.addr, None).await;
    let with = |headers: usize| {
        let mut request = Request::get("http://test/");
        for header in 0..headers {
            request = request.header(format!("x-{header}"), "v");
        }
        request.body(Full::default()).unwrap()
    };
    assert_eq!(send(&mut sender, with(8)).await.0, StatusCode::OK);
    assert_eq!(reached.load(Ordering::SeqCst), 1);
    assert_eq!(
        send(&mut sender, with(9)).await.0,
        StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE
    );
    assert_eq!(reached.load(Ordering::SeqCst), 1, "it got to the app");
    assert_eq!(send(&mut sender, with(1)).await.0, StatusCode::OK);
    assert_eq!(reached.load(Ordering::SeqCst), 2, "the connection goes on");
}

/// An HTTP/2 connection drains on shutdown: GOAWAY, the open stream
/// finishes, and the client sees a clean end.
#[tokio::test]
async fn a_shutdown_finishes_open_streams() {
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let app = {
        let (started, release) = (started.clone(), release.clone());
        answer(move |_request| {
            let (started, release) = (started.clone(), release.clone());
            async move {
                started.notify_one();
                release.notified().await;
                Response::new(Body::from("done"))
            }
        })
    };
    let mut server = start(app, no_limits()).await;
    let (mut sender, connection) = client(server.addr, None).await;
    let request = tokio::spawn(async move { send(&mut sender, get("/")).await });
    started.notified().await;
    server.stop();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!server.finished());
    release.notify_one();
    let (status, _, body) = request.await.unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(&body[..], b"done");
    assert!(connection.await.unwrap(), "a clean end");
    server.stopped().await;
}

/// The server's first frame says it takes WebSockets over HTTP/2.
#[tokio::test]
async fn settings_announce_the_connect_protocol() {
    let server = start(echo_app(), no_limits()).await;
    let mut client = raw(server.addr).await;
    let (kind, payload) = timeout(PATIENCE, next_frame(&mut client))
        .await
        .expect("a frame")
        .expect("the server's settings");
    assert_eq!(kind, SETTINGS);
    let announced = payload
        .chunks(6)
        .any(|setting| setting == [0, 8, 0, 0, 0, 1]);
    assert!(announced, "SETTINGS_ENABLE_CONNECT_PROTOCOL: {payload:?}");
}
