//! The `serve` command against a real socket.

use std::future::Future;
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

/// Serves `routes` with `options` on a free port, runs `client` against it, then stops the
/// server and returns what the client found.
async fn against<F, Fut, T>(routes: Vec<RouteDef>, options: HttpOptions, client: F) -> T
where
    F: FnOnce(SocketAddr) -> Fut,
    Fut: Future<Output = T>,
{
    served_by(Setup::new(routes), options, client).await
}

/// [`against`], for the app `setup` makes.
async fn served_by<F, Fut, T>(setup: Setup, options: HttpOptions, client: F) -> T
where
    F: FnOnce(SocketAddr) -> Fut,
    Fut: Future<Output = T>,
{
    let app = setup
        .options(HttpOptions {
            addr: "127.0.0.1:0".to_owned(),
            ..options
        })
        .boot()
        .await;
    let server = app.app.ctx().get::<HttpServer>().await.unwrap();
    let serve = app.app.run_command("serve", vec![]);
    let run = async {
        let addr = tokio::time::timeout(Duration::from_secs(10), server.wait_bound())
            .await
            .expect("serve binds within 10 s");
        let found = client(addr).await;
        app.app.ctx().shutdown_token().cancel();
        found
    };
    let (served, found) = tokio::join!(serve, run);
    served.expect("serve returns Ok after a graceful shutdown");
    found
}

/// Bodies of at most 1 KB: `POST /api/photo`, which never reads its body, and `GET /api/fast`.
fn photo_routes() -> Vec<RouteDef> {
    vec![
        RouteDef::on(Method::POST, "/photo", || async { "kept" })
            .public()
            .body_limit(1024),
        RouteDef::on(Method::GET, "/fast", || async { "fast" }).public(),
    ]
}

/// The head of a post to `/api/photo` that declares `length` bytes, with the header lines
/// `extra`.
fn photo_head(length: usize, extra: &str) -> Vec<u8> {
    format!(
        "POST /api/photo HTTP/1.1\r\nHost: localhost\r\nContent-Length: {length}\r\n{extra}\r\n"
    )
    .into_bytes()
}

/// Writes `bytes` on a new connection, all before reading as a browser does, then reads until
/// the server closes the connection: the answers, as text.
async fn exchange(addr: SocketAddr, bytes: &[u8]) -> String {
    let mut stream = TcpStream::connect(addr).await.expect("connects");
    stream
        .write_all(bytes)
        .await
        .expect("the server reads what is sent");
    let mut answer = Vec::new();
    tokio::time::timeout(Duration::from_secs(20), stream.read_to_end(&mut answer))
        .await
        .expect("the server closes the connection")
        .expect("the answer arrives, not a reset");
    String::from_utf8_lossy(&answer).into_owned()
}

#[tokio::test]
async fn a_client_that_sends_a_body_over_the_limit_reads_the_413_not_a_reset() {
    let answer = against(photo_routes(), options(), |addr| async move {
        // Three megabytes, all sent before the answer is read, as a browser or curl sends them.
        let mut request = photo_head(3 * 1024 * 1024, "Connection: close\r\n");
        request.extend(vec![b'x'; 3 * 1024 * 1024]);
        exchange(addr, &request).await
    })
    .await;
    assert!(answer.starts_with("HTTP/1.1 413"), "{answer}");
    assert!(answer.contains("payload_too_large"), "{answer}");
}

/// hyper has read the whole body with its head: throwing it away costs nothing, so the
/// connection goes on whatever the body's size.
#[tokio::test]
async fn after_a_refused_body_is_read_the_connection_answers_the_next_request() {
    let answer = against(photo_routes(), options(), |addr| async move {
        // One write: the post, its whole body, and a request after it on the same connection.
        let mut requests = photo_head(2048, "");
        requests.extend(vec![b'x'; 2048]);
        requests.extend(b"GET /api/fast HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
        exchange(addr, &requests).await
    })
    .await;
    assert!(answer.starts_with("HTTP/1.1 413"), "{answer}");
    let next = answer
        .find("HTTP/1.1 200")
        .expect("a second answer, on the same connection");
    let (refused, fast) = answer.split_at(next);
    assert!(
        !refused.contains("connection: close"),
        "the 413 keeps the connection: {refused}"
    );
    assert!(fast.ends_with("fast"), "{fast}");
}

#[tokio::test]
async fn a_stalled_body_gets_the_413_at_once_and_the_connection_closes() {
    let (answer, waited) = against(photo_routes(), options(), |addr| async move {
        // A hundred bytes of the 2,048 it declares, and then nothing.
        let mut request = photo_head(2048, "");
        request.extend(vec![b'x'; 100]);
        let began = std::time::Instant::now();
        let answer = exchange(addr, &request).await;
        (answer, began.elapsed())
    })
    .await;
    assert!(answer.starts_with("HTTP/1.1 413"), "{answer}");
    assert!(answer.contains("connection: close"), "{answer}");
    assert!(
        waited < Duration::from_secs(2),
        "the answer and the close do not wait for the body: {waited:?}"
    );
}

#[tokio::test]
async fn a_body_declared_far_over_the_limit_is_answered_at_once() {
    let (answer, waited) = against(photo_routes(), options(), |addr| async move {
        let began = std::time::Instant::now();
        // Nine megabytes declared and none sent: the server does not wait for them.
        let answer = exchange(addr, &photo_head(9 * 1024 * 1024, "")).await;
        (answer, began.elapsed())
    })
    .await;
    assert!(answer.starts_with("HTTP/1.1 413"), "{answer}");
    assert!(answer.contains("connection: close"), "{answer}");
    assert!(
        waited < Duration::from_secs(2),
        "answered without waiting for the body: {waited:?}"
    );
}

/// What is left of a body after an early answer is read up to the route's own limit (1 KB for
/// the photo, which does not read its body), and up to `http.body_limit_bytes` outside a route.
#[tokio::test]
async fn the_rest_of_a_body_is_read_up_to_its_routes_limit() {
    let answers = against(photo_routes(), options(), |addr| async move {
        let mut answers = Vec::new();
        for (head, length) in [
            ("POST /api/photo", 1024),
            ("POST /api/photo", 1025),
            ("POST /api/nowhere", 4096),
        ] {
            let mut stream = TcpStream::connect(addr).await.expect("connects");
            let head =
                format!("{head} HTTP/1.1\r\nHost: localhost\r\nContent-Length: {length}\r\n\r\n");
            stream.write_all(head.as_bytes()).await.unwrap();
            // The answer comes before the body.
            let answer = read_answer(&mut stream).await;
            stream.write_all(&vec![b'x'; length]).await.unwrap();
            stream
                .write_all(
                    b"GET /api/fast HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
                )
                .await
                .ok();
            let mut rest = Vec::new();
            tokio::time::timeout(Duration::from_secs(20), stream.read_to_end(&mut rest))
                .await
                .expect("the server closes the connection")
                .expect("a clean close, not a reset");
            answers.push((answer, String::from_utf8_lossy(&rest).into_owned()));
        }
        answers
    })
    .await;
    let [(within, next), (over, after), (outside, beyond)] = &answers[..] else {
        panic!("{answers:?}");
    };
    assert!(
        within.starts_with("HTTP/1.1 200") && within.ends_with("kept"),
        "{within}"
    );
    assert!(!within.contains("connection: close"), "{within}");
    assert!(
        next.contains("HTTP/1.1 200") && next.ends_with("fast"),
        "{next}"
    );

    assert!(over.starts_with("HTTP/1.1 413"), "{over}");
    assert!(over.contains("connection: close"), "{over}");
    assert!(
        !after.contains("HTTP/1.1"),
        "the connection closed: {after}"
    );

    assert!(outside.starts_with("HTTP/1.1 404"), "{outside}");
    assert!(!outside.contains("connection: close"), "{outside}");
    assert!(beyond.ends_with("fast"), "{beyond}");
}

/// Until a client has been let in, no more of a body its answer did not wait for is read than
/// outside a route (`http.body_limit_bytes`), however much the route takes: a client refused at
/// the door does not get its upload read. Once it has been let in, the route's own limit is what
/// is read.
#[tokio::test]
async fn before_a_client_is_let_in_little_of_its_body_is_read() {
    let routes = vec![
        RouteDef::on(Method::POST, "/import", || async { "kept" })
            .authenticated()
            .body_limit(1024 * 1024),
        RouteDef::on(Method::GET, "/fast", || async { "fast" }).public(),
    ];
    let options = HttpOptions {
        body_limit_bytes: 16 * 1024,
        ..options()
    };
    let setup = Setup::new(routes).authenticated();
    let answers = served_by(setup, options, |addr| async move {
        let mut answers = Vec::new();
        for user in ["", "x-user-id: 0b9f1e1a-6a62-4c43-9a59-3f2b1c1d5e01\r\n"] {
            let mut stream = TcpStream::connect(addr).await.expect("connects");
            let length = 64 * 1024;
            let head = format!(
                "POST /api/import HTTP/1.1\r\nHost: localhost\r\n{user}Content-Length: {length}\r\n\r\n"
            );
            stream.write_all(head.as_bytes()).await.unwrap();
            // The answer comes before the body.
            let answer = read_answer(&mut stream).await;
            stream.write_all(&vec![b'x'; length]).await.ok();
            stream
                .write_all(
                    b"GET /api/fast HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
                )
                .await
                .ok();
            let mut rest = Vec::new();
            let _ = tokio::time::timeout(Duration::from_secs(20), stream.read_to_end(&mut rest))
                .await
                .expect("the server closes the connection");
            answers.push((answer, String::from_utf8_lossy(&rest).into_owned()));
        }
        answers
    })
    .await;
    let [(refused, after), (kept, next)] = &answers[..] else {
        panic!("{answers:?}");
    };
    assert!(refused.starts_with("HTTP/1.1 401"), "{refused}");
    assert!(refused.contains("connection: close"), "{refused}");
    assert!(
        !after.contains("HTTP/1.1"),
        "the connection closed: {after}"
    );

    assert!(
        kept.starts_with("HTTP/1.1 200") && kept.ends_with("kept"),
        "{kept}"
    );
    assert!(!kept.contains("connection: close"), "{kept}");
    assert!(next.ends_with("fast"), "{next}");
}

/// Reads one answer, up to the end of its `content-length` body.
async fn read_answer(stream: &mut TcpStream) -> String {
    let mut seen = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        let text = String::from_utf8_lossy(&seen).into_owned();
        if let Some(end) = text.find("\r\n\r\n") {
            let length = text[..end]
                .lines()
                .find_map(|line| line.strip_prefix("content-length: "))
                .and_then(|length| length.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if seen.len() >= end + 4 + length {
                return text;
            }
        }
        let read = tokio::time::timeout(Duration::from_secs(20), stream.read(&mut chunk))
            .await
            .expect("the answer comes before the body")
            .unwrap();
        if read == 0 {
            return text;
        }
        seen.extend_from_slice(&chunk[..read]);
    }
}

#[tokio::test]
async fn a_client_waiting_on_100_continue_gets_the_413_and_no_100_continue() {
    let answer = against(photo_routes(), options(), |addr| async move {
        exchange(addr, &photo_head(2048, "Expect: 100-continue\r\n")).await
    })
    .await;
    assert!(answer.starts_with("HTTP/1.1 413"), "{answer}");
    assert!(!answer.contains("100 Continue"), "{answer}");
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
