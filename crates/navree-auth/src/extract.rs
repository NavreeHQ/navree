//! Extractors: [`CurrentUser`] and [`ClientMeta`].

use std::convert::Infallible;
use std::net::IpAddr;

use axum::extract::FromRequestParts;
use http::header::USER_AGENT;
use http::request::Parts;
use navree_contracts::{AuthState, RequestId};
use navree_http::{AppError, ClientIp};
use navree_kernel::AppContext;

use crate::{AuthError, AuthService, ClientMeta, User};

/// The signed-in user, loaded from the database, as a handler argument.
/// 401 for guests and for principals whose user no longer exists. Loaded
/// once per request.
///
/// ```
/// use navree_auth::{CurrentUser, UserResource};
/// use navree_http::Json;
///
/// async fn profile(CurrentUser(user): CurrentUser) -> Json<UserResource> {
///     Json(UserResource::from(&user))
/// }
/// # let _: axum::routing::MethodRouter = axum::routing::get(profile);
/// ```
#[derive(Clone, Debug)]
pub struct CurrentUser(pub User);

impl CurrentUser {
    /// The user.
    pub fn into_inner(self) -> User {
        self.0
    }
}

impl std::ops::Deref for CurrentUser {
    type Target = User;

    fn deref(&self) -> &User {
        &self.0
    }
}

impl<S: Send + Sync> FromRequestParts<S> for CurrentUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        if let Some(user) = parts.extensions.get::<CurrentUser>() {
            return Ok(user.clone());
        }
        let user_id = parts
            .extensions
            .get::<AuthState>()
            .and_then(|state| state.0.as_ref())
            .map(|principal| principal.user_id)
            .ok_or_else(AppError::unauthenticated)?;
        let ctx = parts
            .extensions
            .get::<AppContext>()
            .cloned()
            .ok_or_else(|| AppError::internal("CurrentUser used outside the navree pipeline"))?;
        let auth = ctx.get::<AuthService>().await.map_err(|error| {
            AppError::internal(format!("CurrentUser needs AuthModule in the app: {error}"))
        })?;
        let user = auth
            .find_user(user_id)
            .await
            .map_err(AuthError::into_app_error)?
            .ok_or_else(AppError::unauthenticated)?;
        let current = CurrentUser(user);
        parts.extensions.insert(current.clone());
        Ok(current)
    }
}

impl<S: Send + Sync> FromRequestParts<S> for ClientMeta {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let ClientIp(ip) = ClientIp::from_parts(parts);
        Ok(ClientMeta {
            ip: (!ip.is_unspecified()).then(|| canonical_ip(ip)),
            user_agent: parts
                .headers
                .get(USER_AGENT)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned),
            request_id: parts
                .extensions
                .get::<RequestId>()
                .map(|id| id.as_str().to_owned()),
        })
    }
}

/// IPv4-mapped IPv6 addresses as plain IPv4.
fn canonical_ip(ip: IpAddr) -> String {
    ip.to_canonical().to_string()
}
