//! [`Handler`]: async functions as route endpoints, and [`Endpoint`], a
//! handler ready to mount.
//!
//! A handler is an `async fn` (or a closure returning a future) whose
//! arguments are extractors and whose output is a response. Up to 16
//! arguments: each but the last implements
//! [`FromRequestParts`](crate::extract::FromRequestParts), the last may
//! implement [`FromRequest`](crate::extract::FromRequest).
//!
//! **During the move** an argument may instead implement the previous
//! framework's extractor traits (the crates not yet moved still do). Such
//! an argument runs that framework's own extractor; a handler with one is
//! mounted behind that framework's path matching for its route, so the
//! extractor finds the path parameters it expects. Each type implements one
//! of the two families, never both (both would leave the handler
//! ambiguous).

use std::convert::Infallible;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::extract::DefaultBodyLimit;
use http::request::Parts;
use tower::util::BoxCloneSyncService;
use tower::{Layer, Service, ServiceExt};

use crate::body::Body;
use crate::extract::{FromRequest, FromRequestParts, Request};
use crate::response::{IntoResponse, Response};

/// An async function usable as a route's endpoint. See the [module
/// documentation](self).
///
/// `T` is inferred: it lists the arguments and how each is extracted.
pub trait Handler<T>: Clone + Send + Sync + Sized + 'static {
    /// The future of a call.
    type Future: Future<Output = Response> + Send + 'static;

    /// Extracts the arguments from `request`, runs the function and turns
    /// its output into a response (a failed extractor answers with its
    /// rejection).
    fn call(self, request: Request) -> Self::Future;

    /// Whether an argument uses the previous framework's extractor traits.
    #[doc(hidden)]
    fn uses_previous_extractors() -> bool {
        false
    }
}

/// How an argument is extracted: markers that only steer type inference.
#[doc(hidden)]
pub mod via {
    use std::marker::PhantomData;

    /// Navree's [`FromRequestParts`](crate::extract::FromRequestParts).
    #[derive(Debug)]
    pub enum Parts {}

    /// Navree's [`FromRequest`](crate::extract::FromRequest) with marker `M`.
    #[derive(Debug)]
    pub struct Request<M>(std::convert::Infallible, PhantomData<M>);

    /// The previous framework's head extractor trait.
    #[derive(Debug)]
    pub enum PreviousParts {}

    /// The previous framework's body extractor trait with marker `M`.
    #[derive(Debug)]
    pub struct PreviousRequest<M>(std::convert::Infallible, PhantomData<M>);
}

/// An argument other than the last: extracted from the request head.
#[doc(hidden)]
pub trait FromPartsVia<M>: Sized + Send {
    /// Whether this goes through the previous framework's traits.
    const PREVIOUS: bool;

    /// Extracts the argument, or answers with the rejection.
    fn extract(parts: &mut Parts) -> impl Future<Output = Result<Self, Response>> + Send;
}

impl<T: FromRequestParts> FromPartsVia<via::Parts> for T {
    const PREVIOUS: bool = false;

    async fn extract(parts: &mut Parts) -> Result<Self, Response> {
        T::from_request_parts(parts)
            .await
            .map_err(IntoResponse::into_response)
    }
}

impl<T> FromPartsVia<via::PreviousParts> for T
where
    T: axum::extract::FromRequestParts<()> + Send,
{
    const PREVIOUS: bool = true;

    async fn extract(parts: &mut Parts) -> Result<Self, Response> {
        <T as axum::extract::FromRequestParts<()>>::from_request_parts(parts, &())
            .await
            .map_err(from_previous)
    }
}

/// A rejection of the previous framework's as a response.
fn from_previous(rejection: impl axum::response::IntoResponse) -> Response {
    axum::response::IntoResponse::into_response(rejection).into_response()
}

/// The last argument: extracted from the whole request.
#[doc(hidden)]
pub trait FromRequestVia<M>: Sized + Send {
    /// Whether this goes through the previous framework's traits.
    const PREVIOUS: bool;

    /// Extracts the argument, or answers with the rejection.
    fn extract(request: Request) -> impl Future<Output = Result<Self, Response>> + Send;
}

impl<T: FromRequest<M>, M> FromRequestVia<via::Request<M>> for T {
    const PREVIOUS: bool = false;

    async fn extract(request: Request) -> Result<Self, Response> {
        T::from_request(request)
            .await
            .map_err(IntoResponse::into_response)
    }
}

impl<T, M> FromRequestVia<via::PreviousRequest<M>> for T
where
    T: axum::extract::FromRequest<(), M> + Send,
{
    const PREVIOUS: bool = true;

    async fn extract(request: Request) -> Result<Self, Response> {
        let request = request.map(axum::body::Body::new);
        <T as axum::extract::FromRequest<(), M>>::from_request(request, &())
            .await
            .map_err(from_previous)
    }
}

impl<F, Fut, R> Handler<()> for F
where
    F: FnOnce() -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = R> + Send + 'static,
    R: IntoResponse,
{
    type Future = Pin<Box<dyn Future<Output = Response> + Send>>;

    fn call(self, _request: Request) -> Self::Future {
        Box::pin(async move { self().await.into_response() })
    }
}

macro_rules! handler {
    ($(($arg:ident, $via:ident)),* ; ($last:ident, $last_via:ident)) => {
        #[allow(non_snake_case)]
        impl<F, Fut, R, $($via,)* $last_via, $($arg,)* $last>
            Handler<($($via,)* $last_via, $($arg,)* $last,)> for F
        where
            F: FnOnce($($arg,)* $last) -> Fut + Clone + Send + Sync + 'static,
            Fut: Future<Output = R> + Send + 'static,
            R: IntoResponse,
            $($arg: FromPartsVia<$via> + 'static,)*
            $last: FromRequestVia<$last_via> + 'static,
            $($via: 'static,)*
            $last_via: 'static,
        {
            type Future = Pin<Box<dyn Future<Output = Response> + Send>>;

            fn call(self, request: Request) -> Self::Future {
                Box::pin(async move {
                    #[allow(unused_mut)]
                    let (mut parts, body) = request.into_parts();
                    $(
                        let $arg = match <$arg as FromPartsVia<$via>>::extract(&mut parts).await {
                            Ok(value) => value,
                            Err(rejection) => return rejection,
                        };
                    )*
                    let request = Request::from_parts(parts, body);
                    let $last = match <$last as FromRequestVia<$last_via>>::extract(request).await {
                        Ok(value) => value,
                        Err(rejection) => return rejection,
                    };
                    self($($arg,)* $last).await.into_response()
                })
            }

            fn uses_previous_extractors() -> bool {
                $(<$arg as FromPartsVia<$via>>::PREVIOUS ||)*
                    <$last as FromRequestVia<$last_via>>::PREVIOUS
            }
        }
    };
}

handler!(; (T1, M1));
handler!((T1, M1); (T2, M2));
handler!((T1, M1), (T2, M2); (T3, M3));
handler!((T1, M1), (T2, M2), (T3, M3); (T4, M4));
handler!((T1, M1), (T2, M2), (T3, M3), (T4, M4); (T5, M5));
handler!((T1, M1), (T2, M2), (T3, M3), (T4, M4), (T5, M5); (T6, M6));
handler!((T1, M1), (T2, M2), (T3, M3), (T4, M4), (T5, M5), (T6, M6); (T7, M7));
handler!((T1, M1), (T2, M2), (T3, M3), (T4, M4), (T5, M5), (T6, M6), (T7, M7); (T8, M8));
handler!((T1, M1), (T2, M2), (T3, M3), (T4, M4), (T5, M5), (T6, M6), (T7, M7), (T8, M8); (T9, M9));
handler!((T1, M1), (T2, M2), (T3, M3), (T4, M4), (T5, M5), (T6, M6), (T7, M7), (T8, M8), (T9, M9); (T10, M10));
handler!((T1, M1), (T2, M2), (T3, M3), (T4, M4), (T5, M5), (T6, M6), (T7, M7), (T8, M8), (T9, M9), (T10, M10); (T11, M11));
handler!((T1, M1), (T2, M2), (T3, M3), (T4, M4), (T5, M5), (T6, M6), (T7, M7), (T8, M8), (T9, M9), (T10, M10), (T11, M11); (T12, M12));
handler!((T1, M1), (T2, M2), (T3, M3), (T4, M4), (T5, M5), (T6, M6), (T7, M7), (T8, M8), (T9, M9), (T10, M10), (T11, M11), (T12, M12); (T13, M13));
handler!((T1, M1), (T2, M2), (T3, M3), (T4, M4), (T5, M5), (T6, M6), (T7, M7), (T8, M8), (T9, M9), (T10, M10), (T11, M11), (T12, M12), (T13, M13); (T14, M14));
handler!((T1, M1), (T2, M2), (T3, M3), (T4, M4), (T5, M5), (T6, M6), (T7, M7), (T8, M8), (T9, M9), (T10, M10), (T11, M11), (T12, M12), (T13, M13), (T14, M14); (T15, M15));
handler!((T1, M1), (T2, M2), (T3, M3), (T4, M4), (T5, M5), (T6, M6), (T7, M7), (T8, M8), (T9, M9), (T10, M10), (T11, M11), (T12, M12), (T13, M13), (T14, M14), (T15, M15); (T16, M16));

/// A mounted endpoint as a service: what the router calls.
pub type BoxedService = BoxCloneSyncService<Request, Response, Infallible>;

/// A handler ready to mount: a [`Handler`], or the previous framework's
/// method router (`RouteDef::new`, during the move).
#[derive(Clone)]
pub struct Endpoint {
    service: BoxedService,
    /// Whether it uses the previous framework's extractors (or is its
    /// method router).
    previous: bool,
}

impl Endpoint {
    /// The endpoint of `handler`.
    pub fn new<H, T>(handler: H) -> Self
    where
        H: Handler<T>,
        T: 'static,
    {
        Self {
            service: BoxCloneSyncService::new(HandlerService {
                handler,
                _arguments: PhantomData,
            }),
            previous: H::uses_previous_extractors(),
        }
    }

    /// The previous framework's method router, run as is.
    pub fn from_method_router(router: axum::routing::MethodRouter) -> Self {
        Self {
            service: from_previous_service(router),
            previous: true,
        }
    }

    /// Whether every argument uses Navree's extractors (the endpoint runs
    /// without the previous framework's path matching).
    pub fn is_native(&self) -> bool {
        !self.previous
    }

    /// Wraps the endpoint in `layer`: it runs once a route matched, before
    /// the arguments are extracted (a handler system's own per-route
    /// pipeline, such as message handlers').
    pub fn layer<L>(self, layer: L) -> Self
    where
        L: Layer<BoxedService>,
        L::Service: Service<Request, Response = Response, Error = Infallible>
            + Clone
            + Send
            + Sync
            + 'static,
        <L::Service as Service<Request>>::Future: Send + 'static,
    {
        Self {
            service: BoxCloneSyncService::new(layer.layer(self.service)),
            previous: self.previous,
        }
    }

    /// The service to mount at `pattern`. An endpoint on the previous
    /// framework's extractors runs behind that framework's router with this
    /// one route, which matches the path again and records the parameters
    /// where its extractors look; with `body_limit`, its body limit too.
    pub(crate) fn mount(self, pattern: &str, body_limit: Option<usize>) -> BoxedService {
        if !self.previous {
            return self.service;
        }
        let inner = self
            .service
            .map_request(|request: axum::extract::Request| request.map(Body::new));
        let previous = axum::Router::new().route_service(pattern, inner);
        match body_limit {
            Some(limit) => from_previous_service(DefaultBodyLimit::max(limit).layer(previous)),
            None => from_previous_service(previous),
        }
    }

    /// The endpoint's service, without any path matching.
    pub(crate) fn into_service(self) -> BoxedService {
        self.service
    }
}

/// A service of the previous framework's, answering with Navree's response.
fn from_previous_service<S>(service: S) -> BoxedService
where
    S: Service<Request, Response = axum::response::Response, Error = Infallible>
        + Clone
        + Send
        + Sync
        + 'static,
    S::Future: Send + 'static,
{
    let native = |response: axum::response::Response| response.map(Body::new);
    BoxCloneSyncService::new(ServiceExt::<Request>::map_response(service, native))
}

impl std::fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.previous {
            "Endpoint(previous extractors)"
        } else {
            "Endpoint(native)"
        })
    }
}

/// A [`Handler`] as a [`Service`].
struct HandlerService<H, T> {
    handler: H,
    _arguments: PhantomData<fn() -> T>,
}

impl<H: Clone, T> Clone for HandlerService<H, T> {
    fn clone(&self) -> Self {
        Self {
            handler: self.handler.clone(),
            _arguments: PhantomData,
        }
    }
}

impl<H, T> Service<Request> for HandlerService<H, T>
where
    H: Handler<T>,
{
    type Response = Response;
    type Error = Infallible;
    type Future = OkFuture<H::Future>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request) -> Self::Future {
        OkFuture {
            inner: self.handler.clone().call(request),
        }
    }
}

pin_project_lite::pin_project! {
    /// A response future as an infallible service future.
    pub(crate) struct OkFuture<F> {
        #[pin]
        inner: F,
    }
}

impl<F: Future<Output = Response>> Future for OkFuture<F> {
    type Output = Result<Response, Infallible>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.project().inner.poll(cx).map(Ok)
    }
}

/// Calls `service` once with `request`.
pub(crate) async fn call(service: BoxedService, request: Request) -> Response {
    match service.oneshot(request).await {
        Ok(response) => response,
        Err(never) => match never {},
    }
}

#[cfg(test)]
mod tests {
    use http::StatusCode;
    use http_body_util::BodyExt;

    use super::*;
    use crate::body::Body;
    use crate::{Json, Path, Query};

    async fn run<H: Handler<T>, T: 'static>(handler: H, request: Request) -> (StatusCode, String) {
        let response = handler.call(request).await;
        let status = response.status();
        // nosemgrep: navree-unwrap-on-request-data
        let body = response.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8_lossy(&body).into_owned())
    }

    fn get(uri: &str) -> Request {
        http::Request::get(uri).body(Body::empty()).unwrap()
    }

    #[tokio::test]
    async fn arguments_are_extracted_in_order() {
        async fn none() -> &'static str {
            "none"
        }
        assert_eq!(run(none, get("/")).await, (StatusCode::OK, "none".into()));

        async fn query(Query(q): Query<std::collections::HashMap<String, String>>) -> String {
            q["a"].clone()
        }
        assert_eq!(run(query, get("/?a=1")).await.1, "1");

        async fn both(
            Query(q): Query<std::collections::HashMap<String, u8>>,
            Json(v): Json<u8>,
        ) -> String {
            format!("{} {v}", q["a"])
        }
        let request = http::Request::post("/?a=1")
            .header("content-type", "application/json")
            .body(Body::from("2"))
            .unwrap();
        assert_eq!(run(both, request).await.1, "1 2");
    }

    #[tokio::test]
    async fn a_failed_extractor_answers_with_its_rejection() {
        async fn needs_json(Json(_): Json<u8>) -> &'static str {
            "unreachable"
        }
        let (status, _) = run(needs_json, get("/")).await;
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    #[test]
    fn handlers_tell_whether_they_use_previous_extractors() {
        async fn native(_: Path<u8>, _: Json<u8>) {}
        async fn previous(_: axum::extract::Path<u8>) {}
        async fn mixed(_: Path<u8>, _: axum::Json<u8>) {}
        async fn plain() {}
        fn uses<H: Handler<T>, T>(_: H) -> bool {
            H::uses_previous_extractors()
        }
        assert!(!uses(native));
        assert!(uses(previous));
        assert!(uses(mixed));
        assert!(!uses(plain));
        assert!(Endpoint::new(native).is_native());
        assert!(!Endpoint::new(previous).is_native());
    }
}
