//! The `serve` command against a real socket.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use http::Method;
use navree_http::{HttpOptions, HttpServer, RouteDef};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Notify;

use crate::support::{Setup, options};

/// A minimal HTTP/1.1 client: the raw response text.
async fn http_get(addr: SocketAddr, path: &str) -> String {
    let mut stream = TcpStream::connect(addr).await.expect("connects");
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    String::from_utf8_lossy(&response).into_owned()
}

#[tokio::test]
async fn serve_command_binds_and_shuts_down_gracefully() {
    let started = Arc::new(Notify::new());
    let handler_started = started.clone();
    let finished = Arc::new(AtomicBool::new(false));
    let handler_finished = finished.clone();
    let app = Setup::new(vec![
        RouteDef::on(Method::GET, "/slow", move || {
            let started = handler_started.clone();
            let finished = handler_finished.clone();
            async move {
                started.notify_one();
                tokio::time::sleep(Duration::from_millis(300)).await;
                finished.store(true, Ordering::SeqCst);
                "finished"
            }
        })
        .public(),
        RouteDef::on(Method::GET, "/fast", || async { "fast" }).public(),
    ])
    .options(HttpOptions {
        addr: "127.0.0.1:0".to_owned(),
        ..options()
    })
    .boot()
    .await;
    let server = app.app.ctx().get::<HttpServer>().await.unwrap();
    assert_eq!(server.local_addr(), None, "not serving yet");

    let serve = app.app.run_command("serve", vec![]);
    let client = async {
        let addr = tokio::time::timeout(Duration::from_secs(10), server.wait_bound())
            .await
            .expect("serve binds within 10 s");
        assert_ne!(addr.port(), 0, "bound to a real port");

        let fast = http_get(addr, "/api/fast").await;
        assert!(fast.starts_with("HTTP/1.1 200"), "{fast}");
        assert!(fast.contains("x-request-id: "), "{fast}");
        assert!(fast.ends_with("fast"), "{fast}");

        // Shut down while a request is in flight: it still completes.
        let slow = tokio::spawn(http_get(addr, "/api/slow"));
        started.notified().await;
        app.app.ctx().shutdown_token().cancel();
        let slow = slow.await.unwrap();
        assert!(slow.starts_with("HTTP/1.1 200"), "{slow}");
        assert!(slow.ends_with("finished"), "{slow}");
        addr
    };
    let serve = async {
        let served = tokio::time::timeout(Duration::from_secs(20), serve)
            .await
            .expect("serve returns within 20 s of the shutdown signal");
        assert!(
            finished.load(Ordering::SeqCst),
            "serve returns only after in-flight requests finished"
        );
        served
    };
    let (served, addr) = tokio::join!(serve, client);

    served.expect("serve returns Ok after a graceful shutdown");
    assert_eq!(server.local_addr(), None);
    assert!(
        TcpStream::connect(addr).await.is_err(),
        "the listener is closed"
    );
}

#[tokio::test]
async fn a_client_that_sends_a_body_over_the_limit_reads_the_413_not_a_reset() {
    let app = Setup::new(vec![
        RouteDef::on(Method::POST, "/photo", || async { "kept" })
            .public()
            .body_limit(1024),
    ])
    .options(HttpOptions {
        addr: "127.0.0.1:0".to_owned(),
        ..options()
    })
    .boot()
    .await;
    let server = app.app.ctx().get::<HttpServer>().await.unwrap();

    let serve = app.app.run_command("serve", vec![]);
    let client = async {
        let addr = tokio::time::timeout(Duration::from_secs(10), server.wait_bound())
            .await
            .expect("serve binds within 10 s");
        // Three megabytes, all sent before the answer is read, as a browser or curl sends them.
        let body = vec![b'x'; 3 * 1024 * 1024];
        let mut stream = TcpStream::connect(addr).await.expect("connects");
        let head = format!(
            "POST /api/photo HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(head.as_bytes()).await.unwrap();
        stream
            .write_all(&body)
            .await
            .expect("the server reads the body to its end");
        let mut response = Vec::new();
        stream
            .read_to_end(&mut response)
            .await
            .expect("the answer arrives, not a reset");
        app.app.ctx().shutdown_token().cancel();
        String::from_utf8_lossy(&response).into_owned()
    };
    let (served, response) = tokio::join!(serve, client);
    served.expect("serve returns Ok after a graceful shutdown");
    assert!(response.starts_with("HTTP/1.1 413"), "{response}");
    assert!(response.contains("payload_too_large"), "{response}");
}

#[tokio::test]
async fn serve_reports_an_address_it_cannot_bind() {
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = taken.local_addr().unwrap();
    let app = Setup::new(vec![
        RouteDef::on(Method::GET, "/x", || async { "x" }).public(),
    ])
    .boot()
    .await;

    let error = app
        .app
        .run_command("serve", vec!["--addr".to_owned(), addr.to_string()])
        .await
        .unwrap_err();
    assert!(error.to_string().contains(&addr.to_string()), "{error}");
}
