//! The JSON sign-in routes of a single-page app that keeps its browser on the
//! session cookie (feature `session`) instead of holding tokens.
//!
//! | Route | Body | Answer |
//! |---|---|---|
//! | `POST /auth/session/login` | `{email, password, remember?}` | `200` with the user; `200 {"two_factor_required": true}` when the account asks for a second factor |
//! | `POST /auth/session/2fa/verify` | `{code}` or `{recovery_code}` | `200` with the user |
//! | `POST /auth/session/magic-link/verify` | `{token, remember?}` | like login (when `auth.magic_link.enabled`) |
//! | `POST /auth/session/otp/verify` | `{email, code, remember?}` | like login (when `auth.otp.enabled`) |
//!
//! They take the same steps as the sign-in pages ([`AuthService::sign_in_session`]
//! and its siblings): the sign-in policies, the throttles, two-factor
//! authentication and the audit trail, and the session gets a new id and a
//! new CSRF token. The browser gets no token: the session cookie is all it
//! keeps. The page's first request (any `GET`, or `GET /csrf-cookie`) hands
//! it the `XSRF-TOKEN` cookie, which it sends back as `X-XSRF-TOKEN` on
//! these posts. `POST /auth/logout` signs it out, `GET /auth/me` reads the
//! user.

use std::fmt;

use http::Method;
use navree_http::{ApiMeta, AppError, IntoResponse, Json, Response, RouteDef, schema_fn};
use navree_openapi::ToSchema;
use navree_session::Session;
use navree_validation::{Valid, Validate};
use serde::Deserialize;
use serde_json::json;

use crate::account_controller::two_factor_proof;
use crate::controller::api;
use crate::session_controller::SessionController;
use crate::{AuthError, AuthService, ClientMeta, SessionSignIn, UserResource};

/// `POST /auth/session/login`. `Debug` never shows the password.
#[derive(Deserialize, Validate, ToSchema)]
pub struct SessionLoginInput {
    /// The email address.
    #[validate(email, length(max = 255))]
    pub email: String,
    /// The password.
    #[validate(length(min = 1, max = 1024))]
    #[schema(format = Password)]
    pub password: String,
    /// Keep the browser signed in for `auth.session.remember_days`.
    #[validate(skip)]
    #[serde(default)]
    pub remember: bool,
}

/// `POST /auth/session/2fa/verify`: a code from the authenticator app or a
/// recovery code (exactly one).
#[derive(Deserialize, Validate, ToSchema)]
pub struct SessionTwoFactorInput {
    /// A current code from the authenticator app.
    #[validate(length(min = 1, max = 16))]
    #[serde(default)]
    pub code: Option<String>,
    /// One of the recovery codes (single use).
    #[validate(length(min = 1, max = 64))]
    #[serde(default)]
    pub recovery_code: Option<String>,
}

/// `POST /auth/session/magic-link/verify`.
#[derive(Deserialize, Validate, ToSchema)]
pub struct SessionMagicLinkInput {
    /// The token from the link.
    #[validate(length(min = 1, max = 512))]
    pub token: String,
    /// Keep the browser signed in for `auth.session.remember_days`.
    #[validate(skip)]
    #[serde(default)]
    pub remember: bool,
}

/// `POST /auth/session/otp/verify`.
#[derive(Deserialize, Validate, ToSchema)]
pub struct SessionOtpInput {
    /// The email address the code was sent to.
    #[validate(email, length(max = 255))]
    pub email: String,
    /// The code from the mail.
    #[validate(length(min = 1, max = 16))]
    pub code: String,
    /// Keep the browser signed in for `auth.session.remember_days`.
    #[validate(skip)]
    #[serde(default)]
    pub remember: bool,
}

impl fmt::Debug for SessionLoginInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionLoginInput")
            .field("email", &self.email)
            .field("password", &"[redacted]")
            .field("remember", &self.remember)
            .finish()
    }
}

impl fmt::Debug for SessionTwoFactorInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionTwoFactorInput")
            .field("code", &self.code.as_ref().map(|_| "[redacted]"))
            .field(
                "recovery_code",
                &self.recovery_code.as_ref().map(|_| "[redacted]"),
            )
            .finish()
    }
}

impl fmt::Debug for SessionMagicLinkInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionMagicLinkInput")
            .field("token", &"[redacted]")
            .field("remember", &self.remember)
            .finish()
    }
}

impl fmt::Debug for SessionOtpInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionOtpInput")
            .field("email", &self.email)
            .field("code", &"[redacted]")
            .field("remember", &self.remember)
            .finish()
    }
}

/// The answer to a browser sign-in: the user, or the word that a second
/// factor is next.
async fn answer(service: &AuthService, outcome: SessionSignIn) -> Result<Response, AppError> {
    match outcome {
        SessionSignIn::SignedIn(user) => {
            let user = service
                .present(&user)
                .await
                .map_err(AuthError::into_app_error)?;
            Ok(Json(user).into_response())
        }
        SessionSignIn::TwoFactorRequired => {
            Ok(Json(json!({ "two_factor_required": true })).into_response())
        }
    }
}

impl SessionController {
    /// `POST /session/login`, `POST /session/2fa/verify`, and, when they
    /// are on, `POST /session/magic-link/verify` and `POST
    /// /session/otp/verify`.
    pub(crate) fn browser_routes(&self, path: &dyn Fn(&str) -> String) -> Vec<RouteDef> {
        let mut routes = Vec::new();

        let service = self.service.clone();
        let login = move |session: Session,
                          meta: ClientMeta,
                          Valid(Json(input)): Valid<Json<SessionLoginInput>>| {
            let service = service.clone();
            async move {
                let outcome = service
                    .sign_in_session(
                        &session,
                        &input.email,
                        &input.password,
                        input.remember,
                        &meta,
                    )
                    .await
                    .map_err(AuthError::into_app_error)?;
                answer(&service, outcome).await
            }
        };
        routes.push(
            RouteDef::on(Method::POST, path("/session/login"), login)
                .public()
                .name("auth.session.login")
                .api(ApiMeta {
                    description: Some(
                        "Signs the browser in on the session cookie (no token is issued). \
                         When the account uses two-factor authentication the answer is \
                         `{\"two_factor_required\": true}`; finish with \
                         `POST /auth/session/2fa/verify`. The request carries the CSRF token \
                         of the session as `X-XSRF-TOKEN`."
                            .to_owned(),
                    ),
                    request: Some(schema_fn::<SessionLoginInput>),
                    response: Some(schema_fn::<UserResource>),
                    ..api(
                        "Sign a browser in with email and password",
                        &[400, 401, 403, 429],
                    )
                }),
        );

        let service = self.service.clone();
        let verify =
            move |session: Session,
                  meta: ClientMeta,
                  Valid(Json(input)): Valid<Json<SessionTwoFactorInput>>| {
                let service = service.clone();
                async move {
                    let proof = two_factor_proof(input.code, input.recovery_code)?;
                    let user = service
                        .verify_two_factor_session(&session, proof, &meta)
                        .await
                        .map_err(AuthError::into_app_error)?;
                    answer(&service, SessionSignIn::SignedIn(user)).await
                }
            };
        routes.push(
            RouteDef::on(Method::POST, path("/session/2fa/verify"), verify)
                .public()
                .name("auth.session.two_factor")
                .api(ApiMeta {
                    request: Some(schema_fn::<SessionTwoFactorInput>),
                    response: Some(schema_fn::<UserResource>),
                    ..api(
                        "Finish a browser's two-factor sign-in with a code or a recovery code",
                        &[400, 401, 403, 429],
                    )
                }),
        );

        if self.options.magic_link.enabled {
            let service = self.service.clone();
            let verify =
                move |session: Session,
                      meta: ClientMeta,
                      Valid(Json(input)): Valid<Json<SessionMagicLinkInput>>| {
                    let service = service.clone();
                    async move {
                        let outcome = service
                            .login_with_magic_link_session(
                                &session,
                                &input.token,
                                input.remember,
                                &meta,
                            )
                            .await
                            .map_err(AuthError::into_app_error)?;
                        answer(&service, outcome).await
                    }
                };
            routes.push(
                RouteDef::on(Method::POST, path("/session/magic-link/verify"), verify)
                    .public()
                    .name("auth.session.magic_link")
                    .api(ApiMeta {
                        request: Some(schema_fn::<SessionMagicLinkInput>),
                        response: Some(schema_fn::<UserResource>),
                        ..api(
                            "Sign a browser in with a magic link token",
                            &[400, 401, 403],
                        )
                    }),
            );
        }

        if self.options.otp.enabled {
            let service = self.service.clone();
            let verify =
                move |session: Session,
                      meta: ClientMeta,
                      Valid(Json(input)): Valid<Json<SessionOtpInput>>| {
                    let service = service.clone();
                    async move {
                        let outcome = service
                            .login_with_code_session(
                                &session,
                                &input.email,
                                &input.code,
                                input.remember,
                                &meta,
                            )
                            .await
                            .map_err(AuthError::into_app_error)?;
                        answer(&service, outcome).await
                    }
                };
            routes.push(
                RouteDef::on(Method::POST, path("/session/otp/verify"), verify)
                    .public()
                    .name("auth.session.otp")
                    .api(ApiMeta {
                        request: Some(schema_fn::<SessionOtpInput>),
                        response: Some(schema_fn::<UserResource>),
                        ..api(
                            "Sign a browser in with an emailed code",
                            &[400, 401, 403, 429],
                        )
                    }),
            );
        }
        routes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_hides_credentials() {
        let login = SessionLoginInput {
            email: "navjot@example.com".to_owned(),
            password: "hunter2-hunter2".to_owned(),
            remember: true,
        };
        let second = SessionTwoFactorInput {
            code: Some("123456".to_owned()),
            recovery_code: Some("hunter2-recovery".to_owned()),
        };
        let link = SessionMagicLinkInput {
            token: "hunter2-token".to_owned(),
            remember: false,
        };
        let otp = SessionOtpInput {
            email: "navjot@example.com".to_owned(),
            code: "987654".to_owned(),
            remember: false,
        };
        let printed = format!("{login:?} {second:?} {link:?} {otp:?}");
        for secret in ["hunter2", "123456", "987654"] {
            assert!(!printed.contains(secret), "{secret} in {printed}");
        }
        assert!(printed.contains("navjot@example.com"));
    }
}
