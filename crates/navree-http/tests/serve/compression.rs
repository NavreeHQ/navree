//! Gzipped responses over real connections: a slow stream reaches the
//! client part by part, a client that goes away drops the handler's body,
//! and a client that stops reading holds the handler back.

use std::convert::Infallible;
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use bytes::Bytes;
use futures_util::StreamExt;
use http::{Request, Response, StatusCode};
use http_body_util::{BodyExt, Empty};
use hyper::body::Incoming;
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use navree_http::CompressionLayer;
use navree_http::body::Body;
use tokio::io::AsyncWriteExt;
use tokio::sync::Notify;
use tokio::time::timeout;
use tower::Layer;

use crate::support::{PATIENCE, answer, connect, no_limits, start};

const FIRST: &str = "the first part of a slow response, long enough to be compressed; ";
const SECOND: &str = "and the rest of it, sent when the test says so";

/// Sets its flag when dropped.
struct DropFlag(Arc<AtomicBool>);

impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// Gzips `/`: `FIRST`, then `SECOND` once `go` is notified. The handler's
/// stream sets `dropped` when it is dropped.
fn gated_app(
    go: Arc<Notify>,
    dropped: Arc<AtomicBool>,
) -> impl tower::Service<
    Request<Body>,
    Response = Response<Body>,
    Error = Infallible,
    Future: Send + 'static,
> + Clone
+ Send
+ 'static {
    let app = answer(move |_request| {
        let go = go.clone();
        let guard = DropFlag(dropped.clone());
        async move {
            let parts =
                futures_util::stream::unfold((0, go, guard), |(step, go, guard)| async move {
                    match step {
                        0 => Some((Ok::<_, Infallible>(Bytes::from(FIRST)), (1, go, guard))),
                        1 => {
                            go.notified().await;
                            Some((Ok(Bytes::from(SECOND)), (2, go, guard)))
                        }
                        _ => None,
                    }
                });
            Response::new(Body::from_stream(parts))
        }
    });
    CompressionLayer::new().layer(app)
}

fn gzip_get() -> Request<Empty<Bytes>> {
    Request::get("http://test/")
        .header("accept-encoding", "gzip")
        .body(Empty::new())
        .unwrap()
}

/// Reads frames of `body` into `decoded` until it holds `wanted`.
async fn read_until(
    body: &mut Incoming,
    decoded: &mut flate2::write::GzDecoder<Vec<u8>>,
    wanted: &str,
) {
    timeout(PATIENCE, async {
        while decoded.get_ref().as_slice() != wanted.as_bytes() {
            let frame = body
                .frame()
                .await
                .expect("the body goes on")
                .expect("a frame");
            if let Some(data) = frame.data_ref() {
                decoded.write_all(data).unwrap();
                decoded.flush().unwrap();
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("the client decodes {wanted:?}"));
}

async fn http1(
    addr: std::net::SocketAddr,
) -> (
    hyper::client::conn::http1::SendRequest<Empty<Bytes>>,
    tokio::task::JoinHandle<()>,
) {
    let (sender, connection) =
        hyper::client::conn::http1::handshake(TokioIo::new(connect(addr).await))
            .await
            .unwrap();
    let task = tokio::spawn(async move {
        let _ = connection.await;
    });
    (sender, task)
}

async fn http2(
    addr: std::net::SocketAddr,
) -> (
    hyper::client::conn::http2::SendRequest<Empty<Bytes>>,
    tokio::task::JoinHandle<()>,
) {
    let (sender, connection) = hyper::client::conn::http2::Builder::new(TokioExecutor::new())
        .timer(TokioTimer::new())
        .handshake(TokioIo::new(connect(addr).await))
        .await
        .unwrap();
    let task = tokio::spawn(async move {
        let _ = connection.await;
    });
    (sender, task)
}

fn check_head(response: &Response<Incoming>) {
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-encoding"], "gzip");
    assert_eq!(response.headers()["vary"], "accept-encoding");
}

/// What the handler sent before it waits reaches the client and decodes,
/// while the handler is still waiting; the rest follows.
#[tokio::test]
async fn a_slow_gzipped_stream_reaches_the_client_part_by_part() {
    for version in ["HTTP/1.1", "HTTP/2"] {
        let go = Arc::new(Notify::new());
        let server = start(
            gated_app(go.clone(), Arc::new(AtomicBool::new(false))),
            no_limits(),
        )
        .await;
        let response = if version == "HTTP/2" {
            http2(server.addr).await.0.send_request(gzip_get()).await
        } else {
            http1(server.addr).await.0.send_request(gzip_get()).await
        }
        .unwrap();
        check_head(&response);
        let mut body = response.into_body();
        let mut decoded = flate2::write::GzDecoder::new(Vec::new());
        read_until(&mut body, &mut decoded, FIRST).await;
        go.notify_one();
        let whole = format!("{FIRST}{SECOND}");
        read_until(&mut body, &mut decoded, &whole).await;
        // The end of the gzip stream, then the end of the body.
        let rest = timeout(PATIENCE, body.collect())
            .await
            .expect("the body ends")
            .expect("the body is whole")
            .to_bytes();
        decoded.write_all(&rest).unwrap();
        assert_eq!(decoded.finish().unwrap(), whole.as_bytes(), "{version}");
    }
}

/// Waits until `flag` is set.
async fn set(flag: &AtomicBool) -> bool {
    let started = Instant::now();
    while !flag.load(Ordering::SeqCst) {
        if started.elapsed() > PATIENCE {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    true
}

/// A client that goes away halfway through a gzipped stream (its
/// connection closed on HTTP/1, its stream cancelled on HTTP/2) leaves no
/// handler body behind.
#[tokio::test]
async fn a_client_gone_mid_gzipped_stream_drops_the_handlers_body() {
    for version in ["HTTP/1.1", "HTTP/2"] {
        let dropped = Arc::new(AtomicBool::new(false));
        // `go` is never notified: the handler waits for good.
        let server = start(
            gated_app(Arc::new(Notify::new()), dropped.clone()),
            no_limits(),
        )
        .await;
        let mut decoded = flate2::write::GzDecoder::new(Vec::new());
        if version == "HTTP/2" {
            let (mut sender, connection) = http2(server.addr).await;
            let response = sender.send_request(gzip_get()).await.unwrap();
            check_head(&response);
            let mut body = response.into_body();
            read_until(&mut body, &mut decoded, FIRST).await;
            assert!(!dropped.load(Ordering::SeqCst));
            // The stream is cancelled; the connection stays.
            drop(body);
            assert!(
                set(&dropped).await,
                "{version}: the handler's body is dropped"
            );
            assert!(!connection.is_finished());
        } else {
            let (mut sender, connection) = http1(server.addr).await;
            let response = sender.send_request(gzip_get()).await.unwrap();
            check_head(&response);
            let mut body = response.into_body();
            read_until(&mut body, &mut decoded, FIRST).await;
            assert!(!dropped.load(Ordering::SeqCst));
            connection.abort();
            drop(body);
            assert!(
                set(&dropped).await,
                "{version}: the handler's body is dropped"
            );
        }
    }
}

/// Bytes deflate can't shrink: noise, repeated with a period longer than
/// its 32 KiB window.
fn noise() -> Bytes {
    let mut state = 0x9e37_79b9_u32;
    let bytes: Vec<u8> = (0..64 * 1024)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect();
    Bytes::from(bytes)
}

/// A client that asks for a large gzipped download and reads nothing holds
/// the handler back: compression reads the handler's body only as fast as
/// the connection takes the frames. The handler could send 64 MiB.
#[tokio::test]
async fn a_client_that_stops_reading_holds_back_the_handler_of_a_gzipped_download() {
    const CHUNKS: u64 = 1024;
    let sent = Arc::new(AtomicU64::new(0));
    let counter = sent.clone();
    let chunk = noise();
    let app = CompressionLayer::new().layer(answer(move |_request| {
        let counter = counter.clone();
        let chunk = chunk.clone();
        async move {
            let chunks = futures_util::stream::iter(0..CHUNKS).map(move |_| {
                counter.fetch_add(chunk.len() as u64, Ordering::SeqCst);
                Ok::<_, Infallible>(chunk.clone())
            });
            Response::new(Body::from_stream(chunks))
        }
    }));
    let server = start(app, no_limits()).await;
    let mut client = connect(server.addr).await;
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: test\r\nAccept-Encoding: gzip\r\n\r\n")
        .await
        .unwrap();
    // Once the socket buffers are full, the count stops moving.
    let started = Instant::now();
    let mut last = u64::MAX;
    loop {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let now = sent.load(Ordering::SeqCst);
        if now == last {
            break;
        }
        last = now;
        assert!(
            started.elapsed() < PATIENCE,
            "the handler never stops: {now} bytes"
        );
    }
    assert!(
        last < 32 * 1024 * 1024,
        "the handler sent {last} bytes to a client that reads nothing"
    );
    drop(client);
}
