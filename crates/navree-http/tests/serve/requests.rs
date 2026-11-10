//! What reaches a handler, and the limits on a request head.

use std::net::SocketAddr;

use http::{Method, Request, Response};
use navree_http::body::Body;
use navree_http::routing::Router;
use navree_http::serve::ConnectInfo;
use navree_http::{ClientIp, Extension, HttpOptions, ServerOptions};
use tokio::io::AsyncWriteExt;

use crate::support::{
    GET, answer, connect, echo_app, exchange, no_limits, read_until_closed, start,
};

#[tokio::test]
async fn a_connection_serves_requests_one_after_another() {
    let server = start(echo_app(), no_limits()).await;
    let mut client = connect(server.addr).await;
    for _ in 0..3 {
        let response = exchange(&mut client, GET).await;
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(response.ends_with("hello"), "{response}");
    }
}

/// The default limits leave an ordinary client alone.
#[tokio::test]
async fn an_ordinary_client_is_served_with_the_default_limits() {
    let server = start(echo_app(), ServerOptions::default()).await;
    let mut client = connect(server.addr).await;
    let response = exchange(
        &mut client,
        b"POST /echo HTTP/1.1\r\nHost: test\r\nContent-Length: 5\r\n\r\nhello",
    )
    .await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.ends_with("hello"), "{response}");
}

/// The boot refuses these, but `serve` can be called with any options: it
/// clamps them rather than panicking on a connection.
#[tokio::test]
async fn extreme_options_never_panic_a_connection() {
    let server = start(
        echo_app(),
        ServerOptions {
            header_read_timeout_ms: Some(u64::MAX),
            idle_timeout_ms: Some(u64::MAX),
            body_read_timeout_ms: Some(u64::MAX),
            body_min_bytes_per_sec: Some(0),
            http2_keep_alive_interval_ms: Some(u64::MAX),
            http2_keep_alive_timeout_ms: u64::MAX,
            max_connections: Some(usize::MAX),
            max_header_bytes: 0,
            ..ServerOptions::default()
        },
    )
    .await;
    let mut client = connect(server.addr).await;
    let response = exchange(
        &mut client,
        b"POST /echo HTTP/1.1\r\nHost: test\r\nContent-Length: 5\r\n\r\nhello",
    )
    .await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
}

#[tokio::test]
async fn handlers_see_the_peer_of_the_connection() {
    let app = Router::new().route(
        Method::GET,
        "/peer",
        |peer: ConnectInfo<SocketAddr>, Extension(info): Extension<ConnectInfo<SocketAddr>>| async move {
            assert_eq!(peer, info);
            peer.0.to_string()
        },
    );
    let server = start(app, no_limits()).await;
    let mut client = connect(server.addr).await;
    let local = client.local_addr().unwrap();
    let response = exchange(&mut client, b"GET /peer HTTP/1.1\r\nHost: test\r\n\r\n").await;
    assert!(response.ends_with(&local.to_string()), "{response}");
}

#[tokio::test]
async fn the_client_ip_comes_from_the_connection() {
    let app = answer(|request: Request<Body>| async move {
        let ip = ClientIp::of(
            request.headers(),
            request.extensions(),
            &HttpOptions::default(),
        );
        Response::new(Body::from(ip.0.to_string()))
    });
    let server = start(app, no_limits()).await;
    let mut client = connect(server.addr).await;
    let response = exchange(&mut client, GET).await;
    assert!(response.ends_with("127.0.0.1"), "{response}");
}

fn request_with(headers: usize, size: usize) -> Vec<u8> {
    let mut request = b"GET / HTTP/1.1\r\nHost: test\r\n".to_vec();
    for index in 0..headers {
        let value = "v".repeat(size);
        request.extend_from_slice(format!("x-{index}: {value}\r\n").as_bytes());
    }
    request.extend_from_slice(b"\r\n");
    request
}

#[tokio::test]
async fn a_head_over_the_size_limit_is_a_431() {
    let server = start(
        echo_app(),
        ServerOptions {
            max_header_bytes: 8 * 1024,
            ..no_limits()
        },
    )
    .await;

    let mut client = connect(server.addr).await;
    let response = exchange(&mut client, &request_with(1, 4 * 1024)).await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");

    // Exactly the limit, with the head unfinished: the server has read
    // every byte when it answers, so nothing resets the socket under the 431.
    let mut client = connect(server.addr).await;
    let mut unfinished = b"GET / HTTP/1.1\r\nHost: test\r\nX-Long: ".to_vec();
    unfinished.resize(8 * 1024, b'v');
    client.write_all(&unfinished).await.unwrap();
    let response = read_until_closed(&mut client).await;
    assert!(response.starts_with("HTTP/1.1 431"), "{response}");

    // Far over it, the server closes with bytes unread and the client may
    // see a reset in place of the 431, but never an answer.
    let mut client = connect(server.addr).await;
    let _ = client.write_all(&request_with(1, 64 * 1024)).await;
    let response = read_until_closed(&mut client).await;
    assert!(
        response.is_empty() || response.starts_with("HTTP/1.1 431"),
        "{response}"
    );
}

#[tokio::test]
async fn too_many_headers_are_a_431() {
    for max_headers in [100, 8] {
        let server = start(
            echo_app(),
            ServerOptions {
                max_headers,
                ..no_limits()
            },
        )
        .await;

        // `Host` is one of them.
        let mut client = connect(server.addr).await;
        let response = exchange(&mut client, &request_with(max_headers - 1, 1)).await;
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");

        let mut client = connect(server.addr).await;
        let response = exchange(&mut client, &request_with(max_headers, 1)).await;
        assert!(response.starts_with("HTTP/1.1 431"), "{response}");
    }
}
