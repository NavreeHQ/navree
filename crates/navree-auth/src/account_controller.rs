//! The account routes under `{api_prefix}{routes_prefix}`: password reset,
//! email verification, profile and two-factor authentication.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use http::{Method, StatusCode};
use navree_http::{
    ApiMeta, AppError, Controller, IntoResponse, Json, NoContent, Path, RouteDef, ValidSignature,
    schema_fn,
};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree_validation::Valid;
use navree_validation::Validate;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::controller::{api, signed_in};
use crate::{
    AuthError, AuthOptions, AuthService, ClientMeta, CurrentUser, LoginResponse, ProfileUpdate,
    TokenPair, TwoFactorProof, TwoFactorSetup, UserResource, not_while_impersonating,
};

/// `POST /auth/forgot-password`.
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct ForgotPasswordInput {
    /// The email address of the account.
    #[validate(email, length(max = 255))]
    pub email: String,
}

/// `POST /auth/reset-password`.
#[derive(Deserialize, Validate, ToSchema)]
pub struct ResetPasswordInput {
    /// The email address of the account.
    #[validate(email, length(max = 255))]
    pub email: String,
    /// The token from the reset mail.
    #[validate(length(min = 1, max = 512))]
    pub token: String,
    /// The new password (at least `auth.password_min_length` characters).
    #[validate(length(max = 1024))]
    #[schema(format = Password)]
    pub password: String,
}

/// `PATCH /auth/me/password`.
#[derive(Deserialize, Validate, ToSchema)]
pub struct ChangePasswordInput {
    /// The password in use.
    #[validate(length(min = 1, max = 1024))]
    #[schema(format = Password)]
    pub current_password: String,
    /// The new password (at least `auth.password_min_length` characters).
    #[validate(length(max = 1024))]
    #[schema(format = Password)]
    pub password: String,
}

/// `PATCH /auth/me`: the fields to change.
#[derive(Default, Deserialize, Validate, ToSchema)]
pub struct UpdateProfileInput {
    /// The new display name.
    #[validate(length(min = 1, max = 255))]
    #[serde(default)]
    pub name: Option<String>,
    /// The new email address (unverified until the mailed link is
    /// followed). Changing it requires `current_password`.
    #[validate(email, length(max = 255))]
    #[serde(default)]
    pub email: Option<String>,
    /// The password in use: required when `email` changes.
    #[validate(length(max = 1024))]
    #[schema(format = Password)]
    #[serde(default)]
    pub current_password: Option<String>,
}

/// `POST /auth/2fa/confirm`.
#[derive(Deserialize, Validate, ToSchema)]
pub struct ConfirmTwoFactorInput {
    /// A current code from the authenticator app.
    #[validate(length(min = 1, max = 16))]
    pub code: String,
}

/// `POST /auth/2fa/disable`.
#[derive(Deserialize, Validate, ToSchema)]
pub struct DisableTwoFactorInput {
    /// The account password.
    #[validate(length(min = 1, max = 1024))]
    #[schema(format = Password)]
    pub password: String,
}

/// `POST /auth/2fa/verify`: the challenge token of the login and either a
/// code from the authenticator app or a recovery code.
#[derive(Deserialize, Validate, ToSchema)]
pub struct TwoFactorVerifyInput {
    /// The `challenge_token` of the login response.
    #[validate(length(min = 1, max = 4096))]
    pub challenge_token: String,
    /// A current code from the authenticator app.
    #[validate(length(min = 1, max = 16))]
    #[serde(default)]
    pub code: Option<String>,
    /// One of the recovery codes (single use).
    #[validate(length(min = 1, max = 64))]
    #[serde(default)]
    pub recovery_code: Option<String>,
}

impl fmt::Debug for ResetPasswordInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResetPasswordInput")
            .field("email", &self.email)
            .field("token", &REDACTED)
            .field("password", &REDACTED)
            .finish()
    }
}

impl fmt::Debug for UpdateProfileInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UpdateProfileInput")
            .field("name", &self.name)
            .field("email", &self.email)
            .field(
                "current_password",
                &self.current_password.as_ref().map(|_| REDACTED),
            )
            .finish()
    }
}

impl fmt::Debug for ChangePasswordInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChangePasswordInput")
            .field("current_password", &REDACTED)
            .field("password", &REDACTED)
            .finish()
    }
}

impl fmt::Debug for ConfirmTwoFactorInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConfirmTwoFactorInput")
            .field("code", &REDACTED)
            .finish()
    }
}

impl fmt::Debug for DisableTwoFactorInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DisableTwoFactorInput")
            .field("password", &REDACTED)
            .finish()
    }
}

impl fmt::Debug for TwoFactorVerifyInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwoFactorVerifyInput")
            .field("challenge_token", &REDACTED)
            .field("code", &self.code.as_ref().map(|_| REDACTED))
            .field(
                "recovery_code",
                &self.recovery_code.as_ref().map(|_| REDACTED),
            )
            .finish()
    }
}

/// What the `Debug` output of inputs shows instead of credentials.
const REDACTED: &str = "[redacted]";

/// The recovery codes, shown once (`POST /auth/2fa/confirm`, `POST
/// /auth/2fa/recovery-codes`).
#[derive(Serialize, ToSchema)]
pub struct RecoveryCodesResponse {
    /// Single-use codes that replace an authenticator code at login.
    pub recovery_codes: Vec<String>,
}

/// The codes are credentials: never in debug output.
impl fmt::Debug for RecoveryCodesResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RecoveryCodesResponse")
            .field("recovery_codes", &self.recovery_codes.len())
            .finish()
    }
}

/// A response carrying a human-readable message.
#[derive(Debug, Serialize, ToSchema)]
pub struct AuthMessage {
    /// What happened.
    pub message: String,
}

impl AuthMessage {
    fn new(message: &str) -> Json<Self> {
        Json(Self {
            message: message.to_owned(),
        })
    }
}

/// The answer to every forgot-password request (it does not tell whether
/// the account exists).
const RESET_LINK_SENT: &str =
    "If the email address is registered, a password reset link has been sent.";

/// The account routes.
pub(crate) struct AccountController {
    service: Arc<AuthService>,
    options: Arc<AuthOptions>,
}

impl Injectable for AccountController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![
            ProviderKey::of::<AuthService>(),
            ProviderKey::of::<AuthOptions>(),
        ]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(AccountController {
                service: r.get::<AuthService>().await?,
                options: r.get::<AuthOptions>().await?,
            })
        })
    }
}

impl Controller for AccountController {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let prefix = self.options.routes_prefix.trim_end_matches('/');
        let path = |p: &str| format!("{prefix}{p}");
        let mut routes = Vec::new();

        let service = self.service.clone();
        let forgot =
            move |meta: ClientMeta, Valid(Json(input)): Valid<Json<ForgotPasswordInput>>| {
                let service = service.clone();
                async move {
                    service
                        .send_password_reset_link(&input.email, &meta)
                        .await
                        .map_err(AuthError::into_app_error)?;
                    Ok::<_, AppError>(
                        (StatusCode::ACCEPTED, AuthMessage::new(RESET_LINK_SENT)).into_response(),
                    )
                }
            };
        routes.push(
            RouteDef::on(Method::POST, path("/forgot-password"), forgot)
                .public()
                .name("auth.password.forgot")
                .api(ApiMeta {
                    success_status: 202,
                    request: Some(schema_fn::<ForgotPasswordInput>),
                    response: Some(schema_fn::<AuthMessage>),
                    ..api("Mail a password reset token", &[400])
                }),
        );

        let service = self.service.clone();
        let reset = move |meta: ClientMeta, Valid(Json(input)): Valid<Json<ResetPasswordInput>>| {
            let service = service.clone();
            async move {
                service
                    .reset_password(&input.email, &input.token, &input.password, &meta)
                    .await
                    .map_err(AuthError::into_app_error)?;
                Ok::<_, AppError>(NoContent)
            }
        };
        routes.push(
            RouteDef::on(Method::POST, path("/reset-password"), reset)
                .public()
                .name("auth.password.reset")
                .api(ApiMeta {
                    success_status: 204,
                    request: Some(schema_fn::<ResetPasswordInput>),
                    ..api(
                        "Set a new password with a reset token (signs out every session)",
                        &[400],
                    )
                }),
        );

        let service = self.service.clone();
        let send_verification = move |CurrentUser(user): CurrentUser| {
            let service = service.clone();
            async move {
                let sent = service
                    .send_verification_email(&user)
                    .await
                    .map_err(AuthError::into_app_error)?;
                Ok::<_, AppError>(if sent {
                    (
                        StatusCode::ACCEPTED,
                        AuthMessage::new("A verification link has been sent."),
                    )
                        .into_response()
                } else {
                    NoContent.into_response()
                })
            }
        };
        routes.push(
            RouteDef::on(Method::POST, path("/email/verification"), send_verification)
                .authenticated()
                .name("auth.verification.send")
                .api(ApiMeta {
                    success_status: 202,
                    response: Some(schema_fn::<AuthMessage>),
                    ..api(
                        "Mail a link verifying the email address (204 when already verified)",
                        &[401],
                    )
                }),
        );

        let service = self.service.clone();
        let verify = move |meta: ClientMeta, Path((id, hash)): Path<(Uuid, String)>| {
            let service = service.clone();
            async move {
                service
                    .verify_email(id, &hash, &meta)
                    .await
                    .map_err(AuthError::into_app_error)?;
                Ok::<_, AppError>(AuthMessage::new("The email address has been verified."))
            }
        };
        routes.push(
            RouteDef::on(Method::GET, path("/email/verify/{id}/{hash}"), verify)
                .public()
                .guard(ValidSignature::guard())
                .name("auth.verification.verify")
                .api(ApiMeta {
                    response: Some(schema_fn::<AuthMessage>),
                    ..api(
                        "Verify the email address (the signed link from the mail)",
                        &[401, 403],
                    )
                }),
        );

        let service = self.service.clone();
        let change_password =
            move |CurrentUser(user): CurrentUser,
                  meta: ClientMeta,
                  Valid(Json(input)): Valid<Json<ChangePasswordInput>>| {
                let service = service.clone();
                async move {
                    let tokens = service
                        .change_password(user.id, &input.current_password, &input.password, meta)
                        .await
                        .map_err(AuthError::into_app_error)?;
                    Ok::<_, AppError>(Json(tokens))
                }
            };
        routes.push(
            RouteDef::on(Method::PATCH, path("/me/password"), change_password)
                .authenticated()
                .guard(not_while_impersonating())
                .name("auth.password.change")
                .api(ApiMeta {
                    request: Some(schema_fn::<ChangePasswordInput>),
                    response: Some(schema_fn::<TokenPair>),
                    ..api(
                        "Change the password (signs out every other session; returns a new token pair)",
                        &[400, 401, 429],
                    )
                }),
        );

        let service = self.service.clone();
        let update_profile =
            move |CurrentUser(user): CurrentUser,
                  meta: ClientMeta,
                  Valid(Json(input)): Valid<Json<UpdateProfileInput>>| {
                let service = service.clone();
                async move {
                    let update = ProfileUpdate {
                        name: input.name,
                        email: input.email,
                        current_password: input.current_password,
                    };
                    let user = service
                        .update_profile(user.id, update, &meta)
                        .await
                        .map_err(AuthError::into_app_error)?;
                    let resource = service
                        .present(&user)
                        .await
                        .map_err(AuthError::into_app_error)?;
                    Ok::<_, AppError>(Json(resource))
                }
            };
        routes.push(
            RouteDef::on(Method::PATCH, path("/me"), update_profile)
                .authenticated()
                .guard(not_while_impersonating())
                .name("auth.profile.update")
                .api(ApiMeta {
                    request: Some(schema_fn::<UpdateProfileInput>),
                    response: Some(schema_fn::<UserResource>),
                    ..api(
                        "Change the name or email address (an email change needs current_password)",
                        &[400, 401, 409, 429],
                    )
                }),
        );

        routes.extend(self.two_factor_routes(&path));
        routes
    }
}

impl AccountController {
    /// `POST /auth/2fa/{enable,confirm,disable,recovery-codes,verify}`.
    fn two_factor_routes(&self, path: &dyn Fn(&str) -> String) -> Vec<RouteDef> {
        let mut routes = Vec::new();

        let service = self.service.clone();
        let enable = move |CurrentUser(user): CurrentUser, meta: ClientMeta| {
            let service = service.clone();
            async move {
                let setup = service
                    .enable_two_factor(user.id, &meta)
                    .await
                    .map_err(AuthError::into_app_error)?;
                Ok::<_, AppError>(Json(setup))
            }
        };
        routes.push(
            RouteDef::on(Method::POST, path("/2fa/enable"), enable)
                .authenticated()
                .guard(not_while_impersonating())
                .name("auth.two_factor.enable")
                .api(ApiMeta {
                    response: Some(schema_fn::<TwoFactorSetup>),
                    ..api(
                        "Start two-factor setup: a TOTP secret, its otpauth URL and QR code",
                        &[401, 409],
                    )
                }),
        );

        let service = self.service.clone();
        let confirm =
            move |CurrentUser(user): CurrentUser,
                  meta: ClientMeta,
                  Valid(Json(input)): Valid<Json<ConfirmTwoFactorInput>>| {
                let service = service.clone();
                async move {
                    let recovery_codes = service
                        .confirm_two_factor(user.id, &input.code, &meta)
                        .await
                        .map_err(AuthError::into_app_error)?;
                    Ok::<_, AppError>(Json(RecoveryCodesResponse { recovery_codes }))
                }
            };
        routes.push(
            RouteDef::on(Method::POST, path("/2fa/confirm"), confirm)
                .authenticated()
                .guard(not_while_impersonating())
                .name("auth.two_factor.confirm")
                .api(ApiMeta {
                    request: Some(schema_fn::<ConfirmTwoFactorInput>),
                    response: Some(schema_fn::<RecoveryCodesResponse>),
                    ..api(
                        "Confirm two-factor setup with a code; returns the recovery codes",
                        &[400, 401, 409, 429],
                    )
                }),
        );

        let service = self.service.clone();
        let disable =
            move |CurrentUser(user): CurrentUser,
                  meta: ClientMeta,
                  Valid(Json(input)): Valid<Json<DisableTwoFactorInput>>| {
                let service = service.clone();
                async move {
                    service
                        .disable_two_factor(user.id, &input.password, &meta)
                        .await
                        .map_err(AuthError::into_app_error)?;
                    Ok::<_, AppError>(NoContent)
                }
            };
        routes.push(
            RouteDef::on(Method::POST, path("/2fa/disable"), disable)
                .authenticated()
                .guard(not_while_impersonating())
                .name("auth.two_factor.disable")
                .api(ApiMeta {
                    success_status: 204,
                    request: Some(schema_fn::<DisableTwoFactorInput>),
                    ..api(
                        "Turn two-factor authentication off (password required)",
                        &[400, 401, 429],
                    )
                }),
        );

        let service = self.service.clone();
        let regenerate = move |CurrentUser(user): CurrentUser, meta: ClientMeta| {
            let service = service.clone();
            async move {
                let recovery_codes = service
                    .regenerate_recovery_codes(user.id, &meta)
                    .await
                    .map_err(AuthError::into_app_error)?;
                Ok::<_, AppError>(Json(RecoveryCodesResponse { recovery_codes }))
            }
        };
        routes.push(
            RouteDef::on(Method::POST, path("/2fa/recovery-codes"), regenerate)
                .authenticated()
                .guard(not_while_impersonating())
                .name("auth.two_factor.recovery_codes")
                .api(ApiMeta {
                    response: Some(schema_fn::<RecoveryCodesResponse>),
                    ..api("Replace the recovery codes", &[400, 401])
                }),
        );

        let service = self.service.clone();
        let verify =
            move |meta: ClientMeta, Valid(Json(input)): Valid<Json<TwoFactorVerifyInput>>| {
                let service = service.clone();
                async move {
                    let proof = two_factor_proof(input.code, input.recovery_code)?;
                    let (user, tokens) = service
                        .verify_two_factor(&input.challenge_token, proof, &meta)
                        .await
                        .map_err(AuthError::into_app_error)?;
                    signed_in(&service, &user, tokens, StatusCode::OK).await
                }
            };
        routes.push(
            RouteDef::on(Method::POST, path("/2fa/verify"), verify)
                .public()
                .name("auth.two_factor.verify")
                .api(ApiMeta {
                    request: Some(schema_fn::<TwoFactorVerifyInput>),
                    response: Some(schema_fn::<LoginResponse>),
                    ..api(
                        "Finish a two-factor login with a code or a recovery code",
                        &[400, 401, 429],
                    )
                }),
        );

        routes
    }
}

/// The second factor of a `TwoFactorVerifyInput`: exactly one of `code`
/// and `recovery_code`.
pub(crate) fn two_factor_proof(
    code: Option<String>,
    recovery_code: Option<String>,
) -> Result<TwoFactorProof, AppError> {
    match (code, recovery_code) {
        (Some(code), None) => Ok(TwoFactorProof::Code(code)),
        (None, Some(code)) => Ok(TwoFactorProof::RecoveryCode(code)),
        (None, None) => Err(field_error(
            "code",
            "code should not be empty when recovery_code is missing",
        )),
        (Some(_), Some(_)) => Err(field_error(
            "code",
            "code must be empty when recovery_code is given",
        )),
    }
}

/// A 400 with one field error.
fn field_error(field: &str, message: &str) -> AppError {
    AppError::validation(BTreeMap::from([(
        field.to_owned(),
        vec![message.to_owned()],
    )]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_hides_credentials() {
        let inputs: [&dyn fmt::Debug; 5] = [
            &ResetPasswordInput {
                email: "ada@example.com".to_owned(),
                token: "reset-token-xyz".to_owned(),
                password: "hunter2-hunter2".to_owned(),
            },
            &ChangePasswordInput {
                current_password: "hunter2-hunter2".to_owned(),
                password: "reset-token-xyz".to_owned(),
            },
            &ConfirmTwoFactorInput {
                code: "reset-token-xyz".to_owned(),
            },
            &DisableTwoFactorInput {
                password: "hunter2-hunter2".to_owned(),
            },
            &TwoFactorVerifyInput {
                challenge_token: "reset-token-xyz".to_owned(),
                code: Some("hunter2-hunter2".to_owned()),
                recovery_code: None,
            },
        ];
        for input in inputs {
            let debug = format!("{input:?}");
            assert!(!debug.contains("hunter2"), "{debug}");
            assert!(!debug.contains("reset-token"), "{debug}");
        }
        let codes = RecoveryCodesResponse {
            recovery_codes: vec!["abc-def".to_owned()],
        };
        assert!(!format!("{codes:?}").contains("abc-def"));
    }
}
