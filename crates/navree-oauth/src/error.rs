//! [`OAuthError`]: errors of the OAuth server, rendered in the OAuth format
//! on the protocol endpoints and in Navree's error shape elsewhere.

use std::collections::BTreeMap;
use std::time::Duration;

use axum::response::{IntoResponse, Response};
use http::header::{CACHE_CONTROL, CONTENT_TYPE, PRAGMA, RETRY_AFTER, WWW_AUTHENTICATE};
use http::{HeaderValue, StatusCode};
use navree_http::AppError;
use navree_kernel::BoxError;
use sea_orm::DbErr;
use serde_json::json;

/// An OAuth server error.
///
/// On the protocol endpoints (`/oauth/token`, `/oauth/revoke`,
/// `/oauth/introspect`, `/oauth/device_authorization` and
/// `/oauth/register`) it renders in the OAuth format (RFC 6749 §5.2),
/// `{"error": "invalid_grant", "error_description": "..."}`. Returned from
/// other handlers with `?` it becomes Navree's JSON error shape:
///
/// | Error | OAuth `error` | Status |
/// |---|---|---|
/// | `InvalidRequest` | `invalid_request` | 400 |
/// | `InvalidClient` | `invalid_client` | 401 |
/// | `InvalidGrant` | `invalid_grant` | 400 |
/// | `UnauthorizedClient` | `unauthorized_client` | 400 |
/// | `UnsupportedGrantType` | `unsupported_grant_type` | 400 |
/// | `InvalidScope` | `invalid_scope` | 400 |
/// | `AuthorizationPending` | `authorization_pending` | 400 |
/// | `SlowDown` | `slow_down` | 400 |
/// | `ExpiredToken` | `expired_token` | 400 |
/// | `AccessDenied` | `access_denied` | 400 (403 in Navree's shape) |
/// | `InvalidClientMetadata` | `invalid_client_metadata` | 400 |
/// | `InvalidRedirectUri` | `invalid_redirect_uri` | 400 |
/// | `Invalid` | `invalid_request` | 400, the message under `errors.<field>` |
/// | `NotFound` | | 404 |
/// | `Expired` | | 410 |
/// | `Forbidden` | | 403 |
/// | `Unavailable` | `temporarily_unavailable` | 503 |
/// | `TooManyRequests` | `too_many_requests` | 429, with `Retry-After` |
/// | `Db`, `Other` | `server_error` | 500 (404/409 for database errors navree-db maps) |
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum OAuthError {
    /// The request is missing a parameter or is malformed.
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    /// The client is unknown, revoked or failed to authenticate.
    #[error("invalid client: {0}")]
    InvalidClient(String),
    /// The code, refresh token or device code is invalid, expired or used.
    #[error("invalid grant: {0}")]
    InvalidGrant(String),
    /// The client may not use this grant.
    #[error("unauthorized client: {0}")]
    UnauthorizedClient(String),
    /// The grant type is not supported.
    #[error("unsupported grant type: {0}")]
    UnsupportedGrantType(String),
    /// A scope is unknown or not allowed.
    #[error("invalid scope: {0}")]
    InvalidScope(String),
    /// Device grant: the user has not decided yet.
    #[error("authorization pending")]
    AuthorizationPending,
    /// Device grant: the device polls faster than its interval.
    #[error("slow down")]
    SlowDown,
    /// Device grant: the device code expired.
    #[error("the device code has expired")]
    ExpiredToken,
    /// The user denied the request.
    #[error("access denied: {0}")]
    AccessDenied(String),
    /// Dynamic registration: invalid client metadata.
    #[error("invalid client metadata: {0}")]
    InvalidClientMetadata(String),
    /// Dynamic registration: an invalid redirect URI.
    #[error("invalid redirect URI: {0}")]
    InvalidRedirectUri(String),
    /// An input value is invalid (a 400 naming the field).
    #[error("invalid {field}: {message}")]
    Invalid {
        /// The input field (`name`, `scopes`, `redirect_uris`, ...).
        field: &'static str,
        /// What is wrong, as shown to the client.
        message: String,
    },
    /// The record does not exist (or belongs to someone else).
    #[error("{0}")]
    NotFound(&'static str),
    /// The request or code expired.
    #[error("{0}")]
    Expired(&'static str),
    /// The caller may not do this.
    #[error("{0}")]
    Forbidden(String),
    /// The server is missing configuration for this flow.
    #[error("{0}")]
    Unavailable(String),
    /// A rate limit of the endpoint (`oauth.rate_limits`) was reached.
    #[error("too many requests; retry in {} seconds", retry_after_secs(*retry_after))]
    TooManyRequests {
        /// When the client may send again.
        retry_after: Duration,
    },
    /// A database error.
    #[error("oauth database error: {0}")]
    Db(#[from] DbErr),
    /// Anything else.
    #[error(transparent)]
    Other(BoxError),
}

impl OAuthError {
    /// An [`Invalid`](OAuthError::Invalid) error for `field`.
    pub fn invalid(field: &'static str, message: impl Into<String>) -> Self {
        OAuthError::Invalid {
            field,
            message: message.into(),
        }
    }

    /// The OAuth `error` code.
    pub fn code(&self) -> &'static str {
        match self {
            OAuthError::InvalidRequest(_) | OAuthError::Invalid { .. } => "invalid_request",
            OAuthError::InvalidClient(_) => "invalid_client",
            OAuthError::InvalidGrant(_) => "invalid_grant",
            OAuthError::UnauthorizedClient(_) => "unauthorized_client",
            OAuthError::UnsupportedGrantType(_) => "unsupported_grant_type",
            OAuthError::InvalidScope(_) => "invalid_scope",
            OAuthError::AuthorizationPending => "authorization_pending",
            OAuthError::SlowDown => "slow_down",
            OAuthError::ExpiredToken => "expired_token",
            OAuthError::AccessDenied(_) => "access_denied",
            OAuthError::InvalidClientMetadata(_) => "invalid_client_metadata",
            OAuthError::InvalidRedirectUri(_) => "invalid_redirect_uri",
            OAuthError::NotFound(_) | OAuthError::Expired(_) => "invalid_request",
            OAuthError::Forbidden(_) => "access_denied",
            OAuthError::Unavailable(_) => "temporarily_unavailable",
            OAuthError::TooManyRequests { .. } => "too_many_requests",
            OAuthError::Db(_) | OAuthError::Other(_) => "server_error",
        }
    }

    /// The human-readable `error_description`.
    pub fn description(&self) -> String {
        match self {
            OAuthError::InvalidRequest(message)
            | OAuthError::InvalidClient(message)
            | OAuthError::InvalidGrant(message)
            | OAuthError::UnauthorizedClient(message)
            | OAuthError::UnsupportedGrantType(message)
            | OAuthError::InvalidScope(message)
            | OAuthError::AccessDenied(message)
            | OAuthError::InvalidClientMetadata(message)
            | OAuthError::InvalidRedirectUri(message)
            | OAuthError::Forbidden(message)
            | OAuthError::Unavailable(message) => message.clone(),
            OAuthError::Invalid { message, .. } => message.clone(),
            OAuthError::AuthorizationPending => {
                "The user has not approved the request yet".to_owned()
            }
            OAuthError::SlowDown => "Poll less often".to_owned(),
            OAuthError::ExpiredToken => "The device code has expired".to_owned(),
            OAuthError::TooManyRequests { retry_after } => format!(
                "Too many requests; retry in {} seconds",
                retry_after_secs(*retry_after)
            ),
            OAuthError::NotFound(message) | OAuthError::Expired(message) => (*message).to_owned(),
            OAuthError::Db(_) | OAuthError::Other(_) => "Internal server error".to_owned(),
        }
    }

    /// The HTTP status of the OAuth-format response.
    pub fn protocol_status(&self) -> StatusCode {
        match self {
            OAuthError::InvalidClient(_) => StatusCode::UNAUTHORIZED,
            OAuthError::NotFound(_) => StatusCode::NOT_FOUND,
            OAuthError::Forbidden(_) => StatusCode::FORBIDDEN,
            OAuthError::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
            OAuthError::TooManyRequests { .. } => StatusCode::TOO_MANY_REQUESTS,
            OAuthError::Db(_) | OAuthError::Other(_) => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::BAD_REQUEST,
        }
    }

    /// The error as an OAuth-format response (`{error,
    /// error_description}`, `Cache-Control: no-store`). An
    /// `invalid_client` answer to HTTP Basic authentication carries
    /// `WWW-Authenticate: Basic`.
    pub fn protocol_response(&self, basic_auth: bool) -> Response {
        if matches!(self, OAuthError::Db(_) | OAuthError::Other(_)) {
            tracing::error!(target: "navree::oauth", error = %self, "OAuth endpoint failed");
        }
        let body = json!({ "error": self.code(), "error_description": self.description() });
        let mut response = (self.protocol_status(), body.to_string()).into_response();
        let headers = response.headers_mut();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
        headers.insert(PRAGMA, HeaderValue::from_static("no-cache"));
        if let OAuthError::TooManyRequests { retry_after } = self {
            headers.insert(
                RETRY_AFTER,
                HeaderValue::from(retry_after_secs(*retry_after)),
            );
        }
        if basic_auth && matches!(self, OAuthError::InvalidClient(_)) {
            headers.insert(
                WWW_AUTHENTICATE,
                HeaderValue::from_static("Basic realm=\"oauth\""),
            );
        }
        response
    }

    /// The error in Navree's error shape (what `map_error` registers).
    pub fn to_app_error(&self) -> AppError {
        map_oauth_error(self).unwrap_or_else(|| AppError::internal(self.to_string()))
    }
}

/// The Navree error response of an [`OAuthError`], `None` for errors that
/// stay a 500.
pub(crate) fn map_oauth_error(error: &OAuthError) -> Option<AppError> {
    Some(match error {
        OAuthError::Invalid { field, message } => AppError::validation(BTreeMap::from([(
            (*field).to_owned(),
            vec![message.clone()],
        )])),
        OAuthError::InvalidScope(message) => AppError::validation(BTreeMap::from([(
            "scopes".to_owned(),
            vec![message.clone()],
        )])),
        OAuthError::NotFound(message) => AppError::not_found().with_detail(*message),
        OAuthError::Expired(message) => {
            AppError::from_status(StatusCode::GONE).with_detail(*message)
        }
        OAuthError::Forbidden(message) | OAuthError::AccessDenied(message) => {
            AppError::forbidden().with_detail(message.clone())
        }
        OAuthError::InvalidClient(message) => {
            AppError::unauthenticated().with_detail(message.clone())
        }
        OAuthError::Unavailable(message) => AppError::service_unavailable(message.clone()),
        OAuthError::TooManyRequests { retry_after } => AppError::too_many_requests(*retry_after),
        OAuthError::Db(db) => return navree_db::map_db_error(db),
        OAuthError::Other(_) => return None,
        other => AppError::bad_request(other.description()),
    })
}

/// Whole seconds until `retry_after`, at least one (`Retry-After`).
fn retry_after_secs(retry_after: Duration) -> u64 {
    let secs = retry_after.as_secs();
    let rounded = if retry_after.subsec_nanos() > 0 {
        secs.saturating_add(1)
    } else {
        secs
    };
    rounded.max(1)
}

/// The server's ORM operations fail as database errors
/// ([`OAuthError::Db`]), like its SeaORM ones.
impl From<navree_orm::OrmError> for OAuthError {
    fn from(error: navree_orm::OrmError) -> Self {
        OAuthError::Db(error.into())
    }
}

impl IntoResponse for OAuthError {
    /// Navree's error shape (handlers outside the protocol endpoints).
    fn into_response(self) -> Response {
        self.to_app_error().into_response()
    }
}

#[cfg(test)]
mod tests {
    use http_body_util::BodyExt as _;

    use super::*;

    async fn body(response: Response) -> serde_json::Value {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn protocol_errors_use_the_oauth_format() {
        let error = OAuthError::InvalidGrant("The authorization code has expired".into());
        let response = error.protocol_response(false);
        assert_eq!(response.status(), 400);
        assert_eq!(response.headers()[CACHE_CONTROL], "no-store");
        assert_eq!(
            body(response).await,
            json!({"error": "invalid_grant", "error_description": "The authorization code has expired"})
        );
        let response = OAuthError::InvalidClient("Unknown client".into()).protocol_response(true);
        assert_eq!(response.status(), 401);
        assert_eq!(
            response.headers()[WWW_AUTHENTICATE],
            "Basic realm=\"oauth\""
        );
        let response = OAuthError::Other("boom".into()).protocol_response(false);
        assert_eq!(response.status(), 500);
        assert_eq!(body(response).await["error"], "server_error");
        let response = OAuthError::TooManyRequests {
            retry_after: Duration::from_millis(1500),
        }
        .protocol_response(false);
        assert_eq!(response.status(), 429);
        assert_eq!(response.headers()[RETRY_AFTER], "2");
        assert_eq!(
            body(response).await,
            json!({"error": "too_many_requests", "error_description": "Too many requests; retry in 2 seconds"})
        );
    }

    #[test]
    fn api_errors_use_navrees_shape() {
        let cases = [
            (OAuthError::invalid("name", "name should not be empty"), 400),
            (OAuthError::InvalidScope("unknown scope".into()), 400),
            (
                OAuthError::NotFound("Authorization request not found."),
                404,
            ),
            (
                OAuthError::Expired("The authorization request has expired."),
                410,
            ),
            (OAuthError::Forbidden("no".into()), 403),
            (OAuthError::InvalidClient("no".into()), 401),
            (OAuthError::Unavailable("not configured".into()), 503),
            (
                OAuthError::TooManyRequests {
                    retry_after: Duration::ZERO,
                },
                429,
            ),
            (OAuthError::Db(DbErr::RecordNotFound("x".into())), 404),
            (OAuthError::Other("boom".into()), 500),
        ];
        for (error, status) in cases {
            assert_eq!(error.to_app_error().status.as_u16(), status, "{error}");
        }
        let body = OAuthError::invalid("name", "name should not be empty")
            .to_app_error()
            .body();
        assert_eq!(body["errors"]["name"][0], "name should not be empty");
        let body = OAuthError::InvalidScope("scopes contains an unknown scope".into())
            .to_app_error()
            .body();
        assert_eq!(
            body["errors"]["scopes"][0],
            "scopes contains an unknown scope"
        );
    }
}
