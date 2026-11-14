//! The call pipeline every `#[grpc_service]` method runs through:
//! authentication, the access check, guards, the deadline and the unit of
//! work, then the extractors and the method.

use std::collections::HashMap;
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use http::header::USER_AGENT;
use http::request::Parts;
use http::{Method, StatusCode, Uri};
use navree_contracts::{AuditEntry, AuditKind, AuditSink, AuthState, RequestId};
use navree_db::{Db, Uow};
use navree_http::extract::FromRequestParts;
use navree_http::serve::BodyReadLimit;
use navree_http::{
    AppError, Authenticator, ClientIp, ErrorMapper, PathParams, RouteAuthorizer, RouteMeta,
    rate_limit_key,
};
use navree_kernel::{AppContext, ModuleId, WeakAppContext};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tonic::{Request, Response, Status};

use crate::context::CallContext;
use crate::service::GrpcMethod;
use crate::status::status_from_error;
use crate::tonic_service::rust_name;

/// What every call shares: the app and the providers the pipeline uses,
/// resolved once at boot.
pub(crate) struct Runtime {
    pub(crate) ctx: WeakAppContext,
    pub(crate) authenticator: Option<Arc<dyn Authenticator>>,
    pub(crate) authorizer: Option<Arc<dyn RouteAuthorizer>>,
    pub(crate) audit: Option<Arc<dyn AuditSink>>,
    pub(crate) db: Option<Arc<Db>>,
    pub(crate) mappers: Vec<ErrorMapper>,
    pub(crate) default_timeout: Duration,
    pub(crate) shutdown: CancellationToken,
    /// `http.rate_limit_ipv6_prefix`: IPv6 clients are counted by network.
    pub(crate) ipv6_prefix: u8,
    /// The clients whose refused call to an undeclared method was audited in
    /// the last minute.
    pub(crate) undeclared_audits: UndeclaredAudits,
}

impl Runtime {
    /// The status sent for `error`: a [`Status`] as it is; an [`AppError`]
    /// after the app's error mappers (500s), logged when it is a server
    /// error.
    pub(crate) fn status_of(&self, error: CallError, request_id: Option<&RequestId>) -> Status {
        let error = match error {
            CallError::Status(status) => return status,
            CallError::App(error) => error,
        };
        let error = if error.status == StatusCode::INTERNAL_SERVER_ERROR {
            ErrorMapper::apply(&self.mappers, &error).unwrap_or(error)
        } else {
            error
        };
        if error.status.is_server_error() {
            tracing::error!(
                target: "navree::error",
                request_id = request_id.map(tracing::field::display),
                error = %log_chain(&error),
                "gRPC call failed"
            );
        }
        status_from_error(&error)
    }
}

/// The error and its source chain, for logs.
fn log_chain(error: &AppError) -> String {
    let mut out = error.to_string();
    let mut next: Option<&(dyn std::error::Error + 'static)> = error
        .source
        .as_deref()
        .map(|e| e as &(dyn std::error::Error + 'static));
    while let Some(cause) = next {
        out.push_str(": ");
        out.push_str(&cause.to_string());
        next = cause.source();
    }
    out
}

/// One registered service at run time.
pub(crate) struct ServiceRuntime {
    pub(crate) name: &'static str,
    pub(crate) methods: HashMap<&'static str, MethodRuntime>,
    /// The access and guards of every method without its own: a wrapped
    /// tonic server's service default. `None` for a `#[grpc_service]`.
    pub(crate) default: Option<RouteMeta>,
}

impl ServiceRuntime {
    pub(crate) fn new(
        name: &'static str,
        module: ModuleId,
        module_name: &'static str,
        methods: Vec<GrpcMethod>,
    ) -> Self {
        let methods = methods
            .into_iter()
            .map(|method| {
                let mut meta = method.meta;
                meta.module = Some(module_name);
                meta.module_id = Some(module);
                (
                    method.name,
                    MethodRuntime {
                        template: meta,
                        resolved: OnceLock::new(),
                    },
                )
            })
            .collect();
        Self {
            name,
            methods,
            default: None,
        }
    }

    /// The service with `default` as the access of every method without its
    /// own, as a method of `module`.
    pub(crate) fn with_default(
        mut self,
        default: Option<RouteMeta>,
        module: ModuleId,
        module_name: &'static str,
    ) -> Self {
        self.default = default.map(|mut meta| {
            meta.module = Some(module_name);
            meta.module_id = Some(module);
            meta
        });
        self
    }

    /// What a call to `path` on a wrapped tonic server may do: the metadata
    /// of the method its last segment names, matched by the Rust name of
    /// that segment (`GetDeal` is `get_deal`) against the declared names
    /// (kept without a leading `r#`), or else the service default. Each call
    /// gets its own metadata with its own path: several spellings of a
    /// segment read as one Rust name, and a server answers only its own.
    pub(crate) fn wrapped_access(&self, path: &str) -> WrappedAccess {
        let Some(segment) = path
            .strip_prefix('/')
            .and_then(|rest| rest.strip_prefix(self.name))
            .and_then(|rest| rest.strip_prefix('/'))
        else {
            return WrappedAccess::NoMethod;
        };
        if segment.is_empty() || segment.contains('/') {
            return WrappedAccess::NoMethod;
        }
        let name = rust_name(segment);
        let name = name.strip_prefix("r#").unwrap_or(&name);
        let template = match (self.methods.get(name), &self.default) {
            (Some(method), _) => &method.template,
            (None, Some(default)) => default,
            (None, None) => return WrappedAccess::Undeclared,
        };
        let mut meta = template.clone();
        meta.full_path = path.to_owned();
        WrappedAccess::Declared(Arc::new(meta))
    }

    /// The metadata of `method`, its full path set from the call's.
    fn meta(&self, method: &str, path: &str) -> Option<Arc<RouteMeta>> {
        let runtime = self.methods.get(method)?;
        let meta = runtime.resolved.get_or_init(|| {
            let mut meta = runtime.template.clone();
            meta.full_path = path.to_owned();
            Arc::new(meta)
        });
        Some(meta.clone())
    }
}

/// One method at run time.
pub(crate) struct MethodRuntime {
    template: RouteMeta,
    resolved: OnceLock<Arc<RouteMeta>>,
}

/// What a call to a wrapped tonic server may do, by the method in its path.
pub(crate) enum WrappedAccess {
    /// The method's own declaration, or the service default.
    Declared(Arc<RouteMeta>),
    /// Neither: the call is refused before the server sees it.
    Undeclared,
    /// The path names no method (an empty segment, or one holding `/`).
    NoMethod,
}

/// The call's state, inserted into the request extensions by the edge.
#[derive(Clone)]
pub(crate) struct CallInfo {
    pub(crate) runtime: Arc<Runtime>,
    pub(crate) service: Arc<ServiceRuntime>,
    pub(crate) path: Arc<str>,
    /// When the method must have returned: the client's `grpc-timeout`, up
    /// to `grpc.timeout_secs`.
    pub(crate) deadline: Instant,
    /// Set when the call pipeline takes over ([`call`], or [`call_as`] for a
    /// wrapped tonic server): from then on it keeps the deadline, and rolls
    /// back what the method opened.
    pub(crate) in_the_pipeline: Arc<AtomicBool>,
}

/// The error of a call: an [`AppError`] (mapped to a status) or a
/// [`Status`] (sent as it is). `?` converts both.
#[derive(Debug)]
#[non_exhaustive]
pub enum CallError {
    /// An app error, sent as the status of the same meaning.
    App(AppError),
    /// A gRPC status, sent as it is.
    Status(Status),
}

impl From<AppError> for CallError {
    fn from(error: AppError) -> Self {
        CallError::App(error)
    }
}

impl From<Status> for CallError {
    fn from(status: Status) -> Self {
        CallError::Status(status)
    }
}

impl CallError {
    fn is_forbidden(&self) -> bool {
        match self {
            CallError::App(error) => error.status == StatusCode::FORBIDDEN,
            CallError::Status(status) => status.code() == tonic::Code::PermissionDenied,
        }
    }
}

/// A request message's record id for `can(.., from = "id")`: strings and
/// integers, optional or not. `None` (an empty or missing id) is an
/// `INVALID_ARGUMENT`.
pub trait RecordKey {
    /// The id as a string, if there is one.
    fn record_key(&self) -> Option<String>;
}

impl RecordKey for String {
    fn record_key(&self) -> Option<String> {
        (!self.is_empty()).then(|| self.clone())
    }
}

impl RecordKey for str {
    fn record_key(&self) -> Option<String> {
        (!self.is_empty()).then(|| self.to_owned())
    }
}

macro_rules! integer_keys {
    ($($ty:ty),*) => {$(
        impl RecordKey for $ty {
            fn record_key(&self) -> Option<String> {
                Some(self.to_string())
            }
        }
    )*};
}

integer_keys!(i32, i64, u32, u64);

impl<T: RecordKey> RecordKey for Option<T> {
    fn record_key(&self) -> Option<String> {
        self.as_ref().and_then(RecordKey::record_key)
    }
}

/// Reads the record id of a request message.
pub type RecordKeyFn<Req> = fn(&Req) -> Option<String>;

/// Runs one call of `method` (its Rust name) through the pipeline, then
/// `body` (the extractors and the method itself) inside the unit of work.
pub async fn call<Req, Res, F, Fut>(
    request: Request<Req>,
    method: &'static str,
    record_key: Option<RecordKeyFn<Req>>,
    body: F,
) -> Result<Response<Res>, Status>
where
    Req: Send,
    F: FnOnce(Request<Req>, Parts) -> Fut + Send,
    Fut: Future<Output = Result<Response<Res>, CallError>> + Send,
{
    let Some(info) = request.extensions().get::<CallInfo>().cloned() else {
        return Err(Status::internal(
            "this service is not served by GrpcModule: register it with m.grpc_service::<T>()",
        ));
    };
    info.in_the_pipeline.store(true, Ordering::Relaxed);
    let Some(ctx) = info.runtime.ctx.upgrade() else {
        return Err(Status::unavailable("the server is shutting down"));
    };
    let Some(meta) = info.service.meta(method, &info.path) else {
        return Err(Status::internal(format!(
            "`{method}` is not a method of {}",
            info.service.name
        )));
    };
    call_as(&info, &ctx, meta, request, record_key, body).await
}

/// Runs one call through the pipeline with the metadata `meta` its caller
/// found (the method's declaration), then `body` inside the unit of work,
/// all by the call's deadline.
pub(crate) async fn call_as<Req, Res, F, Fut>(
    info: &CallInfo,
    ctx: &AppContext,
    meta: Arc<RouteMeta>,
    request: Request<Req>,
    record_key: Option<RecordKeyFn<Req>>,
    body: F,
) -> Result<Response<Res>, Status>
where
    Req: Send,
    F: FnOnce(Request<Req>, Parts) -> Fut + Send,
    Fut: Future<Output = Result<Response<Res>, CallError>> + Send,
{
    let request_id = request.extensions().get::<RequestId>().cloned();
    let opened: Arc<Mutex<Option<Uow>>> = Arc::default();
    let run = run(info, ctx, meta, request, record_key, body, opened.clone());
    let result = match tokio::time::timeout_at(info.deadline, run).await {
        Ok(result) => result,
        Err(_) => {
            let uow = opened.lock().unwrap_or_else(PoisonError::into_inner).take();
            if let Some(uow) = uow
                && let Err(error) = uow.rollback().await
            {
                tracing::warn!(target: "navree::grpc", %error, "rolling back after the deadline failed");
            }
            tracing::warn!(
                target: "navree::grpc",
                path = %info.path,
                request_id = request_id.as_ref().map(tracing::field::display),
                "the call's deadline passed"
            );
            Err(CallError::Status(Status::deadline_exceeded(
                "Deadline exceeded",
            )))
        }
    };
    result.map_err(|error| info.runtime.status_of(error, request_id.as_ref()))
}

/// Authentication, the access check, guards, the unit of work and the
/// method.
async fn run<Req, Res, F, Fut>(
    info: &CallInfo,
    ctx: &AppContext,
    meta: Arc<RouteMeta>,
    request: Request<Req>,
    record_key: Option<RecordKeyFn<Req>>,
    body: F,
    opened: Arc<Mutex<Option<Uow>>>,
) -> Result<Response<Res>, CallError>
where
    F: FnOnce(Request<Req>, Parts) -> Fut,
    Fut: Future<Output = Result<Response<Res>, CallError>>,
{
    let runtime = &info.runtime;
    let (metadata, mut extensions, message) = request.into_parts();
    extensions.insert(meta.clone());
    extensions.insert(ctx.clone());
    let mut parts = http::Request::new(()).into_parts().0;
    parts.method = Method::POST;
    parts.uri = info.path.parse::<Uri>().unwrap_or_default();
    parts.headers = metadata.clone().into_headers();
    parts.extensions = extensions;

    // Authentication (the edge does not authenticate; an in-process caller
    // may have).
    if parts.extensions.get::<AuthState>().is_none() {
        let principal = match &runtime.authenticator {
            Some(authenticator) => authenticator.authenticate(&mut parts).await?,
            None => None,
        };
        parts.extensions.insert(AuthState(principal));
    }
    let user = parts
        .extensions
        .get::<AuthState>()
        .and_then(|state| state.0.as_ref())
        .map(|principal| principal.user_id);
    if let Some(user) = user {
        tracing::Span::current().record("user_id", tracing::field::display(user));
    }
    if !meta.access.public && user.is_none() {
        return Err(AppError::unauthenticated().into());
    }

    // The unit of work (lazy: a transaction opens on first use).
    let uow = match &runtime.db {
        Some(db) => {
            let request_id = parts
                .extensions
                .get::<RequestId>()
                .map(|id| id.as_str().to_owned());
            let uow = db.uow(user, request_id).await;
            parts.extensions.insert(uow.clone());
            *opened.lock().unwrap_or_else(PoisonError::into_inner) = Some(uow.clone());
            Some(uow)
        }
        None => None,
    };

    let admitted = admit(runtime, &meta, &mut parts, &message, record_key).await;
    if let Err(error) = admitted {
        rollback(uow.as_ref()).await;
        return Err(error);
    }

    let denied = runtime
        .audit
        .as_ref()
        .map(|_| security_entry(&parts, &meta, "handler"));
    let context = CallContext::of(
        &parts.headers,
        parts.extensions.get::<RequestId>(),
        Some(info.deadline),
    );
    let mut request_extensions = parts.extensions.clone();
    request_extensions.remove::<Uow>();
    let request = Request::from_parts(metadata, request_extensions, message);
    let handled = context.scope(body(request, parts));
    let result = match &uow {
        Some(uow) => uow.scope(handled).await,
        None => handled.await,
    };
    match (result, uow) {
        (Ok(response), Some(uow)) => {
            opened.lock().unwrap_or_else(PoisonError::into_inner).take();
            uow.commit().await.map_err(AppError::internal)?;
            Ok(response)
        }
        (Err(error), uow) => {
            rollback(uow.as_ref()).await;
            if let (Some(entry), Some(sink), true) = (denied, &runtime.audit, error.is_forbidden())
            {
                record_security(sink.as_ref(), entry).await;
            }
            Err(error)
        }
        (Ok(response), None) => Ok(response),
    }
}

/// Guards, then the route authorizer (roles, permissions, `can`).
async fn admit<Req>(
    runtime: &Runtime,
    meta: &RouteMeta,
    parts: &mut Parts,
    message: &Req,
    record_key: Option<RecordKeyFn<Req>>,
) -> Result<(), CallError> {
    for guard in &meta.guards {
        if let Err(error) = guard.check(parts).await {
            if error.status == StatusCode::FORBIDDEN
                && let Some(sink) = &runtime.audit
            {
                tracing::warn!(
                    target: "navree::security",
                    route = %meta.full_path,
                    "access denied by a guard"
                );
                record_security(sink.as_ref(), security_entry(parts, meta, "guard")).await;
            }
            return Err(error.into());
        }
    }
    if !meta.access.needs_authorizer() {
        return Ok(());
    }
    let authorizer = runtime.authorizer.as_ref().ok_or_else(|| {
        AppError::internal("the method declares roles, permissions or can, but no dyn RouteAuthorizer is registered")
    })?;
    // `can(.., from = "id")`: the record id goes, as it is, where the route
    // authorizer (and `Path`) read path parameters.
    if let Some(param) = meta.access.can.as_ref().and_then(|can| can.from.as_deref()) {
        let id = record_key
            .and_then(|key| key(message))
            .ok_or_else(|| Status::invalid_argument(format!("{param} is required")))?;
        parts
            .extensions
            .insert(PathParams::from_pairs([(param, id.as_str())]));
    }
    authorizer.authorize(parts, meta).await?;
    Ok(())
}

async fn rollback(uow: Option<&Uow>) {
    if let Some(uow) = uow
        && let Err(error) = uow.rollback().await
    {
        tracing::warn!(target: "navree::grpc", %error, "rolling back the unit of work failed");
    }
}

/// An `access.denied` security entry about this call.
fn security_entry(parts: &Parts, meta: &RouteMeta, check: &str) -> AuditEntry {
    let mut entry =
        AuditEntry::new(AuditKind::Security, "access.denied").metadata(serde_json::json!({
            "check": check,
            "method": meta.method,
            "route": meta.full_path,
        }));
    if let Some(user) = parts
        .extensions
        .get::<AuthState>()
        .and_then(|state| state.0.as_ref())
    {
        entry = entry.actor(user.user_id);
    }
    if let Some(request_id) = parts.extensions.get::<RequestId>() {
        entry = entry.request_id(request_id.as_str());
    }
    if let Some(ClientIp(ip)) = parts.extensions.get::<ClientIp>() {
        entry = entry.ip(ip.to_string());
    }
    if let Some(agent) = parts.headers.get(USER_AGENT).and_then(|v| v.to_str().ok()) {
        entry = entry.user_agent(agent);
    }
    entry
}

/// The most bytes of a refused call's path that its log line and its audit
/// entry keep.
const UNDECLARED_PATH_BYTES: usize = 128;

/// At most one audit entry per client in this window.
const UNDECLARED_AUDIT_WINDOW: Duration = Duration::from_secs(60);

/// The most clients [`UndeclaredAudits`] remembers at once, and so the most
/// refusals of undeclared methods audited in a minute.
const UNDECLARED_AUDIT_ADDRESSES: usize = 4096;

/// Records a refused call to a wrapped server's method that has no
/// declaration. Every such call logs a warning on `navree::security`. When
/// the app has an audit sink, the first one from a client in a minute is
/// also an `access.denied` security entry (`check` `undeclared`), as a
/// guard's refusal records, written on the app's task tracker, so the
/// refusal never waits for the sink. A client is its address, or for IPv6
/// its network of `http.rate_limit_ipv6_prefix` bits, as the app's other
/// per-address limits count it. Both keep at most the first 128 bytes of the
/// path. The call is refused before authentication, so the entry names no
/// user.
pub(crate) fn record_undeclared(runtime: &Runtime, parts: &Parts, path: &str) {
    let path = cut(path, UNDECLARED_PATH_BYTES);
    tracing::warn!(
        target: "navree::security",
        path,
        request_id = parts
            .extensions
            .get::<RequestId>()
            .map(tracing::field::display),
        "a call to a method without an access declaration was refused"
    );
    let Some(sink) = &runtime.audit else {
        return;
    };
    let key = client_key(parts, runtime.ipv6_prefix);
    if !runtime.undeclared_audits.admit(key, Instant::now()) {
        return;
    }
    let Some(ctx) = runtime.ctx.upgrade() else {
        return;
    };
    let meta = RouteMeta {
        method: "GRPC".to_owned(),
        full_path: path.to_owned(),
        ..RouteMeta::default()
    };
    let entry = security_entry(parts, &meta, "undeclared");
    let sink = sink.clone();
    ctx.tracker().spawn(async move {
        record_security(sink.as_ref(), entry).await;
    });
}

/// The key a call's client is counted under, as the app's other per-address
/// limits count it: its address (`0.0.0.0` when unknown), or for IPv6 its
/// network of `ipv6_prefix` bits.
fn client_key(parts: &Parts, ipv6_prefix: u8) -> IpAddr {
    let ip = parts
        .extensions
        .get::<ClientIp>()
        .map_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED), |ClientIp(ip)| *ip);
    rate_limit_key(ip, ipv6_prefix)
}

/// `text` cut to at most `max` bytes, at a character boundary.
fn cut(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// The clients whose refused call to an undeclared method was audited in the
/// last minute, each by its rate-limit key (an address, or an IPv6 network):
/// at most one entry per client a minute, and at most
/// [`UNDECLARED_AUDIT_ADDRESSES`] clients remembered. While that many are
/// remembered and none is a minute old, a refusal from a new client is
/// logged and not audited.
#[derive(Default)]
pub(crate) struct UndeclaredAudits(Mutex<AuditWindow>);

#[derive(Default)]
struct AuditWindow {
    /// When each client's last entry was recorded.
    seen: HashMap<IpAddr, Instant>,
    /// While the map is full: no client expires before this instant, so no
    /// sweep is needed until then.
    full_until: Option<Instant>,
}

impl UndeclaredAudits {
    /// Whether a refused call from the client keyed `key` at `now` is
    /// audited: the first one of the client's minute, while there is room for
    /// it.
    pub(crate) fn admit(&self, key: IpAddr, now: Instant) -> bool {
        let mut window = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        window.admit(key, now, UNDECLARED_AUDIT_ADDRESSES)
    }
}

impl AuditWindow {
    fn admit(&mut self, key: IpAddr, now: Instant, cap: usize) -> bool {
        let fresh = |at: Instant| now.duration_since(at) < UNDECLARED_AUDIT_WINDOW;
        if let Some(at) = self.seen.get_mut(&key) {
            if fresh(*at) {
                return false;
            }
            *at = now;
            return true;
        }
        if self.seen.len() >= cap {
            if self.full_until.is_some_and(|until| now < until) {
                return false;
            }
            self.seen.retain(|_, at| fresh(*at));
            if self.seen.len() >= cap {
                self.full_until = self
                    .seen
                    .values()
                    .min()
                    .map(|oldest| *oldest + UNDECLARED_AUDIT_WINDOW);
                return false;
            }
            self.full_until = None;
        }
        self.seen.insert(key, now);
        true
    }
}

async fn record_security(sink: &dyn AuditSink, entry: AuditEntry) {
    if let Err(error) = sink.record(entry).await {
        tracing::warn!(target: "navree::audit", %error, "could not write a security audit entry");
    }
}

/// A method that takes a stream is starting: its body is a conversation, and
/// sits silent between messages, so the pace of a request body no longer
/// applies. Runs after the access check, and only for such a method: before
/// that the body is an upload like any other, and a unary request is read
/// whole before tonic calls the method, so it never gets here unread.
pub fn lift_body_limit(parts: &Parts) {
    if let Some(limit) = parts.extensions.get::<BodyReadLimit>() {
        limit.lift();
    }
}

/// Runs one extractor on the call's request parts: any
/// `navree_http::extract::FromRequestParts`; `M` is inferred. It returns the
/// extractor's own future (no async wrapper), so a call's future nests no
/// deeper than the extractor's own.
pub fn extract<T, M>(parts: &mut Parts) -> impl Future<Output = Result<T, CallError>> + Send
where
    T: Extract<M>,
{
    T::extract(parts)
}

/// An extractor [`extract`] runs.
pub trait Extract<M>: Sized {
    /// Runs the extractor.
    fn extract(parts: &mut Parts) -> impl Future<Output = Result<Self, CallError>> + Send;
}

/// Navree's extractors.
#[derive(Debug)]
pub enum ViaNavree {}

impl<T> Extract<ViaNavree> for T
where
    T: FromRequestParts,
    T::Rejection: Into<CallError>,
{
    async fn extract(parts: &mut Parts) -> Result<Self, CallError> {
        T::from_request_parts(parts).await.map_err(Into::into)
    }
}

/// Lets an extractor whose rejection is an [`AppError`] (every Navree
/// extractor) or never fails run in [`extract`].
impl From<std::convert::Infallible> for CallError {
    fn from(never: std::convert::Infallible) -> Self {
        match never {}
    }
}

/// `Valid<Request<T>>`: runs the rules of the message; a broken rule
/// is `INVALID_ARGUMENT` with a `google.rpc.BadRequest` detail.
pub async fn validate<T>(
    request: Request<T>,
    parts: &Parts,
) -> Result<navree_validation::Valid<Request<T>>, CallError>
where
    T: navree_validation::Validate,
{
    let ctx = parts.extensions.get::<AppContext>();
    match navree_validation::validate_in(request.get_ref(), ctx).await {
        Ok(()) => Ok(navree_validation::Valid(request)),
        Err(errors) => Err(errors.into_app_error().into()),
    }
}

/// Fixes the output type of the method's body (so `?` in it converts to
/// the declared error type).
pub fn returns<T, F: Future<Output = T>>(future: F) -> F {
    future
}

/// The method's result as the pipeline's.
pub fn finish<Res, E: Into<CallError>>(
    result: Result<Response<Res>, E>,
) -> Result<Response<Res>, CallError> {
    result.map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The deals service with `get_deal` public, `type` for signed-in
    /// callers, and `default` for every other method.
    fn deals_service(default: Option<RouteMeta>) -> ServiceRuntime {
        let module = ModuleId::named::<ServiceRuntime>("DealsModule");
        let methods = vec![
            GrpcMethod::new("get_deal").public(),
            GrpcMethod::new("type").authenticated(),
        ];
        ServiceRuntime::new("crm.deals.v1.Deals", module, "DealsModule", methods).with_default(
            default,
            module,
            "DealsModule",
        )
    }

    fn declared(access: WrappedAccess) -> Arc<RouteMeta> {
        match access {
            WrappedAccess::Declared(meta) => meta,
            WrappedAccess::Undeclared => panic!("expected a declaration, got none"),
            WrappedAccess::NoMethod => panic!("expected a declaration, got no method"),
        }
    }

    #[test]
    fn a_wrapped_service_finds_a_call_by_the_rust_name_of_its_method() {
        let mut signed_in = RouteMeta {
            method: "GRPC".into(),
            ..RouteMeta::default()
        };
        signed_in.access.authenticated = true;
        let deals = deals_service(Some(signed_in));

        let get = declared(deals.wrapped_access("/crm.deals.v1.Deals/GetDeal"));
        assert!(get.access.public && !get.access.authenticated, "its own");
        assert_eq!(get.full_path, "/crm.deals.v1.Deals/GetDeal");
        assert_eq!(get.module, Some("DealsModule"));
        let typed = declared(deals.wrapped_access("/crm.deals.v1.Deals/Type"));
        assert!(
            typed.access.authenticated,
            "`Type` is `r#type`, declared as `type`"
        );

        let list = declared(deals.wrapped_access("/crm.deals.v1.Deals/ListDeals"));
        assert!(
            list.access.authenticated && !list.access.public,
            "the default"
        );
        assert_eq!(list.full_path, "/crm.deals.v1.Deals/ListDeals");
        assert_eq!(list.module, Some("DealsModule"));

        for path in [
            "/crm.deals.v1.Deals/",
            "/crm.deals.v1.Deals/Get/Deal",
            "/crm.deals.v1.Deals",
        ] {
            assert!(
                matches!(deals.wrapped_access(path), WrappedAccess::NoMethod),
                "{path}"
            );
        }

        let without_default = deals_service(None);
        assert!(matches!(
            without_default.wrapped_access("/crm.deals.v1.Deals/ListDeals"),
            WrappedAccess::Undeclared
        ));
        let get = declared(without_default.wrapped_access("/crm.deals.v1.Deals/GetDeal"));
        assert!(get.access.public);
    }

    #[test]
    fn an_address_is_audited_once_a_minute_and_only_while_there_is_room_for_it() {
        let start = Instant::now();
        let at = |secs: u64| start + Duration::from_secs(secs);
        let address = |n: u32| IpAddr::V4(Ipv4Addr::from(n));
        let mut window = AuditWindow::default();
        assert!(window.admit(address(1), at(0), 2));
        assert!(!window.admit(address(1), at(59), 2), "once a minute");
        assert!(window.admit(address(2), at(1), 2));

        // Two addresses fill it: a third waits until the oldest is a minute
        // old, and takes its place.
        assert!(!window.admit(address(3), at(30), 2));
        assert!(!window.admit(address(3), at(59), 2));
        assert!(window.admit(address(3), at(60), 2));
        assert_eq!(window.seen.len(), 2);
        assert!(window.admit(address(1), at(61), 2), "its minute is over");
        assert_eq!(window.seen.len(), 2);
    }

    #[test]
    fn a_path_is_cut_at_a_character_boundary() {
        assert_eq!(
            cut("/crm.deals.v1.Deals/GetDeal", 128),
            "/crm.deals.v1.Deals/GetDeal"
        );
        assert_eq!(cut("/crm.deals.v1.Deals/GetDeal", 4), "/crm");
        // `₹` is three bytes.
        assert_eq!(cut("₹₹", 4), "₹");
    }

    #[test]
    fn the_addresses_of_one_ipv6_network_are_audited_as_one_client() {
        let from = |address: &str| {
            let (mut parts, ()) = http::Request::new(()).into_parts();
            parts.extensions.insert(ClientIp(address.parse().unwrap()));
            client_key(&parts, 64)
        };
        let start = Instant::now();
        let at = |secs: u64| start + Duration::from_secs(secs);
        let audits = UndeclaredAudits::default();

        // Two addresses of one /64: one entry in the minute.
        assert!(audits.admit(from("2001:db8:1:2::1"), at(0)));
        assert!(!audits.admit(from("2001:db8:1:2:ffff::9"), at(1)));

        // Addresses of two other /64s: one entry each.
        assert!(audits.admit(from("2001:db8:1:3::1"), at(2)));
        assert!(audits.admit(from("2001:db8:1:4::1"), at(3)));
        assert!(!audits.admit(from("2001:db8:1:3::2"), at(4)));
        assert!(!audits.admit(from("2001:db8:1:4::2"), at(5)));

        // Each IPv4 address is a client of its own.
        assert!(audits.admit(from("203.0.113.9"), at(6)));
        assert!(audits.admit(from("203.0.113.10"), at(7)));
    }
}
