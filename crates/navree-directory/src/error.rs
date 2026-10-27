//! [`DirectoryError`] and its HTTP mapping.

use http::StatusCode;
use navree_crypto::CryptoError;
use navree_db::sea_orm::DbErr;
use navree_http::AppError;
use navree_integrations::IntegrationsError;
use navree_kernel::{BoxError, KernelError};
use navree_orm::OrmError;

/// A directory error. Returned from handlers with `?`, it becomes an error
/// response (the module registers the mapping): bad settings and unknown
/// sources are 400, a missing connection or run 404, a sync that is
/// already running or a run that is not held 409, a directory that could
/// not be read 502, and anything internal a 500 whose details are logged,
/// never sent.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DirectoryError {
    /// No connection has this id (or it belongs to someone else).
    #[error("directory connection not found")]
    ConnectionNotFound,
    /// No run has this id.
    #[error("directory run not found")]
    RunNotFound,
    /// No source is registered under this name.
    #[error("unknown directory source `{0}`")]
    UnknownSource(String),
    /// The connection's settings or input are not usable.
    #[error("{0}")]
    InvalidSettings(String),
    /// A SCIM connection is kept up to date by its identity provider; it
    /// has nothing to pull.
    #[error("SCIM connections are kept up to date by the identity provider")]
    PushOnly,
    /// A SCIM token was asked of a connection that is not a SCIM one.
    #[error("only SCIM connections have a token")]
    NotScim,
    /// The connection is paused.
    #[error("the directory connection is paused")]
    Paused,
    /// A run of the connection is queued or running.
    #[error("a sync of this connection is already running")]
    SyncRunning,
    /// Only a held run can be approved.
    #[error("only a held run can be approved")]
    RunNotHeld,
    /// The directory could not be read (an LDAP server that refused the
    /// bind, an API that answered with an error).
    #[error("{source_name} could not be read: {message}")]
    Source {
        /// The source.
        source_name: String,
        /// What went wrong (never a credential).
        message: String,
    },
    /// Your sink failed while applying a change.
    #[error("the directory sink failed: {0}")]
    Sink(BoxError),
    /// The [`Directory`](crate::Directory) service is not part of a booted
    /// app (a fake never passed to `override_value`).
    #[error("the Directory service is not bound to an app")]
    NotBound,
    /// Something the module needs is missing or misconfigured.
    #[error("directory configuration error: {0}")]
    Config(String),
    /// A provider connection failed (Microsoft Graph, Google Workspace,
    /// Okta through navree-integrations).
    #[error(transparent)]
    Integrations(#[from] IntegrationsError),
    /// Encrypting or decrypting the secret failed (no application key).
    #[error("directory crypto error: {0}")]
    Crypto(#[from] CryptoError),
    /// A database error.
    #[error("directory database error: {0}")]
    Db(#[from] DbErr),
    /// A provider the service needs could not be resolved.
    #[error("directory provider error: {0}")]
    Kernel(#[from] KernelError),
    /// Queueing the sync failed.
    #[error("directory queue error: {0}")]
    Queue(String),
}

impl DirectoryError {
    /// A source failure.
    pub fn source_failed(source: impl Into<String>, message: impl Into<String>) -> Self {
        DirectoryError::Source {
            source_name: source.into(),
            message: message.into(),
        }
    }

    /// The error response for this error, `None` for errors that stay a 500
    /// (their details are logged, never sent).
    pub fn to_app_error(&self) -> Option<AppError> {
        let error = match self {
            DirectoryError::ConnectionNotFound => AppError::not_found()
                .with_code("directory_connection_not_found")
                .with_detail("Directory connection not found"),
            DirectoryError::RunNotFound => AppError::not_found()
                .with_code("directory_run_not_found")
                .with_detail("Directory run not found"),
            DirectoryError::UnknownSource(source) => {
                AppError::bad_request(format!("Unknown directory source `{source}`"))
                    .with_code("unknown_directory_source")
            }
            DirectoryError::InvalidSettings(message) => {
                AppError::bad_request(message.clone()).with_code("invalid_directory_settings")
            }
            DirectoryError::PushOnly => AppError::bad_request(
                "SCIM connections are kept up to date by the identity provider",
            )
            .with_code("directory_push_only"),
            DirectoryError::NotScim => AppError::bad_request("Only SCIM connections have a token")
                .with_code("directory_not_scim"),
            DirectoryError::Paused => AppError::conflict("The directory connection is paused")
                .with_code("directory_connection_paused"),
            DirectoryError::SyncRunning => {
                AppError::conflict("A sync of this connection is already running")
                    .with_code("directory_sync_running")
            }
            DirectoryError::RunNotHeld => AppError::conflict("Only a held run can be approved")
                .with_code("directory_run_not_held"),
            DirectoryError::Source {
                source_name,
                message,
            } => AppError::from_status(StatusCode::BAD_GATEWAY)
                .with_code("directory_source_failed")
                .with_detail(format!("{source_name} could not be read: {message}")),
            DirectoryError::Integrations(error) => return error.to_app_error(),
            DirectoryError::Db(error) => return navree_db::map_db_error(error),
            DirectoryError::Sink(_)
            | DirectoryError::NotBound
            | DirectoryError::Config(_)
            | DirectoryError::Crypto(_)
            | DirectoryError::Kernel(_)
            | DirectoryError::Queue(_) => return None,
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

/// The ORM's errors are database errors (`Db`), mapped the same way.
impl From<OrmError> for DirectoryError {
    fn from(error: OrmError) -> Self {
        DirectoryError::Db(DbErr::from(error))
    }
}

/// The mapping registered with `map_error`.
pub(crate) fn map_directory_error(error: &DirectoryError) -> Option<AppError> {
    error.to_app_error()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn body(error: DirectoryError) -> serde_json::Value {
        error.into_app_error().body()
    }

    #[test]
    fn refusals_map_to_the_documented_answers() {
        let cases = [
            (
                DirectoryError::ConnectionNotFound,
                404,
                "directory_connection_not_found",
                "Directory connection not found",
            ),
            (
                DirectoryError::RunNotFound,
                404,
                "directory_run_not_found",
                "Directory run not found",
            ),
            (
                DirectoryError::UnknownSource("workday".into()),
                400,
                "unknown_directory_source",
                "Unknown directory source `workday`",
            ),
            (
                DirectoryError::InvalidSettings("settings.base_dn is required for ldap".into()),
                400,
                "invalid_directory_settings",
                "settings.base_dn is required for ldap",
            ),
            (
                DirectoryError::PushOnly,
                400,
                "directory_push_only",
                "SCIM connections are kept up to date by the identity provider",
            ),
            (
                DirectoryError::NotScim,
                400,
                "directory_not_scim",
                "Only SCIM connections have a token",
            ),
            (
                DirectoryError::Paused,
                409,
                "directory_connection_paused",
                "The directory connection is paused",
            ),
            (
                DirectoryError::SyncRunning,
                409,
                "directory_sync_running",
                "A sync of this connection is already running",
            ),
            (
                DirectoryError::RunNotHeld,
                409,
                "directory_run_not_held",
                "Only a held run can be approved",
            ),
            (
                DirectoryError::source_failed("LDAP", "invalid credentials"),
                502,
                "directory_source_failed",
                "LDAP could not be read: invalid credentials",
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
        let body = body(DirectoryError::Sink("the recipients table is locked".into()));
        assert_eq!(body["statusCode"], json!(500));
        assert!(!body.to_string().contains("recipients"));
        assert!(DirectoryError::NotBound.to_app_error().is_none());
        let passed_on = DirectoryError::Integrations(IntegrationsError::NotConnected {
            provider: "Okta".into(),
            scope_set: "directory".into(),
        })
        .into_app_error()
        .body();
        assert_eq!(passed_on["code"], json!("integration_not_connected"));
    }
}
