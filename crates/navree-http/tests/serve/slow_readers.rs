//! `http.server.write_stall_timeout_ms`: a client that stops taking a response
//! loses it, and one that keeps taking it, however slowly, does not.
//!
//! The clients that keep moving read in gaps twenty times shorter than the
//! limit, so a loaded machine that delays them has to delay them by a whole
//! second to matter. They must still take longer than the limit to finish, or
//! they would prove nothing.

use std::convert::Infallible;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use bytes::Bytes;
use http::{Request, Response, StatusCode};
use http_body_util::{BodyExt, Empty};
use hyper::client::conn::http2::Builder;
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use navree_http::ServerOptions;
use navree_http::body::Body;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::support::{
    GET, OFF_MARGIN, PATIENCE, answer, connect, exchange, no_limits, read_response, start,
    stays_open,
};

const SHORT_LIMIT_MS: u64 = 300;
const SHORT_LIMIT: Duration = Duration::from_millis(SHORT_LIMIT_MS);
/// For the clients that keep moving: see the module documentation.
const LIMIT_MS: u64 = 1000;
const LIMIT: Duration = Duration::from_millis(LIMIT_MS);
const GAP: Duration = Duration::from_millis(50);

const CHUNK: usize = 64 * 1024;
const GET_BIG: &[u8] = b"GET /big HTTP/1.1\r\nHost: test\r\n\r\n";

/// `/big` sends chunks for as long as a client takes them, and anything else
/// says hello.
fn endless_app() -> impl tower::Service<
    Request<Body>,
    Response = Response<Body>,
    Error = Infallible,
    Future: Send + 'static,
> + Clone
+ Send
+ 'static {
    answer(|request: Request<Body>| async move {
        if request.uri().path() != "/big" {
            return Response::new(Body::from("hello"));
        }
        let chunks = futures_util::stream::repeat_with(|| {
            Ok::<_, Infallible>(Bytes::from(vec![7_u8; CHUNK]))
        });
        Response::new(Body::from_stream(chunks))
    })
}

fn stall_limit(ms: u64) -> ServerOptions {
    ServerOptions {
        write_stall_timeout_ms: Some(ms),
        max_connections: Some(1),
        ..no_limits()
    }
}

#[tokio::test]
async fn a_client_that_stops_reading_loses_its_connection() {
    let server = start(endless_app(), stall_limit(SHORT_LIMIT_MS)).await;
    let mut stopped = connect(server.addr).await;
    stopped.write_all(GET_BIG).await.unwrap();
    let started = Instant::now();

    let mut next = connect(server.addr).await;
    let response = exchange(&mut next, GET).await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(started.elapsed() >= SHORT_LIMIT, "{:?}", started.elapsed());
}

/// The hole the limit closes: with no limit that client keeps its slot.
#[tokio::test]
async fn with_no_limit_a_client_that_stops_reading_keeps_its_connection() {
    let server = start(
        endless_app(),
        ServerOptions {
            max_connections: Some(1),
            ..no_limits()
        },
    )
    .await;
    let mut stopped = connect(server.addr).await;
    stopped.write_all(GET_BIG).await.unwrap();
    tokio::time::sleep(SHORT_LIMIT).await;

    let mut next = connect(server.addr).await;
    next.write_all(GET).await.unwrap();
    assert!(stays_open(&mut next, SHORT_LIMIT * OFF_MARGIN).await);
}

/// A download that fills the socket again and again, and takes longer than
/// the limit. `sent` shows the server was waiting on this client: most of the
/// body had not left it when the client was a quarter of the way through.
#[tokio::test]
async fn a_slow_http1_download_that_keeps_moving_is_not_cut_off() {
    const TOTAL: u64 = 12 * 1024 * 1024;
    const READ: usize = 256 * 1024;
    let sent = Arc::new(AtomicU64::new(0));
    let app = {
        let sent = sent.clone();
        answer(move |_request| {
            let sent = sent.clone();
            async move {
                let chunks = futures_util::stream::unfold(0_u64, move |total| {
                    let sent = sent.clone();
                    async move {
                        if total >= TOTAL {
                            return None;
                        }
                        sent.fetch_add(CHUNK as u64, Ordering::Relaxed);
                        let chunk = Bytes::from(vec![7_u8; CHUNK]);
                        Some((Ok::<_, Infallible>(chunk), total + CHUNK as u64))
                    }
                });
                let response = Response::builder().header("content-length", TOTAL);
                response.body(Body::from_stream(chunks)).unwrap()
            }
        })
    };
    let server = start(app, stall_limit(LIMIT_MS)).await;
    let mut client = connect(server.addr).await;
    client.write_all(GET).await.unwrap();

    let started = Instant::now();
    let mut received = 0_u64;
    let mut sent_at_a_quarter = None;
    let mut chunk = vec![0_u8; READ];
    while received < TOTAL {
        tokio::time::sleep(GAP).await;
        let read = tokio::time::timeout(PATIENCE, client.read(&mut chunk))
            .await
            .expect("the download keeps moving")
            .expect("the connection stays");
        assert_ne!(read, 0, "cut off after {received} bytes");
        received += read as u64;
        if received >= TOTAL / 4 && sent_at_a_quarter.is_none() {
            sent_at_a_quarter = Some(sent.load(Ordering::Relaxed));
        }
    }
    let sent_at_a_quarter = sent_at_a_quarter.expect("a quarter of the way");
    assert!(sent_at_a_quarter < TOTAL, "{sent_at_a_quarter} sent");
    assert!(started.elapsed() > LIMIT, "{:?}", started.elapsed());
}

const MIB: usize = 1024 * 1024;

/// Reads the whole response of an HTTP/2 client with a small window, a frame
/// at a time; how many bytes it got. A client this slow takes about 3 s.
async fn slow_http2_download(addr: std::net::SocketAddr) -> usize {
    let stream = connect(addr).await;
    let (mut sender, connection) = Builder::new(TokioExecutor::new())
        .timer(TokioTimer::new())
        .initial_stream_window_size(64 * 1024)
        .handshake::<_, Empty<Bytes>>(TokioIo::new(stream))
        .await
        .unwrap();
    tokio::spawn(connection);
    let request = Request::get("http://test/").body(Empty::new()).unwrap();
    let response = sender.send_request(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body();
    let mut received = 0;
    while let Some(frame) = body.frame().await {
        let frame = frame.expect("the stream is not reset");
        received += frame.data_ref().map_or(0, Bytes::len);
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
    received
}

/// Chunks that each take longer than the limit to go out: they leave HTTP/2's
/// buffer a window at a time, and that has to count as moving.
#[tokio::test]
async fn a_slow_http2_download_of_large_chunks_is_not_cut_off() {
    const CHUNKS: usize = 3;
    let app = answer(|_request| async {
        let chunks = futures_util::stream::iter(
            (0..CHUNKS).map(|_| Ok::<_, Infallible>(Bytes::from(vec![7_u8; MIB]))),
        );
        Response::new(Body::from_stream(chunks))
    });
    let server = start(
        app,
        ServerOptions {
            write_stall_timeout_ms: Some(LIMIT_MS),
            ..no_limits()
        },
    )
    .await;
    let started = Instant::now();
    assert_eq!(slow_http2_download(server.addr).await, CHUNKS * MIB);
    assert!(started.elapsed() > LIMIT, "{:?}", started.elapsed());
}

/// One chunk, queued whole, would look like a finished response, and the idle
/// timeout would run under the download.
#[tokio::test]
async fn a_slow_http2_download_is_not_an_idle_connection() {
    const TOTAL: usize = 3 * MIB;
    let app =
        answer(|_request| async { Response::new(Body::from(Bytes::from(vec![7_u8; TOTAL]))) });
    let server = start(
        app,
        ServerOptions {
            idle_timeout_ms: Some(LIMIT_MS),
            ..no_limits()
        },
    )
    .await;
    let started = Instant::now();
    assert_eq!(slow_http2_download(server.addr).await, TOTAL);
    assert!(started.elapsed() > LIMIT, "{:?}", started.elapsed());
}

/// A response with nothing to send is the handler's business, however long
/// the gaps between its chunks, on both protocols.
#[tokio::test]
async fn a_handler_taking_its_time_is_not_a_stall() {
    let app = answer(|_request| async {
        let chunks = futures_util::stream::unfold(0, |sent| async move {
            if sent == 3 {
                return None;
            }
            tokio::time::sleep(SHORT_LIMIT * 2).await;
            Some((Ok::<_, Infallible>(Bytes::from("chunk;")), sent + 1))
        });
        Response::new(Body::from_stream(chunks))
    });
    let server = start(app, stall_limit(SHORT_LIMIT_MS)).await;

    let mut client = connect(server.addr).await;
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: t\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let response = crate::support::read_until_closed(&mut client).await;
    assert_eq!(response.matches("chunk;").count(), 3, "{response}");

    let stream = connect(server.addr).await;
    let (mut sender, connection) = Builder::new(TokioExecutor::new())
        .timer(TokioTimer::new())
        .handshake::<_, Empty<Bytes>>(TokioIo::new(stream))
        .await
        .unwrap();
    tokio::spawn(connection);
    let request = Request::get("http://test/").body(Empty::new()).unwrap();
    let response = sender.send_request(request).await.unwrap();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&body[..], b"chunk;chunk;chunk;");
}

/// After a stall the connection is gone, and with it what the client was
/// sent: the next client does not wait on it.
#[tokio::test]
async fn a_stalled_connection_gives_back_its_slot_for_good() {
    let server = start(endless_app(), stall_limit(SHORT_LIMIT_MS)).await;
    for _ in 0..3 {
        let mut stopped = connect(server.addr).await;
        stopped.write_all(GET_BIG).await.unwrap();
        let mut next = connect(server.addr).await;
        next.write_all(GET).await.unwrap();
        let response = read_response(&mut next).await;
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    }
}
