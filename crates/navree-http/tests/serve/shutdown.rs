//! Graceful shutdown: in-flight requests finish, and nothing else keeps the
//! server up.

use std::sync::Arc;
use std::time::Duration;

use http::Response;
use navree_http::body::Body;
use navree_http::{HttpOptions, ServerOptions};
use tokio::io::AsyncWriteExt;
use tokio::sync::{Notify, Semaphore};

use crate::support::{
    GET, answer, connect, echo_app, exchange, no_limits, read_response, read_until_closed, start,
    start_with,
};

/// A handler that says when it has started and finishes when told.
fn gated() -> (
    impl tower::Service<
        http::Request<Body>,
        Response = Response<Body>,
        Error = std::convert::Infallible,
        Future: Send + 'static,
    > + Clone
    + Send
    + 'static,
    Arc<Notify>,
    Arc<Semaphore>,
) {
    let started = Arc::new(Notify::new());
    let release = Arc::new(Semaphore::new(0));
    let app = {
        let (started, release) = (started.clone(), release.clone());
        answer(move |_request| {
            let (started, release) = (started.clone(), release.clone());
            async move {
                started.notify_one();
                let _permit = release.acquire().await.unwrap();
                Response::new(Body::from("finished"))
            }
        })
    };
    (app, started, release)
}

#[tokio::test]
async fn a_shutdown_waits_for_requests_in_flight_and_then_ends() {
    let (app, started, release) = gated();
    let mut server = start(app, no_limits()).await;
    let mut client = connect(server.addr).await;
    client.write_all(GET).await.unwrap();
    started.notified().await;

    server.stop();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!server.finished(), "a request is still in flight");

    release.add_permits(1);
    let response = read_response(&mut client).await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.ends_with("finished"), "{response}");
    let head = response.to_ascii_lowercase();
    assert!(head.contains("connection: close"), "{response}");
    server.stopped().await;
}

/// A connection that never sent a request is not in flight: hyper alone
/// would keep waiting for its head, and the shutdown with it.
#[tokio::test]
async fn silent_and_idle_connections_do_not_hold_up_a_shutdown() {
    let mut server = start(echo_app(), no_limits()).await;
    let _silent = connect(server.addr).await;
    let mut half = connect(server.addr).await;
    half.write_all(b"GET / HT").await.unwrap();
    let mut idle = connect(server.addr).await;
    assert!(exchange(&mut idle, GET).await.starts_with("HTTP/1.1 200"));

    // Each connection has been accepted before the signal goes out.
    tokio::time::sleep(Duration::from_millis(50)).await;
    server.stop();
    server.stopped().await;
}

#[tokio::test]
async fn a_request_that_outlasts_the_grace_period_is_closed() {
    let (app, started, _release) = gated();
    let mut server = start_with(
        app,
        HttpOptions {
            server: ServerOptions { ..no_limits() },
            shutdown_grace_secs: 0,
            ..HttpOptions::default()
        },
    )
    .await;
    let mut client = connect(server.addr).await;
    client.write_all(GET).await.unwrap();
    started.notified().await;

    server.stop();
    server.stopped().await;
    assert_eq!(read_until_closed(&mut client).await, "");
}
