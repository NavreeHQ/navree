//! [`IntegrationsError`] and its HTTP mapping.

use http::StatusCode;
use navree_cache::CacheError;
use navree_crypto::CryptoError;
use navree_db::sea_orm::DbErr;
use navree_http::AppError;
use navree_http_client::HttpClientError;
use navree_kernel::KernelError;
use navree_orm::OrmError;

/// An integrations error. Returned from handlers with `?`, it becomes an
/// error response (the module registers the mapping): a bad state, a
/// refused consent or bad credentials are 400, a missing connection 404, a
/// connection whose credentials stopped working 409, an unreachable
/// provider 502, and anything internal a 500 whose details are logged,
/// never sent.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum IntegrationsError {
    /// No provider is configured under this key.
    #[error("unknown integration `{0}`")]
    UnknownProvider(String),
    /// The scope set is not registered, or names no scopes for the
    /// provider.
    #[error("unknown scope set `{scope_set}` for {provider}")]
    UnknownScopeSet {
        /// The provider's display name.
        provider: String,
        /// The scope set asked for.
        scope_set: String,
    },
    /// The provider does not connect this way (Slack with an API key).
    #[error("{provider} does not connect with {kind}")]
    Unsupported {
        /// The provider's display name.
        provider: String,
        /// How it was asked to connect.
        kind: &'static str,
    },
    /// The state is malformed, forged, for another provider, owner or
    /// user, expired or already used.
    #[error("invalid or expired state")]
    InvalidState,
    /// `return_to` is not a relative path.
    #[error("return_to must be a relative path")]
    InvalidReturnTo,
    /// The admin declined the consent screen (or the provider reported an
    /// error on the callback).
    #[error("{provider} consent was not granted: {error}")]
    ConsentDenied {
        /// The provider's display name.
        provider: String,
        /// The provider's error code (`access_denied`).
        error: String,
    },
    /// The provider refused the code or the credentials (an OAuth error
    /// such as `invalid_grant` or `unauthorized_client`).
    #[error("{provider} refused the connection: {error}")]
    Refused {
        /// The provider's display name.
        provider: String,
        /// The provider's error code.
        error: String,
        /// Its description, when it gave one.
        description: Option<String>,
    },
    /// The submitted credentials can't be used (a key file that is not a
    /// service-account key, an empty API key).
    #[error("{0}")]
    InvalidCredentials(String),
    /// The owner has no connection to this provider in this scope set.
    #[error("{provider} is not connected")]
    NotConnected {
        /// The provider's display name.
        provider: String,
        /// The scope set.
        scope_set: String,
    },
    /// No connection has this id (or it belongs to someone else).
    #[error("connection not found")]
    ConnectionNotFound,
    /// The connection's credentials stopped working (consent revoked, a
    /// refresh token or key refused): connect again.
    #[error("{provider} needs to be connected again")]
    Reauthorize {
        /// The provider's display name.
        provider: String,
        /// What the provider answered.
        error: String,
    },
    /// The provider could not be reached or answered with a server error.
    #[error("{provider} is unavailable: {message}")]
    Unavailable {
        /// The provider's display name.
        provider: String,
        /// What went wrong.
        message: String,
    },
    /// An API answered with an error in its body (Slack's `ok: false`).
    #[error("{provider} API error: {error}")]
    Api {
        /// The provider's display name.
        provider: String,
        /// The provider's error code (`channel_not_found`).
        error: String,
    },
    /// A typed client was given an absolute URL on a host outside the
    /// provider's API (tokens are never sent elsewhere).
    #[error("refusing to send a {provider} token to `{host}`")]
    ForeignUrl {
        /// The provider's display name.
        provider: String,
        /// The host that was refused.
        host: String,
    },
    /// The [`Integrations`](crate::Integrations) service is not part of a
    /// booted app (a fake never passed to `override_value`).
    #[error("the Integrations service is not bound to an app")]
    NotBound,
    /// A provider is misconfigured in a way found at run time.
    #[error("integrations configuration error: {0}")]
    Config(String),
    /// An outgoing request failed.
    #[error("integration request failed: {0}")]
    Http(#[from] HttpClientError),
    /// Encrypting or decrypting credentials failed (no application key).
    #[error("integrations crypto error: {0}")]
    Crypto(#[from] CryptoError),
    /// The state cache or a token lock failed.
    #[error("integrations cache error: {0}")]
    Cache(#[from] CacheError),
    /// A database error.
    #[error("integrations database error: {0}")]
    Db(#[from] DbErr),
    /// A provider the service needs could not be resolved.
    #[error("integrations provider error: {0}")]
    Kernel(#[from] KernelError),
    /// No mailbox watch has this id (crate feature `mail`).
    #[cfg(feature = "mail")]
    #[error("mailbox watch not found")]
    MailboxWatchNotFound,
    /// The inbound pipeline refused or failed a message of a watched
    /// mailbox (crate feature `mail`).
    #[cfg(feature = "mail")]
    #[error(transparent)]
    Inbound(#[from] navree_mail::InboundError),
}

impl IntegrationsError {
    /// The error response for this error, `None` for errors that stay a 500
    /// (their details are logged, never sent).
    pub fn to_app_error(&self) -> Option<AppError> {
        let error = match self {
            IntegrationsError::UnknownProvider(key) => AppError::not_found()
                .with_code("unknown_integration")
                .with_detail(format!("Unknown integration `{key}`")),
            IntegrationsError::UnknownScopeSet {
                provider,
                scope_set,
            } => AppError::bad_request(format!("Unknown scope set `{scope_set}` for {provider}"))
                .with_code("unknown_scope_set"),
            IntegrationsError::Unsupported { provider, kind } => {
                AppError::bad_request(format!("{provider} does not connect with {kind}"))
                    .with_code("unsupported_connection")
            }
            IntegrationsError::InvalidState => {
                AppError::bad_request("Invalid or expired state").with_code("invalid_state")
            }
            IntegrationsError::InvalidReturnTo => {
                AppError::bad_request("return_to must be a relative path")
                    .with_code("invalid_return_to")
            }
            IntegrationsError::ConsentDenied { provider, error } => {
                AppError::bad_request(format!("{provider} consent was not granted: {error}"))
                    .with_code("consent_denied")
            }
            IntegrationsError::Refused {
                provider, error, ..
            } => AppError::bad_request(format!("{provider} refused the connection: {error}"))
                .with_code("integration_refused"),
            IntegrationsError::InvalidCredentials(message) => {
                AppError::bad_request(message.clone()).with_code("invalid_credentials")
            }
            IntegrationsError::NotConnected { provider, .. } => AppError::not_found()
                .with_code("integration_not_connected")
                .with_detail(format!("{provider} is not connected")),
            IntegrationsError::ConnectionNotFound => AppError::not_found()
                .with_code("connection_not_found")
                .with_detail("Connection not found"),
            IntegrationsError::Reauthorize { provider, .. } => {
                AppError::conflict(format!("Connect {provider} again to continue"))
                    .with_code("integration_reauthorization_required")
            }
            IntegrationsError::Unavailable { provider, .. } => {
                AppError::from_status(StatusCode::BAD_GATEWAY)
                    .with_code("integration_unavailable")
                    .with_detail(format!("{provider} is unavailable. Please try again."))
            }
            IntegrationsError::Api { provider, error } => {
                AppError::from_status(StatusCode::BAD_GATEWAY)
                    .with_code("integration_api_error")
                    .with_detail(format!("{provider} answered: {error}"))
            }
            IntegrationsError::Http(_) => AppError::from_status(StatusCode::BAD_GATEWAY)
                .with_code("integration_unavailable")
                .with_detail("The integration is unavailable. Please try again."),
            IntegrationsError::Db(error) => return navree_db::map_db_error(error),
            #[cfg(feature = "mail")]
            IntegrationsError::MailboxWatchNotFound => AppError::not_found()
                .with_code("mailbox_watch_not_found")
                .with_detail("Mailbox watch not found"),
            #[cfg(feature = "mail")]
            IntegrationsError::Inbound(error) => return error.to_app_error(),
            IntegrationsError::ForeignUrl { .. }
            | IntegrationsError::NotBound
            | IntegrationsError::Config(_)
            | IntegrationsError::Crypto(_)
            | IntegrationsError::Cache(_)
            | IntegrationsError::Kernel(_) => return None,
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

    /// Whether the provider refused the credentials for good (as opposed
    /// to being unreachable for a while).
    pub fn is_refusal(&self) -> bool {
        matches!(
            self,
            IntegrationsError::Refused { .. }
                | IntegrationsError::Reauthorize { .. }
                | IntegrationsError::ConsentDenied { .. }
                | IntegrationsError::InvalidCredentials(_)
        )
    }
}

/// The ORM's errors are database errors (`Db`), mapped the same way.
impl From<OrmError> for IntegrationsError {
    fn from(error: OrmError) -> Self {
        IntegrationsError::Db(DbErr::from(error))
    }
}

/// The mapping registered with `map_error`.
pub(crate) fn map_integrations_error(error: &IntegrationsError) -> Option<AppError> {
    error.to_app_error()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn body(error: IntegrationsError) -> serde_json::Value {
        error.into_app_error().body()
    }

    #[test]
    fn refusals_map_to_the_documented_answers() {
        let cases = [
            (
                IntegrationsError::UnknownProvider("zoom".into()),
                404,
                "unknown_integration",
                "Unknown integration `zoom`",
            ),
            (
                IntegrationsError::UnknownScopeSet {
                    provider: "Slack".into(),
                    scope_set: "directory".into(),
                },
                400,
                "unknown_scope_set",
                "Unknown scope set `directory` for Slack",
            ),
            (
                IntegrationsError::Unsupported {
                    provider: "Slack".into(),
                    kind: "an API key",
                },
                400,
                "unsupported_connection",
                "Slack does not connect with an API key",
            ),
            (
                IntegrationsError::InvalidState,
                400,
                "invalid_state",
                "Invalid or expired state",
            ),
            (
                IntegrationsError::ConsentDenied {
                    provider: "Microsoft 365".into(),
                    error: "access_denied".into(),
                },
                400,
                "consent_denied",
                "Microsoft 365 consent was not granted: access_denied",
            ),
            (
                IntegrationsError::Refused {
                    provider: "Slack".into(),
                    error: "invalid_code".into(),
                    description: None,
                },
                400,
                "integration_refused",
                "Slack refused the connection: invalid_code",
            ),
            (
                IntegrationsError::NotConnected {
                    provider: "Microsoft 365".into(),
                    scope_set: "default".into(),
                },
                404,
                "integration_not_connected",
                "Microsoft 365 is not connected",
            ),
            (
                IntegrationsError::ConnectionNotFound,
                404,
                "connection_not_found",
                "Connection not found",
            ),
            (
                IntegrationsError::Reauthorize {
                    provider: "Google Workspace".into(),
                    error: "invalid_grant".into(),
                },
                409,
                "integration_reauthorization_required",
                "Connect Google Workspace again to continue",
            ),
            (
                IntegrationsError::Unavailable {
                    provider: "Slack".into(),
                    message: "503".into(),
                },
                502,
                "integration_unavailable",
                "Slack is unavailable. Please try again.",
            ),
            (
                IntegrationsError::Api {
                    provider: "Slack".into(),
                    error: "channel_not_found".into(),
                },
                502,
                "integration_api_error",
                "Slack answered: channel_not_found",
            ),
        ];
        for (error, status, code, message) in cases {
            let body = body(error);
            assert_eq!(body["statusCode"], json!(status), "{body}");
            assert_eq!(body["code"], json!(code), "{body}");
            assert_eq!(body["message"], json!(message), "{body}");
            assert!(navree_http::codes::is_valid(code));
        }
    }

    #[test]
    fn internal_errors_stay_500_without_details() {
        let body = body(IntegrationsError::ForeignUrl {
            provider: "Slack".into(),
            host: "evil.test".into(),
        });
        assert_eq!(body["statusCode"], json!(500));
        assert!(!body.to_string().contains("evil.test"));
        assert!(IntegrationsError::NotBound.to_app_error().is_none());
    }

    #[test]
    fn refusals_are_told_apart_from_outages() {
        assert!(
            IntegrationsError::Refused {
                provider: "x".into(),
                error: "invalid_grant".into(),
                description: None
            }
            .is_refusal()
        );
        assert!(
            !IntegrationsError::Unavailable {
                provider: "x".into(),
                message: "timeout".into()
            }
            .is_refusal()
        );
    }
}
