//! The protocol edge, where gRPC, gRPC-Web and Connect requests enter:
//! request id and tracing, CORS, then the protocol. [`GrpcCore`] serves
//! plain gRPC (the other two are translated to it): it finds the call's
//! service, sets its deadline and guards the response stream.

use std::collections::HashSet;
use std::convert::Infallible;
use std::future::Future;
use std::pin::{Pin, pin};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use futures_util::future::BoxFuture;
use http::header::{CONTENT_TYPE, HeaderName, HeaderValue};
use http::{HeaderMap, Method, Request, Response, StatusCode};
use http_body::Frame;
use navree_contracts::RequestId;
use navree_http::body::Body;
use navree_http::middleware::{Next, State, from_fn_with_state};
use navree_http::{ClientIp, CorsLayer, GlobalLayer, HttpOptions, LayerSlot};
use navree_kernel::{AppContext, RequestScope};
use tokio::sync::OnceCell;
use tokio::time::{Instant, Sleep};
use tokio_util::sync::WaitForCancellationFutureOwned;
use tonic::service::Routes;
use tonic::{Code, Status};
use tower::util::BoxCloneSyncService;
use tower::{Service, ServiceBuilder, ServiceExt};
use tracing::Instrument;

use crate::call::{CallInfo, Runtime, ServiceRuntime};
use crate::connect::{self, Codec, Connect};
use crate::context::{TRACEPARENT, TRACESTATE, X_WORKSPACE};
use crate::server::GrpcServer;

/// The request id header.
static X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

/// What a request is, by its method and content type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// `application/grpc`, `application/grpc+proto`, ...
    Grpc,
    /// `application/grpc-web`, `-web+proto`, `-web-text`, `-web-text+proto`.
    GrpcWeb,
    /// A Connect unary call (`application/proto`, `application/json`).
    ConnectUnary(Codec),
    /// A Connect streaming call (`application/connect+proto`, `+json`).
    ConnectStream(Codec),
    /// A CORS preflight.
    Preflight,
    /// Anything else.
    Other,
}

/// The lower-cased content type without parameters.
fn content_type(headers: &HeaderMap) -> String {
    headers
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
}

pub(crate) fn classify<B>(request: &Request<B>) -> Kind {
    if request.method() == Method::OPTIONS {
        return Kind::Preflight;
    }
    let content_type = content_type(request.headers());
    match content_type.as_str() {
        t if t.starts_with("application/grpc-web") => Kind::GrpcWeb,
        t if t == "application/grpc" || t.starts_with("application/grpc+") => Kind::Grpc,
        "application/proto" => Kind::ConnectUnary(Codec::Proto),
        "application/json" => Kind::ConnectUnary(Codec::Json),
        "application/connect+proto" => Kind::ConnectStream(Codec::Proto),
        "application/connect+json" => Kind::ConnectStream(Codec::Json),
        _ => Kind::Other,
    }
}

/// The service name of a gRPC path (`/crm.deals.v1.Deals/GetDeal`).
pub(crate) fn service_of(path: &str) -> Option<&str> {
    let (service, method) = path.strip_prefix('/')?.split_once('/')?;
    (!service.is_empty() && !method.is_empty() && !method.contains('/')).then_some(service)
}

/// The `grpc-timeout` header: up to eight digits and a unit (`H`, `M`, `S`,
/// `m`, `u`, `n`).
pub(crate) fn grpc_timeout(headers: &HeaderMap) -> Option<Duration> {
    let value = headers.get("grpc-timeout")?.to_str().ok()?;
    if !(2..=9).contains(&value.len()) || !value.is_ascii() {
        return None;
    }
    let (digits, unit) = value.split_at(value.len() - 1);
    let amount: u64 = digits.parse().ok()?;
    Some(match unit {
        "H" => Duration::from_secs(amount.saturating_mul(3600)),
        "M" => Duration::from_secs(amount.saturating_mul(60)),
        "S" => Duration::from_secs(amount),
        "m" => Duration::from_millis(amount),
        "u" => Duration::from_micros(amount),
        "n" => Duration::from_nanos(amount),
        _ => return None,
    })
}

/// Serves plain gRPC: finds the call's service, sets its deadline and
/// guards the response stream (shutdown, the client's deadline, the log
/// line).
#[derive(Clone)]
pub(crate) struct GrpcCore {
    inner: Arc<CoreInner>,
}

struct CoreInner {
    runtime: Arc<Runtime>,
    services: std::collections::HashMap<&'static str, Arc<ServiceRuntime>>,
    routes: Routes,
}

impl GrpcCore {
    pub(crate) fn new(
        runtime: Arc<Runtime>,
        services: std::collections::HashMap<&'static str, Arc<ServiceRuntime>>,
        routes: Routes,
    ) -> Self {
        Self {
            inner: Arc::new(CoreInner {
                runtime,
                services,
                routes: routes.prepare(),
            }),
        }
    }

    /// See [`CoreInner::timeout`].
    pub(crate) fn timeout(&self, client: Option<Duration>) -> Duration {
        self.inner.timeout(client)
    }
}

impl CoreInner {
    /// How long a call may take: what the client asked for, up to the
    /// server's own deadline.
    fn timeout(&self, client: Option<Duration>) -> Duration {
        let server = self.runtime.default_timeout;
        client.map_or(server, |client| client.min(server))
    }
}

impl Service<Request<tonic::body::Body>> for GrpcCore {
    type Response = Response<tonic::body::Body>;
    type Error = Infallible;
    type Future = BoxFuture<'static, Result<Self::Response, Infallible>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, mut request: Request<tonic::body::Body>) -> Self::Future {
        let inner = self.inner.clone();
        Box::pin(async move {
            let path: Arc<str> = request.uri().path().into();
            let now = Instant::now();
            let client_timeout = grpc_timeout(request.headers());
            let client_deadline = client_timeout.map(|timeout| now + timeout);
            let deadline = now + inner.timeout(client_timeout);
            let in_the_pipeline = Arc::new(AtomicBool::new(false));
            if let Some(service) = service_of(&path).and_then(|name| inner.services.get(name)) {
                request.extensions_mut().insert(CallInfo {
                    runtime: inner.runtime.clone(),
                    service: service.clone(),
                    path: path.clone(),
                    deadline,
                    in_the_pipeline: in_the_pipeline.clone(),
                });
            }
            let request_id = request
                .extensions()
                .get::<RequestId>()
                .map(|id| id.as_str().to_owned());
            let routed = inner.routes.clone().oneshot(request);
            let response = until_the_head(routed, deadline, &in_the_pipeline).await;
            let header_code = Status::from_header_map(response.headers()).map(|s| s.code());
            let log = CallLog {
                start: now,
                path,
                request_id,
                code: header_code,
            };
            Ok(response.map(|body| {
                tonic::body::Body::new(CallBody {
                    inner: body,
                    shutdown: inner.runtime.shutdown.clone().cancelled_owned(),
                    deadline: client_deadline.map(tokio::time::sleep_until),
                    log: Some(log),
                    done: false,
                })
            }))
        })
    }
}

/// The response head of `routed`, by `deadline`. tonic reads a unary message
/// before it calls the method, so until the pipeline starts nothing else
/// times the call: not authenticated yet, a client sending it just above the
/// pace of a request body would otherwise hold its slot for hours. Once the
/// pipeline has started it answers at the deadline itself, after rolling back
/// what the method opened, and cutting that off here would lose the rollback.
/// Services outside the pipeline (health, reflection) are held to the
/// deadline until their head.
async fn until_the_head(
    routed: impl Future<Output = Result<Response<tonic::body::Body>, Infallible>>,
    deadline: Instant,
    in_the_pipeline: &AtomicBool,
) -> Response<tonic::body::Body> {
    let mut routed = pin!(routed);
    let response = match tokio::time::timeout_at(deadline, routed.as_mut()).await {
        Ok(response) => response,
        Err(_) if in_the_pipeline.load(Ordering::Relaxed) => routed.await,
        Err(_) => return Status::deadline_exceeded("Deadline exceeded").into_http(),
    };
    response.unwrap_or_else(|never| match never {})
}

/// What the log line of a finished call reports.
struct CallLog {
    start: Instant,
    path: Arc<str>,
    request_id: Option<String>,
    code: Option<Code>,
}

impl CallLog {
    fn finish(self) {
        let code = self.code.map_or("UNKNOWN".to_owned(), code_label);
        let latency = self.start.elapsed();
        let (service, method) = self
            .path
            .trim_start_matches('/')
            .split_once('/')
            .unwrap_or(("", &self.path));
        tracing::info!(
            target: "navree::grpc",
            path = %self.path,
            code = %code,
            latency_ms = latency.as_secs_f64() * 1000.0,
            request_id = self.request_id.as_deref(),
            "call completed"
        );
        let labels = [
            ("service", service.to_owned()),
            ("method", method.to_owned()),
            ("code", code),
        ];
        metrics::counter!("grpc_server_handled_total", &labels).increment(1);
        metrics::histogram!("grpc_server_handling_seconds", &labels).record(latency.as_secs_f64());
    }
}

/// `NOT_FOUND`, the upper-snake name of a code.
fn code_label(code: Code) -> String {
    connect::code_name(code)
        .to_ascii_uppercase()
        .replace("CANCELED", "CANCELLED")
}

/// The trailers carrying `status`.
fn status_trailers(status: &Status) -> HeaderMap {
    let mut trailers = HeaderMap::new();
    let _ = status.add_header(&mut trailers);
    trailers
}

pin_project_lite::pin_project! {
    /// A call's response body: ends an open stream with `UNAVAILABLE` when
    /// the app shuts down and with `DEADLINE_EXCEEDED` when the client's
    /// deadline passes, and writes the call's log line when it ends.
    struct CallBody {
        #[pin]
        inner: tonic::body::Body,
        #[pin]
        shutdown: WaitForCancellationFutureOwned,
        #[pin]
        deadline: Option<Sleep>,
        log: Option<CallLog>,
        done: bool,
    }

    impl PinnedDrop for CallBody {
        fn drop(this: Pin<&mut Self>) {
            if let Some(log) = this.project().log.take() {
                log.finish();
            }
        }
    }
}

impl http_body::Body for CallBody {
    type Data = bytes::Bytes;
    type Error = Status;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<bytes::Bytes>, Status>>> {
        let mut this = self.project();
        if *this.done {
            return Poll::Ready(None);
        }
        match this.inner.as_mut().poll_frame(cx) {
            Poll::Ready(Some(Ok(frame))) => {
                if let Some(trailers) = frame.trailers_ref() {
                    *this.done = true;
                    if let Some(mut log) = this.log.take() {
                        log.code = Status::from_header_map(trailers).map(|s| s.code());
                        log.finish();
                    }
                }
                Poll::Ready(Some(Ok(frame)))
            }
            Poll::Ready(Some(Err(error))) => {
                *this.done = true;
                Poll::Ready(Some(Err(error)))
            }
            Poll::Ready(None) => {
                *this.done = true;
                if let Some(log) = this.log.take() {
                    log.finish();
                }
                Poll::Ready(None)
            }
            Poll::Pending => {
                let cut = if this.shutdown.as_mut().poll(cx).is_ready() {
                    Some(Status::unavailable("the server is shutting down"))
                } else if let Some(deadline) = this.deadline.as_mut().as_pin_mut()
                    && deadline.poll(cx).is_ready()
                {
                    Some(Status::deadline_exceeded("Deadline exceeded"))
                } else {
                    None
                };
                match cut {
                    Some(status) => {
                        *this.done = true;
                        if let Some(mut log) = this.log.take() {
                            log.code = Some(status.code());
                            log.finish();
                        }
                        Poll::Ready(Some(Ok(Frame::trailers(status_trailers(&status)))))
                    }
                    None => Poll::Pending,
                }
            }
        }
    }
}

/// Sends each request to its protocol.
#[derive(Clone)]
struct Dispatch {
    core: GrpcCore,
    web: Option<tonic_web::GrpcWebService<GrpcCore>>,
    connect: Option<Arc<Connect>>,
}

impl Service<Request<Body>> for Dispatch {
    type Response = Response<Body>;
    type Error = Infallible;
    type Future = BoxFuture<'static, Result<Response<Body>, Infallible>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request<Body>) -> Self::Future {
        let this = self.clone();
        let kind = classify(&request);
        Box::pin(async move {
            Ok(match (kind, this.web, this.connect) {
                (Kind::Grpc, _, _) => this
                    .core
                    .oneshot(request.map(tonic::body::Body::new))
                    .await
                    .unwrap_or_else(|never| match never {})
                    .map(Body::new),
                (Kind::GrpcWeb, Some(web), _) => web
                    .oneshot(request)
                    .await
                    .unwrap_or_else(|never| match never {})
                    .map(Body::new),
                (Kind::ConnectUnary(codec), _, Some(connect)) => {
                    connect::unary(connect, this.core, request, codec).await
                }
                (Kind::ConnectStream(codec), _, Some(connect)) => {
                    connect::streaming(connect, this.core, request, codec).await
                }
                (Kind::Preflight, _, _) => plain(StatusCode::NO_CONTENT, ""),
                _ => plain(
                    StatusCode::UNSUPPORTED_MEDIA_TYPE,
                    "Unsupported content type for a gRPC endpoint",
                ),
            })
        })
    }
}

fn plain(status: StatusCode, text: &'static str) -> Response<Body> {
    let mut response = Response::new(Body::from(text));
    *response.status_mut() = status;
    response
}

/// The protocol edge of the app's gRPC services.
pub(crate) struct Edge {
    pub(crate) runtime: Arc<Runtime>,
    service: BoxCloneSyncService<Request<Body>, Response<Body>, Infallible>,
    http: Arc<HttpOptions>,
    /// The service names served (registered, health, reflection).
    known: HashSet<String>,
    web: bool,
    connect: bool,
    /// Whether gRPC shares the HTTP port (no `grpc.addr`).
    shares_http_port: bool,
}

/// What [`Edge::new`] assembles.
pub(crate) struct EdgeParts {
    pub(crate) runtime: Arc<Runtime>,
    pub(crate) core: GrpcCore,
    pub(crate) http: Arc<HttpOptions>,
    pub(crate) known: HashSet<String>,
    pub(crate) web: bool,
    pub(crate) connect: Option<Arc<Connect>>,
    pub(crate) shares_http_port: bool,
}

impl Edge {
    pub(crate) fn new(parts: EdgeParts) -> Result<Self, String> {
        let dispatch = Dispatch {
            web: parts
                .web
                .then(|| tower::Layer::layer(&tonic_web::GrpcWebLayer::new(), parts.core.clone())),
            connect: parts.connect.clone(),
            core: parts.core,
        };
        let cors = cors_layer(&parts.http)?;
        let service = ServiceBuilder::new().option_layer(cors).service(dispatch);
        Ok(Self {
            runtime: parts.runtime,
            service: BoxCloneSyncService::new(service),
            http: parts.http,
            known: parts.known,
            web: parts.web,
            connect: parts.connect.is_some(),
            shares_http_port: parts.shares_http_port,
        })
    }

    /// Whether the HTTP port should hand `request` to this edge.
    pub(crate) fn takes<B>(&self, request: &Request<B>) -> bool {
        if !self.shares_http_port {
            return false;
        }
        let known =
            || service_of(request.uri().path()).is_some_and(|service| self.known.contains(service));
        match classify(request) {
            Kind::Grpc => true,
            Kind::GrpcWeb => self.web,
            Kind::ConnectUnary(_) | Kind::ConnectStream(_) => self.connect && known(),
            Kind::Preflight => (self.web || self.connect) && known(),
            Kind::Other => false,
        }
    }

    /// Serves one request: request id, client IP, app context, request
    /// scope and tracing span, then CORS and the protocol.
    pub(crate) async fn handle(&self, mut request: Request<Body>) -> Response<Body> {
        let Some(ctx) = self.runtime.ctx.upgrade() else {
            return Status::unavailable("the server is shutting down").into_http::<Body>();
        };
        let incoming = request
            .headers()
            .get(&X_REQUEST_ID)
            .and_then(|value| value.to_str().ok());
        let request_id = RequestId::parse_or_generate(incoming);
        let header = HeaderValue::from_str(request_id.as_str())
            .unwrap_or_else(|_| HeaderValue::from_static("invalid"));
        let ip = ClientIp::of(request.headers(), request.extensions(), &self.http);
        request
            .headers_mut()
            .insert(X_REQUEST_ID.clone(), header.clone());
        let span = tracing::info_span!(
            "grpc",
            path = %request.uri().path(),
            request_id = %request_id,
            user_id = tracing::field::Empty,
            traceparent = request
                .headers()
                .get(&TRACEPARENT)
                .and_then(|v| v.to_str().ok()),
        );
        let extensions = request.extensions_mut();
        extensions.insert(request_id);
        extensions.insert(ip);
        extensions.insert(ctx);
        extensions.insert(RequestScope::new());
        let mut response = self
            .service
            .clone()
            .oneshot(request)
            .instrument(span)
            .await
            .unwrap_or_else(|never| match never {});
        response.headers_mut().insert(X_REQUEST_ID.clone(), header);
        response
    }

    /// Serves one request of the separate listener.
    pub(crate) async fn handle_from(&self, request: Request<Body>) -> Response<Body> {
        if classify(&request) == Kind::Other {
            return plain(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "Unsupported content type for a gRPC endpoint",
            );
        }
        self.handle(request).await
    }
}

/// CORS for browser calls (gRPC-Web and Connect), from `http.cors`: the
/// app's origins, with the gRPC-Web and Connect headers allowed and exposed.
fn cors_layer(http: &HttpOptions) -> Result<Option<CorsLayer>, String> {
    let Some(layer) = CorsLayer::from_options(&http.cors)
        .map_err(|error| format!("http.cors.allowed_origins: {error}"))?
    else {
        return Ok(None);
    };
    let allow = [
        "accept",
        "accept-language",
        "authorization",
        "content-type",
        "connect-accept-encoding",
        "connect-content-encoding",
        "connect-protocol-version",
        "connect-timeout-ms",
        "grpc-accept-encoding",
        "grpc-timeout",
        "traceparent",
        "tracestate",
        "x-grpc-web",
        "x-request-id",
        "x-user-agent",
    ]
    .into_iter()
    .map(HeaderName::from_static)
    .chain([X_WORKSPACE.clone(), TRACESTATE.clone()]);
    let expose = [
        "grpc-encoding",
        "grpc-message",
        "grpc-status",
        "grpc-status-details-bin",
        "x-request-id",
    ]
    .map(HeaderName::from_static);
    Ok(Some(
        layer
            .allow_methods([Method::POST, Method::OPTIONS])
            .allow_headers(allow)
            .expose_headers(expose),
    ))
}

/// The `LayerSlot::Edge` layer that hands gRPC, gRPC-Web and Connect
/// requests on the HTTP port to the gRPC edge (when gRPC shares the port).
pub(crate) fn shared_port_layer() -> GlobalLayer {
    GlobalLayer::new(LayerSlot::Edge, "grpc", |ctx, router| {
        let state = Arc::new(SharedPort {
            ctx: ctx.clone(),
            server: OnceCell::new(),
        });
        Ok(router.layer(from_fn_with_state(state, shared_port)))
    })
}

struct SharedPort {
    ctx: AppContext,
    server: OnceCell<Option<Arc<GrpcServer>>>,
}

/// Cheap first look: could this be a gRPC, gRPC-Web or Connect request?
fn might_be_grpc<B>(request: &Request<B>) -> bool {
    let content_type = content_type(request.headers());
    if content_type.starts_with("application/grpc")
        || content_type.starts_with("application/connect+")
    {
        return true;
    }
    let rpc_like = matches!(
        content_type.as_str(),
        "application/json" | "application/proto"
    ) || request.method() == Method::OPTIONS;
    rpc_like && service_of(request.uri().path()).is_some_and(|service| service.contains('.'))
}

async fn shared_port(
    State(state): State<Arc<SharedPort>>,
    request: navree_http::extract::Request,
    next: Next,
) -> navree_http::response::Response {
    if !might_be_grpc(&request) {
        return next.run(request).await;
    }
    let server = state
        .server
        .get_or_init(|| state.ctx.try_get::<GrpcServer>())
        .await;
    match server.as_ref().and_then(|server| server.edge()) {
        Some(edge) if edge.takes(&request) => edge.handle(request).await,
        _ => next.run(request).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(method: Method, path: &str, content_type: &str) -> Request<()> {
        Request::builder()
            .method(method)
            .uri(path)
            .header(CONTENT_TYPE, content_type)
            .body(())
            .unwrap()
    }

    #[test]
    fn requests_are_classified_by_content_type() {
        let path = "/crm.deals.v1.Deals/GetDeal";
        let kind = |ct| classify(&request(Method::POST, path, ct));
        assert_eq!(kind("application/grpc"), Kind::Grpc);
        assert_eq!(kind("application/grpc+proto"), Kind::Grpc);
        assert_eq!(kind("application/grpc-web+proto"), Kind::GrpcWeb);
        assert_eq!(kind("application/grpc-web-text"), Kind::GrpcWeb);
        assert_eq!(
            kind("application/json; charset=utf-8"),
            Kind::ConnectUnary(Codec::Json)
        );
        assert_eq!(kind("application/proto"), Kind::ConnectUnary(Codec::Proto));
        assert_eq!(
            kind("application/connect+json"),
            Kind::ConnectStream(Codec::Json)
        );
        assert_eq!(kind("text/html"), Kind::Other);
        assert_eq!(
            classify(&request(Method::OPTIONS, path, "")),
            Kind::Preflight
        );
    }

    #[test]
    fn only_rpc_shaped_requests_are_looked_at() {
        assert!(might_be_grpc(&request(
            Method::POST,
            "/x",
            "application/grpc"
        )));
        assert!(might_be_grpc(&request(
            Method::POST,
            "/crm.deals.v1.Deals/GetDeal",
            "application/json"
        )));
        assert!(!might_be_grpc(&request(
            Method::POST,
            "/api/deals",
            "application/json"
        )));
        assert!(!might_be_grpc(&request(Method::GET, "/api/v1/deals", "")));
        assert_eq!(
            service_of("/crm.deals.v1.Deals/GetDeal"),
            Some("crm.deals.v1.Deals")
        );
        assert_eq!(service_of("/api/v1/deals"), None);
        assert_eq!(service_of("/x/"), None);
    }

    #[test]
    fn grpc_timeouts_parse_every_unit() {
        let timeout = |value: &str| {
            let mut headers = HeaderMap::new();
            headers.insert("grpc-timeout", HeaderValue::from_str(value).unwrap());
            grpc_timeout(&headers)
        };
        assert_eq!(timeout("1H"), Some(Duration::from_secs(3600)));
        assert_eq!(timeout("2M"), Some(Duration::from_secs(120)));
        assert_eq!(timeout("3S"), Some(Duration::from_secs(3)));
        assert_eq!(timeout("250m"), Some(Duration::from_millis(250)));
        assert_eq!(timeout("7u"), Some(Duration::from_micros(7)));
        assert_eq!(timeout("9n"), Some(Duration::from_nanos(9)));
        assert_eq!(timeout("123456789S"), None, "at most eight digits");
        assert_eq!(timeout("5x"), None);
        assert_eq!(timeout("S"), None);
    }
}
