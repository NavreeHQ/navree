//! Pluggable [`Strategy`]s, the built-in [`JwtStrategy`] and the
//! [`StrategyAuthenticator`] that runs them for navree-http.

use std::fmt;
use std::sync::Arc;

use http::Method;
use http::header::AUTHORIZATION;
use http::request::Parts;
use navree_contracts::{AuthVia, BoxFuture, Impersonator, Principal};
use navree_http::{AppError, Authenticator};

use crate::{AccessClaims, AuthError, JwtService, TokenType};

/// The token types the JWT strategy accepts as credentials.
const BEARER_TYPES: [TokenType; 3] = [
    TokenType::Access,
    TokenType::TwoFactorSetup,
    TokenType::Platform,
];

/// One way of authenticating a request (a strategy). Register
/// strategies as `dyn Strategy` multi-bindings (`m.multi(Factory::<dyn
/// Strategy>::new(..))` or
/// [`ModuleBuilderAuthExt::strategy`](crate::ModuleBuilderAuthExt::strategy));
/// the auth module's `dyn Authenticator` runs them in module dependency
/// order, the built-in [`JwtStrategy`] (`"jwt"`) first.
///
/// - `Ok(None)`: the request carries no credentials of this kind; the next
///   strategy runs (a request no strategy recognises is a guest).
/// - `Ok(Some(principal))`: authenticated; later strategies do not run.
/// - `Err(error)`: credentials of this kind are present but invalid; the
///   request fails with `error` (usually a 401), even on public routes. It
///   never falls back to another strategy or to a guest.
pub trait Strategy: Send + Sync + 'static {
    /// The strategy name, for logs (`"jwt"`, `"api-key"`).
    fn name(&self) -> &'static str;

    /// The principal of the request, if it carries this kind of credentials.
    fn authenticate<'a>(
        &'a self,
        parts: &'a mut Parts,
    ) -> BoxFuture<'a, Result<Option<Principal>, AppError>>;
}

/// The built-in strategy (`"jwt"`): `Authorization: Bearer <access token>`
/// verified by [`JwtService`] gives a [`Principal`] authenticated
/// [`AuthVia::Token`] with no token restrictions and the token's session
/// (its `sid` claim, when it has one) as `session_id`, and stores the token's
/// [`AccessClaims`] in the request (plus navree-contracts' `Impersonator`
/// for an impersonation token, which the unit of work and the audit trail
/// pick up). No `Authorization` header, another scheme, or a bearer value
/// that is not shaped like a JWT (`<header>.<payload>.<signature>`, e.g. an
/// opaque API token or navree-observe's metrics scrape token) is not its
/// business. A JWT that is forged, of another type, for another issuer or
/// expired is a 401 (`"Invalid token"` / `"Token expired"`) with a
/// `WWW-Authenticate: Bearer` challenge, on every route.
///
/// Access tokens of type [`TokenType::TwoFactorSetup`] (a user who must
/// set up two-factor authentication first) authenticate only the setup
/// routes given to [`with_setup_routes`](JwtStrategy::with_setup_routes);
/// anywhere else they are 403 `two_factor_required`.
pub struct JwtStrategy {
    jwt: Arc<JwtService>,
    setup_routes: Arc<[(Method, String)]>,
}

impl JwtStrategy {
    /// The strategy verifying with `jwt`.
    pub fn new(jwt: Arc<JwtService>) -> Self {
        Self {
            jwt,
            setup_routes: Arc::from(Vec::new()),
        }
    }

    /// The routes (method and full path) a two-factor setup token reaches.
    /// The auth module passes its own: `POST {prefix}/2fa/enable`, `POST
    /// {prefix}/2fa/confirm`, `GET {prefix}/me`, `POST {prefix}/logout`
    /// and `POST {prefix}/refresh`.
    pub fn with_setup_routes(mut self, routes: impl IntoIterator<Item = (Method, String)>) -> Self {
        self.setup_routes = routes.into_iter().collect();
        self
    }

    fn is_setup_route(&self, parts: &Parts) -> bool {
        let path = parts.uri.path().trim_end_matches('/');
        self.setup_routes
            .iter()
            .any(|(method, route)| *method == parts.method && route == path)
    }
}

impl fmt::Debug for JwtStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JwtStrategy").finish_non_exhaustive()
    }
}

impl Strategy for JwtStrategy {
    fn name(&self) -> &'static str {
        "jwt"
    }

    fn authenticate<'a>(
        &'a self,
        parts: &'a mut Parts,
    ) -> BoxFuture<'a, Result<Option<Principal>, AppError>> {
        let result = self.principal(parts);
        Box::pin(async move { result })
    }
}

impl JwtStrategy {
    fn principal(&self, parts: &mut Parts) -> Result<Option<Principal>, AppError> {
        let Some(token) = bearer_token(parts) else {
            return Ok(None);
        };
        let claims = self
            .jwt
            .verify_any(token, &BEARER_TYPES)
            .map_err(AuthError::into_app_error)?;
        if claims.typ == TokenType::TwoFactorSetup && !self.is_setup_route(parts) {
            return Err(AuthError::TwoFactorSetupRequired.into_app_error());
        }
        if let Some(act) = claims.act {
            parts.extensions.insert(Impersonator(act.sub));
        }
        let user_id = claims.sub;
        // The token's session (`sid`), so whatever is bound to a session
        // (call tickets, push devices) is bound for bearer clients too.
        let session_id = claims.sid.map(|sid| sid.to_string());
        parts.extensions.insert(AccessClaims(claims));
        Ok(Some(Principal {
            user_id,
            via: AuthVia::Token,
            token_abilities: None,
            session_id,
        }))
    }
}

/// The JWT of the request's `Authorization: Bearer` header, if it carries
/// one. `None` without the header, with another scheme, or with a bearer
/// value that is not shaped like a JWT (an opaque API token, the
/// `/metrics` scrape token of navree-observe): those are left to other
/// strategies and to route guards, and a request nobody recognises is a
/// guest (401 on every non-public route).
fn bearer_token(parts: &Parts) -> Option<&str> {
    let bytes = parts.headers.get(AUTHORIZATION)?.as_bytes();
    const SCHEME: &[u8] = b"bearer ";
    if bytes.len() < SCHEME.len() || !bytes[..SCHEME.len()].eq_ignore_ascii_case(SCHEME) {
        return None;
    }
    let token = std::str::from_utf8(&bytes[SCHEME.len()..]).ok()?.trim();
    is_jwt_shaped(token).then_some(token)
}

/// Whether `token` looks like a compact JWS: three `.`-separated base64url
/// segments, the header and payload not empty (an empty signature is
/// still claimed, so `alg: none` tokens are rejected, not ignored).
fn is_jwt_shaped(token: &str) -> bool {
    let mut segments = token.split('.');
    let (Some(header), Some(payload), Some(signature), None) = (
        segments.next(),
        segments.next(),
        segments.next(),
        segments.next(),
    ) else {
        return false;
    };
    let base64url = |s: &str| {
        s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'='))
    };
    !header.is_empty()
        && !payload.is_empty()
        && base64url(header)
        && base64url(payload)
        && base64url(signature)
}

/// navree-http's `dyn Authenticator` for the auth module: runs the
/// [`Strategy`]s in order (see the trait for the rules).
pub struct StrategyAuthenticator {
    strategies: Vec<Arc<dyn Strategy>>,
}

impl StrategyAuthenticator {
    /// An authenticator running `strategies` in order.
    pub fn new(strategies: Vec<Arc<dyn Strategy>>) -> Self {
        Self { strategies }
    }

    /// The strategy names, in the order they run.
    pub fn names(&self) -> Vec<&'static str> {
        self.strategies.iter().map(|s| s.name()).collect()
    }
}

impl fmt::Debug for StrategyAuthenticator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StrategyAuthenticator")
            .field("strategies", &self.names())
            .finish()
    }
}

impl Authenticator for StrategyAuthenticator {
    fn authenticate<'a>(
        &'a self,
        parts: &'a mut Parts,
    ) -> BoxFuture<'a, Result<Option<Principal>, AppError>> {
        Box::pin(async move {
            for strategy in &self.strategies {
                match strategy.authenticate(parts).await {
                    Ok(None) => continue,
                    Ok(Some(principal)) => return Ok(Some(principal)),
                    Err(error) => {
                        tracing::debug!(
                            target: "navree::auth",
                            strategy = strategy.name(),
                            status = error.status.as_u16(),
                            "authentication failed"
                        );
                        return Err(error);
                    }
                }
            }
            Ok(None)
        })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use http::Request;
    use uuid::Uuid;

    use super::*;
    use crate::JwtOptions;

    fn jwt() -> Arc<JwtService> {
        Arc::new(JwtService::new(
            b"0123456789abcdef0123456789abcdef",
            &JwtOptions::default(),
        ))
    }

    fn parts(authorization: Option<&str>) -> Parts {
        let mut request = Request::builder();
        if let Some(value) = authorization {
            request = request.header(AUTHORIZATION, value);
        }
        request.body(()).unwrap().into_parts().0
    }

    #[tokio::test]
    async fn the_claims_and_the_impersonator_reach_the_request() {
        let jwt = jwt();
        let strategy = JwtStrategy::new(jwt.clone());
        let (user, admin, session) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
        let token = jwt
            .sign_session(
                user,
                TokenType::Access,
                Duration::from_secs(60),
                crate::SessionClaims {
                    session_id: Some(session),
                    auth_time: None,
                    impersonator: Some(admin),
                },
            )
            .unwrap();
        let mut parts = parts(Some(&format!("Bearer {token}")));
        let principal = strategy.authenticate(&mut parts).await.unwrap().unwrap();
        assert_eq!(principal.user_id, user);
        assert_eq!(
            parts.extensions.get::<Impersonator>(),
            Some(&Impersonator(admin))
        );
        let claims = parts.extensions.get::<AccessClaims>().unwrap();
        assert_eq!(claims.session_id(), Some(session));

        let plain = jwt.sign_access(user).unwrap();
        let mut parts = self::parts(Some(&format!("Bearer {plain}")));
        strategy.authenticate(&mut parts).await.unwrap().unwrap();
        assert!(parts.extensions.get::<Impersonator>().is_none());

        let platform = jwt
            .sign(user, TokenType::Platform, Duration::from_secs(60))
            .unwrap();
        let mut parts = self::parts(Some(&format!("Bearer {platform}")));
        strategy.authenticate(&mut parts).await.unwrap().unwrap();
        assert!(
            parts
                .extensions
                .get::<AccessClaims>()
                .unwrap()
                .is_platform()
        );
    }

    #[tokio::test]
    async fn a_bearer_principal_names_the_session_of_its_token() {
        let jwt = jwt();
        let strategy = JwtStrategy::new(jwt.clone());
        let (user, session) = (Uuid::now_v7(), Uuid::now_v7());
        let token = jwt
            .sign_session(
                user,
                TokenType::Access,
                Duration::from_secs(60),
                crate::SessionClaims {
                    session_id: Some(session),
                    auth_time: None,
                    impersonator: None,
                },
            )
            .unwrap();
        let mut parts = parts(Some(&format!("Bearer {token}")));
        let principal = strategy.authenticate(&mut parts).await.unwrap().unwrap();
        assert_eq!(principal.via, AuthVia::Token);
        assert_eq!(principal.session_id, Some(session.to_string()));

        // A token of no session names none.
        let plain = jwt.sign_access(user).unwrap();
        let mut parts = self::parts(Some(&format!("Bearer {plain}")));
        let principal = strategy.authenticate(&mut parts).await.unwrap().unwrap();
        assert_eq!(principal.session_id, None);
    }

    #[tokio::test]
    async fn setup_tokens_reach_only_the_setup_routes() {
        let jwt = jwt();
        let strategy = JwtStrategy::new(jwt.clone()).with_setup_routes([
            (Method::POST, "/api/auth/2fa/enable".to_owned()),
            (Method::GET, "/api/auth/me".to_owned()),
        ]);
        let token = jwt
            .sign(
                Uuid::now_v7(),
                TokenType::TwoFactorSetup,
                Duration::from_secs(60),
            )
            .unwrap();
        let request = |method: Method, path: &str| {
            let mut parts = Request::builder()
                .method(method)
                .uri(path)
                .header(AUTHORIZATION, format!("Bearer {token}"))
                .body(())
                .unwrap()
                .into_parts()
                .0;
            let strategy = &strategy;
            async move { strategy.authenticate(&mut parts).await }
        };
        assert!(request(Method::POST, "/api/auth/2fa/enable").await.is_ok());
        assert!(request(Method::GET, "/api/auth/me/").await.is_ok());
        for (method, path) in [
            (Method::GET, "/api/deals"),
            (Method::PATCH, "/api/auth/me"),
            (Method::POST, "/api/auth/2fa/disable"),
        ] {
            let error = request(method, path).await.unwrap_err();
            assert_eq!(error.status.as_u16(), 403, "{path}");
            assert_eq!(error.code, "two_factor_required", "{path}");
        }
        // Without setup routes, nowhere.
        let bare = JwtStrategy::new(jwt.clone());
        let mut parts = parts(Some(&format!("Bearer {token}")));
        assert!(bare.authenticate(&mut parts).await.is_err());
    }

    #[tokio::test]
    async fn jwt_strategy_reads_bearer_tokens_only() {
        let jwt = jwt();
        let strategy = JwtStrategy::new(jwt.clone());
        let user = Uuid::now_v7();
        let token = jwt.sign_access(user).unwrap();

        for header in [format!("Bearer {token}"), format!("bearer  {token} ")] {
            let principal = strategy
                .authenticate(&mut parts(Some(&header)))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(principal.user_id, user);
            assert_eq!(principal.via, AuthVia::Token);
            assert_eq!(principal.token_abilities, None);
        }
        assert!(
            strategy
                .authenticate(&mut parts(None))
                .await
                .unwrap()
                .is_none()
        );
        // Not JWT-shaped: left to other strategies and route guards.
        for other in [
            "Basic YTpi",
            "Bearer ",
            "Bearer opaque-api-token",
            "Bearer 1|abcdef",
            "Bearer a.b",
            "Bearer a.b.c.d",
            "Bearer .b.c",
            "Bearer a b.c.d",
        ] {
            assert!(
                strategy
                    .authenticate(&mut parts(Some(other)))
                    .await
                    .unwrap()
                    .is_none(),
                "{other}"
            );
        }
        // JWT-shaped but not valid (an unsigned `alg: none` token included).
        let unsigned = {
            let mut segments: Vec<&str> = token.split('.').collect();
            segments[2] = "";
            segments.join(".")
        };
        for bad in [
            "Bearer x.y.z".to_owned(),
            format!("Bearer {unsigned}"),
            format!("Bearer {token}x"),
        ] {
            let error = strategy
                .authenticate(&mut parts(Some(&bad)))
                .await
                .unwrap_err();
            assert_eq!(error.status.as_u16(), 401, "{bad}");
        }
        let challenge = jwt
            .sign(user, TokenType::TwoFactorChallenge, Duration::from_secs(60))
            .unwrap();
        assert!(
            strategy
                .authenticate(&mut parts(Some(&format!("Bearer {challenge}"))))
                .await
                .is_err()
        );
    }

    struct Fixed(Option<Uuid>, bool);

    impl Strategy for Fixed {
        fn name(&self) -> &'static str {
            "fixed"
        }

        fn authenticate<'a>(
            &'a self,
            _parts: &'a mut Parts,
        ) -> BoxFuture<'a, Result<Option<Principal>, AppError>> {
            let result = if self.1 {
                Err(AppError::unauthenticated())
            } else {
                Ok(self.0.map(|user_id| Principal {
                    user_id,
                    via: AuthVia::Token,
                    token_abilities: None,
                    session_id: None,
                }))
            };
            Box::pin(async move { result })
        }
    }

    #[tokio::test]
    async fn the_first_strategy_with_an_answer_wins() {
        let (a, b) = (Uuid::now_v7(), Uuid::now_v7());
        let run = |strategies: Vec<Arc<dyn Strategy>>| async move {
            StrategyAuthenticator::new(strategies)
                .authenticate(&mut parts(None))
                .await
        };
        let found = run(vec![
            Arc::new(Fixed(None, false)),
            Arc::new(Fixed(Some(b), false)),
        ])
        .await;
        assert_eq!(found.unwrap().unwrap().user_id, b);
        let found = run(vec![
            Arc::new(Fixed(Some(a), false)),
            Arc::new(Fixed(Some(b), false)),
        ])
        .await;
        assert_eq!(found.unwrap().unwrap().user_id, a);
        let failed = run(vec![
            Arc::new(Fixed(None, true)),
            Arc::new(Fixed(Some(b), false)),
        ])
        .await;
        assert!(failed.is_err(), "an invalid credential never falls through");
        assert!(
            run(vec![Arc::new(Fixed(None, false))])
                .await
                .unwrap()
                .is_none()
        );
        assert!(run(Vec::new()).await.unwrap().is_none());
    }
}
