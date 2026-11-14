//! [`FallibleLayer`]: a tower layer whose service can fail, made into one the
//! app can mount, and [`FallibleService`], what it makes.

use std::convert::Infallible;
use std::task::{Context, Poll};

use bytes::Bytes;
use navree_kernel::{BoxError, BoxFuture};
use tower::{Layer, Service, ServiceExt};

use crate::body::{Body, HttpBody};
use crate::error::AppError;
use crate::extract::Request;
use crate::response::{IntoResponse, Response};

/// A tower layer whose service can fail, made into one the app can mount
/// ([`AppRouter::layer`](crate::AppRouter::layer),
/// [`Endpoint::layer`](crate::handler::Endpoint::layer),
/// [`Router::route_layer`](crate::routing::Router::route_layer)): the
/// service's error becomes a 500 [`AppError`] with the error as its source,
/// so the app's error mapping
/// ([`m.map_error`](crate::ModuleBuilderHttpExt::map_error)) and JSON error
/// shape apply; its response body is taken as the app's body.
///
/// The service is cloned for each request and asked first whether it is
/// ready: an error there (a load shedder's, say) answers like an error of
/// the call. Without a mapping the client sees `"Internal server error"`,
/// and the error is logged, never sent.
///
/// ```
/// use std::time::Duration;
///
/// use navree_http::{AppError, FallibleLayer, GlobalLayer, LayerSlot, ModuleBuilderHttpExt};
/// use navree_kernel::{Module, ModuleBuilder};
///
/// struct LedgerModule;
///
/// impl Module for LedgerModule {
///     fn register(&self, m: &mut ModuleBuilder) {
///         m.http_layer(GlobalLayer::new(LayerSlot::Context, "timeout", |_, router| {
///             Ok(router.layer(FallibleLayer::new(tower::timeout::TimeoutLayer::new(
///                 Duration::from_secs(10),
///             ))))
///         }))
///         // The timeout's error answers 503 instead of 500.
///         .map_error::<tower::timeout::error::Elapsed>(|_| {
///             Some(AppError::service_unavailable("The khata took too long."))
///         });
///     }
/// }
/// ```
#[derive(Clone, Debug)]
pub struct FallibleLayer<L> {
    /// The layer whose service can fail.
    layer: L,
}

impl<L> FallibleLayer<L> {
    /// Wraps `layer`, whose service may answer with an error.
    pub fn new(layer: L) -> FallibleLayer<L> {
        FallibleLayer { layer }
    }
}

impl<S, L, B> Layer<S> for FallibleLayer<L>
where
    L: Layer<S>,
    L::Service: Service<Request, Response = http::Response<B>> + Clone + Send + Sync + 'static,
    <L::Service as Service<Request>>::Error: Into<BoxError>,
    <L::Service as Service<Request>>::Future: Send + 'static,
    B: HttpBody<Data = Bytes> + Send + 'static,
    B::Error: Into<BoxError>,
{
    type Service = FallibleService<L::Service>;

    fn layer(&self, inner: S) -> Self::Service {
        FallibleService {
            inner: self.layer.layer(inner),
        }
    }
}

/// What [`FallibleLayer`] makes: a service with `Error = Infallible` and
/// `Response = navree_http::response::Response`. An error of the wrapped
/// service, at its readiness or its call, answers as
/// [`AppError::internal`] of that error.
///
/// ```
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() {
/// use http::StatusCode;
/// use navree_http::FallibleLayer;
/// use navree_http::body::Body;
/// use navree_http::extract::Request;
/// use tower::{Layer, ServiceExt};
///
/// // A service of another crate that fails.
/// let lost = tower::service_fn(|_request: Request| async {
///     Err::<http::Response<Body>, _>(std::io::Error::other("the khata of Jaipur is lost"))
/// });
/// let service = FallibleLayer::new(tower::layer::util::Identity::new()).layer(lost);
/// let response = service.oneshot(Request::new(Body::empty())).await.unwrap();
/// assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct FallibleService<S> {
    /// The service whose errors become 500s.
    inner: S,
}

impl<S, B> Service<Request> for FallibleService<S>
where
    S: Service<Request, Response = http::Response<B>> + Clone + Send + Sync + 'static,
    S::Error: Into<BoxError>,
    S::Future: Send + 'static,
    B: HttpBody<Data = Bytes> + Send + 'static,
    B::Error: Into<BoxError>,
{
    type Response = Response;
    type Error = Infallible;
    type Future = BoxFuture<'static, Result<Response, Infallible>>;

    /// Always ready: the inner service's readiness is asked for each call.
    #[inline]
    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request) -> Self::Future {
        let service = self.inner.clone();
        Box::pin(async move {
            Ok(match service.oneshot(request).await {
                Ok(response) => response.map(Body::new),
                Err(error) => AppError::internal(error).into_response(),
            })
        })
    }
}
