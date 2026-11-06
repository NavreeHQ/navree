//! `http.server.max_connections` and `max_connections_per_ip`: what a full
//! server does.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use http::{Request, Response, StatusCode};
use http_body_util::Empty;
use hyper::client::conn::http2::Builder;
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use navree_http::body::Body;
use navree_http::{HttpOptions, ServerOptions};
use tokio::io::AsyncWriteExt;
use tokio::sync::Notify;

use crate::support::{
    GET, PATIENCE, answer, connect, echo_app, exchange, no_limits, read_response,
    read_until_closed, start, start_with, stays_open,
};

fn cap(connections: usize) -> ServerOptions {
    ServerOptions {
        max_connections: Some(connections),
        ..no_limits()
    }
}

/// At the cap a new client is neither answered nor refused: it waits, and
/// it is served as soon as a slot frees up. These two hold theirs with no
/// request finished, so neither is idle.
#[tokio::test]
async fn a_client_over_the_cap_waits_for_a_slot() {
    let server = start(echo_app(), cap(2)).await;
    let first = connect(server.addr).await;
    let _second = connect(server.addr).await;

    let mut third = connect(server.addr).await;
    third.write_all(GET).await.unwrap();
    assert!(
        stays_open(&mut third, Duration::from_millis(500)).await,
        "nothing is read from a connection over the cap"
    );

    drop(first);
    let response = read_response(&mut third).await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
}

/// The cap is on what is served at once, and no burst gets past it: every
/// client is served in the end, never more than the cap at a time.
#[tokio::test]
async fn a_burst_never_exceeds_the_cap_and_every_client_is_served() {
    let open = Arc::new(AtomicUsize::new(0));
    let most = Arc::new(AtomicUsize::new(0));
    let app = {
        let (open, most) = (open.clone(), most.clone());
        answer(move |_request| {
            let (open, most) = (open.clone(), most.clone());
            async move {
                let now = open.fetch_add(1, Ordering::SeqCst) + 1;
                most.fetch_max(now, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(5)).await;
                open.fetch_sub(1, Ordering::SeqCst);
                Response::new(Body::from("ok"))
            }
        })
    };
    let server = start(app, cap(4)).await;
    let clients: Vec<_> = (0..64)
        .map(|_| {
            let addr = server.addr;
            tokio::spawn(async move {
                let mut client = connect(addr).await;
                exchange(
                    &mut client,
                    b"GET / HTTP/1.1\r\nHost: test\r\nConnection: close\r\n\r\n",
                )
                .await
            })
        })
        .collect();
    for client in clients {
        let response = client.await.unwrap();
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    }
    assert!(most.load(Ordering::SeqCst) <= 4);
}

/// Filling the server is no way to take it down: slots held by clients that
/// send nothing come back at the header timeout, and a real client gets in.
#[tokio::test]
async fn clients_holding_the_server_full_lose_their_slots() {
    let server = start(
        echo_app(),
        ServerOptions {
            header_read_timeout_ms: Some(200),
            ..cap(4)
        },
    )
    .await;
    let mut holders = Vec::new();
    for _ in 0..4 {
        holders.push(connect(server.addr).await);
    }
    let mut client = connect(server.addr).await;
    client.write_all(GET).await.unwrap();
    let response = read_response(&mut client).await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
}

/// The client that gets a server full of idle connections in gets the slot
/// of the one idle the longest, and the others are left alone.
#[tokio::test]
async fn the_connection_idle_the_longest_makes_way_for_a_client_that_waits() {
    let server = start(echo_app(), cap(2)).await;
    let mut oldest = connect(server.addr).await;
    assert!(exchange(&mut oldest, GET).await.starts_with("HTTP/1.1 200"));
    tokio::time::sleep(Duration::from_millis(20)).await;
    let mut newest = connect(server.addr).await;
    assert!(exchange(&mut newest, GET).await.starts_with("HTTP/1.1 200"));

    let mut waiting = connect(server.addr).await;
    let response = exchange(&mut waiting, GET).await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert_eq!(read_until_closed(&mut oldest).await, "", "a clean close");
    assert!(exchange(&mut newest, GET).await.starts_with("HTTP/1.1 200"));
}

/// HTTP/2 hears why: a GOAWAY, as at its idle timeout.
#[tokio::test]
async fn an_idle_http2_connection_makes_way_with_a_goaway() {
    let server = start(echo_app(), cap(1)).await;
    let stream = connect(server.addr).await;
    let (mut sender, connection) = Builder::new(TokioExecutor::new())
        .timer(TokioTimer::new())
        .handshake::<_, Empty<bytes::Bytes>>(TokioIo::new(stream))
        .await
        .unwrap();
    let connection = tokio::spawn(async { connection.await.is_ok() });
    let request = Request::get("http://test/").body(Empty::new()).unwrap();
    let response = sender.send_request(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let mut waiting = connect(server.addr).await;
    let response = exchange(&mut waiting, GET).await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    let graceful = tokio::time::timeout(PATIENCE, connection)
        .await
        .expect("the connection ends")
        .unwrap();
    assert!(graceful, "a GOAWAY, not a dropped socket");
}

/// A connection with a request in flight is not idle, however long it takes.
#[tokio::test]
async fn a_request_in_flight_keeps_its_slot_from_a_client_that_waits() {
    let release = Arc::new(Notify::new());
    let app = {
        let release = release.clone();
        answer(move |_request| {
            let release = release.clone();
            async move {
                release.notified().await;
                Response::new(Body::from("done"))
            }
        })
    };
    let server = start(app, cap(1)).await;
    let mut busy = connect(server.addr).await;
    busy.write_all(GET).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut waiting = connect(server.addr).await;
    waiting.write_all(GET).await.unwrap();
    assert!(stays_open(&mut waiting, Duration::from_millis(400)).await);

    release.notify_one();
    let response = read_response(&mut busy).await;
    assert!(response.ends_with("done"), "{response}");
    release.notify_one();
    let response = read_response(&mut waiting).await;
    assert!(response.ends_with("done"), "{response}");
}

fn per_ip(connections: usize) -> ServerOptions {
    ServerOptions {
        max_connections_per_ip: Some(connections),
        ..no_limits()
    }
}

/// The address is only known after the accept, so a client over its limit,
/// whose address has no idle connection to give up, is closed rather than left
/// waiting, and its slot comes back with a connection.
#[tokio::test]
async fn a_client_over_its_address_limit_is_closed_when_none_of_its_connections_is_idle() {
    let server = start(echo_app(), per_ip(2)).await;
    // Neither has finished a request, so neither is idle.
    let mut first = connect(server.addr).await;
    let _second = connect(server.addr).await;

    let mut over = connect(server.addr).await;
    assert_eq!(read_until_closed(&mut over).await, "");

    assert!(exchange(&mut first, GET).await.starts_with("HTTP/1.1 200"));
    drop(first);
    let served = tokio::time::timeout(PATIENCE, async {
        loop {
            let mut next = connect(server.addr).await;
            if next.write_all(GET).await.is_ok()
                && read_response(&mut next).await.starts_with("HTTP/1.1 200")
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    });
    served.await.expect("the address has room again");
}

/// Many users behind one address (an office, a carrier's NAT), or one browser
/// keeping its connections: at the address's limit a new connection takes the
/// place of its connection idle the longest, which closes as at its idle
/// timeout, and the others are left alone.
#[tokio::test]
async fn at_its_limit_an_address_trades_its_longest_idle_connection_for_a_new_one() {
    let server = start(echo_app(), per_ip(2)).await;
    let mut oldest = connect(server.addr).await;
    assert!(exchange(&mut oldest, GET).await.starts_with("HTTP/1.1 200"));
    tokio::time::sleep(Duration::from_millis(20)).await;
    let mut newest = connect(server.addr).await;
    assert!(exchange(&mut newest, GET).await.starts_with("HTTP/1.1 200"));

    let mut new = connect(server.addr).await;
    let response = exchange(&mut new, GET).await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert_eq!(read_until_closed(&mut oldest).await, "", "a clean close");
    assert!(exchange(&mut newest, GET).await.starts_with("HTTP/1.1 200"));
    assert!(exchange(&mut new, GET).await.starts_with("HTTP/1.1 200"));

    // The new one is idle now too, and the next makes way in its turn.
    let mut next = connect(server.addr).await;
    assert!(exchange(&mut next, GET).await.starts_with("HTTP/1.1 200"));
    assert_eq!(read_until_closed(&mut newest).await, "");
}

/// A proxy carries every client, so counting it would limit them all
/// together.
#[tokio::test]
async fn a_listed_proxy_is_not_held_to_the_address_limit() {
    let server = start_with(
        echo_app(),
        HttpOptions {
            server: per_ip(1),
            trusted_proxies: vec!["127.0.0.1".to_owned()],
            ..HttpOptions::default()
        },
    )
    .await;
    let mut clients = Vec::new();
    for _ in 0..4 {
        let mut client = connect(server.addr).await;
        assert!(exchange(&mut client, GET).await.starts_with("HTTP/1.1 200"));
        clients.push(client);
    }
}

/// The two limits together: the slot of a client turned away is free for the
/// next one.
#[tokio::test]
async fn a_client_over_its_address_limit_takes_no_slot() {
    let server = start(
        echo_app(),
        ServerOptions {
            max_connections: Some(2),
            ..per_ip(1)
        },
    )
    .await;
    // On its first request, so it is not idle and does not make way.
    let mut held = connect(server.addr).await;
    held.write_all(b"GET / HT").await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    for _ in 0..20 {
        let mut over = connect(server.addr).await;
        assert_eq!(read_until_closed(&mut over).await, "");
    }
    let response = exchange(&mut held, b"TP/1.1\r\nHost: test\r\n\r\n").await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
}

/// A server waiting for a slot still shuts down.
#[tokio::test]
async fn a_full_server_shuts_down() {
    let mut server = start(echo_app(), cap(1)).await;
    let mut holder = connect(server.addr).await;
    assert!(exchange(&mut holder, GET).await.starts_with("HTTP/1.1 200"));
    let _waiting = connect(server.addr).await;
    server.stop();
    tokio::time::timeout(PATIENCE, server.stopped())
        .await
        .expect("the server ends");
}
