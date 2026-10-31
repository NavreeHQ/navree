//! What a client reads: both apps behind Navree's server, compared byte by
//! byte (the `date` header aside), over HTTP/1 and HTTP/2.

use std::future::IntoFuture;
use std::net::SocketAddr;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Empty};
use hyper_util::rt::{TokioExecutor, TokioIo};
use navree_http::ServerOptions;
use navree_http::body::Body;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use tower::Service;

use crate::stack::both;

const PATIENCE: Duration = Duration::from_secs(20);

struct Server {
    addr: SocketAddr,
    task: JoinHandle<std::io::Result<()>>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn start<S, B>(service: S) -> Server
where
    S: Service<http::Request<Body>, Response = http::Response<B>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Into<navree_kernel::BoxError>,
    B: http_body::Body<Data = Bytes> + Send + 'static,
    B::Error: Into<navree_kernel::BoxError>,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let options = navree_http::HttpOptions {
        server: ServerOptions {
            max_connections_per_ip: None,
            ..ServerOptions::default()
        },
        ..navree_http::HttpOptions::default()
    };
    let serving = navree_http::serve(listener, service).options(&options);
    Server {
        addr,
        task: tokio::spawn(serving.into_future()),
    }
}

/// Sends `request` raw and reads until the server closes, without the `date`
/// line. An error from the connection is part of what was read.
async fn raw(addr: SocketAddr, request: &str) -> (String, bool) {
    let mut client = TcpStream::connect(addr).await.unwrap();
    client.write_all(request.as_bytes()).await.unwrap();
    let mut read = Vec::new();
    let mut buffer = [0_u8; 4096];
    let clean = loop {
        match tokio::time::timeout(PATIENCE, client.read(&mut buffer)).await {
            Ok(Ok(0)) => break true,
            Ok(Ok(count)) => read.extend_from_slice(&buffer[..count]),
            Ok(Err(_)) => break false,
            Err(_) => panic!(
                "the server did not close: {}",
                String::from_utf8_lossy(&read)
            ),
        }
    };
    let text = String::from_utf8_lossy(&read)
        .split("\r\n")
        .filter(|line| !line.to_ascii_lowercase().starts_with("date:"))
        .collect::<Vec<_>>()
        .join("\r\n");
    (text, clean)
}

fn get(path: &str, headers: &str) -> String {
    format!("GET {path} HTTP/1.1\r\nHost: test\r\nConnection: close\r\n{headers}\r\n")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http1_responses_are_the_same_on_the_wire() {
    let (app, previous) = both();
    let ours = start(app).await;
    let theirs = start(previous).await;

    for path in [
        "/text",
        "/json",
        "/redirect",
        "/deals/7",
        "/deals/x",
        "/status",
        "/empty-error",
        "/app-error",
        "/stream",
        "/events",
        "/large",
        "/nothing",
    ] {
        for headers in [
            "",
            "Accept-Encoding: gzip\r\n",
            "Origin: http://localhost:3000\r\n",
        ] {
            let request = get(path, headers);
            let a = raw(ours.addr, &request).await;
            let b = raw(theirs.addr, &request).await;
            assert_eq!(a.0, b.0, "{request}");
        }
        for method in ["HEAD", "OPTIONS", "DELETE"] {
            let request =
                format!("{method} {path} HTTP/1.1\r\nHost: test\r\nConnection: close\r\n\r\n");
            let a = raw(ours.addr, &request).await;
            let b = raw(theirs.addr, &request).await;
            assert_eq!(a.0, b.0, "{request}");
        }
    }
    let request =
        "POST /echo HTTP/1.1\r\nHost: test\r\nConnection: close\r\nContent-Length: 5\r\n\r\nhello";
    let a = raw(ours.addr, request).await;
    assert_eq!(a.0, raw(theirs.addr, request).await.0);
    assert!(a.0.ends_with("hello"), "{}", a.0);

    let request = "CONNECT /tunnel HTTP/1.1\r\nHost: test\r\nConnection: close\r\n\r\n";
    assert_eq!(
        raw(ours.addr, request).await.0,
        raw(theirs.addr, request).await.0
    );
}

/// The lines of a response in order, the head's sorted after its first line.
fn with_head_sorted(response: &str) -> Vec<String> {
    let (head, body) = response.split_once("\r\n\r\n").unwrap_or((response, ""));
    let mut lines: Vec<String> = head.split("\r\n").map(str::to_owned).collect();
    lines[1..].sort();
    lines.push(body.to_owned());
    lines
}

/// Difference 3: a request without a path. The previous router wrote the
/// length into the headers itself for these, ahead of `connection`; here the
/// server writes it. The same fields with the same values.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_request_without_a_path_differs_only_in_the_order_of_two_fields() {
    let (app, previous) = both();
    let ours = start(app).await;
    let theirs = start(previous).await;
    for request in [
        "OPTIONS * HTTP/1.1\r\nHost: test\r\nConnection: close\r\n\r\n",
        "CONNECT example.com:443 HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n",
    ] {
        let a = raw(ours.addr, request).await;
        let b = raw(theirs.addr, request).await;
        assert_eq!(with_head_sorted(&a.0), with_head_sorted(&b.0), "{request}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stream_goes_out_chunked_with_a_length_unknown() {
    let (app, _) = both();
    let ours = start(app).await;
    let (text, _) = raw(ours.addr, &get("/stream", "")).await;
    assert!(text.contains("transfer-encoding: chunked"), "{text}");
    assert!(
        text.ends_with("3\r\none\r\n3\r\ntwo\r\n5\r\nthree\r\n0\r\n\r\n"),
        "{text}"
    );
    let (text, _) = raw(ours.addr, &get("/text", "")).await;
    assert!(text.contains("content-length: 4"), "{text}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn events_go_out_one_at_a_time() {
    let (app, _) = both();
    let ours = start(app).await;
    let (text, _) = raw(ours.addr, &get("/events", "Accept-Encoding: gzip\r\n")).await;
    assert!(text.contains("content-type: text/event-stream"), "{text}");
    assert!(text.contains("cache-control: no-cache"), "{text}");
    assert!(!text.contains("content-encoding"), "{text}");
    assert!(text.contains("9\r\ndata: 1\n\n\r\n"), "{text}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_body_error_cuts_the_response_off_the_same_way() {
    let (app, previous) = both();
    let ours = start(app).await;
    let theirs = start(previous).await;
    let a = raw(ours.addr, &get("/broken", "")).await;
    let b = raw(theirs.addr, &get("/broken", "")).await;
    assert_eq!(a, b);
    assert!(
        a.0.contains("5\r\nstart\r\n"),
        "what was ready was sent: {}",
        a.0
    );
    assert!(
        !a.0.ends_with("0\r\n\r\n"),
        "no final chunk: the client sees a broken response, not a short one: {}",
        a.0
    );
}

/// One `GET` over HTTP/2, and how its body ended.
async fn http2(
    addr: SocketAddr,
    path: &str,
) -> (http::StatusCode, Vec<String>, Vec<u8>, Option<String>) {
    let stream = TcpStream::connect(addr).await.unwrap();
    let (mut sender, connection) =
        hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(stream))
            .await
            .unwrap();
    tokio::spawn(connection);
    let request = http::Request::get(format!("http://test{path}"))
        .body(Empty::<Bytes>::new())
        .unwrap();
    let response = sender.send_request(request).await.unwrap();
    let status = response.status();
    let headers = response
        .headers()
        .iter()
        .filter(|(name, _)| *name != "date")
        .map(|(name, value)| format!("{name}: {}", value.to_str().unwrap()))
        .collect();
    let mut body = response.into_body();
    let mut read = Vec::new();
    let mut error = None;
    while let Some(frame) = body.frame().await {
        match frame {
            Ok(frame) => read.extend_from_slice(&frame.into_data().unwrap_or_default()),
            Err(failure) => {
                error = Some(failure.to_string());
                break;
            }
        }
    }
    (status, headers, read, error)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http2_responses_are_the_same() {
    let (app, previous) = both();
    let ours = start(app).await;
    let theirs = start(previous).await;
    for path in [
        "/text",
        "/json",
        "/stream",
        "/events",
        "/large",
        "/app-error",
        "/broken",
    ] {
        let a = http2(ours.addr, path).await;
        let b = http2(theirs.addr, path).await;
        assert_eq!(a, b, "{path}");
    }
    let broken = http2(ours.addr, "/broken").await;
    assert!(broken.3.is_some(), "the stream is reset, not ended");
}
