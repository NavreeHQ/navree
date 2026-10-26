//! The call pipeline every `#[grpc_service]` method runs through:
//! authentication, the access check, guards, the deadline and the unit of
//! work, then the extractors and the method.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use axum::extract::FromRequestParts;
use http::header::USER_AGENT;
use http::request::Parts;
use http::{Method, StatusCode, Uri};
use navree_contracts::{AuditEntry, AuditKind, AuditSink, AuthState, RequestId};
use navree_db::{Db, Uow};
use navree_http::{AppError, Authenticator, ClientIp, ErrorMapper, RouteAuthorizer, RouteMeta};
use navree_kernel::{AppContext, ModuleId, WeakAppContext};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tonic::{Request, Response, Status};
use tower::ServiceExt;

use crate::context::CallContext;
use crate::service::GrpcMethod;
use crate::status::status_from_error;

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
                let record = meta
                    .access
                    .can
                    .as_ref()
                    .and_then(|can| can.from.clone())
                    .map(|param| record_router(&param));
                (
                    method.name,
                    MethodRuntime {
                        template: meta,
                        resolved: OnceLock::new(),
                        record,
                    },
                )
            })
            .collect();
        Self {
            name,
            methods,
        }
    }

    /// The metadata of `method`, its full path set from the call's.
    fn meta(&self, method: &str, path: &str) -> Option<(Arc<RouteMeta>, Option<&axum::Router>)> {
        let runtime = self.methods.get(method)?;
        let meta = runtime
            .resolved
            .get_or_init(|| {
                let mut meta = runtime.template.clone();
                meta.full_path = path.to_owned();
                Arc::new(meta)
            })
            .clone();
        Some((meta, runtime.record.as_ref()))
    }
}

/// One method at run time.
pub(crate) struct MethodRuntime {
    template: RouteMeta,
    resolved: OnceLock<Arc<RouteMeta>>,
    /// For `can(.., from = "id")`: a router that puts the record id where
    /// the route authorizer reads it (the path parameter `id`).
    record: Option<axum::Router>,
}

/// The call's state, inserted into the request extensions by the edge.
#[derive(Clone)]
pub(crate) struct CallInfo {
    pub(crate) runtime: Arc<Runtime>,
    pub(crate) service: Arc<ServiceRuntime>,
    pub(crate) path: Arc<str>,
    /// When the method must have returned: the client's `grpc-timeout`, or
    /// `grpc.timeout_secs`.
    pub(crate) deadline: Instant,
}

/// The error of a call: an [`AppError`] (mapped to a status) or a
/// [`Status`] (sent as it is). `?` converts both.
#[derive(Debug)]
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
    let Some(ctx) = info.runtime.ctx.upgrade() else {
        return Err(Status::unavailable("the server is shutting down"));
    };
    let Some((meta, record)) = info.service.meta(method, &info.path) else {
        return Err(Status::internal(format!(
            "`{method}` is not a method of {}",
            info.service.name
        )));
    };
    let request_id = request.extensions().get::<RequestId>().cloned();
    let opened: Arc<Mutex<Option<Uow>>> = Arc::default();
    let run = run(
        &info,
        &ctx,
        meta.clone(),
        record,
        request,
        record_key,
        body,
        opened.clone(),
    );
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
            Err(CallError::Status(Status::deadline_exceeded("Deadline exceeded")))
        }
    };
    result.map_err(|error| info.runtime.status_of(error, request_id.as_ref()))
}

/// Authentication, the access check, guards, the unit of work and the
/// method.
#[allow(clippy::too_many_arguments)]
async fn run<Req, Res, F, Fut>(
    info: &CallInfo,
    ctx: &AppContext,
    meta: Arc<RouteMeta>,
    record: Option<&axum::Router>,
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

    let admitted = admit(runtime, &meta, record, &mut parts, &message, record_key).await;
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
            if let (Some(entry), Some(sink), true) = (denied, &runtime.audit, error.is_forbidden()) {
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
    record: Option<&axum::Router>,
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
    if let (Some(router), Some(can)) = (record, &meta.access.can) {
        let param = can.from.as_deref().unwrap_or("id");
        let id = record_key
            .and_then(|key| key(message))
            .ok_or_else(|| Status::invalid_argument(format!("{param} is required")))?;
        put_record_id(parts, router, &id).await?;
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

/// A router whose only route is `/{param}`: routing a request through it
/// stores the record id where `Path` extractors (and navree-access's route
/// authorizer) read path parameters.
fn record_router(param: &str) -> axum::Router {
    axum::Router::new().route(
        &format!("/{{{param}}}"),
        axum::routing::any(|request: axum::extract::Request| async move {
            let (mut parts, _) = request.into_parts();
            if let Some(slot) = parts.extensions.remove::<PartsSlot>() {
                *slot.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(parts);
            }
            StatusCode::NO_CONTENT
        }),
    )
}

/// Carries the request parts out of [`record_router`]'s handler.
#[derive(Clone, Default)]
struct PartsSlot(Arc<Mutex<Option<Parts>>>);

/// Characters escaped in a record id put in a path segment.
const SEGMENT: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'.').remove(b'~');

async fn put_record_id(parts: &mut Parts, router: &axum::Router, id: &str) -> Result<(), CallError> {
    let path = std::mem::take(&mut parts.uri);
    let slot = PartsSlot::default();
    let mut moved = std::mem::replace(parts, http::Request::new(()).into_parts().0);
    moved.uri = format!("/{}", utf8_percent_encode(id, SEGMENT))
        .parse()
        .map_err(|_| Status::invalid_argument("the record id is not valid"))?;
    moved.extensions.insert(slot.clone());
    let request = http::Request::from_parts(moved, axum::body::Body::empty());
    let routed = router.clone().oneshot(request).await;
    let taken = slot.0.lock().unwrap_or_else(PoisonError::into_inner).take();
    match (routed, taken) {
        (Ok(_), Some(mut routed)) => {
            routed.uri = path;
            *parts = routed;
            Ok(())
        }
        _ => Err(AppError::internal("the record id could not be routed").into()),
    }
}

/// An `access.denied` security entry about this call.
fn security_entry(parts: &Parts, meta: &RouteMeta, check: &str) -> AuditEntry {
    let mut entry = AuditEntry::new(AuditKind::Security, "access.denied").metadata(serde_json::json!({
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

async fn record_security(sink: &dyn AuditSink, entry: AuditEntry) {
    if let Err(error) = sink.record(entry).await {
        tracing::warn!(target: "navree::audit", %error, "could not write a security audit entry");
    }
}

/// Runs one Navree extractor on the call's request parts.
pub async fn extract<T>(parts: &mut Parts) -> Result<T, CallError>
where
    T: FromRequestParts<()>,
    T::Rejection: Into<CallError>,
{
    T::from_request_parts(parts, &()).await.map_err(Into::into)
}

/// Lets an extractor whose rejection is an [`AppError`] (every Navree
/// extractor) or never fails run in [`extract`].
impl From<std::convert::Infallible> for CallError {
    fn from(never: std::convert::Infallible) -> Self {
        match never {}
    }
}

/// `Valid<Request<T>>`: runs the garde rules of the message; a broken rule
/// is `INVALID_ARGUMENT` with a `google.rpc.BadRequest` detail.
pub async fn validate<T>(
    request: Request<T>,
    parts: &Parts,
) -> Result<navree_validation::Valid<Request<T>>, CallError>
where
    T: garde::Validate<Context = ()>,
{
    let ctx = parts.extensions.get::<AppContext>();
    match navree_validation::validate(request.get_ref(), ctx).await {
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
