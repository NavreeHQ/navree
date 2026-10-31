//! Clients that take their time: every limit cuts them off, and the same
//! client with the limit off shows the hole it closes.

use std::convert::Infallible;
use std::time::{Duration, Instant};

use bytes::Bytes;
use http::{Request, Response, StatusCode};
use navree_http::ServerOptions;
use navree_http::body::{Body, read_body};
use navree_http::serve::BodyReadLimit;
use tokio::io::AsyncWriteExt;

use crate::support::{
    Dribble, GET, OFF_MARGIN, connect, dribble, echo_app, exchange, no_limits, read_until_closed,
    start, stays_open,
};

const LIMIT: Duration = Duration::from_millis(300);
const LIMIT_MS: u64 = 300;
/// Well under [`LIMIT`], so a dribbling client never stalls.
const EVERY: Duration = Duration::from_millis(25);

/// A head that takes about 2.5 s at [`EVERY`]: the client is still sending
/// long after [`LIMIT`], and it finishes the request if nothing stops it.
fn long_head() -> Vec<u8> {
    let mut head = b"GET / HTTP/1.1\r\nHost: test\r\nX-Slow: ".to_vec();
    head.extend(std::iter::repeat_n(b'a', 64));
    head.extend_from_slice(b"\r\n\r\n");
    head
}

fn header_timeout() -> ServerOptions {
    ServerOptions {
        header_read_timeout_ms: Some(LIMIT_MS),
        ..no_limits()
    }
}

#[tokio::test]
async fn slow_headers_are_cut_off_at_the_timeout() {
    let server = start(echo_app(), header_timeout()).await;
    let started = Instant::now();
    let mut client = connect(server.addr).await;
    let ended = dribble(&mut client, &long_head(), EVERY).await;
    assert_eq!(ended, Dribble::Closed(String::new()));
    assert!(started.elapsed() >= LIMIT, "{:?}", started.elapsed());
}

/// The hole the timeout closes: with no timeout the server waits for this
/// client for as long as it keeps sending, and a client that never finishes
/// holds the connection without end.
#[tokio::test]
async fn with_no_header_timeout_a_slow_client_holds_the_connection() {
    let server = start(echo_app(), no_limits()).await;
    let started = Instant::now();
    let mut client = connect(server.addr).await;
    assert_eq!(
        dribble(&mut client, &long_head(), EVERY).await,
        Dribble::Sent
    );
    assert!(started.elapsed() >= LIMIT * OFF_MARGIN);
    let response = crate::support::read_response(&mut client).await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
}

/// Sending nothing, and every prefix of the HTTP/2 preface: each stops the
/// protocol detection, before any HTTP code has started.
#[tokio::test]
async fn a_client_that_stops_before_the_protocol_is_known_is_cut_off() {
    let server = start(echo_app(), header_timeout()).await;
    for prefix in [
        &b""[..],
        b"P",
        b"PRI * HTTP/2.0\r\n",
        b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r",
    ] {
        let started = Instant::now();
        let mut client = connect(server.addr).await;
        client.write_all(prefix).await.unwrap();
        assert_eq!(read_until_closed(&mut client).await, "", "{prefix:?}");
        assert!(started.elapsed() >= LIMIT, "{prefix:?}");
    }
}

#[tokio::test]
async fn with_no_header_timeout_a_silent_client_holds_the_connection() {
    let server = start(echo_app(), no_limits()).await;
    let mut client = connect(server.addr).await;
    assert!(stays_open(&mut client, LIMIT * OFF_MARGIN).await);
    let response = exchange(&mut client, GET).await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
}

/// A keep-alive connection's next head gets the header timeout from its
/// first byte, not the longer idle timeout.
#[tokio::test]
async fn the_next_head_on_a_connection_is_held_to_the_header_timeout() {
    let server = start(
        echo_app(),
        ServerOptions {
            header_read_timeout_ms: Some(LIMIT_MS),
            idle_timeout_ms: Some(60_000),
            ..no_limits()
        },
    )
    .await;
    let mut client = connect(server.addr).await;
    let response = exchange(&mut client, GET).await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    let started = Instant::now();
    let ended = dribble(&mut client, &long_head(), EVERY).await;
    assert_eq!(ended, Dribble::Closed(String::new()));
    assert!(started.elapsed() >= LIMIT, "{:?}", started.elapsed());
}

#[tokio::test]
async fn an_idle_keep_alive_connection_is_closed() {
    let server = start(
        echo_app(),
        ServerOptions {
            idle_timeout_ms: Some(LIMIT_MS),
            ..no_limits()
        },
    )
    .await;
    let mut client = connect(server.addr).await;
    let response = exchange(&mut client, GET).await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    let started = Instant::now();
    assert_eq!(read_until_closed(&mut client).await, "");
    assert!(
        started.elapsed() >= LIMIT - EVERY,
        "{:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn with_no_idle_timeout_a_keep_alive_connection_stays_open() {
    let server = start(echo_app(), no_limits()).await;
    let mut client = connect(server.addr).await;
    exchange(&mut client, GET).await;
    assert!(stays_open(&mut client, LIMIT * OFF_MARGIN).await);
    let response = exchange(&mut client, GET).await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
}

/// hyper's own header timeout would close this connection: idle time is not
/// head time.
#[tokio::test]
async fn a_connection_idle_for_longer_than_the_header_timeout_stays_open() {
    let server = start(
        echo_app(),
        ServerOptions {
            header_read_timeout_ms: Some(LIMIT_MS),
            idle_timeout_ms: Some(20_000),
            ..no_limits()
        },
    )
    .await;
    let mut client = connect(server.addr).await;
    exchange(&mut client, GET).await;
    assert!(stays_open(&mut client, LIMIT * OFF_MARGIN).await);
    let response = exchange(&mut client, GET).await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
}

/// A response streaming slowly is a request in flight, however long the
/// gaps between its chunks.
#[tokio::test]
async fn a_slow_streaming_response_is_not_an_idle_connection() {
    let app = crate::support::answer(|_request| async {
        let chunks = futures_util::stream::unfold(0, |sent| async move {
            if sent == 3 {
                return None;
            }
            tokio::time::sleep(LIMIT).await;
            Some((Ok::<_, Infallible>(Bytes::from("chunk;")), sent + 1))
        });
        Response::new(Body::from_stream(chunks))
    });
    let server = start(
        app,
        ServerOptions {
            header_read_timeout_ms: Some(LIMIT_MS / 3),
            idle_timeout_ms: Some(LIMIT_MS / 3),
            ..no_limits()
        },
    )
    .await;
    let mut client = connect(server.addr).await;
    client.write_all(GET).await.unwrap();
    let response = read_until_closed(&mut client).await;
    assert_eq!(response.matches("chunk;").count(), 3, "{response}");
}

fn body_timeout(rate: Option<u64>) -> ServerOptions {
    ServerOptions {
        body_read_timeout_ms: Some(LIMIT_MS),
        body_min_bytes_per_sec: rate,
        ..no_limits()
    }
}

#[tokio::test]
async fn a_stalled_body_is_answered_408_and_the_connection_closes() {
    let server = start(echo_app(), body_timeout(None)).await;
    for request in [
        // A declared length, and chunks: the body stops halfway both ways.
        &b"POST /echo HTTP/1.1\r\nHost: test\r\nContent-Length: 100\r\n\r\nhalf"[..],
        b"POST /echo HTTP/1.1\r\nHost: test\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nhalf\r\n",
    ] {
        let started = Instant::now();
        let mut client = connect(server.addr).await;
        client.write_all(request).await.unwrap();
        let response = read_until_closed(&mut client).await;
        assert!(response.starts_with("HTTP/1.1 408"), "{response}");
        assert!(response.ends_with("it arrived too slowly"), "{response}");
        assert!(started.elapsed() >= LIMIT, "{:?}", started.elapsed());
    }
}

#[tokio::test]
async fn a_body_dripping_below_the_minimum_rate_is_cut_off_though_it_never_stalls() {
    let server = start(echo_app(), body_timeout(Some(1_000_000))).await;
    let mut client = connect(server.addr).await;
    client
        .write_all(b"POST /echo HTTP/1.1\r\nHost: test\r\nContent-Length: 200\r\n\r\n")
        .await
        .unwrap();
    let started = Instant::now();
    let ended = dribble(&mut client, &[b'x'; 200], EVERY).await;
    let Dribble::Closed(seen) = ended else {
        panic!("the server let a dripping body finish");
    };
    // A byte sent into the closed socket can reset it before the 408 is read.
    assert!(
        seen.is_empty() || seen.starts_with("HTTP/1.1 408"),
        "{seen}"
    );
    assert!(
        started.elapsed() >= LIMIT - EVERY,
        "{:?}",
        started.elapsed()
    );
}

/// The same client, with no limit: the connection is held for as long as
/// it likes.
#[tokio::test]
async fn with_no_body_limit_a_dripping_body_holds_the_connection() {
    let server = start(echo_app(), no_limits()).await;
    let mut client = connect(server.addr).await;
    client
        .write_all(b"POST /echo HTTP/1.1\r\nHost: test\r\nContent-Length: 60\r\n\r\n")
        .await
        .unwrap();
    let started = Instant::now();
    assert_eq!(
        dribble(&mut client, &[b'x'; 60], EVERY).await,
        Dribble::Sent
    );
    assert!(started.elapsed() >= LIMIT);
    let response = crate::support::read_response(&mut client).await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
}

#[tokio::test]
async fn a_body_at_a_fair_pace_arrives_whole() {
    let server = start(echo_app(), body_timeout(Some(1_000))).await;
    let mut client = connect(server.addr).await;
    let body = vec![b'x'; 256 * 1024];
    let mut request = format!(
        "POST /echo HTTP/1.1\r\nHost: test\r\nContent-Length: {}\r\n\r\n",
        body.len()
    )
    .into_bytes();
    request.extend_from_slice(&body);
    let response = exchange(&mut client, &request).await;
    assert!(response.starts_with("HTTP/1.1 200"), "{}", &response[..64]);
    assert!(response.ends_with(&"x".repeat(1024)));
}

/// A handler busy before it reads costs the client nothing: the budget
/// runs only while the server waits on the client.
#[tokio::test]
async fn a_slow_handler_is_not_the_clients_fault() {
    let app = crate::support::answer(|request: Request<Body>| async move {
        tokio::time::sleep(LIMIT * OFF_MARGIN).await;
        let body = read_body(request.into_body(), 1024).await.unwrap();
        Response::new(Body::from(body))
    });
    let server = start(app, body_timeout(Some(1_000_000))).await;
    let mut client = connect(server.addr).await;
    let response = exchange(
        &mut client,
        b"POST /echo HTTP/1.1\r\nHost: test\r\nContent-Length: 5\r\n\r\nhello",
    )
    .await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.ends_with("hello"), "{response}");
}

/// A body that is a conversation opts out through the request.
#[tokio::test]
async fn a_handler_can_lift_the_limit_for_its_request() {
    let app = crate::support::answer(|request: Request<Body>| async move {
        request
            .extensions()
            .get::<BodyReadLimit>()
            .expect("a request with a body carries the limit")
            .lift();
        let body = read_body(request.into_body(), 1024).await.unwrap();
        Response::new(Body::from(body))
    });
    let server = start(app, body_timeout(Some(1_000_000))).await;
    let mut client = connect(server.addr).await;
    client
        .write_all(b"POST / HTTP/1.1\r\nHost: test\r\nContent-Length: 5\r\n\r\nhel")
        .await
        .unwrap();
    assert!(stays_open(&mut client, LIMIT * OFF_MARGIN).await);
    let response = exchange(&mut client, b"lo").await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.ends_with("hello"), "{response}");
}

/// A route that reads no body is not held to the limit, and the unread
/// body does not keep the connection for the next client.
#[tokio::test]
async fn a_body_nobody_reads_ends_with_its_response() {
    let server = start(echo_app(), body_timeout(Some(1_000_000))).await;
    let mut client = connect(server.addr).await;
    client
        .write_all(b"POST / HTTP/1.1\r\nHost: test\r\nContent-Length: 100\r\n\r\nhalf")
        .await
        .unwrap();
    let response = read_until_closed(&mut client).await;
    assert_eq!(
        response.split("\r\n").next(),
        Some(format!("HTTP/1.1 {}", StatusCode::METHOD_NOT_ALLOWED).as_str())
    );
}
