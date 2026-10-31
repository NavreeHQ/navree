//! The app stack: the same services in the same layers, on [`AppRouter`] and
//! on the previous framework's router, which held Navree's router as its
//! fallback.

use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use futures_util::stream;
use http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use navree_http::body::Body;
use navree_http::extract::Request as NavreeRequest;
use navree_http::middleware::{self, Next, State};
use navree_http::response::sse::{Event, Sse};
use navree_http::response::{IntoResponse, Redirect, Response};
use navree_http::routing::Router;
use navree_http::{AppError, AppRouter, CorsLayer, Json, Path};
use tower::{Service, ServiceExt};
use tower_http::compression::CompressionLayer;

use crate::support::{Seen, ours, theirs};

/// The routes of every comparison: one of each kind of response.
pub fn routes() -> Router {
    Router::new()
        .route(Method::GET, "/text", || async { "text" })
        .route(Method::GET, "/json", || async {
            Json(serde_json::json!({ "a": 1 }))
        })
        .route(Method::POST, "/echo", |body: Body| async move {
            Response::new(body)
        })
        .route(Method::GET, "/redirect", || async { Redirect::to("/text") })
        .route(
            Method::GET,
            "/deals/{id}",
            |Path(id): Path<u32>| async move { format!("deal {id}") },
        )
        .route(Method::GET, "/status", || async {
            (
                StatusCode::ACCEPTED,
                [("x-a", "1"), ("x-b", "2")],
                "accepted",
            )
        })
        .route(Method::GET, "/empty-error", || async {
            StatusCode::BAD_GATEWAY
        })
        .route(Method::GET, "/app-error", || async {
            AppError::conflict("Taken.")
        })
        .route(Method::GET, "/stream", || async {
            let chunks = ["one", "two", "three"].map(Ok::<_, Infallible>);
            Body::from_stream(stream::iter(chunks))
        })
        .route(Method::GET, "/broken", || async {
            // The start goes out on its own, then the body fails.
            Body::from_stream(stream::unfold(0, |step| async move {
                match step {
                    0 => Some((Ok("start"), 1)),
                    1 => {
                        tokio::time::sleep(Duration::from_millis(20)).await;
                        Some((Err(std::io::Error::other("gone")), 2))
                    }
                    _ => None,
                }
            }))
        })
        .route(Method::GET, "/events", || async {
            let events = ["1", "2"].map(|data| Ok::<_, Infallible>(Event::default().data(data)));
            Sse::new(stream::iter(events))
        })
        .route(Method::GET, "/large", || async { "x".repeat(4096) })
        .route(Method::CONNECT, "/tunnel", || async { "not a tunnel" })
        .route(Method::CONNECT, "/refused", || async {
            (StatusCode::FORBIDDEN, "no")
        })
}

/// The previous framework's app around `service`, as `build_router` made it.
fn previous<S>(service: S) -> axum::Router
where
    S: Service<axum::extract::Request, Error = Infallible> + Clone + Send + Sync + 'static,
    S::Response: axum::response::IntoResponse,
    S::Future: Send + 'static,
{
    axum::Router::new().fallback_service(service)
}

/// `routes()` as the previous framework's fallback takes it.
fn previous_routes() -> axum::Router {
    previous(tower::service_fn(move |request: axum::extract::Request| {
        let routes = routes();
        async move { routes.oneshot(request.map(Body::new)).await }
    }))
}

/// `name` in a header on the way in and out, on each side's middleware.
async fn stamp(name: &'static str, mut request: NavreeRequest, next: Next) -> Response {
    let mut seen = request
        .headers()
        .get("x-order")
        .map_or(String::new(), |value| value.to_str().unwrap().to_owned());
    seen.push_str(name);
    request
        .headers_mut()
        .insert("x-order", seen.parse().unwrap());
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .append("x-out", name.parse().unwrap());
    response.headers_mut().append("x-in", seen.parse().unwrap());
    response
}

async fn previous_stamp(
    name: &'static str,
    mut request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let mut seen = request
        .headers()
        .get("x-order")
        .map_or(String::new(), |value| value.to_str().unwrap().to_owned());
    seen.push_str(name);
    request
        .headers_mut()
        .insert("x-order", seen.parse().unwrap());
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .append("x-out", name.parse().unwrap());
    response.headers_mut().append("x-in", seen.parse().unwrap());
    response
}

fn cors() -> CorsLayer {
    let options = navree_http::CorsOptions {
        allowed_origins: vec!["http://localhost:3000".to_owned()],
        ..navree_http::CorsOptions::default()
    };
    CorsLayer::from_options(&options)
        .unwrap()
        .expect("an origin is allowed")
}

/// Both apps with the same layers.
pub fn both() -> (AppRouter, axum::Router) {
    let ours = AppRouter::new(routes())
        .layer(middleware::from_fn(|request: NavreeRequest, next: Next| {
            stamp("a", request, next)
        }))
        .layer(CompressionLayer::new())
        .layer(middleware::from_fn_with_state(
            "b",
            |State(name): State<&'static str>, request: NavreeRequest, next: Next| {
                stamp(name, request, next)
            },
        ))
        .layer(cors());
    let theirs = previous_routes()
        .layer(axum::middleware::from_fn(
            |request: axum::extract::Request, next: axum::middleware::Next| {
                previous_stamp("a", request, next)
            },
        ))
        .layer(CompressionLayer::new())
        .layer(axum::middleware::from_fn_with_state(
            "b",
            |axum::extract::State(name): axum::extract::State<&'static str>,
             request: axum::extract::Request,
             next: axum::middleware::Next| { previous_stamp(name, request, next) },
        ))
        .layer(cors());
    (ours, theirs)
}

fn request(
    method: Method,
    uri: &str,
    headers: &[(&str, &str)],
    body: &'static str,
) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    builder.body(Body::from(body)).unwrap()
}

/// Sends the request to both apps.
async fn compare(method: Method, uri: &str, headers: &[(&str, &str)], body: &'static str) -> Seen {
    let (app, previous) = both();
    let a = app
        .oneshot(request(method.clone(), uri, headers, body))
        .await
        .unwrap();
    let b = previous
        .oneshot(request(method.clone(), uri, headers, body))
        .await
        .unwrap();
    let a = ours(a).await;
    let b = theirs(b).await;
    assert_eq!(a, b, "{method} {uri} {headers:?}");
    a
}

#[tokio::test]
async fn every_response_comes_out_the_same() {
    for uri in [
        "/text",
        "/json",
        "/redirect",
        "/deals/7",
        "/deals/x",
        "/deals/%2F",
        "/status",
        "/empty-error",
        "/app-error",
        "/stream",
        "/broken",
        "/events",
        "/large",
        "/nothing",
        "/a/../b",
    ] {
        for headers in [
            &[][..],
            &[("accept-encoding", "gzip")][..],
            &[("origin", "http://localhost:3000")][..],
            &[("origin", "http://evil.example")][..],
        ] {
            compare(Method::GET, uri, headers, "").await;
        }
        compare(Method::HEAD, uri, &[], "").await;
        compare(Method::POST, uri, &[], "body").await;
        compare(Method::OPTIONS, uri, &[], "").await;
        compare(Method::DELETE, uri, &[], "").await;
    }
}

#[tokio::test]
async fn layers_run_in_the_same_order() {
    let seen = compare(Method::GET, "/text", &[], "").await;
    let value = |name: &str| {
        seen.headers
            .iter()
            .filter(|(header, _)| header == name)
            .map(|(_, value)| value.to_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        value("x-in"),
        ["ba", "b"],
        "the last added runs first on the way in"
    );
    assert_eq!(value("x-out"), ["a", "b"], "and last on the way out");
}

#[tokio::test]
async fn a_cors_preflight_is_answered_outside_the_rest() {
    let seen = compare(
        Method::OPTIONS,
        "/text",
        &[
            ("origin", "http://localhost:3000"),
            ("access-control-request-method", "GET"),
        ],
        "",
    )
    .await;
    assert!(!seen.headers.iter().any(|(name, _)| name == "x-out"));
}

#[tokio::test]
async fn a_body_reaches_a_route_through_the_layers() {
    let seen = compare(Method::POST, "/echo", &[], "sent").await;
    assert_eq!(seen.frames, [Bytes::from_static(b"sent")]);
}

#[tokio::test]
async fn compression_leaves_events_alone_and_keeps_a_stream_streaming() {
    let events = compare(Method::GET, "/events", &[("accept-encoding", "gzip")], "").await;
    assert_eq!(events.frames.len(), 2, "{events:?}");
    assert!(
        !events
            .headers
            .iter()
            .any(|(name, _)| name == "content-encoding")
    );

    let large = compare(Method::GET, "/large", &[("accept-encoding", "gzip")], "").await;
    assert!(
        large
            .headers
            .iter()
            .any(|(name, _)| name == "content-encoding")
    );
}

#[tokio::test]
async fn a_2xx_to_connect_loses_its_body_on_both_sides() {
    let seen = compare(Method::CONNECT, "/tunnel", &[], "").await;
    assert_eq!(seen.status, StatusCode::OK);
    assert!(seen.frames.is_empty());
    let seen = compare(Method::CONNECT, "/refused", &[], "").await;
    assert_eq!(seen.status, StatusCode::FORBIDDEN);
    assert_eq!(seen.frames, [Bytes::from_static(b"no")]);
}

#[tokio::test]
async fn the_edge_of_the_stack_sees_a_response_of_any_body() {
    // A layer that hands the rest of the stack a request body of its own
    // takes the app as it took the previous router.
    let app = AppRouter::new(routes()).layer(tower_http::limit::RequestBodyLimitLayer::new(2));
    let refused = app
        .clone()
        .oneshot(request(
            Method::POST,
            "/echo",
            &[("content-length", "4")],
            "sent",
        ))
        .await
        .unwrap();
    assert_eq!(refused.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let passed = app
        .oneshot(request(Method::POST, "/echo", &[], "ok"))
        .await
        .unwrap();
    let body = passed.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body, "ok");
}

/// A service that is ready only after being polled, as Tower's contract
/// allows, and counts calls made without that.
#[derive(Clone)]
struct Polled {
    ready: bool,
    unready_calls: Arc<Mutex<u32>>,
}

impl Service<NavreeRequest> for Polled {
    type Response = Response;
    type Error = Infallible;
    type Future = std::future::Ready<Result<Response, Infallible>>;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Infallible>> {
        if self.ready {
            return std::task::Poll::Ready(Ok(()));
        }
        self.ready = true;
        cx.waker().wake_by_ref();
        std::task::Poll::Pending
    }

    fn call(&mut self, _request: NavreeRequest) -> Self::Future {
        if !self.ready {
            *self.unready_calls.lock().unwrap() += 1;
        }
        self.ready = false;
        std::future::ready(Ok("called".into_response()))
    }
}

#[tokio::test]
async fn a_service_is_polled_ready_before_every_call() {
    async fn pass(request: NavreeRequest, next: Next) -> Response {
        next.run(request).await
    }
    let unready_calls = Arc::new(Mutex::new(0));
    let app = AppRouter::new(Polled {
        ready: false,
        unready_calls: unready_calls.clone(),
    })
    .layer(middleware::from_fn(pass))
    .layer(middleware::from_fn(pass));
    for _ in 0..3 {
        let response = app
            .clone()
            .oneshot(request(Method::GET, "/", &[], ""))
            .await;
        assert_eq!(response.unwrap().status(), StatusCode::OK);
    }
    assert_eq!(*unready_calls.lock().unwrap(), 0);
}
