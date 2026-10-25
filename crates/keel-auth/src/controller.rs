//! The auth routes under `{api_prefix}{routes_prefix}` (default
//! `/api/auth`): `register`, `login`, `refresh`, `logout` and `me`.

use std::sync::Arc;

use axum::extract::{FromRequest, Request};
use axum::response::{IntoResponse, Response};
use garde::Validate;
use http::header::CONTENT_TYPE;
use http::{Method, StatusCode};
use keel_db::Uow;
use keel_http::{
    ApiMeta, AppError, Controller, CurrentPrincipal, Json, NoContent, RouteDef, schema_fn,
};
use keel_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use keel_validation::Valid;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

use crate::{
    AuthError, AuthOptions, AuthService, ClientMeta, CurrentUser, LoginOutcome, NewUser, TokenPair,
    User, UserResource,
};

/// `POST /auth/register`.
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct RegisterInput {
    /// The display name.
    #[garde(length(min = 1, max = 255))]
    pub name: String,
    /// The email address.
    #[garde(email, length(max = 255))]
    pub email: String,
    /// The password (at least `auth.password_min_length` characters).
    #[garde(length(max = 1024))]
    #[schema(format = Password)]
    pub password: String,
}

/// `POST /auth/login`.
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct LoginInput {
    /// The email address.
    #[garde(email, length(max = 255))]
    pub email: String,
    /// The password.
    #[garde(length(min = 1, max = 1024))]
    #[schema(format = Password)]
    pub password: String,
}

/// `POST /auth/refresh`.
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct RefreshInput {
    /// The refresh token of the last token pair.
    #[garde(length(min = 1, max = 512))]
    pub refresh_token: String,
}

/// `POST /auth/logout` (an optional body).
#[derive(Debug, Default, Deserialize, Validate, ToSchema)]
pub struct LogoutInput {
    /// The session to end (its refresh token).
    #[garde(length(max = 512))]
    #[serde(default)]
    pub refresh_token: Option<String>,
    /// End every session of the user.
    #[garde(skip)]
    #[serde(default)]
    pub all: bool,
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
                match outcome {
                    LoginOutcome::Authenticated { user, tokens } => {
                        signed_in(&service, &user, tokens, StatusCode::OK).await
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
                           OptionalBody(input): OptionalBody<LogoutInput>| {
            let service = service.clone();
            async move {
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

/// A JSON body validated like `Valid<Json<T>>`, or `T::default()` when the
/// request has no `Content-Type` (no body).
struct OptionalBody<T>(T);

impl<T, S> FromRequest<S> for OptionalBody<T>
where
    T: DeserializeOwned + Validate<Context = ()> + Default + Send,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        if !req.headers().contains_key(CONTENT_TYPE) {
            return Ok(OptionalBody(T::default()));
        }
        let Valid(Json(value)) = Valid::<Json<T>>::from_request(req, state).await?;
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
