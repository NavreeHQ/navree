//! [`Handler`]: async functions as route endpoints, and [`Endpoint`], a
//! handler ready to mount.
//!
//! A handler is an `async fn` (or a closure returning a future) whose
//! arguments are extractors and whose output is a response. Up to 16
//! arguments: each but the last implements
//! [`FromRequestParts`], the last may
//! implement [`FromRequest`].

use std::convert::Infallible;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};

use tower::util::BoxCloneSyncService;
use tower::{Layer, Service, ServiceExt};

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
    ($($arg:ident),* ; $last:ident) => {
        #[allow(non_snake_case)]
        impl<F, Fut, R, M, $($arg,)* $last> Handler<(M, $($arg,)* $last,)> for F
        where
            F: FnOnce($($arg,)* $last) -> Fut + Clone + Send + Sync + 'static,
            Fut: Future<Output = R> + Send + 'static,
            R: IntoResponse,
            M: 'static,
            $($arg: FromRequestParts + 'static,)*
            $last: FromRequest<M> + 'static,
        {
            type Future = Pin<Box<dyn Future<Output = Response> + Send>>;

            fn call(self, request: Request) -> Self::Future {
                Box::pin(async move {
                    #[allow(unused_mut)]
                    let (mut parts, body) = request.into_parts();
                    $(
                        let $arg = match $arg::from_request_parts(&mut parts).await {
                            Ok(value) => value,
                            Err(rejection) => return rejection.into_response(),
                        };
                    )*
                    let request = Request::from_parts(parts, body);
                    let $last = match $last::from_request(request).await {
                        Ok(value) => value,
                        Err(rejection) => return rejection.into_response(),
                    };
                    self($($arg,)* $last).await.into_response()
                })
            }
        }
    };
}

handler!(; T1);
handler!(T1; T2);
handler!(T1, T2; T3);
handler!(T1, T2, T3; T4);
handler!(T1, T2, T3, T4; T5);
handler!(T1, T2, T3, T4, T5; T6);
handler!(T1, T2, T3, T4, T5, T6; T7);
handler!(T1, T2, T3, T4, T5, T6, T7; T8);
handler!(T1, T2, T3, T4, T5, T6, T7, T8; T9);
handler!(T1, T2, T3, T4, T5, T6, T7, T8, T9; T10);
handler!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10; T11);
handler!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11; T12);
handler!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12; T13);
handler!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13; T14);
handler!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14; T15);
handler!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15; T16);

/// A mounted endpoint as a service: what the router calls.
pub type BoxedService = BoxCloneSyncService<Request, Response, Infallible>;

/// A handler ready to mount.
#[derive(Clone)]
pub struct Endpoint {
    service: BoxedService,
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
        }
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
        }
    }

    /// The endpoint's service.
    pub(crate) fn into_service(self) -> BoxedService {
        self.service
    }

    /// An endpoint that answers with `guest` when no one is signed in on the request
    /// and with `signed_in` when someone is: see [`RouteDef::on_guest_or`](crate::RouteDef::on_guest_or).
    pub fn guest_or(guest: Endpoint, signed_in: Endpoint) -> Self {
        Self {
            service: BoxCloneSyncService::new(GuestOr {
                guest: guest.service,
                signed_in: signed_in.service,
            }),
        }
    }
}

/// [`Endpoint::guest_or`] as a service.
#[derive(Clone)]
struct GuestOr {
    guest: BoxedService,
    signed_in: BoxedService,
}

impl Service<Request> for GuestOr {
    type Response = Response;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Response, Infallible>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request) -> Self::Future {
        let signed_in = request
            .extensions()
            .get::<navree_contracts::AuthState>()
            .is_some_and(|state| state.0.is_some());
        let service = if signed_in {
            self.signed_in.clone()
        } else {
            self.guest.clone()
        };
        Box::pin(async move {
            let mut response = service.oneshot(request).await?;
            // The same address answers by who is asking: a cache keeps a copy per sign-in.
            response.headers_mut().append(
                http::header::VARY,
                http::HeaderValue::from_static("cookie, authorization"),
            );
            Ok(response)
        })
    }
}

impl std::fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Endpoint").finish_non_exhaustive()
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
    use crate::{Json, Query};

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
}
