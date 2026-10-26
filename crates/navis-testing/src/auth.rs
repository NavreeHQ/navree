//! [`TestAuthenticator`]: authenticates requests as the principal a
//! [`TestClient`](crate::TestClient) is [acting as](crate::TestClient::acting_as).

use std::sync::Arc;

use http::request::Parts;
use navis_contracts::{AuthVia, Principal};
use navis_http::{AppError, Authenticator};
use navis_kernel::{AppBuilder, AppContext, BoxFuture, ProviderKey};
use uuid::Uuid;

/// The principal a test request acts as (a request extension, which an
/// HTTP client cannot forge).
#[derive(Clone, Debug)]
pub(crate) struct ActingAs(pub(crate) Principal);

/// A `dyn Authenticator` for tests: a request sent by a client
/// [acting as](crate::TestClient::acting_as) a principal is authenticated as
/// that principal; every other request is a guest.
///
/// [`TestApp`](crate::TestApp) installs it when the app has no
/// authenticator of its own. To replace an app's authenticator (navis-auth's
/// JWT bearer strategies, which then stop authenticating), use
/// [`TestAppBuilder::fake_auth`](crate::TestAppBuilder::fake_auth), or
/// [`install`](TestAuthenticator::install) for an app built elsewhere:
///
/// ```no_run
/// # async fn demo(builder: navis_kernel::AppBuilder) {
/// use navis_testing::{TestApp, TestAuthenticator, principal};
///
/// let app = TestApp::from_builder(TestAuthenticator::install(builder)).await;
/// let alice = app.acting_as(principal(uuid::Uuid::now_v7()));
/// # }
/// ```
#[derive(Debug, Default)]
pub struct TestAuthenticator {
    _private: (),
}

impl TestAuthenticator {
    /// Registers a `TestAuthenticator` as the app's `dyn Authenticator`,
    /// replacing the app's own authenticator if it has one.
    pub fn install(builder: AppBuilder) -> AppBuilder {
        let authenticator = Arc::new(TestAuthenticator::default());
        builder
            .override_value::<dyn Authenticator>(authenticator.clone())
            .override_value::<TestAuthenticator>(authenticator)
    }

    /// Whether the app authenticates requests with an installed `TestAuthenticator`.
    pub(crate) async fn is_active(ctx: &AppContext) -> bool {
        match (
            ctx.try_get::<dyn Authenticator>().await,
            ctx.try_get::<TestAuthenticator>().await,
        ) {
            (Some(active), Some(test)) => {
                std::ptr::addr_eq(Arc::as_ptr(&active), Arc::as_ptr(&test))
            }
            _ => false,
        }
    }

    /// Whether the app built by `builder` registers no `dyn Authenticator`
    /// (an app whose graph is invalid is left alone: its boot will fail).
    pub(crate) fn is_missing(builder: &AppBuilder) -> bool {
        let key = ProviderKey::of::<dyn Authenticator>().to_string();
        builder.validate().is_ok_and(|report| {
            !report
                .modules
                .iter()
                .flat_map(|m| &m.providers)
                .any(|p| !p.multi && p.key == key)
        })
    }
}

impl Authenticator for TestAuthenticator {
    fn authenticate<'a>(
        &'a self,
        parts: &'a mut Parts,
    ) -> BoxFuture<'a, Result<Option<Principal>, AppError>> {
        let principal = parts.extensions.get::<ActingAs>().map(|a| a.0.clone());
        Box::pin(async move { Ok(principal) })
    }
}

/// A test user: `user_id`, authenticated [`AuthVia::Test`], with no token
/// restrictions and no session.
pub fn principal(user_id: Uuid) -> Principal {
    Principal {
        user_id,
        via: AuthVia::Test,
        token_abilities: None,
        session_id: None,
    }
}
