//! [`AuthError`] and its HTTP mapping.

use std::collections::BTreeMap;
use std::time::Duration;

use http::header::WWW_AUTHENTICATE;
use http::{HeaderValue, StatusCode};
use navree_cache::CacheError;
use navree_crypto::CryptoError;
use navree_http::AppError;
use navree_kernel::{BoxError, KernelError};
use navree_mail::MailError;
use sea_orm::DbErr;

/// An auth-module error. Returned from handlers with `?`, it becomes an
/// error response (the module registers the mapping): bad credentials and
/// bad tokens → 401, too many login attempts → 429, invalid input or a
/// taken email → 400 with field errors, database errors as navree-db maps
/// them, anything else 500.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AuthError {
    /// Unknown email or wrong password (deliberately not telling which).
    #[error("invalid credentials")]
    InvalidCredentials,
    /// Too many failed logins for this email from this address.
    #[error("too many login attempts; retry in {}s", retry_after.as_secs())]
    TooManyAttempts {
        /// When the client may try again.
        retry_after: Duration,
    },
    /// The email address is already registered.
    #[error("the email address is already registered")]
    EmailTaken,
    /// An input value is invalid (`field`: `message`).
    #[error("invalid {field}: {message}")]
    Invalid {
        /// The input field.
        field: &'static str,
        /// A field message in the validation style, e.g. "password must be longer
        /// than or equal to 8 characters".
        message: String,
    },
    /// The token is malformed, forged, of another type or not for this app.
    #[error("invalid token")]
    InvalidToken,
    /// The token has expired.
    #[error("token expired")]
    TokenExpired,
    /// The refresh token is unknown, malformed, expired or revoked.
    #[error("invalid refresh token")]
    InvalidRefreshToken,
    /// A refresh token was used twice: its whole family has been revoked.
    #[error("refresh token reuse detected; the session has been revoked")]
    RefreshTokenReused,
    /// No such user.
    #[error("user not found")]
    UserNotFound,
    /// The password reset token is unknown, wrong, used or expired.
    #[error("invalid or expired password reset token")]
    InvalidResetToken,
    /// The email verification link is not for this user's current address.
    #[error("invalid verification link")]
    InvalidVerificationLink,
    /// The password was right but the account uses two-factor
    /// authentication: [`AuthService::login`](crate::AuthService::login)
    /// cannot finish; use
    /// [`attempt_login`](crate::AuthService::attempt_login) and
    /// [`verify_two_factor`](crate::AuthService::verify_two_factor).
    #[error("two-factor authentication required")]
    TwoFactorRequired,
    /// Two-factor authentication is already enabled (disable it first).
    #[error("two-factor authentication is already enabled")]
    TwoFactorAlreadyEnabled,
    /// Two-factor authentication is not enabled, or not confirmed yet.
    #[error("two-factor authentication is not enabled")]
    TwoFactorNotEnabled,
    /// A wrong or reused TOTP code or recovery code at a two-factor login.
    #[error("invalid two-factor code")]
    InvalidTwoFactorCode,
    /// The TOTP secret, its URL or its QR code could not be built.
    #[error("two-factor error: {0}")]
    TwoFactor(String),
    /// A mail could not be rendered or sent.
    #[error("auth mail error: {0}")]
    Mail(#[from] MailError),
    /// A provider the flow needs (the URL signer, the HTTP options) could
    /// not be resolved.
    #[error("auth provider error: {0}")]
    Kernel(#[from] KernelError),
    /// The application has shut down (the service outlived it).
    #[error("the application has shut down")]
    ShutDown,
    /// A token could not be signed.
    #[error("could not sign a token: {0}")]
    Signing(String),
    /// Hashing or encryption failed.
    #[error("auth crypto error: {0}")]
    Crypto(#[from] CryptoError),
    /// A [`UserResourceExtension`](crate::UserResourceExtension) failed.
    #[error("user resource extension failed: {0}")]
    Extension(BoxError),
    /// The user must set up two-factor authentication before doing anything
    /// else (`auth.two_factor.required_for` or a `TwoFactorPolicy`), or a
    /// platform administrator has none.
    #[error("two-factor authentication must be set up first")]
    TwoFactorSetupRequired,
    /// The route needs a recent sign-in or `POST /auth/confirm`
    /// (`#[recent_auth]`).
    #[error("the user must confirm their identity again")]
    ReauthenticationRequired,
    /// The action is not allowed while impersonating a user.
    #[error("not allowed while impersonating a user")]
    ImpersonationForbidden,
    /// `DELETE /auth/impersonate` without an impersonation session.
    #[error("not impersonating a user")]
    NotImpersonating,
    /// The user may not be impersonated (themselves, a platform
    /// administrator).
    #[error("this user cannot be impersonated: {0}")]
    CannotImpersonate(&'static str),
    /// The session (`/auth/sessions/{id}`) is not one of the user's live
    /// sessions.
    #[error("session not found")]
    SessionNotFound,
    /// A magic link or emailed code is unknown, wrong, used or expired.
    #[error("invalid or expired sign-in token")]
    InvalidLoginToken,
    /// The route needs a platform session (`auth.platform`).
    #[error("a platform session is required")]
    PlatformSessionRequired,
    /// A token presented to `POST /auth/exchange` was refused (the reason
    /// is logged and audited, never sent).
    #[error("token exchange refused: {0}")]
    ExchangeRefused(String),
    /// The passkey (`/auth/passkeys/{id}`) is not one of the user's.
    #[error("passkey not found")]
    PasskeyNotFound,
    /// A passkey registration or sign-in was refused (the reason is
    /// logged, never sent).
    #[error("passkey refused: {0}")]
    PasskeyRefused(String),
    /// The feature is turned off in the configuration.
    #[error("{0} is not enabled")]
    Disabled(&'static str),
    /// The login throttle's cache failed.
    #[error("auth cache error: {0}")]
    Cache(#[from] CacheError),
    /// A database error.
    #[error("auth database error: {0}")]
    Db(#[from] DbErr),
}

impl AuthError {
    /// The error response for this error, `None` for errors that stay a 500
    /// (their details are logged, never sent).
    pub fn to_app_error(&self) -> Option<AppError> {
        match self {
            AuthError::InvalidCredentials => {
                Some(AppError::unauthenticated().with_detail("Invalid credentials"))
            }
            AuthError::TooManyAttempts { retry_after } => Some(
                AppError::too_many_requests(*retry_after).with_detail(format!(
                    "Too many login attempts. Please try again in {} seconds.",
                    retry_after.as_secs().max(1)
                )),
            ),
            AuthError::EmailTaken => Some(validation("email", "email has already been taken")),
            AuthError::Invalid { field, message } => Some(validation(field, message)),
            AuthError::InvalidToken => Some(bearer_challenge("Invalid token")),
            AuthError::TokenExpired => Some(bearer_challenge("Token expired")),
            AuthError::InvalidRefreshToken | AuthError::RefreshTokenReused => {
                Some(AppError::unauthenticated().with_detail("Invalid refresh token"))
            }
            AuthError::UserNotFound => Some(AppError::not_found()),
            AuthError::InvalidResetToken => Some(AppError::bad_request(
                "Invalid or expired password reset token",
            )),
            AuthError::InvalidVerificationLink => {
                Some(AppError::forbidden().with_detail("Invalid verification link"))
            }
            AuthError::TwoFactorRequired => {
                Some(AppError::unauthenticated().with_detail("Two-factor authentication required"))
            }
            AuthError::TwoFactorAlreadyEnabled => Some(AppError::conflict(
                "Two-factor authentication is already enabled",
            )),
            AuthError::TwoFactorNotEnabled => Some(AppError::bad_request(
                "Two-factor authentication is not enabled",
            )),
            AuthError::InvalidTwoFactorCode => {
                Some(AppError::unauthenticated().with_detail("Invalid two-factor code"))
            }
            AuthError::TwoFactorSetupRequired => Some(
                AppError::new(StatusCode::FORBIDDEN, "two_factor_required", "Forbidden")
                    .with_detail("Two-factor authentication required"),
            ),
            AuthError::ReauthenticationRequired => Some(
                AppError::new(
                    StatusCode::FORBIDDEN,
                    "reauthentication_required",
                    "Forbidden",
                )
                .with_detail("Please confirm your identity to continue"),
            ),
            AuthError::ImpersonationForbidden => Some(
                AppError::new(
                    StatusCode::FORBIDDEN,
                    "impersonation_forbidden",
                    "Forbidden",
                )
                .with_detail("Not allowed while impersonating a user"),
            ),
            AuthError::NotImpersonating => Some(AppError::bad_request("Not impersonating a user")),
            AuthError::CannotImpersonate(reason) => Some(
                AppError::new(StatusCode::FORBIDDEN, "cannot_impersonate", "Forbidden")
                    .with_detail(format!("This user cannot be impersonated: {reason}")),
            ),
            AuthError::SessionNotFound | AuthError::PasskeyNotFound => Some(AppError::not_found()),
            AuthError::InvalidLoginToken => {
                Some(AppError::unauthenticated().with_detail("Invalid or expired sign-in code"))
            }
            AuthError::PlatformSessionRequired => Some(
                AppError::new(
                    StatusCode::FORBIDDEN,
                    "platform_session_required",
                    "Forbidden",
                )
                .with_detail("A platform session is required"),
            ),
            AuthError::ExchangeRefused(_) => Some(bearer_challenge("Invalid token")),
            AuthError::PasskeyRefused(_) => {
                Some(AppError::unauthenticated().with_detail("Invalid passkey"))
            }
            AuthError::Disabled(_) => Some(AppError::not_found()),
            AuthError::Db(error) => navree_db::map_db_error(error),
            AuthError::Signing(_)
            | AuthError::Crypto(_)
            | AuthError::Cache(_)
            | AuthError::Extension(_)
            | AuthError::TwoFactor(_)
            | AuthError::Mail(_)
            | AuthError::Kernel(_)
            | AuthError::ShutDown => None,
        }
    }

    /// The error response for this error: [`to_app_error`](Self::to_app_error),
    /// or a 500 carrying the error as its (logged) source.
    pub fn into_app_error(self) -> AppError {
        match self.to_app_error() {
            Some(error) => error,
            None => AppError::internal(self),
        }
    }
}

/// 401 with `message` and an RFC 6750 `WWW-Authenticate` challenge.
fn bearer_challenge(message: &str) -> AppError {
    AppError::unauthenticated()
        .with_detail(message)
        .with_header(
            WWW_AUTHENTICATE,
            HeaderValue::from_static(r#"Bearer error="invalid_token""#),
        )
}

fn validation(field: &str, message: &str) -> AppError {
    AppError::validation(BTreeMap::from([(
        field.to_owned(),
        vec![message.to_owned()],
    )]))
}

/// The mapping registered with `map_error`.
pub(crate) fn map_auth_error(error: &AuthError) -> Option<AppError> {
    error.to_app_error()
}

#[cfg(test)]
mod tests {
    use http::StatusCode;
    use http::header::RETRY_AFTER;
    use serde_json::json;

    use super::*;

    #[test]
    fn credentials_and_tokens_are_401_with_auth_messages() {
        let error = AuthError::InvalidCredentials.into_app_error();
        assert_eq!(
            error.body(),
            json!({"statusCode": 401, "message": "Invalid credentials", "error": "Unauthorized"})
        );
        let expired = AuthError::TokenExpired.into_app_error();
        assert_eq!(expired.body()["message"], "Token expired");
        assert!(
            expired.headers[WWW_AUTHENTICATE]
                .to_str()
                .unwrap()
                .starts_with("Bearer")
        );
        for error in [
            AuthError::InvalidRefreshToken,
            AuthError::RefreshTokenReused,
        ] {
            assert_eq!(
                error.into_app_error().body()["message"],
                "Invalid refresh token",
                "reuse is not revealed to the client"
            );
        }
    }

    #[test]
    fn throttling_is_429_with_retry_after() {
        let error = AuthError::TooManyAttempts {
            retry_after: Duration::from_secs(42),
        }
        .into_app_error();
        assert_eq!(error.status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(error.headers[RETRY_AFTER], "42");
    }

    #[test]
    fn input_errors_are_400_field_errors() {
        let error = AuthError::EmailTaken.into_app_error();
        assert_eq!(error.status, StatusCode::BAD_REQUEST);
        assert_eq!(
            error.body()["errors"],
            json!({"email": ["email has already been taken"]})
        );
        let error = AuthError::Invalid {
            field: "password",
            message: "password must be longer than or equal to 8 characters".to_owned(),
        }
        .into_app_error();
        assert_eq!(
            error.body()["message"],
            json!(["password must be longer than or equal to 8 characters"])
        );
    }

    #[test]
    fn account_flow_errors_map_to_json_responses() {
        let cases = [
            (
                AuthError::InvalidResetToken,
                400,
                "Invalid or expired password reset token",
            ),
            (
                AuthError::InvalidVerificationLink,
                403,
                "Invalid verification link",
            ),
            (
                AuthError::TwoFactorRequired,
                401,
                "Two-factor authentication required",
            ),
            (
                AuthError::TwoFactorAlreadyEnabled,
                409,
                "Two-factor authentication is already enabled",
            ),
            (
                AuthError::TwoFactorNotEnabled,
                400,
                "Two-factor authentication is not enabled",
            ),
            (
                AuthError::InvalidTwoFactorCode,
                401,
                "Invalid two-factor code",
            ),
        ];
        for (error, status, message) in cases {
            let error = error.into_app_error();
            assert_eq!(error.status.as_u16(), status, "{message}");
            assert_eq!(error.body()["message"], message);
        }
        let error = AuthError::TwoFactor("secret=JBSWY3DP".to_owned()).into_app_error();
        assert_eq!(error.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(!error.body().to_string().contains("JBSWY3DP"));
    }

    #[test]
    fn auth_plus_errors_carry_codes() {
        let cases = [
            (
                AuthError::TwoFactorSetupRequired,
                403,
                "two_factor_required",
                "Two-factor authentication required",
            ),
            (
                AuthError::ReauthenticationRequired,
                403,
                "reauthentication_required",
                "Please confirm your identity to continue",
            ),
            (
                AuthError::ImpersonationForbidden,
                403,
                "impersonation_forbidden",
                "Not allowed while impersonating a user",
            ),
            (
                AuthError::NotImpersonating,
                400,
                "bad_request",
                "Not impersonating a user",
            ),
            (
                AuthError::CannotImpersonate("yourself"),
                403,
                "cannot_impersonate",
                "This user cannot be impersonated: yourself",
            ),
            (AuthError::SessionNotFound, 404, "not_found", "Not Found"),
            (
                AuthError::InvalidLoginToken,
                401,
                "unauthenticated",
                "Invalid or expired sign-in code",
            ),
            (
                AuthError::PlatformSessionRequired,
                403,
                "platform_session_required",
                "A platform session is required",
            ),
            (
                AuthError::ExchangeRefused("bad signature".to_owned()),
                401,
                "unauthenticated",
                "Invalid token",
            ),
            (
                AuthError::PasskeyRefused("wrong origin".to_owned()),
                401,
                "unauthenticated",
                "Invalid passkey",
            ),
            (
                AuthError::Disabled("magic links"),
                404,
                "not_found",
                "Not Found",
            ),
        ];
        for (error, status, code, message) in cases {
            let error = error.into_app_error();
            assert_eq!(error.status.as_u16(), status, "{message}");
            assert_eq!(error.code, code, "{message}");
            assert_eq!(error.body()["message"], message);
        }
        let refused = AuthError::ExchangeRefused("kid=secret-key-id".to_owned()).into_app_error();
        assert!(!refused.body().to_string().contains("secret-key-id"));
    }

    #[test]
    fn internal_errors_stay_500_without_details() {
        let error = AuthError::Signing("key=hunter2".to_owned()).into_app_error();
        assert_eq!(error.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(!error.body().to_string().contains("hunter2"));
    }
}
