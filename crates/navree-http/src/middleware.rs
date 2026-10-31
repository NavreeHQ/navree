//! Middleware from an async function: [`from_fn`], [`from_fn_with_state`] and
//! [`Next`], the rest of the stack.
//!
//! A middleware's arguments are extractors like a handler's, then the rest of
//! the stack; its last extractor may read the body, and [`Request`], the whole
//! request, is what most take. It can change the request, answer without
//! running the rest, and change the response. Its output is any
//! [`IntoResponse`].
//!
//! ```
//! use http::StatusCode;
//! use navree_http::AppRouter;
//! use navree_http::extract::Request;
//! use navree_http::middleware::{Next, State, from_fn, from_fn_with_state};
//! use navree_http::response::{IntoResponse, Response};
//! use navree_http::routing::Router;
//!
//! async fn require_tenant(request: Request, next: Next) -> Response {
//!     if !request.headers().contains_key("x-tenant") {
//!         return StatusCode::BAD_REQUEST.into_response();
//!     }
//!     next.run(request).await
//! }
//!
//! async fn only_from(
//!     State(allowed): State<&'static str>,
//!     request: Request,
//!     next: Next,
//! ) -> Response {
//!     match request.headers().get("x-tenant") {
//!         Some(tenant) if tenant == allowed => next.run(request).await,
//!         _ => StatusCode::FORBIDDEN.into_response(),
//!     }
//! }
//!
//! let app = AppRouter::new(Router::new())
//!     .layer(from_fn_with_state("acme", only_from))
//!     .layer(from_fn(require_tenant));
//! # let _ = app;
//! ```
//!
//! A panic in a middleware unwinds through the stack like one in a handler:
//! the pipeline's panic catcher answers 500 for everything inside it.
//!
//! [`Request`]: crate::extract::Request

use std::any::type_name;
use std::convert::Infallible;
use std::fmt;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};

use navree_kernel::BoxFuture;
use tower::{Layer, Service};

use crate::AppRouter;
use crate::extract::Request;
use crate::handler::{FromPartsVia, FromRequestVia};
use crate::response::{IntoResponse, Response};

/// A middleware from the async function `f`. See the [module
/// documentation](self).
pub fn from_fn<F, T>(f: F) -> FromFnLayer<F, (), T> {
    from_fn_with_state((), f)
}

/// A middleware from the async function `f`, whose first argument is
/// [`State`] with a clone of `state`.
///
/// ```
/// use std::sync::Arc;
/// use std::sync::atomic::{AtomicU64, Ordering};
///
/// use navree_http::AppRouter;
/// use navree_http::extract::Request;
/// use navree_http::middleware::{Next, State, from_fn_with_state};
/// use navree_http::response::Response;
/// use navree_http::routing::Router;
///
/// async fn count(State(seen): State<Arc<AtomicU64>>, request: Request, next: Next) -> Response {
///     seen.fetch_add(1, Ordering::Relaxed);
///     next.run(request).await
/// }
///
/// let seen = Arc::new(AtomicU64::new(0));
/// let app = AppRouter::new(Router::new()).layer(from_fn_with_state(seen, count));
/// # let _ = app;
/// ```
pub fn from_fn_with_state<F, S, T>(state: S, f: F) -> FromFnLayer<F, S, T> {
    FromFnLayer {
        f,
        state,
        _arguments: PhantomData,
    }
}

/// The state of a [`from_fn_with_state`] middleware: always its first
/// argument.
#[derive(Clone, Copy, Debug, Default)]
pub struct State<S>(pub S);

/// An async function usable as a middleware with state `S`. `T` is inferred:
/// it lists the arguments and how each is extracted.
pub trait Middleware<S, T>: Clone + Send + 'static {
    /// Extracts the arguments from `request` and runs the function, with
    /// `next` as the rest of the stack (a failed extractor answers with its
    /// rejection).
    fn call(self, state: S, request: Request, next: Next) -> BoxFuture<'static, Response>;
}

/// What tells a function that takes [`State`] from one that does not.
#[doc(hidden)]
pub mod with {
    /// The function takes no state.
    #[derive(Debug)]
    pub enum NoState {}

    /// The function's first argument is [`State`](super::State).
    #[derive(Debug)]
    pub enum State {}
}

macro_rules! middleware {
    ($(($arg:ident, $via:ident)),* ; ($last:ident, $last_via:ident)) => {
        #[allow(non_snake_case, unused_mut)]
        impl<F, Fut, Out, $($via,)* $last_via, $($arg,)* $last>
            Middleware<(), (with::NoState, $($via,)* $last_via, $($arg,)* $last,)> for F
        where
            F: FnMut($($arg,)* $last, Next) -> Fut + Clone + Send + 'static,
            Fut: Future<Output = Out> + Send + 'static,
            Out: IntoResponse + 'static,
            $($arg: FromPartsVia<$via> + 'static,)*
            $last: FromRequestVia<$last_via> + 'static,
            $($via: 'static,)*
            $last_via: 'static,
        {
            fn call(
                mut self,
                (): (),
                request: Request,
                next: Next,
            ) -> BoxFuture<'static, Response> {
                Box::pin(async move {
                    middleware!(@extract request; $(($arg, $via)),* ; ($last, $last_via));
                    self($($arg,)* $last, next).await.into_response()
                })
            }
        }

        #[allow(non_snake_case, unused_mut)]
        impl<F, Fut, Out, S, $($via,)* $last_via, $($arg,)* $last>
            Middleware<S, (with::State, $($via,)* $last_via, $($arg,)* $last,)> for F
        where
            F: FnMut(State<S>, $($arg,)* $last, Next) -> Fut + Clone + Send + 'static,
            Fut: Future<Output = Out> + Send + 'static,
            Out: IntoResponse + 'static,
            S: Send + 'static,
            $($arg: FromPartsVia<$via> + 'static,)*
            $last: FromRequestVia<$last_via> + 'static,
            $($via: 'static,)*
            $last_via: 'static,
        {
            fn call(
                mut self,
                state: S,
                request: Request,
                next: Next,
            ) -> BoxFuture<'static, Response> {
                Box::pin(async move {
                    middleware!(@extract request; $(($arg, $via)),* ; ($last, $last_via));
                    self(State(state), $($arg,)* $last, next).await.into_response()
                })
            }
        }
    };
    (@extract $request:ident; $(($arg:ident, $via:ident)),* ; ($last:ident, $last_via:ident)) => {
        let (mut parts, body) = $request.into_parts();
        $(
            let $arg = match <$arg as FromPartsVia<$via>>::extract(&mut parts).await {
                Ok(value) => value,
                Err(rejection) => return rejection,
            };
        )*
        let $last = match <$last as FromRequestVia<$last_via>>::extract(
            Request::from_parts(parts, body),
        )
        .await
        {
            Ok(value) => value,
            Err(rejection) => return rejection,
        };
    };
}

middleware!(; (T1, M1));
middleware!((T1, M1); (T2, M2));
middleware!((T1, M1), (T2, M2); (T3, M3));
middleware!((T1, M1), (T2, M2), (T3, M3); (T4, M4));
middleware!((T1, M1), (T2, M2), (T3, M3), (T4, M4); (T5, M5));
middleware!((T1, M1), (T2, M2), (T3, M3), (T4, M4), (T5, M5); (T6, M6));
middleware!((T1, M1), (T2, M2), (T3, M3), (T4, M4), (T5, M5), (T6, M6); (T7, M7));
middleware!((T1, M1), (T2, M2), (T3, M3), (T4, M4), (T5, M5), (T6, M6), (T7, M7); (T8, M8));

/// The [`tower::Layer`] of [`from_fn`] and [`from_fn_with_state`].
#[must_use]
pub struct FromFnLayer<F, S, T> {
    f: F,
    state: S,
    _arguments: PhantomData<fn() -> T>,
}

impl<F: Clone, S: Clone, T> Clone for FromFnLayer<F, S, T> {
    fn clone(&self) -> Self {
        Self {
            f: self.f.clone(),
            state: self.state.clone(),
            _arguments: PhantomData,
        }
    }
}

impl<I, F: Clone, S: Clone, T> Layer<I> for FromFnLayer<F, S, T> {
    type Service = FromFn<F, S, I, T>;

    fn layer(&self, inner: I) -> Self::Service {
        FromFn {
            f: self.f.clone(),
            state: self.state.clone(),
            inner,
            _arguments: PhantomData,
        }
    }
}

impl<F, S: fmt::Debug, T> fmt::Debug for FromFnLayer<F, S, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FromFnLayer")
            .field("f", &format_args!("{}", type_name::<F>()))
            .field("state", &self.state)
            .finish()
    }
}

/// The service of a [`FromFnLayer`] around `I`.
pub struct FromFn<F, S, I, T> {
    f: F,
    state: S,
    inner: I,
    _arguments: PhantomData<fn() -> T>,
}

impl<F: Clone, S: Clone, I: Clone, T> Clone for FromFn<F, S, I, T> {
    fn clone(&self) -> Self {
        Self {
            f: self.f.clone(),
            state: self.state.clone(),
            inner: self.inner.clone(),
            _arguments: PhantomData,
        }
    }
}

impl<F, S: fmt::Debug, I: fmt::Debug, T> fmt::Debug for FromFn<F, S, I, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FromFn")
            .field("f", &format_args!("{}", type_name::<F>()))
            .field("state", &self.state)
            .field("inner", &self.inner)
            .finish()
    }
}

impl<F, S, I, T> Service<Request> for FromFn<F, S, I, T>
where
    F: Middleware<S, T>,
    S: Clone,
    I: Service<Request, Error = Infallible> + Clone + Send + Sync + 'static,
    I::Response: IntoResponse + 'static,
    I::Future: Send + 'static,
{
    type Response = Response;
    type Error = Infallible;
    type Future = ResponseFuture;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request) -> ResponseFuture {
        // The clone stays behind: `inner` was polled ready, its clone was not.
        let not_ready = self.inner.clone();
        let ready = std::mem::replace(&mut self.inner, not_ready);
        let next = Next {
            inner: AppRouter::new(ready),
        };
        ResponseFuture {
            inner: self.f.clone().call(self.state.clone(), request, next),
        }
    }
}

/// The future of a [`FromFn`].
pub struct ResponseFuture {
    inner: BoxFuture<'static, Response>,
}

impl Future for ResponseFuture {
    type Output = Result<Response, Infallible>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.inner.as_mut().poll(cx).map(Ok)
    }
}

impl fmt::Debug for ResponseFuture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResponseFuture").finish_non_exhaustive()
    }
}

/// The rest of the stack, down to the handler.
#[derive(Clone, Debug)]
pub struct Next {
    inner: AppRouter,
}

impl Next {
    /// Runs the rest of the stack with `request`.
    pub async fn run(self, request: Request) -> Response {
        match self.inner.call_owned(request).await {
            Ok(response) => response,
            Err(never) => match never {},
        }
    }
}

impl Service<Request> for Next {
    type Response = Response;
    type Error = Infallible;
    type Future = crate::app::ResponseFuture;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request) -> Self::Future {
        self.inner.clone().call_owned(request)
    }
}

#[cfg(test)]
mod tests {
    use http::{HeaderMap, Method, StatusCode, Uri};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::*;
    use crate::body::Body;
    use crate::routing::Router;
    use crate::{Json, Path, Query};

    fn routes() -> AppRouter {
        Router::new()
            .route(Method::GET, "/{id}", |Path(id): Path<u32>| async move {
                format!("{id}")
            })
            .into()
    }

    fn get(uri: &str) -> Request {
        http::Request::get(uri).body(Body::empty()).unwrap()
    }

    async fn run(app: AppRouter, request: Request) -> (StatusCode, HeaderMap, String) {
        let response = app.oneshot(request).await.unwrap();
        let (parts, body) = response.into_parts();
        let body = body.collect().await.unwrap().to_bytes();
        (
            parts.status,
            parts.headers,
            String::from_utf8_lossy(&body).into_owned(),
        )
    }

    #[tokio::test]
    async fn a_middleware_changes_the_request_and_the_response() {
        async fn rewrite(mut request: Request, next: Next) -> Response {
            *request.uri_mut() = Uri::from_static("/7");
            let mut response = next.run(request).await;
            response
                .headers_mut()
                .insert("x-seen", "yes".parse().unwrap());
            response
        }
        let (status, headers, body) = run(routes().layer(from_fn(rewrite)), get("/1")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers["x-seen"], "yes");
        assert_eq!(body, "7");
    }

    #[tokio::test]
    async fn a_middleware_answers_without_the_rest() {
        async fn refuse(_request: Request, _next: Next) -> impl IntoResponse {
            (StatusCode::IM_A_TEAPOT, "no")
        }
        let (status, _, body) = run(routes().layer(from_fn(refuse)), get("/1")).await;
        assert_eq!((status, body.as_str()), (StatusCode::IM_A_TEAPOT, "no"));
    }

    #[tokio::test]
    async fn state_is_the_first_argument() {
        async fn add(State(name): State<String>, request: Request, next: Next) -> Response {
            let mut response = next.run(request).await;
            response
                .headers_mut()
                .insert("x-name", name.parse().unwrap());
            response
        }
        let app = routes().layer(from_fn_with_state("value".to_owned(), add));
        let (_, headers, _) = run(app, get("/1")).await;
        assert_eq!(headers["x-name"], "value");
    }

    #[tokio::test]
    async fn arguments_are_extractors_and_the_last_may_read_the_body() {
        async fn head(
            Query(query): Query<std::collections::HashMap<String, String>>,
            request: Request,
            next: Next,
        ) -> Response {
            let mut response = next.run(request).await;
            let value = query["a"].parse().unwrap();
            response.headers_mut().insert("x-a", value);
            response
        }
        let (_, headers, _) = run(routes().layer(from_fn(head)), get("/1?a=b")).await;
        assert_eq!(headers["x-a"], "b");

        async fn body(Json(value): Json<u8>, _next: Next) -> String {
            format!("got {value}")
        }
        let request = http::Request::post("/1")
            .header("content-type", "application/json")
            .body(Body::from("5"))
            .unwrap();
        let (_, _, text) = run(routes().layer(from_fn(body)), request).await;
        assert_eq!(text, "got 5");
    }

    #[tokio::test]
    async fn a_failed_extractor_answers_with_its_rejection() {
        async fn needs_json(Json(_): Json<u8>, next: Next) -> Response {
            next.run(get("/1")).await
        }
        let (status, _, _) = run(routes().layer(from_fn(needs_json)), get("/1")).await;
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    #[tokio::test]
    async fn the_body_reaches_the_handler_unread() {
        async fn pass(request: Request, next: Next) -> Response {
            next.run(request).await
        }
        let app = AppRouter::from_handler(|body: String| async move { body })
            .layer(from_fn(pass))
            .layer(from_fn(pass));
        let chunks = futures_util::stream::iter(["a", "b"].map(Ok::<_, std::io::Error>));
        let request = http::Request::post("/")
            .body(Body::from_stream(chunks))
            .unwrap();
        let (_, _, text) = run(app, request).await;
        assert_eq!(text, "ab");
    }

    #[tokio::test]
    async fn a_closure_can_capture_what_it_needs() {
        let name = "closure".to_owned();
        let app = routes().layer(from_fn(move |request: Request, next: Next| {
            let name = name.clone();
            async move {
                let mut response = next.run(request).await;
                response
                    .headers_mut()
                    .insert("x-name", name.parse().unwrap());
                response
            }
        }));
        let (_, headers, _) = run(app, get("/1")).await;
        assert_eq!(headers["x-name"], "closure");
    }

    #[tokio::test]
    async fn it_layers_a_route_as_well() {
        async fn stamp(request: Request, next: Next) -> Response {
            let mut response = next.run(request).await;
            response
                .headers_mut()
                .insert("x-route", "yes".parse().unwrap());
            response
        }
        let router = Router::new()
            .route(Method::GET, "/", || async { "ok" })
            .route_layer(from_fn(stamp));
        let response = router.oneshot(get("/")).await.unwrap();
        assert_eq!(response.headers()["x-route"], "yes");
    }

    #[tokio::test]
    async fn a_panic_in_a_middleware_reaches_whoever_is_outside_it() {
        async fn panics(_request: Request, _next: Next) -> Response {
            panic!("in the middleware")
        }
        let app = routes().layer(from_fn(panics));
        let outcome = futures_util::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(
            app.oneshot(get("/1")),
        ))
        .await;
        let payload = outcome.unwrap_err();
        assert_eq!(
            payload.downcast_ref::<&str>(),
            Some(&"in the middleware"),
            "the panic is not swallowed or changed on its way out"
        );
    }

    #[tokio::test]
    async fn next_is_a_service() {
        async fn twice(request: Request, next: Next) -> Response {
            let first = next.clone().oneshot(get("/1")).await.unwrap();
            assert_eq!(first.status(), StatusCode::OK);
            next.run(request).await
        }
        let (_, _, body) = run(routes().layer(from_fn(twice)), get("/2")).await;
        assert_eq!(body, "2");
    }
}
