//! [`AuthError`] and its HTTP mapping.

use std::collections::BTreeMap;
use std::time::Duration;

use http::HeaderValue;
use http::header::WWW_AUTHENTICATE;
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
///
/// Each refusal carries its own `code` for front ends to translate:
///
/// | Error | Status | `code` |
/// |---|---|---|
/// | `InvalidCredentials` | 401 | `invalid_credentials` |
/// | `TooManyAttempts` | 429 | `too_many_login_attempts` |
/// | `EmailTaken`, `Invalid` | 400 | `validation_failed` |
/// | `InvalidToken` | 401 | `invalid_token` |
/// | `TokenExpired` | 401 | `token_expired` |
/// | `InvalidRefreshToken`, `RefreshTokenReused` | 401 | `invalid_refresh_token` |
/// | `UserNotFound` | 404 | `user_not_found` |
/// | `InvalidResetToken` | 400 | `invalid_reset_token` |
/// | `InvalidVerificationLink` | 403 | `invalid_verification_link` |
/// | `TwoFactorRequired` | 401 | `two_factor_required` |
/// | `TwoFactorAlreadyEnabled` | 409 | `two_factor_already_enabled` |
/// | `TwoFactorNotEnabled` | 400 | `two_factor_not_enabled` |
/// | `InvalidTwoFactorCode` | 401 | `invalid_two_factor_code` |
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
            AuthError::InvalidCredentials => Some(
                AppError::unauthenticated()
                    .with_code("invalid_credentials")
                    .with_detail("Invalid credentials"),
            ),
            AuthError::TooManyAttempts { retry_after } => Some(
                AppError::too_many_requests(*retry_after)
                    .with_code("too_many_login_attempts")
                    .with_detail(format!(
                        "Too many login attempts. Please try again in {} seconds.",
                        retry_after.as_secs().max(1)
                    )),
            ),
            AuthError::EmailTaken => Some(validation("email", "email has already been taken")),
            AuthError::Invalid { field, message } => Some(validation(field, message)),
            AuthError::InvalidToken => Some(bearer_challenge("invalid_token", "Invalid token")),
            AuthError::TokenExpired => Some(bearer_challenge("token_expired", "Token expired")),
            AuthError::InvalidRefreshToken | AuthError::RefreshTokenReused => Some(
                AppError::unauthenticated()
                    .with_code("invalid_refresh_token")
                    .with_detail("Invalid refresh token"),
            ),
            AuthError::UserNotFound => Some(AppError::not_found().with_code("user_not_found")),
            AuthError::InvalidResetToken => Some(
                AppError::bad_request("Invalid or expired password reset token")
                    .with_code("invalid_reset_token"),
            ),
            AuthError::InvalidVerificationLink => Some(
                AppError::forbidden()
                    .with_code("invalid_verification_link")
                    .with_detail("Invalid verification link"),
            ),
            AuthError::TwoFactorRequired => Some(AppError::two_factor_required()),
            AuthError::TwoFactorAlreadyEnabled => Some(
                AppError::conflict("Two-factor authentication is already enabled")
                    .with_code("two_factor_already_enabled"),
            ),
            AuthError::TwoFactorNotEnabled => Some(
                AppError::bad_request("Two-factor authentication is not enabled")
                    .with_code("two_factor_not_enabled"),
            ),
            AuthError::InvalidTwoFactorCode => Some(
                AppError::unauthenticated()
                    .with_code("invalid_two_factor_code")
                    .with_detail("Invalid two-factor code"),
            ),
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

/// 401 with `code`, `message` and an RFC 6750 `WWW-Authenticate` challenge.
fn bearer_challenge(code: &str, message: &str) -> AppError {
    AppError::unauthenticated()
        .with_code(code)
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
            json!({
                "statusCode": 401,
                "message": "Invalid credentials",
                "error": "Unauthorized",
                "code": "invalid_credentials",
            })
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

    /// The code a front end maps to a translation key, per refusal.
    #[test]
    fn every_refusal_carries_its_code() {
        let cases = [
            (AuthError::InvalidCredentials, 401, "invalid_credentials"),
            (
                AuthError::TooManyAttempts {
                    retry_after: Duration::from_secs(30),
                },
                429,
                "too_many_login_attempts",
            ),
            (AuthError::EmailTaken, 400, "validation_failed"),
            (
                AuthError::Invalid {
                    field: "password",
                    message: "password should not be empty".to_owned(),
                },
                400,
                "validation_failed",
            ),
            (AuthError::InvalidToken, 401, "invalid_token"),
            (AuthError::TokenExpired, 401, "token_expired"),
            (AuthError::InvalidRefreshToken, 401, "invalid_refresh_token"),
            (AuthError::RefreshTokenReused, 401, "invalid_refresh_token"),
            (AuthError::UserNotFound, 404, "user_not_found"),
            (AuthError::InvalidResetToken, 400, "invalid_reset_token"),
            (
                AuthError::InvalidVerificationLink,
                403,
                "invalid_verification_link",
            ),
            (AuthError::TwoFactorRequired, 401, "two_factor_required"),
            (
                AuthError::TwoFactorAlreadyEnabled,
                409,
                "two_factor_already_enabled",
            ),
            (
                AuthError::TwoFactorNotEnabled,
                400,
                "two_factor_not_enabled",
            ),
            (
                AuthError::InvalidTwoFactorCode,
                401,
                "invalid_two_factor_code",
            ),
            (AuthError::ShutDown, 500, "internal_error"),
        ];
        for (error, status, code) in cases {
            let what = error.to_string();
            let error = error.into_app_error();
            assert_eq!(error.status.as_u16(), status, "{what}");
            assert_eq!(error.body()["code"], code, "{what}");
            assert!(navree_http::codes::is_valid(code), "{code}");
        }
        assert_eq!(
            AuthError::TwoFactorRequired.into_app_error().body(),
            AppError::two_factor_required().body(),
            "the framework's stable two_factor_required error"
        );
    }

    #[test]
    fn internal_errors_stay_500_without_details() {
        let error = AuthError::Signing("key=hunter2".to_owned()).into_app_error();
        assert_eq!(error.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(!error.body().to_string().contains("hunter2"));
    }
}
