//! Passport-style [`Strategy`]s, the built-in [`JwtStrategy`] and the
//! [`StrategyAuthenticator`] that runs them for keel-http.

use std::fmt;
use std::sync::Arc;

use http::header::AUTHORIZATION;
use http::request::Parts;
use keel_contracts::{AuthVia, BoxFuture, Principal};
use keel_http::{AppError, Authenticator};

use crate::{AuthError, JwtService, TokenType};

/// One way of authenticating a request (a Passport strategy). Register
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
/// [`AuthVia::Token`] with no token restrictions. No `Authorization`
/// header, another scheme, or a bearer value that is not shaped like a JWT
/// (`<header>.<payload>.<signature>`, e.g. an opaque API token or
/// keel-observe's metrics scrape token) is not its business. A JWT that is
/// forged, of another type, for another issuer or expired is a 401
/// (`"Invalid token"` / `"Token expired"`) with a `WWW-Authenticate:
/// Bearer` challenge, on every route.
pub struct JwtStrategy {
    jwt: Arc<JwtService>,
}

impl JwtStrategy {
    /// The strategy verifying with `jwt`.
    pub fn new(jwt: Arc<JwtService>) -> Self {
        Self { jwt }
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
        let result = match bearer_token(parts) {
            None => Ok(None),
            Some(token) => self
                .jwt
                .verify(token, TokenType::Access)
                .map(|claims| {
                    Some(Principal {
                        user_id: claims.sub,
                        via: AuthVia::Token,
                        token_abilities: None,
                        session_id: None,
                    })
                })
                .map_err(AuthError::into_app_error),
        };
        Box::pin(async move { result })
    }
}

/// The JWT of the request's `Authorization: Bearer` header, if it carries
/// one. `None` without the header, with another scheme, or with a bearer
/// value that is not shaped like a JWT (an opaque API token, the
/// `/metrics` scrape token of keel-observe): those are left to other
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

/// keel-http's `dyn Authenticator` for the auth module: runs the
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
                            target: "keel::auth",
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
