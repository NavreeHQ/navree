//! [`SocialError`] and its HTTP mapping.

use navree_auth::AuthError;
use navree_cache::CacheError;
use navree_crypto::CryptoError;
use navree_db::DbError;
use navree_http::AppError;
use navree_http_client::HttpClientError;
use navree_kernel::{BoxError, KernelError};
use navree_orm::OrmError;

/// A social sign-in error. Returned from handlers with `?`, it becomes an
/// error response (the module registers the mapping): a bad or used state
/// and a provider refusing the code are 400, the linking rules give 403 and
/// 409, an unreachable provider 502, and anything internal a 500 whose
/// details are logged, never sent.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SocialError {
    /// The state is malformed, forged, for another provider, expired or
    /// already used.
    #[error("invalid or expired state")]
    InvalidState,
    /// No provider is configured under this key.
    #[error("unknown provider `{0}`")]
    UnknownProvider(String),
    /// The provider refused the code, the token request or the profile
    /// request (an OAuth error code such as `invalid_grant`, or a status).
    #[error("{provider} refused the sign-in: {message}")]
    Provider {
        /// The provider's display name.
        provider: String,
        /// What the provider answered (never a token).
        message: String,
    },
    /// The provider could not be reached or answered with a server error.
    #[error("{provider} is unavailable: {message}")]
    ProviderUnavailable {
        /// The provider's display name.
        provider: String,
        /// What went wrong.
        message: String,
    },
    /// The ID token failed verification (signature, issuer, audience,
    /// expiry or nonce).
    #[error("invalid ID token from {provider}: {reason}")]
    InvalidIdToken {
        /// The provider's display name.
        provider: String,
        /// Which check failed.
        reason: String,
    },
    /// The account may not sign in here: another Google Workspace domain
    /// or Slack workspace, or an email domain outside
    /// `allowed_email_domains`.
    #[error("{0}")]
    NotAllowed(String),
    /// The provider's verified email belongs to an existing user, and
    /// automatic linking does not apply.
    #[error("an account with this email already exists")]
    EmailTaken {
        /// The provider's display name.
        provider: String,
    },
    /// No user matches and registration is off.
    #[error("registration is closed")]
    RegistrationClosed,
    /// A new user would be created, but the provider shared no email
    /// address.
    #[error("{provider} did not share an email address")]
    MissingEmail {
        /// The provider's display name.
        provider: String,
    },
    /// The provider account is already linked to another user.
    #[error("this {provider} account is linked to another user")]
    AlreadyLinked {
        /// The provider's display name.
        provider: String,
    },
    /// The user has already connected another account of this provider.
    #[error("{provider} is already connected")]
    ProviderAlreadyConnected {
        /// The provider's display name.
        provider: String,
    },
    /// The user has not connected this provider.
    #[error("{provider} is not connected")]
    NotLinked {
        /// The provider's display name.
        provider: String,
    },
    /// Disconnecting would leave the user no way to sign in (no password
    /// and no other provider).
    #[error("{provider} is the last way to sign in")]
    LastSignInMethod {
        /// The provider's display name.
        provider: String,
    },
    /// A state for linking was used by another user, or a sign-in state to
    /// link (or the other way round).
    #[error("the state was not issued for this request")]
    WrongIntent,
    /// `return_to` is not a relative path.
    #[error("return_to must be a relative path")]
    InvalidReturnTo,
    /// The provider's access token has expired and could not be refreshed
    /// (the user revoked access, or no refresh token was stored): send the
    /// user through the provider again. The account stays linked.
    #[error("{provider} needs to be authorized again")]
    Reauthorize {
        /// The provider's display name.
        provider: String,
    },
    /// No provider tokens are stored for this account (`social.store_tokens`
    /// is off, or the account was linked before it was turned on).
    #[error("no {provider} tokens are stored for this user")]
    NoToken {
        /// The provider's display name.
        provider: String,
    },
    /// Too many failed callbacks from this client.
    #[error("too many sign-in attempts")]
    TooManyAttempts {
        /// When the client may try again.
        retry_after: std::time::Duration,
    },
    /// The provider has no sign-in from native apps: it is neither Apple
    /// nor Google, or (Apple) no app is configured in `auth.apps.apple`.
    #[error("{provider} sign-in from an app is not set up")]
    NativeUnavailable {
        /// The provider's display name.
        provider: String,
    },
    /// A provider's server-to-server notification failed verification
    /// (signature, issuer, audience, age or shape).
    #[error("invalid notification from {provider}: {reason}")]
    InvalidNotification {
        /// The provider's display name.
        provider: String,
        /// Which check failed.
        reason: String,
    },
    /// The [`Social`](crate::Social) service is not part of a booted app
    /// (a [`Social::fake`](crate::Social::fake) never passed to
    /// `override_value`).
    #[error("the Social service is not bound to an app")]
    NotBound,
    /// A provider is misconfigured (detected while signing in).
    #[error("provider configuration error: {0}")]
    Config(String),
    /// An auth error (issuing tokens, the user table).
    #[error(transparent)]
    Auth(#[from] AuthError),
    /// An outgoing request failed.
    #[error("provider request failed: {0}")]
    Http(#[from] HttpClientError),
    /// Sealing a state or encrypting a token failed.
    #[error("social crypto error: {0}")]
    Crypto(#[from] CryptoError),
    /// The state cache failed.
    #[error("social cache error: {0}")]
    Cache(#[from] CacheError),
    /// A database error (navree-db's statements and units of work).
    #[error("social database error: {0}")]
    Db(#[from] DbError),
    /// An ORM error (the module's models and their queries).
    #[error("social database error: {0}")]
    Orm(#[from] OrmError),
    /// A provider the service needs could not be resolved.
    #[error("social provider error: {0}")]
    Kernel(#[from] KernelError),
    /// Any other error (a custom provider's).
    #[error("{0}")]
    Other(BoxError),
}

impl SocialError {
    /// A provider refusal (for custom providers).
    pub fn provider(provider: impl Into<String>, message: impl Into<String>) -> Self {
        SocialError::Provider {
            provider: provider.into(),
            message: message.into(),
        }
    }

    /// The error response for this error, `None` for errors that stay a 500
    /// (their details are logged, never sent).
    pub fn to_app_error(&self) -> Option<AppError> {
        let error = match self {
            SocialError::InvalidState | SocialError::WrongIntent => {
                AppError::bad_request("Invalid or expired state").with_code("invalid_state")
            }
            SocialError::UnknownProvider(key) => AppError::not_found()
                .with_code("unknown_provider")
                .with_detail(format!("Unknown sign-in provider `{key}`")),
            SocialError::Provider { provider, message } => AppError::bad_request(format!(
                "{provider} refused the sign-in: {message}"
            ))
            .with_code("provider_refused"),
            SocialError::InvalidIdToken { provider, .. } => {
                AppError::bad_request(format!("{provider} sent an invalid ID token"))
                    .with_code("invalid_id_token")
            }
            SocialError::ProviderUnavailable { provider, .. } => {
                AppError::from_status(http::StatusCode::BAD_GATEWAY)
                    .with_code("provider_unavailable")
                    .with_detail(format!("{provider} is unavailable. Please try again."))
            }
            SocialError::Http(_) => AppError::from_status(http::StatusCode::BAD_GATEWAY)
                .with_code("provider_unavailable")
                .with_detail("The sign-in provider is unavailable. Please try again."),
            SocialError::NotAllowed(message) => AppError::forbidden()
                .with_code("sign_in_not_allowed")
                .with_detail(message),
            SocialError::EmailTaken { provider } => AppError::conflict(format!(
                "An account with this email already exists. Sign in, then connect {provider} in your settings."
            ))
            .with_code("email_taken"),
            SocialError::RegistrationClosed => AppError::forbidden()
                .with_code("registration_closed")
                .with_detail("Registration is closed"),
            SocialError::MissingEmail { provider } => {
                AppError::bad_request(format!("{provider} did not share an email address"))
                    .with_code("missing_email")
            }
            SocialError::AlreadyLinked { provider } => {
                AppError::conflict(format!("This {provider} account is linked to another user"))
                    .with_code("account_already_linked")
            }
            SocialError::ProviderAlreadyConnected { provider } => AppError::conflict(format!(
                "{provider} is already connected. Disconnect it first."
            ))
            .with_code("provider_already_connected"),
            SocialError::NotLinked { provider } => AppError::not_found()
                .with_code("provider_not_connected")
                .with_detail(format!("{provider} is not connected")),
            SocialError::LastSignInMethod { provider } => AppError::conflict(format!(
                "Set a password or connect another provider before disconnecting {provider}"
            ))
            .with_code("last_sign_in_method"),
            SocialError::InvalidReturnTo => {
                AppError::bad_request("return_to must be a relative path")
                    .with_code("invalid_return_to")
            }
            SocialError::Reauthorize { provider } => {
                AppError::conflict(format!("Connect {provider} again to continue"))
                    .with_code("reauthorization_required")
            }
            SocialError::NoToken { provider } => AppError::not_found()
                .with_code("no_provider_token")
                .with_detail(format!("No {provider} tokens are stored")),
            SocialError::TooManyAttempts { retry_after } => {
                AppError::too_many_requests(*retry_after)
                    .with_code("too_many_login_attempts")
                    .with_detail(format!(
                        "Too many sign-in attempts. Please try again in {} seconds.",
                        retry_after.as_secs().max(1)
                    ))
            }
            SocialError::NativeUnavailable { provider } => AppError::not_found()
                .with_code("native_sign_in_unavailable")
                .with_detail(format!("{provider} sign-in from an app is not set up")),
            SocialError::InvalidNotification { provider, .. } => {
                AppError::bad_request(format!("{provider} sent an invalid notification"))
                    .with_code("invalid_notification")
            }
            SocialError::Auth(error) => return error.to_app_error(),
            SocialError::Db(error) => return navree_db::map_db_error(error),
            SocialError::Orm(error) => {
                return error.db_error().and_then(navree_db::map_db_error);
            }
            SocialError::NotBound
            | SocialError::Config(_)
            | SocialError::Crypto(_)
            | SocialError::Cache(_)
            | SocialError::Kernel(_)
            | SocialError::Other(_) => return None,
        };
        Some(error)
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

impl From<serde_json::Error> for SocialError {
    fn from(error: serde_json::Error) -> Self {
        SocialError::Other(Box::new(error))
    }
}

/// The mapping registered with `map_error`.
pub(crate) fn map_social_error(error: &SocialError) -> Option<AppError> {
    error.to_app_error()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use http::StatusCode;
    use http::header::RETRY_AFTER;
    use serde_json::json;

    use super::*;

    #[test]
    fn state_errors_are_400_without_detail() {
        let body = SocialError::InvalidState.into_app_error().body();
        assert_eq!(
            body,
            json!({
                "statusCode": 400,
                "message": "Invalid or expired state",
                "error": "Bad Request",
                "code": "invalid_state",
            })
        );
        assert_eq!(
            SocialError::WrongIntent.into_app_error().body()["message"],
            "Invalid or expired state"
        );
    }

    #[test]
    fn linking_rules_map_to_the_documented_answers() {
        let cases = [
            (
                SocialError::EmailTaken {
                    provider: "GitHub".to_owned(),
                },
                409,
                "An account with this email already exists. Sign in, then connect GitHub in your settings.",
            ),
            (
                SocialError::RegistrationClosed,
                403,
                "Registration is closed",
            ),
            (
                SocialError::AlreadyLinked {
                    provider: "GitHub".to_owned(),
                },
                409,
                "This GitHub account is linked to another user",
            ),
            (
                SocialError::LastSignInMethod {
                    provider: "Google".to_owned(),
                },
                409,
                "Set a password or connect another provider before disconnecting Google",
            ),
            (
                SocialError::NotLinked {
                    provider: "Google".to_owned(),
                },
                404,
                "Google is not connected",
            ),
            (
                SocialError::Reauthorize {
                    provider: "Google".to_owned(),
                },
                409,
                "Connect Google again to continue",
            ),
        ];
        for (error, status, message) in cases {
            let error = error.into_app_error();
            assert_eq!(error.status.as_u16(), status, "{message}");
            assert_eq!(error.body()["message"], message);
        }
    }

    #[test]
    fn provider_failures_are_400_or_502() {
        let refused = SocialError::provider("GitHub", "bad_verification_code").into_app_error();
        assert_eq!(refused.status, StatusCode::BAD_REQUEST);
        assert_eq!(
            refused.body()["message"],
            "GitHub refused the sign-in: bad_verification_code"
        );
        let down = SocialError::ProviderUnavailable {
            provider: "GitHub".to_owned(),
            message: "status 503".to_owned(),
        }
        .into_app_error();
        assert_eq!(down.status, StatusCode::BAD_GATEWAY);
        assert!(!down.body().to_string().contains("503"));
    }

    #[test]
    fn throttling_is_429_with_retry_after() {
        let error = SocialError::TooManyAttempts {
            retry_after: Duration::from_secs(30),
        }
        .into_app_error();
        assert_eq!(error.status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(error.headers[RETRY_AFTER], "30");
    }

    #[test]
    fn internal_errors_stay_500_without_details() {
        let error = SocialError::Config("client_secret=hunter2".to_owned()).into_app_error();
        assert_eq!(error.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(!error.body().to_string().contains("hunter2"));
    }

    #[test]
    fn every_refusal_has_its_own_code() {
        let github = || "GitHub".to_owned();
        let cases = [
            (SocialError::InvalidState, "invalid_state"),
            (SocialError::WrongIntent, "invalid_state"),
            (
                SocialError::UnknownProvider("gitlab".to_owned()),
                "unknown_provider",
            ),
            (
                SocialError::provider("GitHub", "access_denied"),
                "provider_refused",
            ),
            (
                SocialError::InvalidIdToken {
                    provider: github(),
                    reason: "bad issuer".to_owned(),
                },
                "invalid_id_token",
            ),
            (
                SocialError::ProviderUnavailable {
                    provider: github(),
                    message: "status 503".to_owned(),
                },
                "provider_unavailable",
            ),
            (
                SocialError::NotAllowed("Use your acme.test address".to_owned()),
                "sign_in_not_allowed",
            ),
            (
                SocialError::EmailTaken { provider: github() },
                "email_taken",
            ),
            (SocialError::RegistrationClosed, "registration_closed"),
            (
                SocialError::MissingEmail { provider: github() },
                "missing_email",
            ),
            (
                SocialError::AlreadyLinked { provider: github() },
                "account_already_linked",
            ),
            (
                SocialError::ProviderAlreadyConnected { provider: github() },
                "provider_already_connected",
            ),
            (
                SocialError::NotLinked { provider: github() },
                "provider_not_connected",
            ),
            (
                SocialError::LastSignInMethod { provider: github() },
                "last_sign_in_method",
            ),
            (SocialError::InvalidReturnTo, "invalid_return_to"),
            (
                SocialError::Reauthorize { provider: github() },
                "reauthorization_required",
            ),
            (
                SocialError::NoToken { provider: github() },
                "no_provider_token",
            ),
            (
                SocialError::TooManyAttempts {
                    retry_after: Duration::from_secs(30),
                },
                "too_many_login_attempts",
            ),
            (
                SocialError::NativeUnavailable { provider: github() },
                "native_sign_in_unavailable",
            ),
            (
                SocialError::InvalidNotification {
                    provider: github(),
                    reason: "bad signature".to_owned(),
                },
                "invalid_notification",
            ),
            (
                SocialError::Config("x".to_owned()),
                navree_http::codes::INTERNAL_ERROR,
            ),
        ];
        for (error, code) in cases {
            let message = error.to_string();
            let error = error.into_app_error();
            assert_eq!(error.code, code, "{message}");
            assert!(navree_http::codes::is_valid(&error.code), "{message}");
        }
    }

    #[test]
    fn auth_errors_keep_their_mapping() {
        let error = SocialError::Auth(AuthError::InvalidToken).into_app_error();
        assert_eq!(error.status, StatusCode::UNAUTHORIZED);
    }
}
