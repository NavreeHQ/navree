//! [`Routes`]: the tonic servers of an app, found by service name.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::task::{Context, Poll};

use http::{Request, Response};
use navree_kernel::BoxFuture;
use tonic::Status;
use tonic::body::Body;
use tonic::codegen::{Body as HttpBody, Bytes, StdError};
use tonic::server::NamedService;
use tower::util::BoxCloneSyncService;
use tower::{Service, ServiceExt};

type BoxedService = BoxCloneSyncService<Request<Body>, Response<Body>, Infallible>;

/// The tonic servers of an app. A call goes to the server named in its path
/// (`/crm.deals.v1.Deals/GetDeal` to `crm.deals.v1.Deals`), compared as the
/// client sent it. A call to a service that is not here, or to a path that
/// names no method, is answered with the gRPC status `UNIMPLEMENTED`.
#[derive(Clone, Default)]
pub struct Routes {
    services: Arc<HashMap<&'static str, BoxedService>>,
}

impl Routes {
    /// Adds `service` under its name. A service of the same name is
    /// replaced.
    pub fn add_service<S, B>(mut self, service: S) -> Self
    where
        S: Service<Request<Body>, Response = Response<B>, Error = Infallible>
            + NamedService
            + Clone
            + Send
            + Sync
            + 'static,
        S::Future: Send + 'static,
        B: HttpBody<Data = Bytes> + Send + 'static,
        B::Error: Into<StdError>,
    {
        let service = service.map_response(|response: Response<B>| response.map(Body::new));
        Arc::make_mut(&mut self.services).insert(S::NAME, BoxCloneSyncService::new(service));
        self
    }

    /// The server a call to `path` is for.
    fn service_for(&self, path: &str) -> Option<&BoxedService> {
        let (name, method) = path.strip_prefix('/')?.split_once('/')?;
        if method.is_empty() {
            return None;
        }
        self.services.get(name)
    }
}

impl std::fmt::Debug for Routes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut names: Vec<_> = self.services.keys().collect();
        names.sort();
        f.debug_struct("Routes").field("services", &names).finish()
    }
}

impl Service<Request<Body>> for Routes {
    type Response = Response<Body>;
    type Error = Infallible;
    type Future = BoxFuture<'static, Result<Response<Body>, Infallible>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request<Body>) -> Self::Future {
        match self.service_for(request.uri().path()) {
            Some(service) => Box::pin(service.clone().oneshot(request)),
            None => Box::pin(std::future::ready(Ok(unimplemented()))),
        }
    }
}

/// The answer to a call nothing serves: a trailers-only response whose
/// status is `UNIMPLEMENTED`.
fn unimplemented() -> Response<Body> {
    let (parts, ()) = Status::unimplemented("").into_http::<()>().into_parts();
    Response::from_parts(parts, Body::empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A server that answers 200 and its own name.
    #[derive(Clone)]
    struct Named(&'static str);

    impl NamedService for Named {
        const NAME: &'static str = "pkg.Named";
    }

    impl Service<Request<Body>> for Named {
        type Response = Response<Body>;
        type Error = Infallible;
        type Future = std::future::Ready<Result<Response<Body>, Infallible>>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, _request: Request<Body>) -> Self::Future {
            let mut response = Response::new(Body::empty());
            response
                .headers_mut()
                .insert("x-served-by", self.0.parse().unwrap());
            std::future::ready(Ok(response))
        }
    }

    async fn call(routes: &Routes, path: &str) -> Response<Body> {
        let request = Request::post(path).body(Body::empty()).unwrap();
        routes.clone().oneshot(request).await.unwrap()
    }

    fn unimplemented_status(response: &Response<Body>) -> bool {
        Status::from_header_map(response.headers()).map(|status| status.code())
            == Some(tonic::Code::Unimplemented)
    }

    #[tokio::test]
    async fn a_call_goes_to_the_service_in_its_path() {
        let routes = Routes::default().add_service(Named("first"));
        for path in ["/pkg.Named/Get", "/pkg.Named/Get/with/more"] {
            let response = call(&routes, path).await;
            assert_eq!(response.headers()["x-served-by"], "first", "{path}");
        }
    }

    #[tokio::test]
    async fn anything_else_is_unimplemented() {
        let routes = Routes::default().add_service(Named("first"));
        for path in [
            "/",
            "/pkg.Named",
            "/pkg.Named/",
            "/pkg.Other/Get",
            "/pkg.named/Get",
            "/pkg.%4Eamed/Get",
        ] {
            let response = call(&routes, path).await;
            assert!(unimplemented_status(&response), "{path}");
            assert!(response.headers().get("x-served-by").is_none(), "{path}");
        }
        assert!(unimplemented_status(
            &call(&Routes::default(), "/a/b").await
        ));
    }

    #[tokio::test]
    async fn a_service_added_twice_is_replaced_and_clones_do_not_share() {
        let before = Routes::default().add_service(Named("first"));
        let after = before.clone().add_service(Named("second"));
        let response = call(&before, "/pkg.Named/Get").await;
        assert_eq!(response.headers()["x-served-by"], "first");
        let response = call(&after, "/pkg.Named/Get").await;
        assert_eq!(response.headers()["x-served-by"], "second");
    }
}
