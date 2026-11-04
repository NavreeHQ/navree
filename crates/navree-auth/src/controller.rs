//! The auth routes under `{api_prefix}{routes_prefix}` (default
//! `/api/auth`): `register`, `login`, `refresh`, `logout` and `me`.

use std::convert::Infallible;
use std::fmt;
use std::sync::Arc;

use http::header::CONTENT_TYPE;
use http::request::Parts;
use http::{Method, StatusCode};
use navree_db::Uow;
use navree_http::extract::{FromRequest, FromRequestParts, Request};
use navree_http::{
    ApiMeta, AppError, Controller, CurrentPrincipal, IntoResponse, Json, NoContent, Response,
    RouteDef, schema_fn,
};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree_openapi::ToSchema;
use navree_validation::Valid;
use navree_validation::Validate;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    AuthError, AuthOptions, AuthService, ClientMeta, CurrentUser, LoginOutcome, NewUser, TokenPair,
    User, UserResource,
};

/// `POST /auth/register`. `Debug` never shows the password.
#[derive(Deserialize, Validate, ToSchema)]
pub struct RegisterInput {
    /// The display name.
    #[validate(length(min = 1, max = 255))]
    pub name: String,
    /// The email address.
    #[validate(email, length(max = 255))]
    pub email: String,
    /// The password (at least `auth.password_min_length` characters).
    #[validate(length(max = 1024))]
    #[schema(format = Password)]
    pub password: String,
}

/// `POST /auth/login`. `Debug` never shows the password.
#[derive(Deserialize, Validate, ToSchema)]
pub struct LoginInput {
    /// The email address.
    #[validate(email, length(max = 255))]
    pub email: String,
    /// The password.
    #[validate(length(min = 1, max = 1024))]
    #[schema(format = Password)]
    pub password: String,
}

/// `POST /auth/refresh`. `Debug` never shows the token.
#[derive(Deserialize, Validate, ToSchema)]
pub struct RefreshInput {
    /// The refresh token of the last token pair.
    #[validate(length(min = 1, max = 512))]
    pub refresh_token: String,
}

/// `POST /auth/logout` (an optional body). `Debug` never shows the token.
#[derive(Default, Deserialize, Validate, ToSchema)]
pub struct LogoutInput {
    /// The session to end (its refresh token).
    #[validate(length(max = 512))]
    #[serde(default)]
    pub refresh_token: Option<String>,
    /// End every session of the user.
    #[validate(skip)]
    #[serde(default)]
    pub all: bool,
}

/// Passwords and refresh tokens are credentials: never in debug output.
impl fmt::Debug for RegisterInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RegisterInput")
            .field("name", &self.name)
            .field("email", &self.email)
            .field("password", &"[redacted]")
            .finish()
    }
}

impl fmt::Debug for LoginInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoginInput")
            .field("email", &self.email)
            .field("password", &"[redacted]")
            .finish()
    }
}

impl fmt::Debug for RefreshInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RefreshInput")
            .field("refresh_token", &"[redacted]")
            .finish()
    }
}

impl fmt::Debug for LogoutInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LogoutInput")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "[redacted]"),
            )
            .field("all", &self.all)
            .finish()
    }
}

/// The login and register response: the user and a token pair.
#[derive(Debug, Serialize, ToSchema)]
pub struct LoginResponse {
    /// The signed-in user.
    pub user: UserResource,
    /// The tokens.
    #[serde(flatten)]
    pub tokens: TokenPair,
}

/// The login response of an account with two-factor authentication:
/// `{"two_factor_required": true, "challenge_token", "expires_in"}`.
#[derive(Serialize, ToSchema)]
pub struct TwoFactorChallenge {
    /// Always `true`.
    pub two_factor_required: bool,
    /// The challenge token for `POST /auth/2fa/verify` (not an access token).
    pub challenge_token: String,
    /// The challenge token's lifetime in seconds.
    pub expires_in: u64,
}

/// The challenge token is a credential: never in debug output.
impl std::fmt::Debug for TwoFactorChallenge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TwoFactorChallenge")
            .field("challenge_token", &"[redacted]")
            .field("expires_in", &self.expires_in)
            .finish()
    }
}

/// The documented `POST /auth/login` response (OpenAPI `oneOf`): the user
/// and tokens, or a [`TwoFactorChallenge`] for accounts with two-factor
/// authentication, so generated clients see both shapes.
#[derive(Serialize, ToSchema)]
#[serde(untagged)]
pub(crate) enum LoginResult {
    /// Signed in. (The handler renders this through [`signed_in`], which
    /// adds the user resource's extension fields.)
    #[allow(dead_code)]
    Authenticated(LoginResponse),
    /// A second factor is needed.
    TwoFactor(TwoFactorChallenge),
}

/// The auth routes.
pub(crate) struct AuthController {
    service: Arc<AuthService>,
    options: Arc<AuthOptions>,
}

impl Injectable for AuthController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![
            ProviderKey::of::<AuthService>(),
            ProviderKey::of::<AuthOptions>(),
        ]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(AuthController {
                service: r.get::<AuthService>().await?,
                options: r.get::<AuthOptions>().await?,
            })
        })
    }
}

/// `{"user": <resource>, "access_token", "refresh_token", "token_type", "expires_in"}`.
pub(crate) async fn signed_in(
    service: &AuthService,
    user: &User,
    tokens: TokenPair,
    status: StatusCode,
) -> Result<Response, AppError> {
    let user = service
        .present(user)
        .await
        .map_err(AuthError::into_app_error)?;
    let mut body = serde_json::to_value(&tokens).map_err(AppError::internal)?;
    if let Value::Object(fields) = &mut body {
        fields.insert("user".to_owned(), user);
    }
    Ok((status, Json(body)).into_response())
}

impl Controller for AuthController {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let prefix = self.options.routes_prefix.trim_end_matches('/');
        let path = |p: &str| format!("{prefix}{p}");
        let mut routes = Vec::new();

        if self.options.registration {
            let service = self.service.clone();
            let register =
                move |uow: Uow,
                      meta: ClientMeta,
                      Valid(Json(input)): Valid<Json<RegisterInput>>| {
                    let service = service.clone();
                    async move {
                        let new = NewUser {
                            name: input.name,
                            email: input.email,
                            password: input.password,
                        };
                        let user = service
                            .register(&uow, new)
                            .await
                            .map_err(AuthError::into_app_error)?;
                        let tokens = service
                            .issue_tokens(&user, meta)
                            .await
                            .map_err(AuthError::into_app_error)?;
                        signed_in(&service, &user, tokens, StatusCode::CREATED).await
                    }
                };
            routes.push(
                RouteDef::on(Method::POST, path("/register"), register)
                    .public()
                    .name("auth.register")
                    .api(ApiMeta {
                        success_status: 201,
                        request: Some(schema_fn::<RegisterInput>),
                        response: Some(schema_fn::<LoginResponse>),
                        ..api("Create an account and sign in", &[400, 409])
                    }),
            );
        }

        let service = self.service.clone();
        let login = move |meta: ClientMeta, Valid(Json(input)): Valid<Json<LoginInput>>| {
            let service = service.clone();
            async move {
                let outcome = service
                    .attempt_login(&input.email, &input.password, &meta)
                    .await
                    .map_err(AuthError::into_app_error)?;
                login_response(&service, outcome).await
            }
        };
        routes.push(
            RouteDef::on(Method::POST, path("/login"), login)
                .public()
                .name("auth.login")
                .api(ApiMeta {
                    description: Some(
                        "When the account uses two-factor authentication the answer is a \
                         `TwoFactorChallenge` (`{two_factor_required: true, challenge_token, \
                         expires_in}`) instead; finish with `POST /auth/2fa/verify`."
                            .to_owned(),
                    ),
                    request: Some(schema_fn::<LoginInput>),
                    response: Some(schema_fn::<LoginResult>),
                    ..api("Sign in with email and password", &[400, 401, 429])
                }),
        );

        let service = self.service.clone();
        let refresh = move |meta: ClientMeta, Valid(Json(input)): Valid<Json<RefreshInput>>| {
            let service = service.clone();
            async move {
                let tokens = service
                    .refresh(&input.refresh_token, meta)
                    .await
                    .map_err(AuthError::into_app_error)?;
                Ok::<_, AppError>(Json(tokens))
            }
        };
        routes.push(
            RouteDef::on(Method::POST, path("/refresh"), refresh)
                .public()
                .name("auth.refresh")
                .api(ApiMeta {
                    request: Some(schema_fn::<RefreshInput>),
                    response: Some(schema_fn::<TokenPair>),
                    ..api("Exchange a refresh token for a new token pair", &[400, 401])
                }),
        );

        let service = self.service.clone();
        let logout = move |CurrentPrincipal(principal): CurrentPrincipal,
                           meta: ClientMeta,
                           browser: BrowserSession,
                           OptionalBody(input): OptionalBody<LogoutInput>| {
            let service = service.clone();
            async move {
                // A browser signed in on its session: that sign-in ends, and
                // with `all` every other session of the user too.
                #[cfg(feature = "session")]
                if let Some(session) = browser.0 {
                    service
                        .sign_out_session(&session, &meta)
                        .await
                        .map_err(AuthError::into_app_error)?;
                    if input.all {
                        service
                            .logout(None, principal.user_id, true)
                            .await
                            .map_err(AuthError::into_app_error)?;
                    }
                    return Ok::<_, AppError>(NoContent);
                }
                #[cfg(not(feature = "session"))]
                let _ = (browser, meta);
                service
                    .logout(input.refresh_token.as_deref(), principal.user_id, input.all)
                    .await
                    .map_err(AuthError::into_app_error)?;
                Ok::<_, AppError>(NoContent)
            }
        };
        routes.push(
            RouteDef::on(Method::POST, path("/logout"), logout)
                .authenticated()
                .name("auth.logout")
                .api(ApiMeta {
                    success_status: 204,
                    request: Some(schema_fn::<LogoutInput>),
                    description: Some(
                        "A browser signed in on the session cookie is signed out of it (and, with \
                         `all`, every other session ends too); it sends its CSRF token."
                            .to_owned(),
                    ),
                    ..api("Sign out: revoke a session, or every session", &[400, 401])
                }),
        );

        let service = self.service.clone();
        let me = move |CurrentUser(user): CurrentUser| {
            let service = service.clone();
            async move {
                let user = service
                    .present(&user)
                    .await
                    .map_err(AuthError::into_app_error)?;
                Ok::<_, AppError>(Json(user))
            }
        };
        routes.push(
            RouteDef::on(Method::GET, path("/me"), me)
                .authenticated()
                .name("auth.me")
                .api(ApiMeta {
                    response: Some(schema_fn::<UserResource>),
                    ..api("The signed-in user", &[401])
                }),
        );
        routes
    }
}

/// The response to a sign-in: the user and tokens, or a
/// [`TwoFactorChallenge`].
pub(crate) async fn login_response(
    service: &AuthService,
    outcome: LoginOutcome,
) -> Result<Response, AppError> {
    match outcome {
        LoginOutcome::Authenticated { user, tokens } => {
            signed_in(service, &user, tokens, StatusCode::OK).await
        }
        LoginOutcome::TwoFactorRequired {
            challenge_token, ..
        } => Ok(Json(LoginResult::TwoFactor(TwoFactorChallenge {
            two_factor_required: true,
            challenge_token,
            expires_in: service.options().two_factor.challenge_ttl_secs,
        }))
        .into_response()),
    }
}

/// The request's navree-session session, when the request is signed in
/// with it (a browser, feature `session`); nothing otherwise. The routes
/// that answer a browser differently from a token client take it.
pub(crate) struct BrowserSession(
    #[cfg(feature = "session")] pub(crate) Option<navree_session::Session>,
);

impl FromRequestParts for BrowserSession {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        #[cfg(feature = "session")]
        {
            let via_session = parts
                .extensions
                .get::<navree_contracts::AuthState>()
                .and_then(|state| state.0.as_ref())
                .is_some_and(|principal| principal.via == navree_contracts::AuthVia::Session);
            let session = parts
                .extensions
                .get::<navree_session::Session>()
                .filter(|_| via_session)
                .cloned();
            Ok(BrowserSession(session))
        }
        #[cfg(not(feature = "session"))]
        {
            let _ = parts;
            Ok(BrowserSession())
        }
    }
}

/// A JSON body validated like `Valid<Json<T>>`, or `T::default()` when the
/// request has no `Content-Type` (no body).
pub(crate) struct OptionalBody<T>(pub(crate) T);

impl<T> FromRequest for OptionalBody<T>
where
    T: DeserializeOwned + Validate + Default + Send + 'static,
{
    type Rejection = AppError;

    async fn from_request(req: Request) -> Result<Self, Self::Rejection> {
        if !req.headers().contains_key(CONTENT_TYPE) {
            return Ok(OptionalBody(T::default()));
        }
        let Valid(Json(value)) = <Valid<Json<T>> as FromRequest>::from_request(req).await?;
        Ok(OptionalBody(value))
    }
}

pub(crate) fn api(summary: &str, responses: &[u16]) -> ApiMeta {
    ApiMeta {
        summary: Some(summary.to_owned()),
        tags: vec!["Auth".to_owned()],
        responses: responses.to_vec(),
        ..ApiMeta::default()
    }
}

#[cfg(test)]
mod tests {
    use navree_http::handler::Endpoint;

    use super::*;
    use crate::{AccessClaims, Actor, PlatformAdmin};

    #[test]
    fn request_bodies_never_show_passwords_or_tokens() {
        let register: RegisterInput = serde_json::from_value(serde_json::json!({
            "name": "Navjot Kaur",
            "email": "navjot@example.com",
            "password": "register-pa55word",
        }))
        .unwrap();
        let login: LoginInput = serde_json::from_value(serde_json::json!({
            "email": "navjot@example.com",
            "password": "login-pa55word",
        }))
        .unwrap();
        let refresh: RefreshInput =
            serde_json::from_value(serde_json::json!({ "refresh_token": "refresh-t0ken" }))
                .unwrap();
        let logout: LogoutInput =
            serde_json::from_value(serde_json::json!({ "refresh_token": "logout-t0ken" })).unwrap();
        let printed = format!("{register:?} {login:?} {refresh:?} {logout:?}");
        for secret in [
            "register-pa55word",
            "login-pa55word",
            "refresh-t0ken",
            "logout-t0ken",
        ] {
            assert!(!printed.contains(secret), "{secret} in {printed}");
        }
        assert!(printed.contains("navjot@example.com"), "{printed}");
    }

    #[test]
    fn handlers_on_the_auth_extractors_are_native() {
        async fn handler(
            _: CurrentUser,
            _: ClientMeta,
            _: AccessClaims,
            _: Option<AccessClaims>,
            _: Actor,
            _: PlatformAdmin,
            _: OptionalBody<LogoutInput>,
        ) {
        }
        let _ = Endpoint::new(handler);
    }

    fn logout_request(body: &'static str, limit: Option<usize>) -> Request {
        let mut request = http::Request::post("/logout")
            .header(CONTENT_TYPE, "application/json")
            .body(navree_http::body::Body::from(body))
            .unwrap();
        if let Some(limit) = limit {
            request
                .extensions_mut()
                .insert(navree_http::body::BodyLimit(limit));
        }
        request
    }

    #[tokio::test]
    async fn the_optional_body_is_read_within_the_routes_limit() {
        let body = r#"{"refresh_token":"abcdefghij","all":true}"#;
        let OptionalBody(input) =
            OptionalBody::<LogoutInput>::from_request(logout_request(body, Some(64)))
                .await
                .unwrap();
        assert_eq!(input.refresh_token.as_deref(), Some("abcdefghij"));
        assert!(input.all);

        let error = OptionalBody::<LogoutInput>::from_request(logout_request(body, Some(16)))
            .await
            .err()
            .expect("past the limit");
        assert_eq!(error.status, StatusCode::PAYLOAD_TOO_LARGE);
    }
}
