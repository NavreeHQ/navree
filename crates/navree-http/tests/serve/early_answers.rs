//! An answer that goes out before its body has arrived (a 401, a 404, a 413, a
//! handler that does not need the body): the rest of the body is read after
//! it, so the connection goes on, or the connection closes after it with a
//! lingering close, so the client reads the answer and not a reset.

use std::time::{Duration, Instant};

use http::{Method, Request, Response, StatusCode};
use navree_http::body::{Body, read_body};
use navree_http::routing::Router;
use navree_http::{HttpOptions, ServerOptions};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

use crate::support::{
    GET, PATIENCE, answer, connect, exchange, no_limits, read_response, start, start_with,
};

/// `GET /` says hello, `POST /ignore` answers without reading its body, and
/// `POST /small` reads a kilobyte of it at most (413 past that).
fn app() -> Router {
    Router::new()
        .route(Method::GET, "/", || async { "hello" })
        .route(Method::POST, "/ignore", || async { "ignored" })
        .route(
            Method::POST,
            "/small",
            |request: Request<Body>| async move {
                match read_body(request.into_body(), 1024).await {
                    Ok(body) => Response::new(Body::from(body)),
                    Err(error) => Response::builder()
                        .status(error.status())
                        .body(Body::from(error.to_string()))
                        .unwrap(),
                }
            },
        )
}

/// The head of a request that declares `length` bytes of body.
fn head(request_line: &str, length: usize) -> Vec<u8> {
    format!("{request_line} HTTP/1.1\r\nHost: test\r\nContent-Length: {length}\r\n\r\n")
        .into_bytes()
}

/// Whether an answer says the connection closes.
fn closes(answer: &str) -> bool {
    answer.to_ascii_lowercase().contains("connection: close")
}

/// Reads until the server closes the connection: what it sent until then. The
/// close must be a clean one, not a reset.
async fn clean_close(stream: &mut TcpStream) -> String {
    let mut seen = Vec::new();
    timeout(PATIENCE, stream.read_to_end(&mut seen))
        .await
        .expect("the server closes the connection")
        .expect("a clean close, not a reset");
    String::from_utf8_lossy(&seen).into_owned()
}

/// The answer comes before the body: the client sends the rest after reading
/// it, and the same connection answers the next request.
#[tokio::test]
async fn the_rest_of_a_body_is_read_after_an_early_answer_and_the_connection_goes_on() {
    let server = start(app(), no_limits()).await;
    for (request_line, status) in [
        ("POST /ignore", "200"),
        ("POST /missing", "404"),
        ("PUT /", "405"),
    ] {
        let mut client = connect(server.addr).await;
        client
            .write_all(&head(request_line, 32 * 1024))
            .await
            .unwrap();
        client.write_all(&[b'x'; 1024]).await.unwrap();
        let answer = read_response(&mut client).await;
        assert!(
            answer.starts_with(&format!("HTTP/1.1 {status}")),
            "{request_line}: {answer}"
        );
        assert!(!closes(&answer), "{request_line}: {answer}");
        client.write_all(&[b'x'; 31 * 1024]).await.unwrap();
        let next = exchange(&mut client, GET).await;
        assert!(next.ends_with("hello"), "{request_line}: {next}");
    }

    // All of it in one go, as a browser sends it, with the next request behind.
    let mut client = connect(server.addr).await;
    let mut requests = head("POST /ignore", 256 * 1024);
    requests.extend(vec![b'x'; 256 * 1024]);
    requests.extend(b"GET / HTTP/1.1\r\nHost: test\r\nConnection: close\r\n\r\n");
    client.write_all(&requests).await.unwrap();
    let answers = clean_close(&mut client).await;
    assert!(answers.starts_with("HTTP/1.1 200"), "{answers}");
    assert_eq!(answers.matches("HTTP/1.1 200").count(), 2, "{answers}");
    assert!(answers.ends_with("hello"), "{answers}");
}

/// What is left is read up to `http.body_limit_bytes` (in an app, the route's
/// own limit): a body that declares one byte more closes the connection after
/// its answer, and the client still reads the answer and a clean close.
#[tokio::test]
async fn a_body_that_declares_more_than_is_read_closes_the_connection_after_its_answer() {
    let limit = 16 * 1024;
    let server = start_with(
        app(),
        HttpOptions {
            body_limit_bytes: limit,
            server: no_limits(),
            ..HttpOptions::default()
        },
    )
    .await;

    let mut client = connect(server.addr).await;
    client
        .write_all(&head("POST /ignore", limit))
        .await
        .unwrap();
    let answer = read_response(&mut client).await;
    assert!(answer.ends_with("ignored") && !closes(&answer), "{answer}");
    client.write_all(&vec![b'x'; limit]).await.unwrap();
    assert!(exchange(&mut client, GET).await.ends_with("hello"));

    let mut client = connect(server.addr).await;
    client
        .write_all(&head("POST /ignore", limit + 1))
        .await
        .unwrap();
    let answer = read_response(&mut client).await;
    assert!(answer.ends_with("ignored") && closes(&answer), "{answer}");
    client
        .write_all(&vec![b'x'; limit + 1])
        .await
        .expect("the server reads what it is sent");
    assert_eq!(clean_close(&mut client).await, "");
}

/// A client that stops sending gets its answer at once, and the connection
/// closes once the body's pace runs out, cleanly.
#[tokio::test]
async fn a_stalled_body_gets_its_answer_at_once_and_then_a_clean_close() {
    let server = start(
        app(),
        ServerOptions {
            body_read_timeout_ms: Some(300),
            ..no_limits()
        },
    )
    .await;
    let mut client = connect(server.addr).await;
    client.write_all(&head("POST /ignore", 4096)).await.unwrap();
    client.write_all(&[b'x'; 100]).await.unwrap();
    let began = Instant::now();
    let answer = read_response(&mut client).await;
    assert!(answer.ends_with("ignored") && !closes(&answer), "{answer}");
    assert_eq!(clean_close(&mut client).await, "");
    let waited = began.elapsed();
    assert!(
        waited >= Duration::from_millis(250),
        "not before the body's time ran out: {waited:?}"
    );
}

/// Whatever the early answer, a client that sends its whole body before it
/// reads gets that answer, not a reset.
#[tokio::test]
async fn a_client_that_sends_its_whole_body_first_reads_the_early_answer_not_a_reset() {
    let app = answer(|request: Request<Body>| async move {
        let status = request
            .uri()
            .path()
            .trim_start_matches('/')
            .parse::<u16>()
            .ok()
            .and_then(|code| StatusCode::from_u16(code).ok())
            .unwrap_or(StatusCode::OK);
        Response::builder()
            .status(status)
            .body(Body::from(status.to_string()))
            .unwrap()
    });
    let server = start(app, no_limits()).await;
    // Over `http.body_limit_bytes` (2 MiB): none of it is read for the next
    // request, so all of it is read by the close.
    let length = 3 * 1024 * 1024;
    for status in [401, 403, 404, 405, 413, 429, 503] {
        let mut client = connect(server.addr).await;
        let mut request = head(&format!("POST /{status}"), length);
        request.extend(vec![b'x'; length]);
        client
            .write_all(&request)
            .await
            .expect("the server reads what it is sent");
        let answer = clean_close(&mut client).await;
        assert!(
            answer.starts_with(&format!("HTTP/1.1 {status}")),
            "{answer}"
        );
        assert!(closes(&answer), "{answer}");
    }
}

/// A client waiting on `100 Continue` has sent nothing: an early answer is
/// all it gets, and the connection closes.
#[tokio::test]
async fn a_client_waiting_on_100_continue_gets_the_answer_and_a_close() {
    let server = start(app(), no_limits()).await;
    let mut client = connect(server.addr).await;
    client
        .write_all(
            b"POST /ignore HTTP/1.1\r\nHost: test\r\nContent-Length: 4096\r\nExpect: 100-continue\r\n\r\n",
        )
        .await
        .unwrap();
    let answer = clean_close(&mut client).await;
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
    assert!(!answer.contains("100 Continue"), "{answer}");
    assert!(closes(&answer), "{answer}");
}

/// A body without a length that goes over what its handler reads gets the 413,
/// and the rest of it is read after: the connection takes the next request.
#[tokio::test]
async fn a_chunked_body_over_its_handlers_limit_gets_its_413_and_the_connection_goes_on() {
    let server = start(app(), no_limits()).await;
    let mut client = connect(server.addr).await;
    let mut request =
        b"POST /small HTTP/1.1\r\nHost: test\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
    for _ in 0..4 {
        request.extend(b"400\r\n");
        request.extend([b'x'; 1024]);
        request.extend(b"\r\n");
    }
    request.extend(b"0\r\n\r\n");
    request.extend(GET);
    let refused = exchange(&mut client, &request).await;
    assert!(refused.starts_with("HTTP/1.1 413"), "{refused}");
    assert!(!closes(&refused), "{refused}");
    let next = read_response(&mut client).await;
    assert!(next.ends_with("hello"), "{next}");
}
