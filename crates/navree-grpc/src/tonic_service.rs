//! [`TonicService`] and [`ModuleBuilderTonicExt`]: a tonic server built
//! elsewhere, served by `GrpcModule` behind the call pipeline of
//! `#[grpc_service]` methods; and the Rust name of a proto method, by which
//! a call finds its declaration (`GetDeal` is `get_deal`).

use std::collections::HashSet;
use std::convert::Infallible;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::task::{Context, Poll};

use http::StatusCode;
use navree_contracts::RequestId;
use navree_http::{AppError, CanSpec, Guard, RouteMeta};
use navree_kernel::{BoxFuture, ModuleBuilder};
use tonic::metadata::MetadataMap;
use tonic::server::NamedService;
use tonic::{Code, Status};
use tower::{Service, ServiceExt};

use crate::Routes;
use crate::call::{CallError, CallInfo, WrappedAccess, call_as, lift_body_limit};
use crate::server::access_problems;
use crate::service::GrpcMethod;
use crate::status::status_from_error;

/// What declares a wrapped server's access, in its boot texts.
const DECLARE: &str = ".public(), .authenticated(), .roles(..), .permissions(..) or .can(..)";

/// A tonic server built elsewhere (a crate's generated `XServer<T>`, such as
/// `DealsServer`), served by `GrpcModule` with Navree's access checks,
/// guards and unit of work.
///
/// The access check runs before the server reads a message; a method without
/// a declaration of its own and without a service default is refused with
/// `PERMISSION_DENIED`. `grpc.max_message_bytes` does not reach a server
/// built elsewhere: set its limits before wrapping it. The server's methods
/// reach the app's providers through the `AppContext` in each request's
/// extensions.
///
/// Each call runs the pipeline of a `#[grpc_service]` method before the
/// server sees it: authentication, the access check of its method (or of
/// the service default), guards, the route authorizer, the deadline, then
/// the server inside a unit of work. The unit of work commits when the
/// server's response head is HTTP 200 with no error `grpc-status`, and rolls
/// back otherwise; a method that answers with a stream commits when its head
/// is sent, and its messages run outside it. Once a call is let in, its
/// request body is not held to the read pace of an upload, so a client or
/// two-way stream may sit silent between messages.
///
/// A service whose access cannot be honoured fails the boot: no declaration
/// at all, a method of its own without access, a method declared twice or
/// under another name than the generated server trait spells (`GetDeal` for
/// `get_deal`), a method whose `can` reads a field of the message, and a
/// service named like `GrpcModule`'s own health or reflection service while
/// that one is on.
///
/// ```
/// # use std::convert::Infallible;
/// # use std::sync::Arc;
/// # use std::task::{Context, Poll};
/// # trait Deals: Send + Sync + 'static {}
/// # struct DealsServer<T>(Arc<T>);
/// # impl<T: Deals> DealsServer<T> {
/// #     fn new(inner: T) -> Self {
/// #         Self(Arc::new(inner))
/// #     }
/// # }
/// # impl<T> Clone for DealsServer<T> {
/// #     fn clone(&self) -> Self {
/// #         Self(self.0.clone())
/// #     }
/// # }
/// # impl<T> tonic::server::NamedService for DealsServer<T> {
/// #     const NAME: &'static str = "crm.deals.v1.Deals";
/// # }
/// # impl<T: Deals> tower::Service<http::Request<tonic::body::Body>> for DealsServer<T> {
/// #     type Response = http::Response<tonic::body::Body>;
/// #     type Error = Infallible;
/// #     type Future = std::future::Ready<Result<Self::Response, Infallible>>;
/// #     fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
/// #         Poll::Ready(Ok(()))
/// #     }
/// #     fn call(&mut self, _: http::Request<tonic::body::Body>) -> Self::Future {
/// #         std::future::ready(Ok(tonic::Status::unimplemented("").into_http()))
/// #     }
/// # }
/// # struct KhataDeals;
/// # impl Deals for KhataDeals {}
/// use navree_grpc::{GrpcMethod, ModuleBuilderTonicExt, TonicService};
/// use navree_kernel::{Module, ModuleBuilder};
///
/// // `DealsServer` is generated from the proto; `KhataDeals` is a crate's own
/// // implementation of the generated `Deals` trait, without `#[grpc_service]`.
/// struct DealsModule;
///
/// impl Module for DealsModule {
///     fn register(&self, m: &mut ModuleBuilder) {
///         // Every method needs a signed-in caller, except `GetDeal`.
///         m.tonic_service(
///             TonicService::new(DealsServer::new(KhataDeals))
///                 .authenticated()
///                 .method(GrpcMethod::new("get_deal").public()),
///         );
///     }
/// }
/// ```
#[derive(Clone)]
pub struct TonicService<S> {
    server: S,
    default: Option<RouteMeta>,
    methods: Vec<GrpcMethod>,
}

impl<S> TonicService<S>
where
    S: Service<
            http::Request<tonic::body::Body>,
            Response = http::Response<tonic::body::Body>,
            Error = Infallible,
        > + NamedService
        + Clone
        + Send
        + Sync
        + 'static,
    S::Future: Send + 'static,
{
    /// `server` with no access declared yet: give it a service default
    /// ([`public`](Self::public), [`authenticated`](Self::authenticated),
    /// ...) or give each method its own ([`method`](Self::method)), or the
    /// boot fails.
    pub fn new(server: S) -> TonicService<S> {
        TonicService {
            server,
            default: None,
            methods: Vec::new(),
        }
    }

    /// Every method without its own declaration needs no authentication or
    /// authorization.
    pub fn public(mut self) -> Self {
        self.default_meta().access.public = true;
        self
    }

    /// Every method without its own declaration needs a signed-in caller.
    pub fn authenticated(mut self) -> Self {
        self.default_meta().access.authenticated = true;
        self
    }

    /// Every method without its own declaration needs any of `roles`.
    pub fn roles(mut self, roles: &[&str]) -> Self {
        self.default_meta()
            .access
            .roles
            .extend(roles.iter().map(|role| (*role).to_owned()));
        self
    }

    /// Every method without its own declaration needs all of `permissions`.
    pub fn permissions(mut self, permissions: &[&str]) -> Self {
        self.default_meta().access.permissions.extend(
            permissions
                .iter()
                .map(|permission| (*permission).to_owned()),
        );
        self
    }

    /// Every method without its own declaration needs the ability `action`
    /// on `subject` (a type-level check: a wrapped server's messages are
    /// not read before it runs).
    pub fn can(mut self, action: &str, subject: &str) -> Self {
        self.default_meta().access.can = Some(CanSpec {
            action: action.to_owned(),
            subject: subject.to_owned(),
            from: None,
        });
        self
    }

    /// Adds a guard to every method without its own declaration, run before
    /// authorization and the server. A guard declares no access: the
    /// service still needs one of the declarations above.
    pub fn guard(mut self, guard: Arc<dyn Guard>) -> Self {
        self.default_meta().guards.push(guard);
        self
    }

    /// One method's own access and guards, in place of the service
    /// default's. Name the method as the generated server trait spells it
    /// (`get_deal`); another spelling (`GetDeal`) fails the boot. Give the
    /// service default the strictest access and loosen it per method: a
    /// name that matches no method cannot be detected. A leading `r#` is
    /// set aside (`r#type` and `type` are one method).
    pub fn method(mut self, mut method: GrpcMethod) -> Self {
        method.name = method.name.strip_prefix("r#").unwrap_or(method.name);
        self.methods.push(method);
        self
    }
}

impl<S> TonicService<S> {
    /// The service default, made on first use.
    fn default_meta(&mut self) -> &mut RouteMeta {
        self.default.get_or_insert_with(|| RouteMeta {
            method: "GRPC".to_owned(),
            ..RouteMeta::default()
        })
    }
}

/// Wrapped tonic servers on a [`ModuleBuilder`].
///
/// ```
/// # use std::convert::Infallible;
/// # use std::task::{Context, Poll};
/// # #[derive(Clone)]
/// # struct DealsServer;
/// # impl tonic::server::NamedService for DealsServer {
/// #     const NAME: &'static str = "crm.deals.v1.Deals";
/// # }
/// # impl tower::Service<http::Request<tonic::body::Body>> for DealsServer {
/// #     type Response = http::Response<tonic::body::Body>;
/// #     type Error = Infallible;
/// #     type Future = std::future::Ready<Result<Self::Response, Infallible>>;
/// #     fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
/// #         Poll::Ready(Ok(()))
/// #     }
/// #     fn call(&mut self, _: http::Request<tonic::body::Body>) -> Self::Future {
/// #         std::future::ready(Ok(tonic::Status::unimplemented("").into_http()))
/// #     }
/// # }
/// use navree_grpc::{ModuleBuilderTonicExt, TonicService};
/// use navree_kernel::ModuleBuilder;
///
/// // `DealsServer`: a tonic server generated from the proto.
/// fn register(m: &mut ModuleBuilder, deals: DealsServer) {
///     m.tonic_service(TonicService::new(deals).roles(&["munshi"]));
/// }
/// ```
pub trait ModuleBuilderTonicExt {
    /// Serves `service`'s server through `GrpcModule`, behind the call
    /// pipeline and the access it declares. `GrpcModule` checks it at boot
    /// (see [`TonicService`]).
    fn tonic_service<S>(&mut self, service: TonicService<S>) -> &mut Self
    where
        S: Service<
                http::Request<tonic::body::Body>,
                Response = http::Response<tonic::body::Body>,
                Error = Infallible,
            > + NamedService
            + Clone
            + Send
            + Sync
            + 'static,
        S::Future: Send + 'static;
}

impl ModuleBuilderTonicExt for ModuleBuilder {
    fn tonic_service<S>(&mut self, service: TonicService<S>) -> &mut Self
    where
        S: Service<
                http::Request<tonic::body::Body>,
                Response = http::Response<tonic::body::Body>,
                Error = Infallible,
            > + NamedService
            + Clone
            + Send
            + Sync
            + 'static,
        S::Future: Send + 'static,
    {
        let TonicService {
            server,
            default,
            methods,
        } = service;
        self.contribute(TonicRegistration {
            name: S::NAME,
            type_name: std::any::type_name::<S>(),
            default,
            methods,
            add: Arc::new(move |routes: Routes| {
                routes.add_service(TonicPipeline {
                    server: server.clone(),
                })
            }),
        })
    }
}

/// Adds a wrapped server, behind its pipeline, to the routes.
type AddTonic = Arc<dyn Fn(Routes) -> Routes + Send + Sync>;

/// A wrapped server registered by a module (a contribution).
#[derive(Clone)]
pub(crate) struct TonicRegistration {
    /// The service name, e.g. `crm.deals.v1.Deals`.
    pub(crate) name: &'static str,
    /// The server's type, for the boot texts.
    pub(crate) type_name: &'static str,
    /// The access and guards of every method without its own.
    pub(crate) default: Option<RouteMeta>,
    /// The methods with their own access, names without a leading `r#`.
    pub(crate) methods: Vec<GrpcMethod>,
    /// Adds the server, behind [`TonicPipeline`], to the routes.
    pub(crate) add: AddTonic,
}

/// Every reason `registration`, registered by `module`, cannot be served as
/// declared. `authenticator` and `authorizer` say whether the app has a
/// `dyn Authenticator` and a `dyn RouteAuthorizer`; `health` and
/// `reflection` whether `GrpcModule` serves its own of each.
pub(crate) fn boot_problems(
    registration: &TonicRegistration,
    module: &str,
    authenticator: bool,
    authorizer: bool,
    health: bool,
    reflection: bool,
) -> Vec<String> {
    let name = registration.name;
    let service = format!("gRPC service `{name}` in `{module}`");
    let mut problems = Vec::new();
    match &registration.default {
        // A guard alone declares no access, as for a route.
        Some(default) if !default.access.is_declared() => {
            problems.push(no_declaration(&service));
        }
        Some(default) => problems.extend(access_problems(
            &service,
            &default.access,
            DECLARE,
            "service",
            authenticator,
            authorizer,
        )),
        None if registration.methods.is_empty() => problems.push(no_declaration(&service)),
        None => {}
    }
    let mut declared = HashSet::new();
    for method in &registration.methods {
        let label = format!(
            "gRPC method `{}` of `{name}` (module `{module}`)",
            method.name
        );
        // A name the generated trait cannot spell matches no call: the
        // method would fall to the service default.
        let rust = rust_name(method.name);
        if method.name.is_empty() || rust.strip_prefix("r#").unwrap_or(&rust) != method.name {
            problems.push(format!(
                "{label} is not a Rust method name: write `{rust}` (the name the generated server trait spells)"
            ));
            continue;
        }
        if !declared.insert(method.name) {
            problems.push(format!(
                "{label} is declared twice in TonicService; declare it once"
            ));
            continue;
        }
        if let Some(CanSpec {
            action,
            subject,
            from: Some(field),
        }) = &method.meta.access.can
        {
            problems.push(format!(
                "{label} checks `{action} {subject}` on the record named by the message's `{field}` field, which a TonicService cannot read: give it a .can(..) without a field, or write the service with #[grpc_service]"
            ));
        }
        problems.extend(access_problems(
            &label,
            &method.meta.access,
            DECLARE,
            "method",
            authenticator,
            authorizer,
        ));
    }
    let own = match name {
        "grpc.health.v1.Health" if health => Some("health"),
        "grpc.reflection.v1.ServerReflection" | "grpc.reflection.v1alpha.ServerReflection"
            if reflection =>
        {
            Some("reflection")
        }
        _ => None,
    };
    if let Some(own) = own {
        problems.push(format!(
            "{service} has the name of GrpcModule's own {own} service; set grpc.{own} = false to serve your own"
        ));
    }
    problems
}

/// The text of a wrapped server with no access declaration.
fn no_declaration(service: &str) -> String {
    format!(
        "{service} has no access declaration: add {DECLARE} to TonicService, or give each method its own with .method(..) (deny by default)"
    )
}

/// A wrapped server behind the call pipeline: the access check, guards, the
/// deadline and the unit of work run on the HTTP request before the server
/// reads a message.
#[derive(Clone)]
struct TonicPipeline<S> {
    server: S,
}

impl<S: NamedService> NamedService for TonicPipeline<S> {
    const NAME: &'static str = S::NAME;
}

impl<S> Service<http::Request<tonic::body::Body>> for TonicPipeline<S>
where
    S: Service<
            http::Request<tonic::body::Body>,
            Response = http::Response<tonic::body::Body>,
            Error = Infallible,
        > + Clone
        + Send
        + Sync
        + 'static,
    S::Future: Send + 'static,
{
    type Response = http::Response<tonic::body::Body>;
    type Error = Infallible;
    type Future = BoxFuture<'static, Result<Self::Response, Infallible>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: http::Request<tonic::body::Body>) -> Self::Future {
        let server = self.server.clone();
        Box::pin(async move { Ok(serve_wrapped(server, request).await) })
    }
}

/// One call to a wrapped server, in this order: the call's declaration by
/// the Rust name of its method (a path that names no method is
/// `UNIMPLEMENTED`, a method with neither its own declaration nor a service
/// default `PERMISSION_DENIED`, and neither reaches the server), then the
/// call pipeline, inside which the request body's read pace is lifted and
/// the server runs. Its head commits the unit of work only when it is
/// HTTP 200 with no error `grpc-status`.
async fn serve_wrapped<S>(
    server: S,
    request: http::Request<tonic::body::Body>,
) -> http::Response<tonic::body::Body>
where
    S: Service<
            http::Request<tonic::body::Body>,
            Response = http::Response<tonic::body::Body>,
            Error = Infallible,
        > + Send
        + 'static,
    S::Future: Send + 'static,
{
    // `GrpcCore` gives every call to a registered service its `CallInfo`;
    // it gives none to a path that names no method.
    let Some(info) = request.extensions().get::<CallInfo>().cloned() else {
        return Status::unimplemented("").into_http();
    };
    let meta = match info.service.wrapped_access(&info.path) {
        WrappedAccess::Declared(meta) => meta,
        WrappedAccess::NoMethod => return Status::unimplemented("").into_http(),
        WrappedAccess::Undeclared => {
            tracing::warn!(
                target: "navree::security",
                path = %info.path,
                request_id = request.extensions().get::<RequestId>().map(tracing::field::display),
                "a call to a method without an access declaration was refused"
            );
            return status_from_error(&AppError::forbidden()).into_http();
        }
    };
    info.in_the_pipeline.store(true, Ordering::Relaxed);
    let Some(ctx) = info.runtime.ctx.upgrade() else {
        return Status::unavailable("the server is shutting down").into_http();
    };
    // The pipeline reads the head; the body, which nothing reads before the
    // server decodes it, goes to the server as it is.
    let (head, body) = request.into_parts();
    let http::request::Parts {
        method,
        uri,
        version,
        headers,
        extensions,
        ..
    } = head;
    let request = tonic::Request::from_parts(MetadataMap::from_headers(headers), extensions, ());
    let result = call_as(
        &info,
        &ctx,
        meta,
        request,
        None,
        move |request, parts| async move {
            // Let in: a wrapped server cannot say which methods take a stream,
            // so no call's body is held to the read pace of an upload from here.
            lift_body_limit(&parts);
            let (metadata, extensions, ()) = request.into_parts();
            let mut forwarded = http::Request::new(body);
            *forwarded.method_mut() = method;
            *forwarded.uri_mut() = uri;
            *forwarded.version_mut() = version;
            *forwarded.headers_mut() = metadata.into_headers();
            *forwarded.extensions_mut() = extensions;
            let response = server
                .oneshot(forwarded)
                .await
                .unwrap_or_else(|never| match never {});
            // The unit of work commits only on HTTP 200 with no error
            // `grpc-status` in the head.
            match Status::from_header_map(response.headers()) {
                Some(status) if status.code() != Code::Ok => Err(CallError::Status(status)),
                _ if response.status() != StatusCode::OK => Err(CallError::Status(Status::new(
                    code_of_http_status(response.status()),
                    format!("HTTP {}", response.status().as_u16()),
                ))),
                _ => Ok(tonic::Response::new(response)),
            }
        },
    )
    .await;
    match result {
        Ok(response) => response.into_inner(),
        Err(status) => status.into_http(),
    }
}

/// The gRPC status a client gives a response head with HTTP status `status`
/// and no `grpc-status`, as the gRPC specification maps it: 400 `INTERNAL`,
/// 401 `UNAUTHENTICATED`, 403 `PERMISSION_DENIED`, 404 `UNIMPLEMENTED`, 429,
/// 502, 503 and 504 `UNAVAILABLE`, anything else `UNKNOWN`.
pub(crate) fn code_of_http_status(status: StatusCode) -> Code {
    match status {
        StatusCode::BAD_REQUEST => Code::Internal,
        StatusCode::UNAUTHORIZED => Code::Unauthenticated,
        StatusCode::FORBIDDEN => Code::PermissionDenied,
        StatusCode::NOT_FOUND => Code::Unimplemented,
        StatusCode::TOO_MANY_REQUESTS
        | StatusCode::BAD_GATEWAY
        | StatusCode::SERVICE_UNAVAILABLE
        | StatusCode::GATEWAY_TIMEOUT => Code::Unavailable,
        _ => Code::Unknown,
    }
}

/// The Rust name prost-build gives the proto method `proto_method`, the name
/// the generated server trait spells: snake case (`GetDeal` is `get_deal`,
/// `GetHTTPStatus` is `get_http_status`), then `r#` before a Rust keyword
/// (`Type` is `r#type`), `_` after `_`, `super`, `self`, `Self`, `extern`
/// and `crate` (`Self` is `self_`), and `_` before a leading digit (`3D` is
/// `_3d`). A name already in that form reads as itself.
pub(crate) fn rust_name(proto_method: &str) -> String {
    sanitize(snake_case(proto_method))
}

/// Snake case as prost-build writes it (heck 0.5's `to_snake_case`): words
/// split on every character that is not alphanumeric and, inside a word,
/// before an upper-case character that follows a lower-case one (a digit
/// keeps the case before it), and before an upper-case character that
/// follows an upper-case one and precedes a lower-case one; each word
/// lower-cased, joined with `_`.
fn snake_case(name: &str) -> String {
    /// The case of the last cased character of the current word.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Mode {
        Boundary,
        Lower,
        Upper,
    }

    let mut out = String::with_capacity(name.len() + 4);
    let mut first_word = true;
    for word in name.split(|c: char| !c.is_alphanumeric()) {
        let mut chars = word.char_indices().peekable();
        let mut start = 0;
        let mut mode = Mode::Boundary;
        while let Some((i, c)) = chars.next() {
            let Some(&(next_i, next)) = chars.peek() else {
                push_word(&mut out, &mut first_word, &word[start..]);
                break;
            };
            let next_mode = if c.is_lowercase() {
                Mode::Lower
            } else if c.is_uppercase() {
                Mode::Upper
            } else {
                mode
            };
            if next_mode == Mode::Lower && next.is_uppercase() {
                push_word(&mut out, &mut first_word, &word[start..next_i]);
                start = next_i;
                mode = Mode::Boundary;
            } else if mode == Mode::Upper && c.is_uppercase() && next.is_lowercase() {
                push_word(&mut out, &mut first_word, &word[start..i]);
                start = i;
                mode = Mode::Boundary;
            } else {
                mode = next_mode;
            }
        }
    }
    out
}

/// Appends `word` lower-cased, after a `_` unless it is the first word.
fn push_word(out: &mut String, first_word: &mut bool, word: &str) {
    if !std::mem::take(first_word) {
        out.push('_');
    }
    let mut chars = word.chars().peekable();
    while let Some(c) = chars.next() {
        // A final capital sigma lower-cases to the final form.
        if c == 'Σ' && chars.peek().is_none() {
            out.push('ς');
        } else {
            out.extend(c.to_lowercase());
        }
    }
}

/// prost-build's `sanitize_identifier`: a name Rust would not take as it is
/// gets `r#` before it, `_` after it or `_` before it.
fn sanitize(name: String) -> String {
    let keyword = matches!(
        name.as_str(),
        "as" | "break"
            | "const"
            | "continue"
            | "else"
            | "enum"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "static"
            | "struct"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "dyn"
            | "abstract"
            | "become"
            | "box"
            | "do"
            | "final"
            | "macro"
            | "override"
            | "priv"
            | "typeof"
            | "unsized"
            | "virtual"
            | "yield"
            | "async"
            | "await"
            | "try"
            | "gen"
    );
    if keyword {
        format!("r#{name}")
    } else if matches!(
        name.as_str(),
        "_" | "super" | "self" | "Self" | "extern" | "crate"
    ) {
        format!("{name}_")
    } else if name.starts_with(|c: char| c.is_numeric()) {
        format!("_{name}")
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proto_method_names_turn_into_rust_names_as_prost_build_writes_them() {
        for (proto, rust) in [
            ("GetDeal", "get_deal"),
            ("ListDeals", "list_deals"),
            ("GetHTTPStatus", "get_http_status"),
            ("XMLHttpRequest", "xml_http_request"),
            ("ListDealsV2", "list_deals_v2"),
            ("Get2FA", "get2_fa"),
            ("Get_Deal", "get_deal"),
            ("ABC", "abc"),
            ("ping", "ping"),
            ("Type", "r#type"),
            ("Self", "self_"),
            ("3D", "_3d"),
        ] {
            assert_eq!(rust_name(proto), rust, "{proto}");
            // A name already in Rust form reads as itself, so every Rust name
            // passes the boot check.
            let bare = rust.trim_start_matches("r#");
            assert_eq!(rust_name(bare).trim_start_matches("r#"), bare, "{rust}");
        }
        // Not in Rust form: each reads as another name (the boot check
        // refuses them).
        for declared in ["DeleteDeal", "deleteDeal", "get-deal", "ListDeals"] {
            assert_ne!(rust_name(declared), declared, "{declared}");
        }
    }

    #[test]
    fn an_http_status_without_a_grpc_status_maps_as_the_grpc_specification_says() {
        for (status, code) in [
            (400, Code::Internal),
            (401, Code::Unauthenticated),
            (403, Code::PermissionDenied),
            (404, Code::Unimplemented),
            (429, Code::Unavailable),
            (502, Code::Unavailable),
            (503, Code::Unavailable),
            (504, Code::Unavailable),
            (500, Code::Unknown),
            (418, Code::Unknown),
        ] {
            let status = StatusCode::from_u16(status).unwrap();
            assert_eq!(code_of_http_status(status), code, "{status}");
        }
    }
}
