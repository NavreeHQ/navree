//! [`AuthError`] and its HTTP mapping.

use std::collections::BTreeMap;
use std::time::Duration;

use http::HeaderValue;
use http::header::WWW_AUTHENTICATE;
use navree_cache::CacheError;
use navree_crypto::CryptoError;
use navree_http::AppError;
use navree_kernel::BoxError;
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
        /// A class-validator style message, e.g. "password must be longer
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
            AuthError::Db(error) => navree_db::map_db_error(error),
            AuthError::Signing(_)
            | AuthError::Crypto(_)
            | AuthError::Cache(_)
            | AuthError::Extension(_) => None,
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
    fn credentials_and_tokens_are_401_with_nest_messages() {
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
    fn internal_errors_stay_500_without_details() {
        let error = AuthError::Signing("key=hunter2".to_owned()).into_app_error();
        assert_eq!(error.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(!error.body().to_string().contains("hunter2"));
    }
}
