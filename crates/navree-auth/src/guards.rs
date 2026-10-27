//! The request's access token ([`AccessClaims`]), who is acting
//! ([`Actor`]), and the route guards built on them:
//! [`not_while_impersonating`] (`#[not_while_impersonating]`),
//! [`recent_auth`] (`#[recent_auth(minutes = 10)]`) and
//! [`platform_session`].

use std::fmt;
use std::ops::Deref;
use std::sync::Arc;
use std::time::Duration;

use std::convert::Infallible;

use axum::extract::{FromRequestParts, OptionalFromRequestParts};
use http::request::Parts;
use navree_contracts::{AuthState, BoxFuture, Impersonator};
use navree_http::{AppError, Guard};
use navree_kernel::AppContext;
use uuid::Uuid;

use crate::service::clock_of;
use crate::{AuthError, Claims, TokenType};

/// The verified claims of the request's access token: a request extension
/// the JWT strategy sets, and an extractor (401 for guests and for requests
/// authenticated another way, such as a session cookie or an API token).
///
/// ```
/// use navree_auth::AccessClaims;
/// use navree_http::Json;
///
/// async fn session(claims: AccessClaims) -> Json<Option<uuid::Uuid>> {
///     Json(claims.session_id())
/// }
/// # let _: axum::routing::MethodRouter = axum::routing::get(session);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct AccessClaims(pub Claims);

impl AccessClaims {
    /// The session (refresh-token family) of the token (`sid`).
    pub fn session_id(&self) -> Option<Uuid> {
        self.0.sid
    }

    /// The impersonating user, in an impersonation session (`act.sub`).
    pub fn impersonator(&self) -> Option<Uuid> {
        self.0.act.map(|act| act.sub)
    }

    /// When the user last authenticated, in Unix seconds (`auth_time`).
    pub fn auth_time(&self) -> Option<i64> {
        self.0.auth_time
    }

    /// Whether the token belongs to a platform session.
    pub fn is_platform(&self) -> bool {
        self.0.typ == TokenType::Platform
    }
}

impl Deref for AccessClaims {
    type Target = Claims;

    fn deref(&self) -> &Claims {
        &self.0
    }
}

impl<S: Send + Sync> FromRequestParts<S> for AccessClaims {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<AccessClaims>()
            .cloned()
            .ok_or_else(AppError::unauthenticated)
    }
}

/// `Option<AccessClaims>`: the claims when the request carries an access
/// token.
impl<S: Send + Sync> OptionalFromRequestParts<S> for AccessClaims {
    type Rejection = Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> Result<Option<Self>, Self::Rejection> {
        Ok(parts.extensions.get::<AccessClaims>().cloned())
    }
}

/// Who is acting in the request: the signed-in user and, in an
/// impersonation session, the impersonator (the person really at the
/// keyboard). An extractor; 401 for guests.
///
/// ```
/// use navree_auth::Actor;
/// use navree_http::Json;
///
/// async fn banner(actor: Actor) -> Json<bool> {
///     // Show "You are impersonating ..." in the front end.
///     Json(actor.is_impersonating())
/// }
/// # let _: axum::routing::MethodRouter = axum::routing::get(banner);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Actor {
    /// The signed-in user (the impersonated user while impersonating).
    pub user_id: Uuid,
    /// The impersonating user, if any.
    pub impersonator_id: Option<Uuid>,
}

impl Actor {
    /// Whether someone is impersonating the user.
    pub fn is_impersonating(&self) -> bool {
        self.impersonator_id.is_some()
    }

    /// The person at the keyboard: the impersonator while impersonating,
    /// the user otherwise.
    pub fn real_user_id(&self) -> Uuid {
        self.impersonator_id.unwrap_or(self.user_id)
    }
}

impl<S: Send + Sync> FromRequestParts<S> for Actor {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let user_id = parts
            .extensions
            .get::<AuthState>()
            .and_then(|state| state.0.as_ref())
            .map(|principal| principal.user_id)
            .ok_or_else(AppError::unauthenticated)?;
        Ok(Actor {
            user_id,
            impersonator_id: parts.extensions.get::<Impersonator>().map(|i| i.0),
        })
    }
}

/// A guard refusing requests of an impersonation session with 403
/// `impersonation_forbidden` ("Not allowed while impersonating a user"):
/// what `#[not_while_impersonating]` adds to a route. The auth module's own
/// account-security routes (password, email, two-factor, passkeys,
/// confirmation, sessions) carry it.
pub fn not_while_impersonating() -> Arc<dyn Guard> {
    Arc::new(NotWhileImpersonating)
}

struct NotWhileImpersonating;

impl fmt::Debug for NotWhileImpersonating {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("not_while_impersonating")
    }
}

impl Guard for NotWhileImpersonating {
    fn check<'a>(&'a self, parts: &'a mut Parts) -> BoxFuture<'a, Result<(), AppError>> {
        let impersonating = parts.extensions.get::<Impersonator>().is_some();
        Box::pin(async move {
            if impersonating {
                Err(AuthError::ImpersonationForbidden.into_app_error())
            } else {
                Ok(())
            }
        })
    }
}

/// A guard requiring that the user authenticated at most `max_age` ago
/// (the access token's `auth_time`: signing in, or `POST /auth/confirm`):
/// what `#[recent_auth(minutes = 10)]` adds to a route. Anything else
/// (an older sign-in, an impersonation session, a request not
/// authenticated with an access token) is 403 `reauthentication_required`;
/// the client confirms the password or a code at `POST /auth/confirm`,
/// then retries with the new access token.
pub fn recent_auth(max_age: Duration) -> Arc<dyn Guard> {
    Arc::new(RecentAuth { max_age })
}

struct RecentAuth {
    max_age: Duration,
}

impl fmt::Debug for RecentAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "recent_auth({}s)", self.max_age.as_secs())
    }
}

impl Guard for RecentAuth {
    fn check<'a>(&'a self, parts: &'a mut Parts) -> BoxFuture<'a, Result<(), AppError>> {
        let auth_time = parts
            .extensions
            .get::<AccessClaims>()
            .filter(|claims| claims.act.is_none())
            .and_then(|claims| claims.auth_time);
        let ctx = parts.extensions.get::<AppContext>().cloned();
        Box::pin(async move {
            let refused = || Err(AuthError::ReauthenticationRequired.into_app_error());
            let Some(auth_time) = auth_time else {
                return refused();
            };
            let now = match &ctx {
                Some(ctx) => clock_of(ctx).await.now().timestamp(),
                None => chrono::Utc::now().timestamp(),
            };
            let max_age = i64::try_from(self.max_age.as_secs()).unwrap_or(i64::MAX);
            if now.saturating_sub(auth_time) > max_age {
                return refused();
            }
            Ok(())
        })
    }
}

/// A guard admitting only platform sessions (`auth.platform`: a platform
/// administrator signed in with two-factor authentication); anything else
/// is 403 `platform_session_required`. Platform consoles put it on their
/// routes; [`PlatformAdmin`](crate::PlatformAdmin) checks the same as an
/// extractor.
pub fn platform_session() -> Arc<dyn Guard> {
    Arc::new(PlatformSession)
}

struct PlatformSession;

impl fmt::Debug for PlatformSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("platform_session")
    }
}

impl Guard for PlatformSession {
    fn check<'a>(&'a self, parts: &'a mut Parts) -> BoxFuture<'a, Result<(), AppError>> {
        let platform = parts
            .extensions
            .get::<AccessClaims>()
            .is_some_and(AccessClaims::is_platform);
        Box::pin(async move {
            if platform {
                Ok(())
            } else {
                Err(AuthError::PlatformSessionRequired.into_app_error())
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use http::Request;

    use super::*;
    use crate::ActorClaim;

    fn claims(typ: TokenType, auth_time: Option<i64>, act: Option<Uuid>) -> AccessClaims {
        AccessClaims(Claims {
            sub: Uuid::now_v7(),
            iat: 0,
            exp: 0,
            iss: "navree".to_owned(),
            aud: None,
            jti: "j".to_owned(),
            typ,
            sid: Some(Uuid::now_v7()),
            auth_time,
            act: act.map(|sub| ActorClaim { sub }),
        })
    }

    fn parts() -> Parts {
        Request::builder().body(()).unwrap().into_parts().0
    }

    #[tokio::test]
    async fn not_while_impersonating_refuses_impersonation_sessions() {
        let guard = not_while_impersonating();
        let mut plain = parts();
        assert!(guard.check(&mut plain).await.is_ok());
        let mut impersonated = parts();
        impersonated.extensions.insert(Impersonator(Uuid::now_v7()));
        let error = guard.check(&mut impersonated).await.unwrap_err();
        assert_eq!(error.status.as_u16(), 403);
        assert_eq!(error.code, "impersonation_forbidden");
        assert_eq!(format!("{guard:?}"), "not_while_impersonating");
    }

    #[tokio::test]
    async fn recent_auth_compares_auth_time_with_the_clock() {
        let guard = recent_auth(Duration::from_secs(600));
        let now = chrono::Utc::now().timestamp();
        let check = |claims: Option<AccessClaims>| {
            let guard = guard.clone();
            async move {
                let mut parts = parts();
                if let Some(claims) = claims {
                    parts.extensions.insert(claims);
                }
                guard.check(&mut parts).await
            }
        };
        assert!(
            check(Some(claims(TokenType::Access, Some(now - 60), None)))
                .await
                .is_ok()
        );
        for refused in [
            Some(claims(TokenType::Access, Some(now - 601), None)),
            Some(claims(TokenType::Access, None, None)),
            Some(claims(TokenType::Access, Some(now), Some(Uuid::now_v7()))),
            None,
        ] {
            let error = check(refused).await.unwrap_err();
            assert_eq!(error.code, "reauthentication_required");
        }
        assert_eq!(format!("{guard:?}"), "recent_auth(600s)");
    }

    #[tokio::test]
    async fn platform_session_admits_platform_tokens_only() {
        let guard = platform_session();
        let mut platform = parts();
        platform
            .extensions
            .insert(claims(TokenType::Platform, Some(0), None));
        assert!(guard.check(&mut platform).await.is_ok());
        let mut app = parts();
        app.extensions
            .insert(claims(TokenType::Access, Some(0), None));
        assert_eq!(
            guard.check(&mut app).await.unwrap_err().code,
            "platform_session_required"
        );
        assert!(guard.check(&mut parts()).await.is_err());
    }

    #[test]
    fn actor_names_the_real_user() {
        let (user, admin) = (Uuid::now_v7(), Uuid::now_v7());
        let plain = Actor {
            user_id: user,
            impersonator_id: None,
        };
        assert!(!plain.is_impersonating());
        assert_eq!(plain.real_user_id(), user);
        let impersonated = Actor {
            user_id: user,
            impersonator_id: Some(admin),
        };
        assert!(impersonated.is_impersonating());
        assert_eq!(impersonated.real_user_id(), admin);
        let claims = claims(TokenType::Access, Some(5), Some(admin));
        assert_eq!(claims.impersonator(), Some(admin));
        assert_eq!(claims.auth_time(), Some(5));
        assert!(claims.session_id().is_some());
        assert!(!claims.is_platform());
    }
}
