//! [`TestChannel`]: tonic clients on a [`TestApp`], in-process, through the
//! whole call pipeline and without a port.

use std::convert::Infallible;
use std::fmt;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::body::Body;
use futures_util::future::BoxFuture;
use http::header::{AUTHORIZATION, HeaderName, HeaderValue};
use http::HeaderMap;
use navree_contracts::Principal;
use navree_kernel::WeakAppContext;
use navree_testing::{ActingAs, TestApp};
use tokio::sync::OnceCell;

use crate::GrpcServer;

/// A channel to a [`TestApp`]'s gRPC services, for tonic's generated
/// clients: `DealsClient::new(TestChannel::new(&app))`. Calls go through
/// the same pipeline as calls over the network (request id,
/// authentication, guards, access, deadline, unit of work) and Connect and
/// gRPC-Web are not involved.
///
/// ```ignore
/// let mut owner = DealsClient::new(TestChannel::new(&app).acting_as(principal(rep)));
/// let deal = owner.get_deal(GetDealRequest { id }).await?.into_inner();
/// ```
#[derive(Clone)]
pub struct TestChannel {
    ctx: WeakAppContext,
    server: Arc<OnceCell<Option<Arc<GrpcServer>>>>,
    metadata: HeaderMap,
    acting_as: Option<Principal>,
}

impl TestChannel {
    /// A channel to `app`'s services, as a guest.
    pub fn new(app: &TestApp) -> Self {
        Self {
            ctx: app.ctx().downgrade(),
            server: Arc::new(OnceCell::new()),
            metadata: HeaderMap::new(),
            acting_as: None,
        }
    }

    /// Authenticates every call as `principal`, like
    /// `TestApp::acting_as` for HTTP requests (through the app's
    /// `TestAuthenticator`; apps with their own authenticator need
    /// `TestAppBuilder::fake_auth`).
    pub fn acting_as(mut self, principal: Principal) -> Self {
        self.acting_as = Some(principal);
        self
    }

    /// Sends `authorization: Bearer <token>`, checked by the app's real
    /// authenticator.
    pub fn bearer(self, token: &str) -> Self {
        self.metadata(AUTHORIZATION.as_str(), &format!("Bearer {token}"))
    }

    /// Adds metadata to every call.
    ///
    /// # Panics
    ///
    /// When `key` or `value` is not a valid ASCII metadata key or value.
    pub fn metadata(mut self, key: &str, value: &str) -> Self {
        let name = HeaderName::from_bytes(key.as_bytes())
            .unwrap_or_else(|_| panic!("`{key}` is not a valid metadata key"));
        let value = HeaderValue::from_str(value)
            .unwrap_or_else(|_| panic!("the value of `{key}` is not valid metadata"));
        self.metadata.append(name, value);
        self
    }
}

impl fmt::Debug for TestChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TestChannel")
            .field("metadata", &self.metadata.keys().collect::<Vec<_>>())
            .field("acting_as", &self.acting_as.as_ref().map(|p| p.user_id))
            .finish_non_exhaustive()
    }
}

impl tower::Service<http::Request<tonic::body::Body>> for TestChannel {
    type Response = http::Response<Body>;
    type Error = Infallible;
    type Future = BoxFuture<'static, Result<http::Response<Body>, Infallible>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, mut request: http::Request<tonic::body::Body>) -> Self::Future {
        for (name, value) in &self.metadata {
            request.headers_mut().append(name.clone(), value.clone());
        }
        if let Some(principal) = &self.acting_as {
            request.extensions_mut().insert(ActingAs(principal.clone()));
        }
        let this = self.clone();
        Box::pin(async move {
            let server = this
                .server
                .get_or_init(|| async {
                    match this.ctx.upgrade() {
                        Some(ctx) => ctx.try_get::<GrpcServer>().await,
                        None => None,
                    }
                })
                .await;
            Ok(match server {
                Some(server) => server.serve_in_process(request.map(Body::new)).await,
                None => tonic::Status::unavailable(
                    "the test app has no GrpcModule, or it has shut down",
                )
                .into_http::<Body>(),
            })
        })
    }
}
