//! The real transport against local sockets (127.0.0.1 is allow-listed).

use std::time::{Duration, Instant};

use http::StatusCode;
use navis_contracts::RequestId;
use navis_http_client::HttpClientError;
use tokio::net::TcpListener;

use crate::support::{OK, TestServer, client_allowing};

#[tokio::test]
async fn sends_headers_and_reads_the_response() {
    let server = TestServer::start(
        "HTTP/1.1 201 Created\r\ncontent-type: application/json\r\nx-trace: t1\r\n\
         content-length: 9\r\nconnection: close\r\n\r\n{\"id\":42}",
    )
    .await;
    let id = RequestId::parse("abc-123").unwrap();

    let response = client_allowing(&["127.0.0.1"])
        .get(server.url("/things"))
        .with_request_id(&id)
        .bearer("tok")
        .query(&[("page", "2")])
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(response.header("x-trace"), Some("t1"));
    assert_eq!(response.json::<serde_json::Value>().unwrap()["id"], 42);
    let head = &server.heads()[0];
    assert!(head.starts_with("get /things?page=2 http/1.1"), "{head}");
    assert!(head.contains("x-request-id: abc-123"), "{head}");
    assert!(head.contains("authorization: bearer tok"), "{head}");
    assert!(head.contains("user-agent: navis-http-client/"), "{head}");
}

#[tokio::test]
async fn timeout_errors() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    // Accept connections and never answer.
    tokio::spawn(async move {
        let mut open = Vec::new();
        while let Ok((stream, _)) = listener.accept().await {
            open.push(stream);
        }
    });
    let started = Instant::now();

    let error = client_allowing(&["127.0.0.1"])
        .get(format!("http://{addr}/hang"))
        .timeout(Duration::from_millis(300))
        .send()
        .await
        .unwrap_err();

    assert!(
        matches!(error, HttpClientError::Timeout { .. }),
        "{error:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn connection_refused_is_a_connect_error() {
    // Bind then drop: nothing listens on the port any more. One attempt only:
    // Windows takes about 2 s to report a refused connection, and retrying
    // connect errors is covered by the fake tests.
    let addr = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap();

    let error = client_allowing(&["127.0.0.1"])
        .get(format!("http://{addr}/"))
        .retries(0)
        .send()
        .await
        .unwrap_err();

    assert!(
        matches!(error, HttpClientError::Connect { .. }),
        "{error:?}"
    );
}

#[tokio::test]
async fn ok_helper_server_answers() {
    let server = TestServer::start(OK).await;
    let response = client_allowing(&["127.0.0.1"])
        .post(server.url("/submit"))
        .json(&serde_json::json!({ "a": 1 }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.text(), "ok");
    assert!(server.heads()[0].contains("content-type: application/json"));
}

#[tokio::test]
async fn oversized_body_is_refused() {
    let server = TestServer::start(
        "HTTP/1.1 200 OK\r\ncontent-length: 10\r\nconnection: close\r\n\r\n0123456789",
    )
    .await;
    // Chunked: no Content-Length to check up front.
    let chunked = TestServer::start(
        "HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n\
         5\r\n01234\r\n5\r\n56789\r\n0\r\n\r\n",
    )
    .await;
    let client = navis_http_client::HttpClient::new(navis_http_client::HttpClientOptions {
        max_response_bytes: 8,
        ssrf: navis_http_client::SsrfPolicy {
            block_private: true,
            allow_hosts: vec!["127.0.0.1".into()],
        },
        ..Default::default()
    })
    .unwrap();

    for url in [server.url("/sized"), chunked.url("/chunked")] {
        let error = client.get(&url).send().await.unwrap_err();
        assert!(
            matches!(error, HttpClientError::BodyTooLarge { limit: 8, .. }),
            "{url}: {error:?}"
        );
    }
}
