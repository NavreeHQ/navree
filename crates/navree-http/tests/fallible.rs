//! FallibleLayer: a tower layer whose service can fail, on the whole app and
//! on one route, answers through the app's error mapping.

#[path = "http/support.rs"]
mod support;

use std::convert::Infallible;
use std::fmt;
use std::future::{Ready, ready};
use std::task::{Context, Poll};
use std::time::Duration;

use http::{Method, StatusCode};
use http_body_util::Full;
use navree_http::body::{Body, Bytes};
use navree_http::extract::Request;
use navree_http::handler::Endpoint;
use navree_http::{
    AppError, FallibleLayer, GlobalLayer, LayerSlot, ModuleBuilderHttpExt, Response, RouteDef,
};
use navree_kernel::ModuleBuilder;
use tower::{Layer, Service};

use support::Setup;

/// The error a ledger service answers with.
#[derive(Debug)]
struct LedgerClosed;

impl fmt::Display for LedgerClosed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the khata is closed for Diwali")
    }
}

impl std::error::Error for LedgerClosed {}

/// A layer whose service answers every request with `LedgerClosed`.
#[derive(Clone, Copy)]
struct Refuse;

impl<S> Layer<S> for Refuse {
    type Service = Refusing;

    fn layer(&self, _inner: S) -> Refusing {
        Refusing
    }
}

#[derive(Clone, Copy)]
struct Refusing;

impl Service<Request> for Refusing {
    type Response = Response;
    type Error = LedgerClosed;
    type Future = Ready<Result<Response, LedgerClosed>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), LedgerClosed>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _request: Request) -> Self::Future {
        ready(Err(LedgerClosed))
    }
}

/// A layer whose service is never ready: its readiness fails with
/// `LedgerClosed`. Were it called anyway, it would answer 200 `served`.
#[derive(Clone, Copy)]
struct NotReady;

impl<S> Layer<S> for NotReady {
    type Service = Unready;

    fn layer(&self, _inner: S) -> Unready {
        Unready
    }
}

#[derive(Clone, Copy)]
struct Unready;

impl Service<Request> for Unready {
    type Response = Response;
    type Error = LedgerClosed;
    type Future = Ready<Result<Response, LedgerClosed>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), LedgerClosed>> {
        Poll::Ready(Err(LedgerClosed))
    }

    fn call(&mut self, _request: Request) -> Self::Future {
        ready(Ok(Response::new(Body::from("served"))))
    }
}

/// A layer whose service answers with a body type of its own.
#[derive(Clone, Copy)]
struct Rewrap;

impl<S> Layer<S> for Rewrap {
    type Service = Rewrapped;

    fn layer(&self, _inner: S) -> Rewrapped {
        Rewrapped
    }
}

#[derive(Clone, Copy)]
struct Rewrapped;

impl Service<Request> for Rewrapped {
    type Response = http::Response<Full<Bytes>>;
    type Error = Infallible;
    type Future = Ready<Result<http::Response<Full<Bytes>>, Infallible>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _request: Request) -> Self::Future {
        ready(Ok(http::Response::new(Full::from("₹1,00,000 received"))))
    }
}

/// A timeout of 50 ms around the whole app, in the `Context` slot.
fn timeout(m: &mut ModuleBuilder) {
    m.http_layer(GlobalLayer::new(
        LayerSlot::Context,
        "timeout",
        |_, router| {
            Ok(
                router.layer(FallibleLayer::new(tower::timeout::TimeoutLayer::new(
                    Duration::from_millis(50),
                ))),
            )
        },
    ));
}

fn routes() -> Vec<RouteDef> {
    vec![
        RouteDef::on(Method::GET, "/slow", || async {
            tokio::time::sleep(Duration::from_millis(500)).await;
            "late"
        })
        .public(),
        RouteDef::on(Method::GET, "/fast", || async { "on time" }).public(),
        RouteDef::with_endpoint(
            Method::GET,
            "/closed",
            Endpoint::new(|| async { "open" }).layer(FallibleLayer::new(Refuse)),
        )
        .public(),
    ]
}

#[tokio::test]
async fn a_failing_tower_layer_answers_through_the_error_mapping() {
    // Without a mapping: a 500 that tells the client nothing of the error.
    let app = Setup::new(routes()).register(timeout).boot().await;
    let slow = app.get("/api/slow").await;
    assert_eq!(slow.assert_error(500)["message"], "Internal server error");
    assert!(!slow.text().contains("elapsed"), "{}", slow.text());
    let closed = app.get("/api/closed").await;
    assert_eq!(closed.assert_error(500)["message"], "Internal server error");
    assert!(!closed.text().contains("Diwali"), "{}", closed.text());
    assert_eq!(app.get("/api/fast").await.text(), "on time");

    // The app maps the layer's error, and an error of its own on one route.
    let app = Setup::new(routes())
        .register(|m| {
            timeout(m);
            m.map_error::<tower::timeout::error::Elapsed>(|_| {
                Some(AppError::service_unavailable("The khata took too long."))
            });
            m.map_error::<LedgerClosed>(|_| {
                Some(AppError::conflict("The khata is closed for Diwali."))
            });
        })
        .boot()
        .await;
    assert_eq!(
        app.get("/api/slow").await.assert_error(503)["message"],
        "The khata took too long."
    );
    assert_eq!(
        app.get("/api/closed").await.assert_error(409)["message"],
        "The khata is closed for Diwali."
    );
    assert_eq!(app.get("/api/fast").await.text(), "on time");
}

#[tokio::test]
async fn a_fallible_layer_answers_a_service_that_is_not_ready_like_a_failed_call() {
    let app = Setup::new(vec![
        RouteDef::with_endpoint(
            Method::GET,
            "/not-ready",
            Endpoint::new(|| async { "open" }).layer(FallibleLayer::new(NotReady)),
        )
        .public(),
        RouteDef::with_endpoint(
            Method::GET,
            "/rewrapped",
            Endpoint::new(|| async { "open" }).layer(FallibleLayer::new(Rewrap)),
        )
        .public(),
    ])
    .register(|m| {
        m.map_error::<LedgerClosed>(|_| {
            Some(AppError::conflict("The khata is closed for Diwali."))
        });
    })
    .boot()
    .await;

    // The readiness error is mapped as a call's would be; the service is never called.
    let not_ready = app.get("/api/not-ready").await;
    assert_eq!(
        not_ready.assert_error(409)["message"],
        "The khata is closed for Diwali."
    );
    assert!(!not_ready.text().contains("served"), "{}", not_ready.text());

    // A body of another type reaches the client byte for byte.
    let rewrapped = app.get("/api/rewrapped").await;
    assert_eq!(rewrapped.status, StatusCode::OK);
    assert_eq!(rewrapped.body, "₹1,00,000 received".as_bytes());
}
