//! Navree's router: route patterns, matching, method dispatch and path
//! parameters.
//!
//! - **Patterns** ([`PathPattern`]): static segments, `{name}` and a last
//!   `{*name}`, checked when the route is added.
//! - **Matching** ([`PathTree`]): segment by segment on the raw path, static
//!   before parameter before catch-all, with backtracking; precedence never
//!   depends on the order routes were added. Two routes of the same shape
//!   and method are a conflict, refused when the router is built.
//! - **Paths are not normalized**: a `.` or `..` segment (raw or
//!   percent-encoded) or a backslash is a 400 before matching; an empty
//!   segment or a trailing slash matches no route (404). Parameters are
//!   percent-decoded strictly ([`decode_param`]) into [`PathParams`].
//! - **Methods**: `HEAD` runs the `GET` route without its body; `OPTIONS`
//!   on a path with routes answers 204 with `Allow` unless a route handles
//!   it; any other method the path has no route for is a 405 with `Allow`.
//!   A path no route matches is a 404. Every error is an
//!   [`AppError`](crate::AppError) in the JSON error format.
//!
//! [`build_router`](crate::build_router) mounts every controller route here,
//! behind its per-route pipeline.

use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use http::header::{ALLOW, CONTENT_LENGTH, HeaderValue};
use http::{Method, StatusCode};
use tower::Service;

mod path;
mod pattern;
mod tree;

pub use path::{ParamProblem, PathParams, PathProblem, check_path, decode_param, percent_decode};
pub use pattern::{MAX_PARAMS, PathPattern, PatternError, PatternErrorKind, Segment};
pub use tree::{InsertError, Matched, PathTree};

use crate::AppError;
use crate::body::{Body, HttpBody};
use crate::extract::Request;
use crate::handler::{BoxedService, Endpoint, Handler};
use crate::response::{IntoResponse, Response};

/// Routes to endpoints: a [`tower::Service`] for requests. See the [module
/// documentation](self).
///
/// ```
/// use http::Method;
/// use navree_http::Path;
/// use navree_http::routing::Router;
///
/// async fn show(Path(id): Path<u32>) -> String {
///     format!("deal {id}")
/// }
///
/// let router = Router::new()
///     .route(Method::GET, "/deals/{id}", show)
///     .route(Method::POST, "/deals", || async { "created" });
/// # let _ = router;
/// ```
#[derive(Clone, Default)]
pub struct Router {
    tree: Arc<PathTree<Methods>>,
}

/// The endpoints of one path shape, by method.
#[derive(Clone, Default)]
struct Methods {
    /// `(method, endpoint)` in the order added.
    endpoints: Vec<(Method, BoxedService)>,
    /// The `Allow` header: the methods, plus `HEAD` with `GET` and `OPTIONS`.
    allow: Option<HeaderValue>,
}

impl Methods {
    fn get(&self, method: &Method) -> Option<&BoxedService> {
        self.endpoints
            .iter()
            .find(|(candidate, _)| candidate == method)
            .map(|(_, endpoint)| endpoint)
    }

    fn has(&self, method: &Method) -> bool {
        self.get(method).is_some()
    }

    /// `GET, HEAD, POST, ...` in the order of RFC 9110, then extension
    /// methods in the order added.
    fn compute_allow(&mut self) {
        const ORDER: [Method; 9] = [
            Method::GET,
            Method::HEAD,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
            Method::TRACE,
            Method::CONNECT,
        ];
        let listed = |method: &Method| match *method {
            Method::HEAD => self.has(&Method::HEAD) || self.has(&Method::GET),
            Method::OPTIONS => true,
            _ => self.has(method),
        };
        let mut names: Vec<&str> = ORDER
            .iter()
            .filter(|method| listed(method))
            .map(Method::as_str)
            .collect();
        names.extend(
            self.endpoints
                .iter()
                .map(|(method, _)| method)
                .filter(|method| !ORDER.contains(method))
                .map(Method::as_str),
        );
        self.allow = HeaderValue::from_str(&names.join(", ")).ok();
    }
}

/// Why a route cannot be added.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RouteError {
    /// The pattern is malformed.
    Pattern(PatternError),
    /// The same method is already routed for a pattern of the same shape.
    Conflict {
        /// The method.
        method: Method,
        /// The pattern being added.
        pattern: String,
        /// The pattern already routed.
        existing: String,
    },
}

impl std::fmt::Display for RouteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RouteError::Pattern(error) => error.fmt(f),
            RouteError::Conflict {
                method,
                pattern,
                existing,
            } if pattern == existing => write!(f, "`{method} {pattern}` is routed twice"),
            RouteError::Conflict {
                method,
                pattern,
                existing,
            } => write!(
                f,
                "`{method} {pattern}` matches the same paths as `{method} {existing}` (only parameter names differ)"
            ),
        }
    }
}

impl std::error::Error for RouteError {}

impl Router {
    /// A router without routes (every request is a 404).
    pub fn new() -> Self {
        Self::default()
    }

    /// Routes `method` on `pattern` to `handler`.
    ///
    /// # Panics
    ///
    /// When the pattern is malformed or the method is already routed for a
    /// pattern of the same shape; [`try_route`](Router::try_route) returns
    /// the error instead.
    pub fn route<H, T>(mut self, method: Method, pattern: &str, handler: H) -> Self
    where
        H: Handler<T>,
        T: 'static,
    {
        if let Err(error) = self.try_route(method, pattern, Endpoint::new(handler)) {
            panic!("{error}");
        }
        self
    }

    /// Routes `method` on `pattern` to `endpoint`.
    pub fn try_route(
        &mut self,
        method: Method,
        pattern: &str,
        endpoint: Endpoint,
    ) -> Result<(), RouteError> {
        self.try_route_service(method, pattern, |pattern| endpoint.mount(pattern, None))
    }

    /// Routes `method` on `pattern` to the service `mount` builds for the
    /// parsed pattern.
    pub(crate) fn try_route_service(
        &mut self,
        method: Method,
        pattern: &str,
        mount: impl FnOnce(&str) -> BoxedService,
    ) -> Result<(), RouteError> {
        let parsed = PathPattern::parse(pattern).map_err(RouteError::Pattern)?;
        let tree = Arc::make_mut(&mut self.tree);
        if let Some((existing, methods)) = tree.same_shape_mut(&parsed) {
            if methods.has(&method) {
                return Err(RouteError::Conflict {
                    method,
                    pattern: parsed.as_str().to_owned(),
                    existing: existing.as_str().to_owned(),
                });
            }
            methods.endpoints.push((method, mount(pattern)));
            methods.compute_allow();
            return Ok(());
        }
        let mut methods = Methods {
            endpoints: vec![(method, mount(pattern))],
            allow: None,
        };
        methods.compute_allow();
        tree.insert_parsed(parsed, methods)
            .map_err(|error| match error {
                InsertError::Pattern(error) => RouteError::Pattern(error),
                InsertError::Conflict { .. } => unreachable!("checked by shape above"),
            })
    }

    /// Wraps every route added so far in `layer` (it runs after matching,
    /// only for requests a route answers).
    pub fn route_layer<L>(mut self, layer: L) -> Self
    where
        L: tower::Layer<BoxedService>,
        L::Service: Service<Request, Response = Response, Error = Infallible>
            + Clone
            + Send
            + Sync
            + 'static,
        <L::Service as Service<Request>>::Future: Send + 'static,
    {
        let tree = Arc::make_mut(&mut self.tree);
        for methods in tree.values_mut() {
            for (_, endpoint) in &mut methods.endpoints {
                *endpoint = BoxedService::new(layer.layer(endpoint.clone()));
            }
        }
        self
    }

    /// Whether the router has no route.
    pub fn is_empty(&self) -> bool {
        self.tree.is_empty()
    }

    /// Routes `request`: the decision is synchronous; the returned future
    /// runs the endpoint, if any.
    fn dispatch(&self, mut request: Request) -> Dispatch {
        let path = request.uri().path();
        if let Err(problem) = check_path(path) {
            return Dispatch::Ready(AppError::bad_request(problem.to_string()).into_response());
        }
        let Some(found) = self.tree.at(path) else {
            return Dispatch::Ready(AppError::not_found().into_response());
        };
        let methods = found.value;
        let method = request.method();
        let (endpoint, head) = match methods.get(method) {
            Some(endpoint) => (endpoint, false),
            None if *method == Method::HEAD => match methods.get(&Method::GET) {
                Some(endpoint) => (endpoint, true),
                None => return Dispatch::Ready(method_not_allowed(methods)),
            },
            None if *method == Method::OPTIONS => {
                let mut response = StatusCode::NO_CONTENT.into_response();
                if let Some(allow) = &methods.allow {
                    response.headers_mut().insert(ALLOW, allow.clone());
                }
                return Dispatch::Ready(response);
            }
            None => return Dispatch::Ready(method_not_allowed(methods)),
        };
        let endpoint = endpoint.clone();
        if !found.raw_values().is_empty() {
            let decoded = PathParams::decode(
                found.pattern.shared_names(),
                found.raw_values(),
                found.ends_in_catch_all(),
            );
            match decoded {
                Ok(params) => {
                    request.extensions_mut().insert(params);
                }
                Err((name, problem)) => {
                    return Dispatch::Ready(
                        AppError::bad_request(problem.message(&name)).into_response(),
                    );
                }
            }
        }
        Dispatch::Endpoint {
            future: Box::pin(crate::handler::call(endpoint, request)),
            head,
        }
    }
}

/// 405 with the path's `Allow`.
fn method_not_allowed(methods: &Methods) -> Response {
    let mut error = AppError::from_status(StatusCode::METHOD_NOT_ALLOWED);
    if let Some(allow) = &methods.allow {
        error = error.with_header(ALLOW, allow.clone());
    }
    error.into_response()
}

enum Dispatch {
    Ready(Response),
    Endpoint {
        future: Pin<Box<dyn Future<Output = Response> + Send>>,
        head: bool,
    },
}

impl Service<Request> for Router {
    type Response = Response;
    type Error = Infallible;
    type Future = RouteFuture;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request) -> Self::Future {
        RouteFuture(match self.dispatch(request) {
            Dispatch::Ready(response) => State::Ready(Some(response)),
            Dispatch::Endpoint { future, head } => State::Endpoint { future, head },
        })
    }
}

/// The future of a routed request.
pub struct RouteFuture(State);

enum State {
    Ready(Option<Response>),
    Endpoint {
        future: Pin<Box<dyn Future<Output = Response> + Send>>,
        head: bool,
    },
}

impl Future for RouteFuture {
    type Output = Result<Response, Infallible>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match &mut self.0 {
            State::Ready(response) => Poll::Ready(Ok(response
                .take()
                .expect("a route future is polled after it completed"))),
            State::Endpoint { future, head } => {
                let head = *head;
                future.as_mut().poll(cx).map(|response| {
                    Ok(if head {
                        without_body(response)
                    } else {
                        response
                    })
                })
            }
        }
    }
}

/// A `HEAD` response: the `GET` response's head, its length kept when it is
/// known, and no body.
fn without_body(mut response: Response) -> Response {
    if !response.headers().contains_key(CONTENT_LENGTH)
        && let Some(length) = response.body().size_hint().exact()
    {
        response
            .headers_mut()
            .insert(CONTENT_LENGTH, HeaderValue::from(length));
    }
    *response.body_mut() = Body::empty();
    response
}

impl std::fmt::Debug for Router {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.tree.iter().flat_map(|(pattern, methods)| {
                methods
                    .endpoints
                    .iter()
                    .map(move |(method, _)| format!("{method} {pattern}"))
            }))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::*;
    use crate::Path;

    async fn send(
        router: &Router,
        method: Method,
        uri: &str,
    ) -> (StatusCode, http::HeaderMap, String) {
        let request = http::Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        (status, headers, String::from_utf8_lossy(&body).into_owned())
    }

    fn deals() -> Router {
        async fn show(Path(id): Path<String>) -> String {
            format!("deal {id}")
        }
        async fn file(Path((id, path)): Path<(String, String)>) -> String {
            format!("{id}:{path}")
        }
        Router::new()
            .route(Method::GET, "/deals", || async { "index" })
            .route(Method::POST, "/deals", || async { "store" })
            .route(Method::GET, "/deals/{id}", show)
            .route(Method::DELETE, "/deals/{deal}", || async { "destroy" })
            .route(Method::GET, "/deals/{id}/files/{*path}", file)
    }

    #[tokio::test]
    async fn requests_reach_their_route_with_decoded_parameters() {
        let router = deals();
        assert_eq!(send(&router, Method::GET, "/deals").await.2, "index");
        assert_eq!(send(&router, Method::POST, "/deals").await.2, "store");
        assert_eq!(
            send(&router, Method::GET, "/deals/caf%C3%A9").await.2,
            "deal café"
        );
        assert_eq!(send(&router, Method::DELETE, "/deals/7").await.2, "destroy");
        assert_eq!(
            send(&router, Method::GET, "/deals/7/files/a%2Fb/c.txt?x=1")
                .await
                .2,
            "7:a/b/c.txt"
        );
    }

    #[tokio::test]
    async fn unmatched_paths_are_404_and_other_methods_405() {
        let router = deals();
        let (status, headers, body) = send(&router, Method::GET, "/nope").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(headers["content-type"], "application/json");
        assert!(body.contains("\"code\":\"not_found\""), "{body}");
        for path in ["/deals/", "//deals", "/deals/7/files", "/deals/7/files/"] {
            assert_eq!(
                send(&router, Method::GET, path).await.0,
                StatusCode::NOT_FOUND,
                "{path}"
            );
        }
        let (status, headers, body) = send(&router, Method::PUT, "/deals").await;
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(headers["allow"], "GET, HEAD, POST, OPTIONS");
        assert!(body.contains("\"code\":\"method_not_allowed\""), "{body}");
        let (_, headers, _) = send(&router, Method::PATCH, "/deals/7").await;
        assert_eq!(headers["allow"], "GET, HEAD, DELETE, OPTIONS");
    }

    #[tokio::test]
    async fn head_runs_get_without_the_body_and_options_lists_the_methods() {
        let router = deals();
        let (status, headers, body) = send(&router, Method::HEAD, "/deals/7").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, "");
        assert_eq!(headers["content-length"], "6");
        let (status, headers, body) = send(&router, Method::OPTIONS, "/deals").await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(headers["allow"], "GET, HEAD, POST, OPTIONS");
        assert_eq!(body, "");
        assert_eq!(
            send(&router, Method::OPTIONS, "/nope").await.0,
            StatusCode::NOT_FOUND
        );
        let explicit = Router::new()
            .route(Method::POST, "/x", || async { "post" })
            .route(Method::OPTIONS, "/x", || async { "mine" });
        assert_eq!(send(&explicit, Method::OPTIONS, "/x").await.2, "mine");
        assert_eq!(
            send(&explicit, Method::HEAD, "/x").await.0,
            StatusCode::METHOD_NOT_ALLOWED,
            "HEAD needs a GET route"
        );
    }

    #[tokio::test]
    async fn ambiguous_paths_are_400_before_matching() {
        let router = deals();
        for path in [
            "/deals/..",
            "/deals/./7",
            "/deals/%2e%2E",
            "/deals/7/files/a/../b",
            "/deals/7/files/a/%2E%2E/b",
            "/deals\\7",
        ] {
            let (status, _, body) = send(&router, Method::GET, path).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{path}");
            assert!(body.contains("Invalid URL"), "{path}: {body}");
        }
        for (path, message) in [
            ("/deals/a%2Fb", "Invalid URL: `id` holds an encoded slash"),
            ("/deals/%FF", "Invalid URL: Invalid UTF-8 in `id`"),
            ("/deals/a%00", "Invalid URL: `id` holds a NUL character"),
            ("/deals/a%5Cb", "Invalid URL: `id` holds a backslash"),
            (
                "/deals/7/files/a%2F..%2Fb",
                "Invalid URL: `path` is or holds a `.` or `..` segment",
            ),
        ] {
            let (status, _, body) = send(&router, Method::GET, path).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{path}");
            assert!(body.contains(message), "{path}: {body}");
        }
    }

    #[test]
    fn conflicts_and_bad_patterns_are_refused() {
        let mut router = deals();
        let error = router
            .try_route(Method::GET, "/deals/{deal}", Endpoint::new(|| async {}))
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "`GET /deals/{deal}` matches the same paths as `GET /deals/{id}` (only parameter names differ)"
        );
        let error = router
            .try_route(Method::GET, "/deals", Endpoint::new(|| async {}))
            .unwrap_err();
        assert_eq!(error.to_string(), "`GET /deals` is routed twice");
        assert!(matches!(
            router.try_route(Method::GET, "/deals/:id", Endpoint::new(|| async {})),
            Err(RouteError::Pattern(_))
        ));
        assert!(
            router
                .try_route(Method::PUT, "/deals/{deal}", Endpoint::new(|| async {}))
                .is_ok()
        );
        assert_eq!(
            format!("{router:?}"),
            r#"["GET /deals", "POST /deals", "GET /deals/{id}", "DELETE /deals/{id}", "PUT /deals/{id}", "GET /deals/{id}/files/{*path}"]"#
        );
    }

    #[tokio::test]
    async fn extension_methods_route_and_are_allowed() {
        let report = Method::from_bytes(b"REPORT").unwrap();
        let router = Router::new()
            .route(report.clone(), "/cal", || async { "report" })
            .route(Method::GET, "/cal", || async { "get" });
        assert_eq!(send(&router, report, "/cal").await.2, "report");
        let (_, headers, _) = send(&router, Method::DELETE, "/cal").await;
        assert_eq!(headers["allow"], "GET, HEAD, OPTIONS, REPORT");
    }
}
