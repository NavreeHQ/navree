//! TestServer: the app's real router on a real port.

use navree_browser::TestServer;

use crate::support;

/// A raw HTTP/1.1 exchange, so these tests need no browser and no client
/// crate.
async fn http(server: &TestServer, request: &str) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(server.addr()).await.unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    String::from_utf8_lossy(&response).into_owned()
}

#[tokio::test]
async fn it_serves_the_app_on_a_free_local_port() {
    let app = support::app().await;
    let server = TestServer::start(app.ctx()).await.unwrap();
    assert!(server.addr().ip().is_loopback());
    assert_ne!(server.addr().port(), 0);
    assert_eq!(
        server.url("/api/v1/deals"),
        format!("http://127.0.0.1:{}/api/v1/deals", server.addr().port())
    );
    let response = http(
        &server,
        "GET /second HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.contains("Second page"), "{response}");
}

#[tokio::test]
async fn it_runs_every_layer_of_the_router() {
    let app = support::app().await;
    let server = TestServer::start(app.ctx()).await.unwrap();
    // The request id layer and the JSON 404 of the real router.
    let response = http(
        &server,
        "GET /nowhere HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(response.starts_with("HTTP/1.1 404"), "{response}");
    assert!(response.to_ascii_lowercase().contains("x-request-id:"), "{response}");
    assert!(response.contains("\"statusCode\":404"), "{response}");
    // Cookies set by the app reach the client.
    let body = "email=rep1%40navree.test&password=password123";
    let response = http(
        &server,
        &format!(
            "POST /login HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
    .await;
    assert!(response.starts_with("HTTP/1.1 303"), "{response}");
    assert!(response.contains("navree_at=token-rep1"), "{response}");
}

#[tokio::test]
async fn dropping_the_server_stops_it() {
    let app = support::app().await;
    let server = TestServer::start(app.ctx()).await.unwrap();
    let addr = server.addr();
    drop(server);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(tokio::net::TcpStream::connect(addr).await.is_err());
}

#[tokio::test]
async fn an_app_without_http_cannot_be_served() {
    let app = navree_kernel::Navree::app().boot().await.unwrap();
    let error = TestServer::start(app.ctx()).await.unwrap_err();
    assert!(error.to_string().contains("cannot build the app's router"), "{error}");
}
