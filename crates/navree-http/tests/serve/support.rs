//! A server on a free port, and a client that takes its time.

#![allow(dead_code)]

use std::convert::Infallible;
use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use bytes::Bytes;
use http::{Method, Request, Response, StatusCode};
use http_body::Body as HttpBody;
use navree_http::body::{Body, read_body};
use navree_http::routing::Router;
use navree_http::{HttpOptions, ServerOptions};
use navree_kernel::BoxError;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tower::Service;

/// How late anything may happen before a test gives up. Only a broken limit
/// takes this long.
pub const PATIENCE: Duration = Duration::from_secs(30);

/// How much longer than a limit a test keeps a connection open to show the
/// limit is really off.
pub const OFF_MARGIN: u32 = 3;

/// Limits with nothing on: each test turns on the one it is about.
pub fn no_limits() -> ServerOptions {
    ServerOptions {
        header_read_timeout_ms: None,
        idle_timeout_ms: None,
        body_read_timeout_ms: None,
        body_min_bytes_per_sec: None,
        http2_keep_alive_interval_ms: None,
        max_connections: None,
        max_connections_per_ip: None,
        write_stall_timeout_ms: None,
        ..ServerOptions::default()
    }
}

/// A server on `127.0.0.1:0`, stopped by [`Served::stop`] or dropped.
pub struct Served {
    pub addr: SocketAddr,
    stop: Option<oneshot::Sender<()>>,
    task: JoinHandle<io::Result<()>>,
}

pub async fn start<S, B>(app: S, server: ServerOptions) -> Served
where
    S: Service<Request<Body>, Response = Response<B>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Into<BoxError>,
    B: HttpBody<Data = Bytes> + Send + 'static,
    B::Error: Into<BoxError>,
{
    start_with(
        app,
        HttpOptions {
            server,
            ..HttpOptions::default()
        },
    )
    .await
}

pub async fn start_with<S, B>(app: S, options: HttpOptions) -> Served
where
    S: Service<Request<Body>, Response = Response<B>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Into<BoxError>,
    B: HttpBody<Data = Bytes> + Send + 'static,
    B::Error: Into<BoxError>,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (stop, stopped) = oneshot::channel::<()>();
    let task = tokio::spawn(async move {
        navree_http::serve(listener, app)
            .options(&options)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
    });
    Served {
        addr,
        stop: Some(stop),
        task,
    }
}

impl Served {
    /// Starts the graceful shutdown.
    pub fn stop(&mut self) {
        let _ = self.stop.take().expect("stopped once").send(());
    }

    /// Whether the server has ended.
    pub fn finished(&self) -> bool {
        self.task.is_finished()
    }

    /// Waits for the server to end.
    pub async fn stopped(self) {
        timeout(PATIENCE, self.task)
            .await
            .expect("the server ends")
            .unwrap()
            .unwrap();
    }
}

/// `GET /`, `POST /echo` (the body, read with `Json`'s limit), and anything
/// else this app needs.
pub fn echo_app() -> Router {
    Router::new()
        .route(Method::GET, "/", || async { "hello" })
        .route(Method::POST, "/echo", |request: Request<Body>| async move {
            let (status, body) = match read_body(request.into_body(), 4 * 1024 * 1024).await {
                Ok(body) => (StatusCode::OK, body),
                Err(error) => (error.status(), Bytes::from(error.to_string())),
            };
            Response::builder()
                .status(status)
                .body(Body::from(body))
                .unwrap()
        })
}

/// One raw connection.
pub async fn connect(addr: SocketAddr) -> TcpStream {
    TcpStream::connect(addr).await.expect("connects")
}

/// The request head of `GET /`.
pub const GET: &[u8] = b"GET / HTTP/1.1\r\nHost: test\r\n\r\n";

/// Reads one response, with its `content-length` body.
pub async fn read_response(stream: &mut TcpStream) -> String {
    timeout(PATIENCE, async {
        let mut seen = Vec::new();
        let mut chunk = [0_u8; 4096];
        loop {
            if let Some(end) = find(&seen, b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&seen[..end]).to_ascii_lowercase();
                let length = head
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length: "))
                    .and_then(|length| length.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                if seen.len() >= end + 4 + length {
                    return String::from_utf8_lossy(&seen).into_owned();
                }
            }
            match stream.read(&mut chunk).await {
                Ok(0) | Err(_) => return String::from_utf8_lossy(&seen).into_owned(),
                Ok(read) => seen.extend_from_slice(&chunk[..read]),
            }
        }
    })
    .await
    .expect("a response arrives")
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// One request and its response on `stream`.
pub async fn exchange(stream: &mut TcpStream, request: &[u8]) -> String {
    stream
        .write_all(request)
        .await
        .expect("the request goes out");
    read_response(stream).await
}

/// Waits for the server to close the connection; what it sent until then.
pub async fn read_until_closed(stream: &mut TcpStream) -> String {
    timeout(PATIENCE, async {
        let mut seen = Vec::new();
        let mut chunk = [0_u8; 4096];
        loop {
            match stream.read(&mut chunk).await {
                Ok(0) | Err(_) => return String::from_utf8_lossy(&seen).into_owned(),
                Ok(read) => seen.extend_from_slice(&chunk[..read]),
            }
        }
    })
    .await
    .expect("the server closes the connection")
}

/// Whether the connection stays open for `duration` with nothing sent.
pub async fn stays_open(stream: &mut TcpStream, duration: Duration) -> bool {
    let mut chunk = [0_u8; 1];
    timeout(duration, stream.read(&mut chunk)).await.is_err()
}

/// How a [`dribble`] ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Dribble {
    /// Every byte went out and the connection is open.
    Sent,
    /// The server closed it first, having sent this.
    Closed(String),
}

/// Sends `bytes` one at a time, `every` apart, however long that takes,
/// until they run out or the server closes the connection.
pub async fn dribble(stream: &mut TcpStream, bytes: &[u8], every: Duration) -> Dribble {
    let mut seen = Vec::new();
    let mut chunk = [0_u8; 4096];
    for (sent, byte) in bytes.iter().enumerate() {
        if stream.write_all(&[*byte]).await.is_err() {
            return Dribble::Closed(String::from_utf8_lossy(&seen).into_owned());
        }
        if sent + 1 == bytes.len() {
            break;
        }
        let pause = tokio::time::sleep(every);
        tokio::pin!(pause);
        loop {
            tokio::select! {
                () = &mut pause => break,
                read = stream.read(&mut chunk) => match read {
                    Ok(0) | Err(_) => {
                        return Dribble::Closed(String::from_utf8_lossy(&seen).into_owned());
                    }
                    Ok(read) => seen.extend_from_slice(&chunk[..read]),
                },
            }
        }
    }
    Dribble::Sent
}

/// A service that answers from a closure, for tests that want no router.
pub fn answer<F, Fut>(
    handler: F,
) -> impl Service<Request<Body>, Response = Response<Body>, Error = Infallible, Future: Send + 'static>
+ Clone
+ Send
+ 'static
where
    F: Fn(Request<Body>) -> Fut + Clone + Send + 'static,
    Fut: Future<Output = Response<Body>> + Send + 'static,
{
    tower::service_fn(move |request| {
        let response = handler(request);
        async move { Ok::<_, Infallible>(response.await) }
    })
}
