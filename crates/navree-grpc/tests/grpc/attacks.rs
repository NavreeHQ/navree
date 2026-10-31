//! What a hostile client can do to the gRPC edge over a real socket, with
//! no credentials.

use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use navree_grpc::{
    GrpcModule, GrpcServer, ModuleBuilderGrpcExt, Request, Response, Streaming, grpc_service,
};
use navree_http::{AppError, Authenticator, HttpServer};
use navree_kernel::{
    BoxFuture, Injectable, KernelError, Module, ModuleBuilder, ProviderKey, Resolver,
};
use navree_observe::{ObserveModule, ObserveOptions};
use navree_testing::TestApp;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

use crate::support::pb::deals::deals_client::DealsClient;
use crate::support::pb::deals::deals_server::{Deals, DealsServer};
use crate::support::pb::deals::{
    Deal, DealEvent, GetDealRequest, ImportDealsResponse, ListDealsRequest, ListDealsResponse,
    MoveDealRequest, WatchDealsRequest,
};
use crate::support::pb::ping::PingRequest;
use crate::support::pb::ping::ping_client::PingClient;
use crate::support::{BearerAuthenticator, DealEvents, deals_app, ping_app};

/// Every limit that could end these calls, short.
const LIMIT_MS: u64 = 200;
/// Many times longer than every limit, the request timeouts included.
const WELL_PAST_THE_LIMITS: Duration = Duration::from_secs(6);

/// A unary call of the health service, which every app has by default,
/// whose five byte frame header and message never arrive.
const STALLED_CALL: &[u8] = b"POST /grpc.health.v1.Health/Check HTTP/1.1\r\n\
    Host: test\r\n\
    Content-Type: application/grpc\r\n\
    Content-Length: 5\r\n\r\n";

/// The same call as a Connect unary request, whose body keeps the pace.
const STALLED_CONNECT_CALL: &[u8] = b"POST /grpc.health.v1.Health/Check HTTP/1.1\r\n\
    Host: test\r\n\
    Content-Type: application/proto\r\n\
    Content-Length: 5\r\n\r\n";

fn limited(app: navree_testing::TestAppBuilder) -> navree_testing::TestAppBuilder {
    app.config("http.addr", "127.0.0.1:0")
        .config("http.shutdown_grace_secs", 1)
        .config("http.timeout_secs", 1)
        .config("grpc.timeout_secs", 1)
        .config("http.server.header_read_timeout_ms", LIMIT_MS)
        .config("http.server.idle_timeout_ms", LIMIT_MS)
        .config("http.server.body_read_timeout_ms", LIMIT_MS)
}

async fn serving<F: Future<Output = ()>>(app: &TestApp, calls: F) {
    let serve = app.run_command("serve", &[]);
    let calls = async {
        calls.await;
        app.ctx().shutdown_token().cancel();
    };
    let (served, ()) = tokio::join!(serve, calls);
    served.expect("serve stops cleanly");
}

/// How long the server took to close `stream` or answer on it; `None` if
/// it did neither in [`WELL_PAST_THE_LIMITS`].
async fn ended(stream: &mut TcpStream) -> Option<Duration> {
    let started = Instant::now();
    let mut byte = [0_u8; 1];
    timeout(WELL_PAST_THE_LIMITS, stream.read(&mut byte))
        .await
        .ok()
        .map(|_| started.elapsed())
}

/// The same stalled body as a Connect unary call, which the edge reads itself.
#[tokio::test]
async fn a_stalled_connect_unary_body_is_cut_off() {
    let app = limited(ping_app()).build().await;
    serving(&app, async {
        let addr = app.get::<HttpServer>().await.wait_bound().await;
        let mut client = TcpStream::connect(addr).await.unwrap();
        client.write_all(STALLED_CONNECT_CALL).await.unwrap();
        ended(&mut client).await.expect("the body read timeout");
    })
    .await;
}

/// tonic reads a unary request's body before it calls the method, so neither
/// the call's deadline nor its authentication has started: the pace of a
/// request body is all that ends this request. A content type must not lift
/// it, whoever sends it.
#[tokio::test]
async fn a_unary_grpc_call_whose_body_never_arrives_is_cut_off() {
    let app = limited(ping_app()).build().await;
    serving(&app, async {
        let addr = app.get::<HttpServer>().await.wait_bound().await;
        let mut client = TcpStream::connect(addr).await.unwrap();
        client.write_all(STALLED_CALL).await.unwrap();
        assert!(
            ended(&mut client).await.is_some(),
            "still held {WELL_PAST_THE_LIMITS:?} after limits of {LIMIT_MS} ms"
        );
    })
    .await;
}

/// The same on the listener of its own.
#[tokio::test]
async fn a_unary_grpc_call_whose_body_never_arrives_is_cut_off_on_the_grpc_port() {
    let app = limited(ping_app())
        .config("grpc.addr", "127.0.0.1:0")
        .build()
        .await;
    serving(&app, async {
        app.get::<HttpServer>().await.wait_bound().await;
        let addr = timeout(
            Duration::from_secs(5),
            app.get::<GrpcServer>().await.wait_bound(),
        )
        .await
        .expect("the gRPC listener starts with serve");
        let mut client = TcpStream::connect(addr).await.unwrap();
        client.write_all(STALLED_CALL).await.unwrap();
        assert!(
            ended(&mut client).await.is_some(),
            "still held {WELL_PAST_THE_LIMITS:?} after limits of {LIMIT_MS} ms"
        );
    })
    .await;
}

/// One stalled call per slot, a few hundred bytes in all, must not stop the
/// server serving everyone else.
#[tokio::test]
async fn stalled_grpc_calls_cannot_fill_the_server() {
    const SLOTS: usize = 4;
    let app = limited(ping_app())
        .config("http.server.max_connections", SLOTS as u64)
        .build()
        .await;
    serving(&app, async {
        let addr = app.get::<HttpServer>().await.wait_bound().await;
        let mut stalled = Vec::new();
        for _ in 0..SLOTS {
            let mut client = TcpStream::connect(addr).await.unwrap();
            client.write_all(STALLED_CALL).await.unwrap();
            stalled.push(client);
        }
        let mut honest = TcpStream::connect(addr).await.unwrap();
        honest
            .write_all(b"GET /health/live HTTP/1.1\r\nHost: test\r\n\r\n")
            .await
            .unwrap();
        assert!(
            ended(&mut honest).await.is_some(),
            "an honest client got nothing in {WELL_PAST_THE_LIMITS:?}"
        );
    })
    .await;
}

/// A streaming method's pace lifts once a call has passed its access check, so
/// a call with no credentials gets no further than a stalled unary one.
#[tokio::test]
async fn a_stream_with_no_credentials_never_lifts_the_limit() {
    let app = limited(deals_app()).build().await;
    serving(&app, async {
        let addr = app.get::<HttpServer>().await.wait_bound().await;
        let mut client = TcpStream::connect(addr).await.unwrap();
        client
            .write_all(
                b"POST /crm.deals.v1.Deals/Echo HTTP/1.1\r\n\
                Host: test\r\n\
                Content-Type: application/grpc\r\n\
                Content-Length: 5\r\n\r\n",
            )
            .await
            .unwrap();
        assert!(
            ended(&mut client).await.is_some(),
            "still held {WELL_PAST_THE_LIMITS:?} after limits of {LIMIT_MS} ms"
        );
    })
    .await;
}

fn frame(kind: u8, flags: u8, stream: u32, payload: &[u8]) -> Vec<u8> {
    let mut frame = (payload.len() as u32).to_be_bytes()[1..].to_vec();
    frame.push(kind);
    frame.push(flags);
    frame.extend_from_slice(&stream.to_be_bytes());
    frame.extend_from_slice(payload);
    frame
}

/// A header field as an HPACK literal, without Huffman coding.
fn literal(name: &str, value: &str) -> Vec<u8> {
    let mut encoded = vec![0x00, name.len() as u8];
    encoded.extend_from_slice(name.as_bytes());
    encoded.push(value.len() as u8);
    encoded.extend_from_slice(value.as_bytes());
    encoded
}

async fn http2(addr: SocketAddr) -> TcpStream {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n")
        .await
        .unwrap();
    stream.write_all(&frame(4, 0, 0, &[])).await.unwrap();
    stream
}

/// Reads frames until the connection ends; how long it took, `None` if it
/// outlived [`WELL_PAST_THE_LIMITS`]. Answers no ping: only the clock can
/// end this connection.
async fn connection_ends(stream: &mut TcpStream) -> Option<Duration> {
    let started = Instant::now();
    timeout(WELL_PAST_THE_LIMITS, async {
        let mut chunk = [0_u8; 4096];
        loop {
            match stream.read(&mut chunk).await {
                Ok(0) | Err(_) => return started.elapsed(),
                Ok(_) => {}
            }
        }
    })
    .await
    .ok()
}

/// The stalled unary call as a real gRPC client would make it, on a stream
/// whose message never comes. The stream is cut off at the pace of a request
/// body, the connection then has nothing in flight, and the idle timeout ends
/// it. With the pace lifted the stream, and so the connection, would stay.
#[tokio::test]
async fn a_unary_call_whose_message_never_arrives_is_cut_off_on_http2() {
    let app = limited(ping_app()).build().await;
    serving(&app, async {
        let addr = app.get::<HttpServer>().await.wait_bound().await;
        let mut client = http2(addr).await;
        let mut block = vec![0x83, 0x86];
        block.extend(literal(":path", "/grpc.health.v1.Health/Check"));
        block.extend(literal(":authority", "test"));
        block.extend(literal("content-type", "application/grpc"));
        const END_HEADERS: u8 = 0x04;
        client
            .write_all(&frame(1, END_HEADERS, 1, &block))
            .await
            .unwrap();
        assert!(
            connection_ends(&mut client).await.is_some(),
            "still held {WELL_PAST_THE_LIMITS:?} after limits of {LIMIT_MS} ms"
        );
    })
    .await;
}

/// The control: a plain request on the same connection, and the idle
/// timeout ends it (a GOAWAY, then the linger).
#[tokio::test]
async fn an_idle_http2_connection_on_the_shared_port_is_closed() {
    let app = limited(ping_app()).build().await;
    serving(&app, async {
        let addr = app.get::<HttpServer>().await.wait_bound().await;
        let mut client = http2(addr).await;
        let mut block = vec![0x82, 0x86, 0x84];
        block.extend(literal(":authority", "test"));
        block.extend(literal("content-type", "application/grpc"));
        client.write_all(&frame(1, 0x05, 1, &block)).await.unwrap();
        connection_ends(&mut client)
            .await
            .expect("the idle timeout ends it");
    })
    .await;
}

/// One `CONNECT` with a gRPC content type: tonic answers an unknown method
/// with HTTP 200, and nobody upgrades anything. A 2xx to `CONNECT` is not an
/// upgrade by itself, so the connection goes idle like any other.
#[tokio::test]
async fn a_connect_with_a_grpc_content_type_does_not_stop_the_clock() {
    let app = limited(ping_app()).build().await;
    serving(&app, async {
        let addr = app.get::<HttpServer>().await.wait_bound().await;
        let mut client = http2(addr).await;
        let mut block = Vec::new();
        block.extend(literal(":method", "CONNECT"));
        block.extend(literal(":protocol", "websocket"));
        block.extend(literal(":scheme", "http"));
        block.extend(literal(":path", "/no.such.Service/Method"));
        block.extend(literal(":authority", "test"));
        block.extend(literal("content-type", "application/grpc"));
        client.write_all(&frame(1, 0x05, 1, &block)).await.unwrap();
        assert!(
            connection_ends(&mut client).await.is_some(),
            "an idle connection outlived its limits of {LIMIT_MS} ms"
        );
    })
    .await;
}

// What the body's pace does not reach.

/// Drips a body a little above `body_min_bytes_per_sec` until the server
/// answers: the answer, and how long that took. Fails after
/// [`WELL_PAST_THE_LIMITS`].
async fn drip_until_answered(client: &mut TcpStream) -> (String, Duration) {
    let started = Instant::now();
    let mut chunk = [0_u8; 1024];
    while started.elapsed() < WELL_PAST_THE_LIMITS {
        client.write_all(&[0_u8; 32]).await.expect("still reading");
        match timeout(Duration::from_millis(50), client.read(&mut chunk)).await {
            Ok(Ok(read)) if read > 0 => {
                let answer = String::from_utf8_lossy(&chunk[..read]).into_owned();
                return (answer, started.elapsed());
            }
            Ok(_) => panic!("closed with no answer"),
            Err(_) => {}
        }
    }
    panic!("still reading {WELL_PAST_THE_LIMITS:?} past timeouts of 1 s");
}

/// A unary call dripped a little above `body_min_bytes_per_sec`. The pace
/// never trips, and tonic is still reading the message, so the method has not
/// started and nobody has been authenticated. At the defaults, 4 MiB at 256
/// bytes a second would be four and a half hours: the call's deadline is what
/// ends it.
#[tokio::test]
async fn a_dripped_unary_call_is_cut_off_at_its_deadline() {
    let app = limited(ping_app()).build().await;
    serving(&app, async {
        let addr = app.get::<HttpServer>().await.wait_bound().await;
        let mut client = TcpStream::connect(addr).await.unwrap();
        const MESSAGE: u32 = 1024 * 1024;
        let head = format!(
            "POST /grpc.health.v1.Health/Check HTTP/1.1\r\nHost: test\r\n\
             Content-Type: application/grpc\r\nContent-Length: {}\r\n\r\n",
            MESSAGE + 5
        );
        client.write_all(head.as_bytes()).await.unwrap();
        let mut frame_head = vec![0_u8];
        frame_head.extend_from_slice(&MESSAGE.to_be_bytes());
        client.write_all(&frame_head).await.unwrap();
        let (answer, took) = drip_until_answered(&mut client).await;
        assert!(
            answer.to_ascii_lowercase().contains("grpc-status: 4"),
            "{answer}"
        );
        assert!(took >= Duration::from_millis(900), "{took:?}");
    })
    .await;
}

/// The same drip as a Connect unary call, whose body the edge reads itself
/// before anything of gRPC has started.
#[tokio::test]
async fn a_dripped_connect_unary_call_is_cut_off_at_its_deadline() {
    let app = limited(ping_app()).build().await;
    serving(&app, async {
        let addr = app.get::<HttpServer>().await.wait_bound().await;
        let mut client = TcpStream::connect(addr).await.unwrap();
        let head = format!(
            "POST /grpc.health.v1.Health/Check HTTP/1.1\r\nHost: test\r\n\
             Content-Type: application/proto\r\nContent-Length: {}\r\n\r\n",
            1024 * 1024
        );
        client.write_all(head.as_bytes()).await.unwrap();
        let (answer, took) = drip_until_answered(&mut client).await;
        assert!(answer.starts_with("HTTP/1.1 504"), "{answer}");
        assert!(took >= Duration::from_millis(900), "{took:?}");
    })
    .await;
}

/// The message has arrived and the body never ends: tonic reads to the end of
/// the body before it calls the method, so this too is held to the pace, and
/// a method with a `Request` alias, which the macro takes for a stream, is
/// only reached once there is no body left to lift a limit from.
#[tokio::test]
async fn a_unary_call_whose_body_never_ends_is_cut_off() {
    let app = limited(ping_app()).build().await;
    serving(&app, async {
        let addr = app.get::<HttpServer>().await.wait_bound().await;
        let mut client = TcpStream::connect(addr).await.unwrap();
        client
            .write_all(
                b"POST /grpc.health.v1.Health/Check HTTP/1.1\r\n\
                Host: test\r\n\
                Content-Type: application/grpc\r\n\
                Content-Length: 10\r\n\r\n\0\0\0\0\0",
            )
            .await
            .unwrap();
        ended(&mut client).await.expect("the body read timeout");
    })
    .await;
}

/// The other content types the edge once lifted the limit for.
#[tokio::test]
async fn stalled_calls_of_the_other_protocols_are_cut_off() {
    let app = limited(ping_app()).build().await;
    serving(&app, async {
        let addr = app.get::<HttpServer>().await.wait_bound().await;
        for content_type in [
            "application/grpc-web",
            "application/grpc-web+proto",
            "application/grpc-web-text",
            "application/connect+proto",
            "application/connect+json",
        ] {
            let mut client = TcpStream::connect(addr).await.unwrap();
            let head = format!(
                "POST /grpc.health.v1.Health/Check HTTP/1.1\r\nHost: test\r\n\
                 Content-Type: {content_type}\r\nContent-Length: 5\r\n\r\n"
            );
            client.write_all(head.as_bytes()).await.unwrap();
            assert!(
                ended(&mut client).await.is_some(),
                "{content_type} still held after {WELL_PAST_THE_LIMITS:?}"
            );
        }
    })
    .await;
}

/// A silent client stream, `extra` being more of its head.
fn silent_import(extra: &str) -> String {
    format!(
        "POST /crm.deals.v1.Deals/ImportDeals HTTP/1.1\r\nHost: test\r\n\
         Content-Type: application/grpc\r\nContent-Length: 5\r\n\
         Authorization: Bearer {}\r\n{extra}\r\n",
        uuid::Uuid::now_v7()
    )
}

/// The control: a client stream past the access check is lifted, and its
/// deadline ends it.
#[tokio::test]
async fn a_silent_client_stream_ends_at_the_servers_deadline() {
    let app = limited(deals_app())
        .override_value::<dyn Authenticator>(Arc::new(BearerAuthenticator))
        .build()
        .await;
    serving(&app, async {
        let addr = app.get::<HttpServer>().await.wait_bound().await;
        let mut client = TcpStream::connect(addr).await.unwrap();
        client
            .write_all(silent_import("").as_bytes())
            .await
            .unwrap();
        ended(&mut client).await.expect("the call's deadline");
    })
    .await;
}

/// The client picks its own deadline, up to `grpc.timeout_secs`: with the pace
/// lifted that is all that ends this call, and it asks for eleven thousand
/// years.
#[tokio::test]
async fn a_grpc_timeout_cannot_extend_the_servers_deadline() {
    let app = limited(deals_app())
        .override_value::<dyn Authenticator>(Arc::new(BearerAuthenticator))
        .build()
        .await;
    serving(&app, async {
        let addr = app.get::<HttpServer>().await.wait_bound().await;
        let mut client = TcpStream::connect(addr).await.unwrap();
        let head = silent_import("grpc-timeout: 99999999H\r\n");
        client.write_all(head.as_bytes()).await.unwrap();
        assert!(
            ended(&mut client).await.is_some(),
            "still held {WELL_PAST_THE_LIMITS:?} past a deadline of 1 s"
        );
    })
    .await;
}

/// The same for a unary method: asking for an hour does not let it run past
/// `grpc.timeout_secs`.
#[tokio::test]
async fn a_grpc_timeout_cannot_extend_a_unary_call() {
    let app = limited(ping_app()).build().await;
    serving(&app, async {
        let addr = app.get::<HttpServer>().await.wait_bound().await;
        let channel = tonic::transport::Endpoint::from_shared(format!("http://{addr}"))
            .unwrap()
            .connect()
            .await
            .unwrap();
        let mut request = tonic::Request::new(PingRequest {
            message: "slow".to_owned(),
            sleep_ms: 2500,
        });
        request.set_timeout(Duration::from_secs(3600));
        let started = Instant::now();
        let answered = PingClient::new(channel).ping(request).await;
        let took = started.elapsed();
        let status = answered.expect_err("the method ran past a deadline of 1 s");
        assert_eq!(status.code(), tonic::Code::DeadlineExceeded, "{status}");
        assert!(took < Duration::from_millis(2000), "{took:?}");
    })
    .await;
}

/// `connect-timeout-ms` is capped like `grpc-timeout`, and the time spent
/// reading the message comes out of it.
#[tokio::test]
async fn a_connect_timeout_cannot_extend_the_servers_deadline() {
    let app = limited(ping_app()).build().await;
    serving(&app, async {
        let addr = app.get::<HttpServer>().await.wait_bound().await;
        let body = serde_json::json!({ "message": "slow", "sleepMs": "2500" }).to_string();
        let mut client = TcpStream::connect(addr).await.unwrap();
        let head = format!(
            "POST /navree.test.v1.Ping/Ping HTTP/1.1\r\nHost: test\r\n\
             Content-Type: application/json\r\nConnect-Timeout-Ms: 3600000\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        client.write_all(head.as_bytes()).await.unwrap();
        let started = Instant::now();
        let mut answer = String::new();
        timeout(WELL_PAST_THE_LIMITS, client.read_to_string(&mut answer))
            .await
            .expect("the deadline answers")
            .unwrap();
        assert!(answer.starts_with("HTTP/1.1 504"), "{answer}");
        assert!(started.elapsed() < Duration::from_millis(2000));
    })
    .await;
}

// Failing: every test below this line fails today, and passes once what it
// shows is fixed.

/// `Health.Watch` needs no credentials, is on in every app, and is a stream
/// with nothing to say: its response is not held to `grpc.timeout_secs`, a
/// body that has arrived has no pace to keep, a stream with nothing to send
/// has no write stall, and with something in flight the connection is neither
/// idle nor reclaimed. Every limit is on here, short, and one client takes the
/// only slot for good.
#[tokio::test]
async fn a_health_watch_with_no_credentials_cannot_keep_a_slot() {
    let app = limited(ping_app())
        .config("http.server.max_connections", 1)
        .config("http.server.write_stall_timeout_ms", LIMIT_MS)
        .config("http.server.upgraded_idle_timeout_ms", LIMIT_MS)
        .build()
        .await;
    serving(&app, async {
        let addr = app.get::<HttpServer>().await.wait_bound().await;
        let mut watcher = TcpStream::connect(addr).await.unwrap();
        watcher
            .write_all(
                b"POST /grpc.health.v1.Health/Watch HTTP/1.1\r\n\
                Host: test\r\n\
                Content-Type: application/grpc\r\n\
                Content-Length: 5\r\n\r\n\0\0\0\0\0",
            )
            .await
            .unwrap();
        let mut chunk = [0_u8; 4096];
        let read = timeout(WELL_PAST_THE_LIMITS, watcher.read(&mut chunk))
            .await
            .expect("the first status arrives")
            .unwrap();
        assert!(chunk[..read].starts_with(b"HTTP/1.1 200"));
        let mut honest = TcpStream::connect(addr).await.unwrap();
        honest
            .write_all(b"GET /health/live HTTP/1.1\r\nHost: test\r\n\r\n")
            .await
            .unwrap();
        assert!(
            ended(&mut honest).await.is_some(),
            "an honest client got nothing in {WELL_PAST_THE_LIMITS:?}"
        );
    })
    .await;
}

/// The service and method of a call are labels of its metrics, and the path
/// is the client's to choose: with no credentials, each path nobody serves
/// adds a series to the process, for good, and to every scrape.
#[tokio::test]
async fn a_path_nobody_serves_adds_no_metric() {
    let app = limited(ping_app())
        .module(ObserveModule::for_root(ObserveOptions::default()))
        .build()
        .await;
    serving(&app, async {
        let addr = app.get::<HttpServer>().await.wait_bound().await;
        for call in 0..3 {
            let mut client = TcpStream::connect(addr).await.unwrap();
            let head = format!(
                "POST /no.such.Service{call}/Method HTTP/1.1\r\nHost: test\r\n\
                 Content-Type: application/grpc\r\nContent-Length: 0\r\n\
                 Connection: close\r\n\r\n"
            );
            client.write_all(head.as_bytes()).await.unwrap();
            let mut answer = Vec::new();
            timeout(WELL_PAST_THE_LIMITS, client.read_to_end(&mut answer))
                .await
                .expect("the call is answered")
                .unwrap();
        }
        let body = app
            .client()
            .get("/metrics")
            .send()
            .await
            .assert_status(200)
            .text();
        let series = body
            .lines()
            .filter(|line| line.contains("no.such.Service"))
            .count();
        assert_eq!(series, 0, "{body}");
    })
    .await;
}

/// `grpc.timeout_secs` takes any number, and a number that large means never.
/// The deadline is added to the time of each call, and the sum overflows.
#[tokio::test]
async fn a_server_deadline_of_any_length_answers_calls() {
    let app = limited(ping_app())
        .config("grpc.timeout_secs", u64::MAX)
        .build()
        .await;
    serving(&app, async {
        let addr = app.get::<HttpServer>().await.wait_bound().await;
        let channel = tonic::transport::Endpoint::from_shared(format!("http://{addr}"))
            .unwrap()
            .connect()
            .await
            .unwrap();
        let answered = PingClient::new(channel)
            .ping(PingRequest {
                message: "hello".to_owned(),
                sleep_ms: 0,
            })
            .await;
        assert_eq!(answered.expect("an answer").into_inner().message, "hello");
    })
    .await;
}

/// `http.timeout_secs` is `grpc.timeout_secs` when that is unset, and with
/// the gRPC module in the app every plain HTTP request adds it to the time too.
#[tokio::test]
async fn a_server_deadline_of_any_length_answers_http_requests() {
    let app = limited(ping_app())
        .config("http.timeout_secs", u64::MAX)
        .build()
        .await;
    serving(&app, async {
        let addr = app.get::<HttpServer>().await.wait_bound().await;
        let mut client = TcpStream::connect(addr).await.unwrap();
        client
            .write_all(b"GET /health/live HTTP/1.1\r\nHost: test\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut answer = Vec::new();
        timeout(WELL_PAST_THE_LIMITS, client.read_to_end(&mut answer))
            .await
            .expect("the server answers or closes")
            .ok();
        let answer = String::from_utf8_lossy(&answer);
        assert!(answer.starts_with("HTTP/1.1 200"), "{answer:?}");
    })
    .await;
}

/// A service whose only method answers a stream that always has more.
struct EndlessDeals;

impl Injectable for EndlessDeals {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_resolver: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

#[grpc_service(server = DealsServer)]
#[public]
impl Deals for EndlessDeals {
    async fn get_deal(
        &self,
        _request: Request<GetDealRequest>,
    ) -> navree_http::Result<Response<Deal>> {
        Err(AppError::not_found())
    }

    async fn list_deals(
        &self,
        _request: Request<ListDealsRequest>,
    ) -> navree_http::Result<Response<ListDealsResponse>> {
        Err(AppError::not_found())
    }

    async fn move_deal(
        &self,
        _request: Request<MoveDealRequest>,
    ) -> navree_http::Result<Response<Deal>> {
        Err(AppError::not_found())
    }

    type WatchDealsStream = DealEvents;

    async fn watch_deals(
        &self,
        _request: Request<WatchDealsRequest>,
    ) -> navree_http::Result<Response<Self::WatchDealsStream>> {
        let event = DealEvent {
            deal_id: "1".to_owned(),
            stage: "lead".to_owned(),
        };
        let events = futures_util::stream::repeat_with(move || Ok(event.clone()));
        Ok(Response::new(Box::pin(events) as DealEvents))
    }

    async fn import_deals(
        &self,
        _request: Request<Streaming<Deal>>,
    ) -> navree_http::Result<Response<ImportDealsResponse>> {
        Err(AppError::not_found())
    }

    type EchoStream = DealEvents;

    async fn echo(
        &self,
        _request: Request<Streaming<DealEvent>>,
    ) -> navree_http::Result<Response<Self::EchoStream>> {
        Err(AppError::not_found())
    }
}

struct EndlessDealsModule;

impl Module for EndlessDealsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.grpc_service::<EndlessDeals>();
    }
}

/// The client's deadline ends a response stream when the stream has nothing
/// to give: one that always has more never gets as far as the check, and
/// neither does a shutdown, which is looked at in the same place.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stream_that_always_has_more_ends_at_the_clients_deadline() {
    let app = TestApp::builder()
        .config("http.addr", "127.0.0.1:0")
        .config("http.shutdown_grace_secs", 1)
        .module(GrpcModule::from_config())
        .module(EndlessDealsModule)
        .build()
        .await;
    serving(&app, async {
        let addr = app.get::<HttpServer>().await.wait_bound().await;
        let channel = tonic::transport::Endpoint::from_shared(format!("http://{addr}"))
            .unwrap()
            .connect()
            .await
            .unwrap();
        let mut request = tonic::Request::new(WatchDealsRequest {});
        request.set_timeout(Duration::from_secs(1));
        let mut events = DealsClient::new(channel)
            .watch_deals(request)
            .await
            .expect("the stream starts")
            .into_inner();
        let ended = timeout(WELL_PAST_THE_LIMITS, async {
            loop {
                match events.message().await {
                    Ok(Some(_)) => {}
                    Ok(None) => return None,
                    Err(status) => return Some(status.code()),
                }
            }
        })
        .await;
        assert_eq!(
            ended.expect("the stream outlived its deadline of 1 s"),
            Some(tonic::Code::DeadlineExceeded)
        );
    })
    .await;
}
