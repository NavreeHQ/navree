//! The session and sign-in routes under `{api_prefix}{routes_prefix}`:
//! sessions, step-up confirmation, impersonation, magic links, emailed
//! codes, the platform audience and token exchange. Optional routes are
//! mounted only when their configuration turns them on.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use http::{Method, StatusCode};
use navree_http::{
    ApiMeta, AppError, Controller, CurrentPrincipal, IntoResponse, Json, NoContent, Path, Response,
    RouteDef, schema_fn,
};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree_openapi::ToSchema;
use navree_validation::Valid;
use navree_validation::Validate;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::account_controller::{AuthMessage, TwoFactorVerifyInput, two_factor_proof};
use crate::controller::{BrowserSession, api, login_response, signed_in};
use crate::{
    AccessClaims, AuthError, AuthOptions, AuthService, ClientMeta, ConfirmProof, ConfirmedToken,
    LoginInput, LoginResponse, SessionResource, TwoFactorChallenge, not_while_impersonating,
};

/// How many passwordless mails one client may request per minute (on top
/// of the per-address `throttle_secs`).
const PASSWORDLESS_REQUESTS_PER_MINUTE: u32 = 10;

/// `GET /auth/sessions`: the user's live sessions.
#[derive(Debug, Serialize, ToSchema)]
pub struct SessionList {
    /// The sessions, most recently used first.
    pub data: Vec<SessionResource>,
}

/// `POST /auth/confirm`: the password or a code from the authenticator
/// app (exactly one).
#[derive(Default, Deserialize, Validate, ToSchema)]
pub struct ConfirmInput {
    /// The account password.
    #[validate(length(min = 1, max = 1024))]
    #[schema(format = Password)]
    #[serde(default)]
    pub password: Option<String>,
    /// A current code from the authenticator app.
    #[validate(length(min = 1, max = 16))]
    #[serde(default)]
    pub code: Option<String>,
}

/// `POST /auth/impersonate/{user}`.
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct ImpersonateInput {
    /// Why (required; recorded in the audit trail).
    #[validate(length(min = 1, max = 500))]
    pub reason: String,
}

/// `POST /auth/magic-link` and `POST /auth/otp`.
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct PasswordlessInput {
    /// The email address of the account.
    #[validate(email, length(max = 255))]
    pub email: String,
}

/// `POST /auth/magic-link/verify`.
#[derive(Deserialize, Validate, ToSchema)]
pub struct MagicLinkVerifyInput {
    /// The token from the link.
    #[validate(length(min = 1, max = 512))]
    pub token: String,
}

/// `POST /auth/otp/verify`.
#[derive(Deserialize, Validate, ToSchema)]
pub struct OtpVerifyInput {
    /// The email address the code was sent to.
    #[validate(email, length(max = 255))]
    pub email: String,
    /// The code from the mail.
    #[validate(length(min = 1, max = 16))]
    pub code: String,
}

/// `POST /auth/exchange`.
#[derive(Deserialize, Validate, ToSchema)]
pub struct ExchangeInput {
    /// A JWT signed by a trusted issuer.
    #[validate(length(min = 1, max = 16384))]
    pub token: String,
}

/// What the `Debug` output of inputs shows instead of credentials.
const REDACTED: &str = "[redacted]";

impl fmt::Debug for ConfirmInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConfirmInput")
            .field("password", &self.password.as_ref().map(|_| REDACTED))
            .field("code", &self.code.as_ref().map(|_| REDACTED))
            .finish()
    }
}

impl fmt::Debug for MagicLinkVerifyInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MagicLinkVerifyInput")
            .field("token", &REDACTED)
            .finish()
    }
}

impl fmt::Debug for OtpVerifyInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OtpVerifyInput")
            .field("email", &self.email)
            .field("code", &REDACTED)
            .finish()
    }
}

impl fmt::Debug for ExchangeInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExchangeInput")
            .field("token", &REDACTED)
            .finish()
    }
}

/// The answer to every passwordless request (it does not tell whether the
/// account exists).
const PASSWORDLESS_SENT: &str =
    "If the email address is registered, a sign-in message has been sent.";

/// The session and sign-in routes.
pub(crate) struct SessionController {
    service: Arc<AuthService>,
    options: Arc<AuthOptions>,
}

impl Injectable for SessionController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![
            ProviderKey::of::<AuthService>(),
            ProviderKey::of::<AuthOptions>(),
        ]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(SessionController {
                service: r.get::<AuthService>().await?,
                options: r.get::<AuthOptions>().await?,
            })
        })
    }
}

impl Controller for SessionController {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let prefix = self.options.routes_prefix.trim_end_matches('/');
        let path = |p: &str| format!("{prefix}{p}");
        let mut routes = self.session_routes(&path);
        if self.options.impersonation.enabled {
            routes.extend(self.impersonation_routes(&path));
        }
        if self.options.magic_link.enabled {
            routes.extend(self.magic_link_routes(&path));
        }
        if self.options.otp.enabled {
            routes.extend(self.otp_routes(&path));
        }
        if self.options.platform.enabled {
            routes.extend(self.platform_routes(&path));
        }
        if !self.options.trusted_issuers.is_empty() {
            routes.extend(self.exchange_routes(&path));
        }
        routes
    }
}

impl SessionController {
    /// `GET /sessions`, `DELETE /sessions/{id}`, `POST /confirm`.
    fn session_routes(&self, path: &dyn Fn(&str) -> String) -> Vec<RouteDef> {
        let mut routes = Vec::new();

        let service = self.service.clone();
        let list = move |CurrentPrincipal(principal): CurrentPrincipal,
                         claims: Option<AccessClaims>| {
            let service = service.clone();
            async move {
                let current = claims.and_then(|claims| claims.session_id());
                let data = service
                    .sessions(principal.user_id, current)
                    .await
                    .map_err(AuthError::into_app_error)?;
                Ok::<_, AppError>(Json(SessionList { data }))
            }
        };
        routes.push(
            RouteDef::on(Method::GET, path("/sessions"), list)
                .authenticated()
                .name("auth.sessions.index")
                .api(ApiMeta {
                    response: Some(schema_fn::<SessionList>),
                    ..api("The signed-in user's sessions", &[401])
                }),
        );

        let service = self.service.clone();
        let revoke = move |CurrentPrincipal(principal): CurrentPrincipal,
                           meta: ClientMeta,
                           Path(id): Path<Uuid>| {
            let service = service.clone();
            async move {
                service
                    .revoke_session(principal.user_id, id, &meta)
                    .await
                    .map_err(AuthError::into_app_error)?;
                Ok::<_, AppError>(NoContent)
            }
        };
        routes.push(
            RouteDef::on(Method::DELETE, path("/sessions/{id}"), revoke)
                .authenticated()
                .guard(not_while_impersonating())
                .name("auth.sessions.destroy")
                .api(ApiMeta {
                    success_status: 204,
                    ..api("End one of the user's sessions", &[401, 403, 404])
                }),
        );

        let service = self.service.clone();
        let confirm = move |claims: AccessClaims,
                            meta: ClientMeta,
                            browser: BrowserSession,
                            Valid(Json(input)): Valid<Json<ConfirmInput>>| {
            let service = service.clone();
            async move {
                let proof = match (input.password, input.code) {
                    (Some(password), None) => ConfirmProof::Password(password),
                    (None, Some(code)) => ConfirmProof::Code(code),
                    (None, None) => {
                        return Err(field_error(
                            "password",
                            "password should not be empty when code is missing",
                        ));
                    }
                    (Some(_), Some(_)) => {
                        return Err(field_error(
                            "password",
                            "password must be empty when code is given",
                        ));
                    }
                };
                // A browser signed in on its session: the sign-in's time
                // moves on; it gets no token.
                #[cfg(feature = "session")]
                if let Some(session) = browser.0 {
                    service
                        .confirm_session(&session, proof, &meta)
                        .await
                        .map_err(AuthError::into_app_error)?;
                    return Ok::<_, AppError>(NoContent.into_response());
                }
                #[cfg(not(feature = "session"))]
                let _ = browser;
                let confirmed = service
                    .confirm(&claims, proof, &meta)
                    .await
                    .map_err(AuthError::into_app_error)?;
                Ok::<_, AppError>(Json(confirmed).into_response())
            }
        };
        routes.push(
            RouteDef::on(Method::POST, path("/confirm"), confirm)
                .authenticated()
                .guard(not_while_impersonating())
                .name("auth.confirm")
                .api(ApiMeta {
                    description: Some(
                        "Confirms the password or a two-factor code and returns a new access \
                         token whose `auth_time` is now, for routes that need a recent sign-in. \
                         A browser signed in on the session cookie gets 204 No Content: its \
                         sign-in counts as recent from now."
                            .to_owned(),
                    ),
                    request: Some(schema_fn::<ConfirmInput>),
                    response: Some(schema_fn::<ConfirmedToken>),
                    ..api("Confirm your identity (step-up)", &[400, 401, 403, 429])
                }),
        );
        routes
    }

    /// `POST /impersonate/{user}`, `DELETE /impersonate`.
    fn impersonation_routes(&self, path: &dyn Fn(&str) -> String) -> Vec<RouteDef> {
        let mut routes = Vec::new();

        let service = self.service.clone();
        let start = move |CurrentPrincipal(principal): CurrentPrincipal,
                          claims: Option<AccessClaims>,
                          meta: ClientMeta,
                          Path(target): Path<Uuid>,
                          Valid(Json(input)): Valid<Json<ImpersonateInput>>| {
            let service = service.clone();
            async move {
                let (user, tokens) = service
                    .impersonate(
                        principal.user_id,
                        claims.as_ref().map(|claims| &claims.0),
                        target,
                        &input.reason,
                        &meta,
                    )
                    .await
                    .map_err(AuthError::into_app_error)?;
                let response = signed_in(&service, &user, tokens, StatusCode::OK).await?;
                with_impersonator(response, principal.user_id).await
            }
        };
        let permission = self.options.impersonation.permission.clone();
        routes.push(
            RouteDef::on(Method::POST, path("/impersonate/{user}"), start)
                .permissions(&[permission.as_str()])
                .guard(not_while_impersonating())
                .name("auth.impersonate.start")
                .api(ApiMeta {
                    description: Some(
                        "Signs in as the user for support. The tokens carry an `act` claim \
                         naming the impersonator; the session ends after \
                         `auth.impersonation.ttl_secs`."
                            .to_owned(),
                    ),
                    request: Some(schema_fn::<ImpersonateInput>),
                    response: Some(schema_fn::<LoginResponse>),
                    ..api("Impersonate a user", &[400, 401, 403, 404])
                }),
        );

        let service = self.service.clone();
        let stop = move |claims: AccessClaims, meta: ClientMeta| {
            let service = service.clone();
            async move {
                service
                    .stop_impersonating(&claims, &meta)
                    .await
                    .map_err(AuthError::into_app_error)?;
                Ok::<_, AppError>(NoContent)
            }
        };
        routes.push(
            RouteDef::on(Method::DELETE, path("/impersonate"), stop)
                .authenticated()
                .name("auth.impersonate.stop")
                .api(ApiMeta {
                    success_status: 204,
                    ..api("Stop impersonating", &[400, 401])
                }),
        );
        routes
    }

    /// `POST /magic-link`, `POST /magic-link/verify`.
    fn magic_link_routes(&self, path: &dyn Fn(&str) -> String) -> Vec<RouteDef> {
        let mut routes = Vec::new();
        let service = self.service.clone();
        let send = move |meta: ClientMeta, Valid(Json(input)): Valid<Json<PasswordlessInput>>| {
            let service = service.clone();
            async move {
                service
                    .send_magic_link(&input.email, &meta)
                    .await
                    .map_err(AuthError::into_app_error)?;
                Ok::<_, AppError>(sent())
            }
        };
        routes.push(
            RouteDef::on(Method::POST, path("/magic-link"), send)
                .public()
                .throttle(PASSWORDLESS_REQUESTS_PER_MINUTE, Duration::from_secs(60))
                .name("auth.magic_link.send")
                .api(ApiMeta {
                    success_status: 202,
                    request: Some(schema_fn::<PasswordlessInput>),
                    response: Some(schema_fn::<AuthMessage>),
                    ..api("Mail a sign-in link", &[400, 429])
                }),
        );

        let service = self.service.clone();
        let verify =
            move |meta: ClientMeta, Valid(Json(input)): Valid<Json<MagicLinkVerifyInput>>| {
                let service = service.clone();
                async move {
                    let outcome = service
                        .login_with_magic_link(&input.token, &meta)
                        .await
                        .map_err(AuthError::into_app_error)?;
                    login_response(&service, outcome).await
                }
            };
        routes.push(
            RouteDef::on(Method::POST, path("/magic-link/verify"), verify)
                .public()
                .name("auth.magic_link.verify")
                .api(ApiMeta {
                    request: Some(schema_fn::<MagicLinkVerifyInput>),
                    response: Some(schema_fn::<LoginResponse>),
                    ..api("Sign in with a magic link token", &[400, 401])
                }),
        );
        routes
    }

    /// `POST /otp`, `POST /otp/verify`.
    fn otp_routes(&self, path: &dyn Fn(&str) -> String) -> Vec<RouteDef> {
        let mut routes = Vec::new();
        let service = self.service.clone();
        let send = move |meta: ClientMeta, Valid(Json(input)): Valid<Json<PasswordlessInput>>| {
            let service = service.clone();
            async move {
                service
                    .send_login_code(&input.email, &meta)
                    .await
                    .map_err(AuthError::into_app_error)?;
                Ok::<_, AppError>(sent())
            }
        };
        routes.push(
            RouteDef::on(Method::POST, path("/otp"), send)
                .public()
                .throttle(PASSWORDLESS_REQUESTS_PER_MINUTE, Duration::from_secs(60))
                .name("auth.otp.send")
                .api(ApiMeta {
                    success_status: 202,
                    request: Some(schema_fn::<PasswordlessInput>),
                    response: Some(schema_fn::<AuthMessage>),
                    ..api("Mail a sign-in code", &[400, 429])
                }),
        );

        let service = self.service.clone();
        let verify = move |meta: ClientMeta, Valid(Json(input)): Valid<Json<OtpVerifyInput>>| {
            let service = service.clone();
            async move {
                let outcome = service
                    .login_with_code(&input.email, &input.code, &meta)
                    .await
                    .map_err(AuthError::into_app_error)?;
                login_response(&service, outcome).await
            }
        };
        routes.push(
            RouteDef::on(Method::POST, path("/otp/verify"), verify)
                .public()
                .name("auth.otp.verify")
                .api(ApiMeta {
                    request: Some(schema_fn::<OtpVerifyInput>),
                    response: Some(schema_fn::<LoginResponse>),
                    ..api("Sign in with an emailed code", &[400, 401, 429])
                }),
        );
        routes
    }

    /// `POST /platform/login`, `POST /platform/2fa/verify`.
    fn platform_routes(&self, path: &dyn Fn(&str) -> String) -> Vec<RouteDef> {
        let mut routes = Vec::new();
        let service = self.service.clone();
        let login = move |meta: ClientMeta, Valid(Json(input)): Valid<Json<LoginInput>>| {
            let service = service.clone();
            async move {
                let challenge_token = service
                    .platform_login(&input.email, &input.password, &meta)
                    .await
                    .map_err(AuthError::into_app_error)?;
                Ok::<_, AppError>(Json(TwoFactorChallenge {
                    two_factor_required: true,
                    challenge_token,
                    expires_in: service.options().two_factor.challenge_ttl_secs,
                }))
            }
        };
        routes.push(
            RouteDef::on(Method::POST, path("/platform/login"), login)
                .public()
                .name("auth.platform.login")
                .api(ApiMeta {
                    description: Some(
                        "The password step of a platform sign-in: always answers a two-factor \
                         challenge; finish with `POST /auth/platform/2fa/verify`."
                            .to_owned(),
                    ),
                    request: Some(schema_fn::<LoginInput>),
                    response: Some(schema_fn::<TwoFactorChallenge>),
                    ..api("Sign in to the platform", &[400, 401, 403, 429])
                }),
        );

        let service = self.service.clone();
        let verify =
            move |meta: ClientMeta, Valid(Json(input)): Valid<Json<TwoFactorVerifyInput>>| {
                let service = service.clone();
                async move {
                    let proof = two_factor_proof(input.code, input.recovery_code)?;
                    let (user, tokens) = service
                        .verify_platform_two_factor(&input.challenge_token, proof, &meta)
                        .await
                        .map_err(AuthError::into_app_error)?;
                    signed_in(&service, &user, tokens, StatusCode::OK).await
                }
            };
        routes.push(
            RouteDef::on(Method::POST, path("/platform/2fa/verify"), verify)
                .public()
                .name("auth.platform.verify")
                .api(ApiMeta {
                    request: Some(schema_fn::<TwoFactorVerifyInput>),
                    response: Some(schema_fn::<LoginResponse>),
                    ..api(
                        "Finish a platform sign-in with a code or a recovery code",
                        &[400, 401, 429],
                    )
                }),
        );
        routes
    }

    /// `POST /exchange`.
    fn exchange_routes(&self, path: &dyn Fn(&str) -> String) -> Vec<RouteDef> {
        let service = self.service.clone();
        let exchange = move |meta: ClientMeta, Valid(Json(input)): Valid<Json<ExchangeInput>>| {
            let service = service.clone();
            async move {
                let (user, tokens) = service
                    .exchange_token(&input.token, &meta)
                    .await
                    .map_err(AuthError::into_app_error)?;
                signed_in(&service, &user, tokens, StatusCode::OK).await
            }
        };
        vec![
            RouteDef::on(Method::POST, path("/exchange"), exchange)
                .public()
                .name("auth.exchange")
                .api(ApiMeta {
                    description: Some(
                        "Accepts a short-lived JWT signed by a system configured under \
                         `auth.trusted_issuers` (once) and signs its user in."
                            .to_owned(),
                    ),
                    request: Some(schema_fn::<ExchangeInput>),
                    response: Some(schema_fn::<LoginResponse>),
                    ..api(
                        "Exchange a trusted issuer's token for a session",
                        &[400, 401],
                    )
                }),
        ]
    }
}

/// 202 with the passwordless answer.
fn sent() -> Response {
    (
        StatusCode::ACCEPTED,
        Json(AuthMessage {
            message: PASSWORDLESS_SENT.to_owned(),
        }),
    )
        .into_response()
}

/// Adds `impersonator_id` to a sign-in response body.
async fn with_impersonator(response: Response, impersonator: Uuid) -> Result<Response, AppError> {
    let (parts, body) = response.into_parts();
    let bytes = navree_http::body::to_bytes(body, usize::MAX)
        .await
        .map_err(AppError::internal)?;
    let mut value: Value = serde_json::from_slice(&bytes).map_err(AppError::internal)?;
    if let Value::Object(fields) = &mut value {
        fields.insert(
            "impersonator_id".to_owned(),
            Value::String(impersonator.to_string()),
        );
    }
    Ok((parts.status, Json(value)).into_response())
}

/// A 400 with one field error.
fn field_error(field: &str, message: &str) -> AppError {
    AppError::validation(std::collections::BTreeMap::from([(
        field.to_owned(),
        vec![message.to_owned()],
    )]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_hides_credentials() {
        let inputs: [&dyn fmt::Debug; 4] = [
            &ConfirmInput {
                password: Some("hunter2-hunter2".to_owned()),
                code: None,
            },
            &MagicLinkVerifyInput {
                token: "hunter2-token".to_owned(),
            },
            &OtpVerifyInput {
                email: "ada@example.com".to_owned(),
                code: "hunter2".to_owned(),
            },
            &ExchangeInput {
                token: "hunter2.jwt.sig".to_owned(),
            },
        ];
        for input in inputs {
            let debug = format!("{input:?}");
            assert!(!debug.contains("hunter2"), "{debug}");
        }
    }
}
