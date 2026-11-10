//! Routing micro-benchmarks: Navree's router on a table of 240 routes shaped
//! like a real app's (`/api/v1/<resource>`, `/{id}`, `/{id}/<sub>`, ...), each
//! behind a boxing per-route layer, as `build_router` mounts them.
//!
//! - `match/navree`: the matcher alone: `PathTree::at`.
//! - `route/navree/*`: a request through the router to its handler and back,
//!   behind a minimal error stage (404 and 405 end as JSON errors, as the
//!   pipeline renders them), the handlers reading their parameters with
//!   `Path`. `static`, `one_param`, `two_params`, `not_found`,
//!   `method_not_allowed`.
//! - `stack/navree`: a request through four `from_fn` layers around the
//!   router, on [`AppRouter`].
//!
//! Run with `cargo bench -p navree-http --bench routing` (add `-- --quick`
//! for a shorter run).
#![allow(missing_docs)]

use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};
use http::{Method, Request, StatusCode};
use navree_http::body::Body;
use navree_http::handler::Endpoint;
use navree_http::middleware::{self, Next};
use navree_http::response::{IntoResponse, Response};
use navree_http::routing::{PathTree, Router};
use navree_http::{AppError, AppRouter, Path};
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

impl<S, B> Service<Request<B>> for Boxed<S>
where
    S: Service<Request<B>, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send + 'static,
    B: Send + 'static,
{
    type Response = S::Response;
    type Error = Infallible;
    type Future = BoxFuture<Result<Self::Response, Infallible>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request<B>) -> Self::Future {
        let inner = self.0.clone();
        Box::pin(async move { inner.oneshot(request).await })
    }
}

/// The error stage of the pipeline, reduced to what the router needs: an
/// `AppError` a response carries is rendered again (what the stage does to
/// map and log it).
#[derive(Clone)]
struct Errors<S>(S);

impl<S> Service<Request<Body>> for Errors<S>
where
    S: Service<Request<Body>, Response = Response, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = Infallible;
    type Future = BoxFuture<Result<Self::Response, Infallible>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request<Body>) -> Self::Future {
        let inner = self.0.clone();
        Box::pin(async move {
            let mut response = inner.oneshot(request).await?;
            if let Some(error) = AppError::take_from(&mut response) {
                return Ok(error.into_response());
            }
            Ok(response)
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

/// Navree's router over the table, every route behind [`Boxing`].
fn navree() -> Router {
    let mut router = Router::new();
    for (method, pattern) in table() {
        let endpoint = match pattern.matches('{').count() {
            0 => Endpoint::new(index),
            1 => Endpoint::new(show),
            _ => Endpoint::new(note),
        };
        router
            .try_route(method, &pattern, endpoint)
            .expect("the table mounts");
    }
    router.route_layer(Boxing)
}

async fn pass(request: navree_http::extract::Request, next: Next) -> Response {
    next.run(request).await
}

/// Navree's router behind four middlewares on [`AppRouter`].
fn app_stack() -> AppRouter {
    let mut app = AppRouter::new(navree());
    for _ in 0..4 {
        app = app.layer(middleware::from_fn(pass));
    }
    app
}

async fn send<S>(service: S, method: Method, path: &str, expected: StatusCode)
where
    S: Service<Request<Body>, Response = Response, Error = Infallible>,
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
    for (index, (_, pattern)) in table().into_iter().enumerate() {
        // A shape once: its methods share it.
        let _ = tree.insert(&pattern, index);
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
    group.finish();

    let router = Errors(navree());
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
                .iter(|| send(router.clone(), method.clone(), path, *status));
        });
    }
    group.finish();

    let app = app_stack();
    let mut group = c.benchmark_group("stack");
    group.measurement_time(Duration::from_secs(4));
    group.bench_function("navree", |b| {
        b.to_async(&rt).iter(|| {
            send(
                app.clone(),
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
