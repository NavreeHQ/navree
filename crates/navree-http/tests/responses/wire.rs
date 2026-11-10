//! What a client reads: the app behind Navree's server, byte by byte (the
//! `date` header aside), over HTTP/1 and HTTP/2. Each is pinned in a file of
//! `golden/`.

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

use crate::stack::app;
use crate::support::golden;

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

/// A response as the golden files show it: every line end visible, and a
/// compressed body left out (what the compressor makes of it is its own
/// business; `stack` reads it decompressed).
fn readable((text, clean): (String, bool)) -> String {
    let text = match text.split_once("\r\n\r\n") {
        Some((head, body)) if head.to_ascii_lowercase().contains("content-encoding: gzip") => {
            assert!(body.ends_with("0\r\n\r\n"), "the body ends: {text}");
            format!("{head}\r\n\r\n<a gzip body, chunked>")
        }
        _ => text,
    };
    format!("{}\nclean: {clean}", text.replace("\r\n", "\\r\\n\n"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http1_responses_on_the_wire() {
    let server = start(app()).await;
    let mut requests = Vec::new();

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
            requests.push(get(path, headers));
        }
        for method in ["HEAD", "OPTIONS", "DELETE"] {
            requests.push(format!(
                "{method} {path} HTTP/1.1\r\nHost: test\r\nConnection: close\r\n\r\n"
            ));
        }
    }
    let post =
        "POST /echo HTTP/1.1\r\nHost: test\r\nConnection: close\r\nContent-Length: 5\r\n\r\nhello";
    requests.push(post.to_owned());
    requests.push("CONNECT /tunnel HTTP/1.1\r\nHost: test\r\nConnection: close\r\n\r\n".to_owned());

    let mut text = String::new();
    for request in &requests {
        let response = raw(server.addr, request).await;
        if request == post {
            assert!(response.0.ends_with("hello"), "{}", response.0);
        }
        text.push_str(&format!(
            "{}\n=>\n{}\n\n",
            request.replace("\r\n", "\\r\\n"),
            readable(response)
        ));
    }
    golden("http1.txt", &text);
}

/// A request without a path: `OPTIONS *` and `CONNECT host:port`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_request_without_a_path() {
    let server = start(app()).await;
    let mut text = String::new();
    for request in [
        "OPTIONS * HTTP/1.1\r\nHost: test\r\nConnection: close\r\n\r\n",
        "CONNECT example.com:443 HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n",
    ] {
        text.push_str(&format!(
            "{}\n=>\n{}\n\n",
            request.replace("\r\n", "\\r\\n"),
            readable(raw(server.addr, request).await)
        ));
    }
    golden("http1-no-path.txt", &text);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stream_goes_out_chunked_with_a_length_unknown() {
    let ours = start(app()).await;
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
    let ours = start(app()).await;
    let (text, _) = raw(ours.addr, &get("/events", "Accept-Encoding: gzip\r\n")).await;
    assert!(text.contains("content-type: text/event-stream"), "{text}");
    assert!(text.contains("cache-control: no-cache"), "{text}");
    assert!(!text.contains("content-encoding"), "{text}");
    assert!(text.contains("9\r\ndata: 1\n\n\r\n"), "{text}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_body_error_cuts_the_response_off() {
    let ours = start(app()).await;
    let a = raw(ours.addr, &get("/broken", "")).await;
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
    golden("http1-broken.txt", &readable(a));
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
async fn http2_responses() {
    let ours = start(app()).await;
    let mut text = String::new();
    for path in [
        "/text",
        "/json",
        "/stream",
        "/events",
        "/large",
        "/app-error",
        "/broken",
    ] {
        let (status, headers, body, error) = http2(ours.addr, path).await;
        text.push_str(&format!(
            "GET {path}\n{status} | {} | \"{}\" | error: {error:?}\n\n",
            headers.join(" / "),
            body.escape_ascii()
        ));
    }
    golden("http2.txt", &text);
    let broken = http2(ours.addr, "/broken").await;
    assert!(broken.3.is_some(), "the stream is reset, not ended");
}
