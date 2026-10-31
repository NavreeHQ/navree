//! [`AppRouter`]: the app as one service, what [`build_router`] returns and
//! what [`GlobalLayer`]s wrap.
//!
//! [`build_router`]: crate::build_router
//! [`GlobalLayer`]: crate::GlobalLayer

use std::convert::Infallible;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, ready};

use bytes::Bytes;
use http::Method;
use http::header::{CONTENT_LENGTH, TRANSFER_ENCODING};
use navree_kernel::BoxError;
use pin_project_lite::pin_project;
use tower::util::{BoxCloneSyncService, Oneshot};
use tower::{Layer, Service, ServiceExt};

use crate::body::{Body, HttpBody, try_downcast};
use crate::extract::Request;
use crate::handler::{BoxedService, Endpoint, Handler};
use crate::response::{IntoResponse, Response};

/// The app as one [`tower::Service`]: [`routing::Router`](crate::routing::Router)
/// or any other service, wrapped in layers.
///
/// Layers run from the last added inward, like an onion. `AppRouter` is cheap
/// to clone: a clone shares its stack. Call it as any tower service
/// (`tower::ServiceExt::oneshot` in a test), or give it to
/// [`serve()`](crate::serve()).
///
/// ```
/// use http::Method;
/// use http::header::{HeaderName, HeaderValue};
/// use navree_http::AppRouter;
/// use navree_http::extract::Request;
/// use navree_http::middleware::{Next, from_fn};
/// use navree_http::response::Response;
/// use navree_http::routing::Router;
///
/// async fn stamp(request: Request, next: Next) -> Response {
///     let mut response = next.run(request).await;
///     response
///         .headers_mut()
///         .insert(HeaderName::from_static("x-stamp"), HeaderValue::from_static("1"));
///     response
/// }
///
/// let routes = Router::new().route(Method::GET, "/", || async { "hello" });
/// let app = AppRouter::new(routes).layer(from_fn(stamp));
/// # let _ = app;
/// ```
#[derive(Clone)]
pub struct AppRouter {
    // Shared, so a stack of layers clones in one step, not one per layer.
    service: Arc<BoxedService>,
}

impl AppRouter {
    /// `service` as an app.
    pub fn new<S>(service: S) -> Self
    where
        S: Service<Request, Error = Infallible> + Clone + Send + Sync + 'static,
        S::Response: IntoResponse + 'static,
        S::Future: Send + 'static,
    {
        try_downcast(service).unwrap_or_else(|service| Self {
            service: Arc::new(try_downcast(service).unwrap_or_else(|service: S| {
                BoxCloneSyncService::new(service.map_response(IntoResponse::into_response))
            })),
        })
    }

    /// An app that answers every request with `handler`, whatever its method
    /// and path: a catch-all, such as a proxy.
    pub fn from_handler<H, T>(handler: H) -> Self
    where
        H: Handler<T>,
        T: 'static,
    {
        Self::new(Endpoint::new(handler).into_service())
    }

    /// Wraps the app in `layer`, which runs before everything added so far.
    pub fn layer<L>(self, layer: L) -> Self
    where
        L: Layer<AppRouter>,
        L::Service: Service<Request, Error = Infallible> + Clone + Send + Sync + 'static,
        <L::Service as Service<Request>>::Response: IntoResponse + 'static,
        <L::Service as Service<Request>>::Future: Send + 'static,
    {
        Self::new(layer.layer(self))
    }

    /// Calls the app with `request`, without cloning the handle's service
    /// first when nothing else shares it.
    pub(crate) fn call_owned(self, request: Request) -> ResponseFuture {
        let connect = request.method() == Method::CONNECT;
        let service = Arc::unwrap_or_clone(self.service);
        ResponseFuture {
            inner: service.oneshot(request),
            connect,
        }
    }
}

impl<B> Service<http::Request<B>> for AppRouter
where
    B: HttpBody<Data = Bytes> + Send + 'static,
    B::Error: Into<BoxError>,
{
    type Response = Response;
    type Error = Infallible;
    type Future = ResponseFuture;

    #[inline]
    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: http::Request<B>) -> ResponseFuture {
        self.clone().call_owned(request.map(Body::new))
    }
}

impl From<crate::routing::Router> for AppRouter {
    fn from(router: crate::routing::Router) -> Self {
        Self::new(router)
    }
}

impl fmt::Debug for AppRouter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppRouter").finish_non_exhaustive()
    }
}

pin_project! {
    /// The future of a request to an [`AppRouter`].
    pub struct ResponseFuture {
        #[pin]
        inner: Oneshot<BoxedService, Request>,
        connect: bool,
    }
}

impl Future for ResponseFuture {
    type Output = Result<Response, Infallible>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let mut response = ready!(this.inner.poll(cx))?;
        // RFC 9110: a 2xx to CONNECT has no content, and the connection is a
        // tunnel from here on.
        if *this.connect
            && response.status().is_success()
            && (response.headers().contains_key(CONTENT_LENGTH)
                || response.headers().contains_key(TRANSFER_ENCODING)
                || response.body().size_hint().lower() != 0)
        {
            tracing::error!(target: "navree::http", "response to CONNECT with a body");
            *response.body_mut() = Body::empty();
        }
        Poll::Ready(Ok(response))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use http::{HeaderValue, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::*;
    use crate::middleware::{Next, from_fn};
    use crate::routing::Router;

    fn get(uri: &str) -> Request {
        http::Request::get(uri).body(Body::empty()).unwrap()
    }

    async fn text(response: Response) -> String {
        let body = response.into_body().collect().await.unwrap().to_bytes();
        String::from_utf8_lossy(&body).into_owned()
    }

    #[tokio::test]
    async fn a_layer_added_later_runs_first() {
        let order = Arc::new(Mutex::new(Vec::new()));
        let step = |name: &'static str| {
            let order = order.clone();
            from_fn(move |request: Request, next: Next| {
                let order = order.clone();
                async move {
                    order.lock().unwrap().push(name);
                    let response = next.run(request).await;
                    order.lock().unwrap().push(name);
                    response
                }
            })
        };
        let app = AppRouter::new(Router::new().route(http::Method::GET, "/", || async { "hi" }))
            .layer(step("inner"))
            .layer(step("outer"));
        let response = app.oneshot(get("/")).await.unwrap();
        assert_eq!(text(response).await, "hi");
        assert_eq!(*order.lock().unwrap(), ["outer", "inner", "inner", "outer"]);
    }

    #[tokio::test]
    async fn a_service_is_taken_with_any_request_body() {
        let app = AppRouter::from_handler(|body: String| async move { body });
        let request = http::Request::post("/anything")
            .body(http_body_util::Full::new(Bytes::from_static(b"sent")))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(text(response).await, "sent");
    }

    #[tokio::test]
    async fn a_handler_takes_every_method_and_path() {
        let app = AppRouter::from_handler(|request: Request| async move {
            format!("{} {}", request.method(), request.uri().path())
        });
        let request = http::Request::delete("/a/b").body(Body::empty()).unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(text(response).await, "DELETE /a/b");
    }

    #[tokio::test]
    async fn a_2xx_to_connect_has_no_body() {
        let app = AppRouter::from_handler(|| async { "tunnel" });
        let connect = http::Request::connect("example.com:443")
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(connect).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(text(response).await, "");

        // Only a 2xx: a refusal keeps its body, and so does any other method.
        let refused = AppRouter::from_handler(|| async {
            (
                StatusCode::FORBIDDEN,
                [("x-why", HeaderValue::from_static("no"))],
                "no",
            )
        });
        let connect = http::Request::connect("example.com:443")
            .body(Body::empty())
            .unwrap();
        let response = refused.oneshot(connect).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(text(response).await, "no");
        let response = app.oneshot(get("/")).await.unwrap();
        assert_eq!(text(response).await, "tunnel");
    }

    #[test]
    fn it_can_be_shared_between_threads() {
        fn assert_shareable<T: Clone + Send + Sync + 'static>() {}
        assert_shareable::<AppRouter>();
    }
}
