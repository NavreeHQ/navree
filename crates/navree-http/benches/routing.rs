//! Routing micro-benchmarks: Navree's router against the previous one, on
//! the same route table, handlers and per-route layer.
//!
//! - `match/*`: the matcher alone: `PathTree::at` against the previous
//!   router's matcher (`matchit`) on a table of 240 routes shaped like a
//!   real app's (`/api/v1/<resource>`, `/{id}`, `/{id}/<sub>`, ...).
//! - `route/*`: a request through the router to its handler and back:
//!   Navree's `routing::Router` against the previous `Router`, each route
//!   behind a boxing per-route layer (as `build_router` mounts them), both
//!   behind a minimal error stage (404 and 405 end as JSON errors, as the
//!   pipeline renders them), the handlers reading their parameters with each
//!   framework's `Path`. `static`, `one_param`, `two_params`, `not_found`,
//!   `method_not_allowed`.
//! - `previous_extractors/one_param`: a handler still on the previous
//!   framework's extractors, mounted on Navree's router (it runs behind the
//!   previous router's matching for its route, as during the move).
//!
//! Run with `cargo bench -p navree-http --bench routing` (add `-- --quick`
//! for a shorter run).
#![allow(missing_docs)]

use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::body::Body;
use criterion::{Criterion, criterion_group, criterion_main};
use http::{Method, Request, StatusCode};
use navree_http::handler::Endpoint;
use navree_http::routing::{PathTree, Router};
use navree_http::{AppError, Path};
use tower::{Layer, Service, ServiceExt};

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

const RESOURCES: [&str; 24] = [
    "accounts",
    "activities",
    "addresses",
    "attachments",
    "audits",
    "billing",
    "calendars",
    "comments",
    "companies",
    "contacts",
    "deals",
    "documents",
    "emails",
    "events",
    "files",
    "invoices",
    "notes",
    "orders",
    "pipelines",
    "products",
    "projects",
    "quotes",
    "tasks",
    "users",
];

/// The table: per resource, ten routes.
fn table() -> Vec<(Method, String)> {
    let mut routes = Vec::new();
    for resource in RESOURCES {
        let base = format!("/api/v1/{resource}");
        routes.push((Method::GET, base.clone()));
        routes.push((Method::POST, base.clone()));
        routes.push((Method::GET, format!("{base}/{{id}}")));
        routes.push((Method::PATCH, format!("{base}/{{id}}")));
        routes.push((Method::DELETE, format!("{base}/{{id}}")));
        routes.push((Method::GET, format!("{base}/{{id}}/notes")));
        routes.push((Method::GET, format!("{base}/{{id}}/notes/{{note}}")));
        routes.push((Method::POST, format!("{base}/{{id}}/archive")));
        routes.push((Method::GET, format!("{base}/export")));
        routes.push((Method::GET, format!("{base}/search")));
    }
    routes
}

/// A per-route layer that boxes the future, as the route pipeline does.
#[derive(Clone)]
struct Boxing;

impl<S> Layer<S> for Boxing {
    type Service = Boxed<S>;

    fn layer(&self, inner: S) -> Boxed<S> {
        Boxed(inner)
    }
}

#[derive(Clone)]
struct Boxed<S>(S);

impl<S> Service<Request<Body>> for Boxed<S>
where
    S: Service<Request<Body>, Response = axum::response::Response, Error = Infallible>
        + Clone
        + Send
        + 'static,
    S::Future: Send + 'static,
{
    type Response = axum::response::Response;
    type Error = Infallible;
    type Future = BoxFuture<Result<Self::Response, Infallible>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request<Body>) -> Self::Future {
        let inner = self.0.clone();
        Box::pin(async move { inner.oneshot(request).await })
    }
}

/// The error stage of the pipeline, reduced to what both routers need: an
/// `AppError` a response carries is rendered again (what the stage does to
/// map and log it), and an empty error response (the previous router's 405)
/// becomes one, keeping its `Allow`.
#[derive(Clone)]
struct Errors<S>(S);

impl<S> Service<Request<Body>> for Errors<S>
where
    S: Service<Request<Body>, Response = axum::response::Response, Error = Infallible>
        + Clone
        + Send
        + 'static,
    S::Future: Send + 'static,
{
    type Response = axum::response::Response;
    type Error = Infallible;
    type Future = BoxFuture<Result<Self::Response, Infallible>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request<Body>) -> Self::Future {
        use axum::response::IntoResponse;
        use http_body_util::BodyExt;

        let inner = self.0.clone();
        Box::pin(async move {
            let mut response = inner.oneshot(request).await?;
            if let Some(error) = AppError::take_from(&mut response) {
                return Ok(error.into_response());
            }
            let status = response.status();
            if !status.is_client_error() {
                return Ok(response);
            }
            let (parts, body) = response.into_parts();
            let _ = body.collect().await;
            let mut error = AppError::from_status(status);
            if let Some(allow) = parts.headers.get(http::header::ALLOW) {
                error = error.with_header(http::header::ALLOW, allow.clone());
            }
            Ok(error.into_response())
        })
    }
}

async fn index() -> &'static str {
    "index"
}

async fn show(Path(id): Path<String>) -> String {
    id
}

async fn note(Path((id, note)): Path<(String, String)>) -> String {
    format!("{id}/{note}")
}

async fn previous_show(axum::extract::Path(id): axum::extract::Path<String>) -> String {
    id
}

async fn previous_note(
    axum::extract::Path((id, note)): axum::extract::Path<(String, String)>,
) -> String {
    format!("{id}/{note}")
}

/// Navree's router over the table, every route behind [`Boxing`].
fn navree(previous_extractors: bool) -> Router {
    let mut router = Router::new();
    for (method, pattern) in table() {
        let endpoint = match (pattern.matches('{').count(), previous_extractors) {
            (0, _) => Endpoint::new(index),
            (1, false) => Endpoint::new(show),
            (1, true) => Endpoint::new(previous_show),
            (_, false) => Endpoint::new(note),
            (_, true) => Endpoint::new(previous_note),
        };
        router
            .try_route(method, &pattern, endpoint)
            .expect("the table mounts");
    }
    router.route_layer(Boxing)
}

/// The previous router over the table, every route behind [`Boxing`].
fn previous() -> axum::Router {
    let mut methods: Vec<(String, axum::routing::MethodRouter)> = Vec::new();
    for (method, pattern) in table() {
        let handler = match pattern.matches('{').count() {
            0 => axum::routing::on(filter(&method), index),
            1 => axum::routing::on(filter(&method), previous_show),
            _ => axum::routing::on(filter(&method), previous_note),
        }
        .route_layer(Boxing);
        match methods
            .iter_mut()
            .find(|(existing, _)| *existing == pattern)
        {
            Some((_, router)) => *router = std::mem::take(router).merge(handler),
            None => methods.push((pattern, handler)),
        }
    }
    let mut router = axum::Router::new();
    for (pattern, handler) in methods {
        router = router.route(&pattern, handler);
    }
    // The JSON 404 `build_router` mounted as the previous router's fallback,
    // as Navree's router answers it.
    router.fallback(|| async { AppError::not_found() })
}

fn filter(method: &Method) -> axum::routing::MethodFilter {
    axum::routing::MethodFilter::try_from(method.clone()).expect("a standard method")
}

async fn send<S>(service: S, method: Method, path: &str, expected: StatusCode)
where
    S: Service<Request<Body>, Response = axum::response::Response, Error = Infallible>,
{
    let request = Request::builder()
        .method(method)
        .uri(path)
        .body(Body::empty())
        .expect("request");
    let response = service.oneshot(request).await.expect("infallible");
    assert_eq!(response.status(), expected, "{path}");
}

fn routing(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    let mut tree = PathTree::new();
    let mut matcher = matchit::Router::new();
    for (index, (_, pattern)) in table().into_iter().enumerate() {
        if tree.insert(&pattern, index).is_ok() {
            matcher.insert(pattern, index).expect("the table inserts");
        }
    }
    let paths = [
        "/api/v1/deals",
        "/api/v1/deals/123",
        "/api/v1/users/42/notes/7",
        "/api/v1/tasks/export",
        "/api/v1/nope",
    ];
    let mut group = c.benchmark_group("match");
    group.measurement_time(Duration::from_secs(4));
    group.bench_function("navree", |b| {
        b.iter(|| {
            for path in paths {
                std::hint::black_box(tree.at(std::hint::black_box(path)).map(|m| *m.value));
            }
        });
    });
    group.bench_function("previous", |b| {
        b.iter(|| {
            for path in paths {
                std::hint::black_box(
                    matcher
                        .at(std::hint::black_box(path))
                        .ok()
                        .map(|m| *m.value),
                );
            }
        });
    });
    group.finish();

    let ours = Errors(navree(false));
    let theirs = Errors(previous());
    let cases: [(&str, Method, &str, StatusCode); 5] = [
        ("static", Method::GET, "/api/v1/deals", StatusCode::OK),
        (
            "one_param",
            Method::GET,
            "/api/v1/deals/123",
            StatusCode::OK,
        ),
        (
            "two_params",
            Method::GET,
            "/api/v1/users/42/notes/7",
            StatusCode::OK,
        ),
        (
            "not_found",
            Method::GET,
            "/api/v1/nope",
            StatusCode::NOT_FOUND,
        ),
        (
            "method_not_allowed",
            Method::PUT,
            "/api/v1/deals/123",
            StatusCode::METHOD_NOT_ALLOWED,
        ),
    ];
    let mut group = c.benchmark_group("route");
    group.measurement_time(Duration::from_secs(4));
    for (name, method, path, status) in &cases {
        group.bench_function(format!("navree/{name}"), |b| {
            b.to_async(&rt)
                .iter(|| send(ours.clone(), method.clone(), path, *status));
        });
        group.bench_function(format!("previous/{name}"), |b| {
            b.to_async(&rt)
                .iter(|| send(theirs.clone(), method.clone(), path, *status));
        });
    }
    group.finish();

    let bridged = Errors(navree(true));
    let mut group = c.benchmark_group("previous_extractors");
    group.measurement_time(Duration::from_secs(4));
    group.bench_function("one_param", |b| {
        b.to_async(&rt).iter(|| {
            send(
                bridged.clone(),
                Method::GET,
                "/api/v1/deals/123",
                StatusCode::OK,
            )
        });
    });
    group.finish();
}

criterion_group!(benches, routing);
criterion_main!(benches);
