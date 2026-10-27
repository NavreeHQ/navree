//! `RequestBuilder::send_streaming`: the body read piece by piece, behind the
//! same SSRF guard and attempt timeout as `send`.

use std::time::Duration;

use bytes::Bytes;
use futures_util::StreamExt;
use http::{Method, StatusCode};
use navree_http_client::{FakeResponse, HttpClient, HttpClientError};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::support::{OK, TestServer, client_allowing};

/// A server that writes the response head, then each of `chunks` (with a
/// pause between them) as HTTP/1.1 chunked encoding.
async fn chunked_server(chunks: &'static [&'static str]) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buffer = Vec::new();
                let mut chunk = [0u8; 1024];
                while !buffer.windows(4).any(|w| w == b"\r\n\r\n") {
                    let read = stream.read(&mut chunk).await.unwrap_or(0);
                    if read == 0 {
                        return;
                    }
                    buffer.extend_from_slice(&chunk[..read]);
                }
                let head = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\
                            transfer-encoding: chunked\r\nconnection: close\r\n\r\n";
                stream.write_all(head.as_bytes()).await.unwrap();
                for piece in chunks {
                    let frame = format!("{:x}\r\n{piece}\r\n", piece.len());
                    stream.write_all(frame.as_bytes()).await.unwrap();
                    stream.flush().await.unwrap();
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                stream.write_all(b"0\r\n\r\n").await.unwrap();
                let _ = stream.shutdown().await;
            });
        }
    });
    format!("http://{addr}/events")
}

#[tokio::test]
async fn returns_at_the_headers_and_streams_the_body() {
    let url = chunked_server(&["data: one\n\n", "data: two\n\n"]).await;

    let mut response = client_allowing(&["127.0.0.1"])
        .post(&url)
        .json(&serde_json::json!({ "stream": true }))
        .send_streaming()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.header("content-type"), Some("text/event-stream"));
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.unwrap() {
        body.extend_from_slice(&chunk);
    }
    assert_eq!(
        String::from_utf8(body).unwrap(),
        "data: one\n\ndata: two\n\n"
    );
}

#[tokio::test]
async fn the_body_is_also_a_stream() {
    let url = chunked_server(&["a", "b", "c"]).await;

    let response = client_allowing(&["127.0.0.1"])
        .get(&url)
        .send_streaming()
        .await
        .unwrap();
    let pieces: Vec<Bytes> = response
        .into_stream()
        .map(|chunk| chunk.unwrap())
        .collect()
        .await;

    assert_eq!(pieces.concat(), b"abc");
}

#[tokio::test]
async fn bytes_reads_the_rest_of_the_body() {
    let server = TestServer::start(OK).await;

    let response = client_allowing(&["127.0.0.1"])
        .get(server.url("/"))
        .send_streaming()
        .await
        .unwrap();

    assert_eq!(response.bytes().await.unwrap(), Bytes::from_static(b"ok"));
}

#[tokio::test]
async fn the_ssrf_policy_applies() {
    let server = TestServer::start(OK).await;

    let error = client_allowing(&[])
        .get(server.url("/"))
        .send_streaming()
        .await
        .unwrap_err();

    assert!(
        matches!(error, HttpClientError::Blocked { .. }),
        "{error:?}"
    );
    assert!(server.heads().is_empty());
}

#[tokio::test]
async fn the_timeout_covers_the_wait_for_the_headers() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let mut open = Vec::new();
        while let Ok((stream, _)) = listener.accept().await {
            open.push(stream);
        }
    });

    let error = client_allowing(&["127.0.0.1"])
        .get(format!("http://{addr}/hang"))
        .timeout(Duration::from_millis(200))
        .send_streaming()
        .await
        .unwrap_err();

    assert!(
        matches!(error, HttpClientError::Timeout { .. }),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_fake_client_streams_the_stubbed_body() {
    let (client, server) = HttpClient::fake();
    server.on(
        Method::POST,
        "https://api.example.com/stream",
        FakeResponse::new(200)
            .header("content-type", "text/event-stream")
            .body("data: hi\n\n"),
    );

    let response = client
        .post("https://api.example.com/stream")
        .body("{}")
        .send_streaming()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.bytes().await.unwrap(),
        Bytes::from_static(b"data: hi\n\n")
    );
    server.assert_sent_count(1);
}

#[tokio::test]
async fn streaming_requests_are_never_retried() {
    let (client, server) = HttpClient::fake();
    server.on_sequence(
        Method::GET,
        "https://api.example.com/flaky",
        [FakeResponse::new(503), FakeResponse::new(200)],
    );

    let response = client
        .get("https://api.example.com/flaky")
        .send_streaming()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    server.assert_sent_count(1);
}
