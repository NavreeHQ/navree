//! [`SsoError`] and its HTTP mapping.

use std::collections::BTreeMap;

use navree_auth::AuthError;
use navree_cache::CacheError;
use navree_crypto::CryptoError;
use navree_db::DbError;
use navree_http::AppError;
use navree_http_client::HttpClientError;
use navree_kernel::KernelError;
use navree_orm::OrmError;
use navree_workspace::WorkspaceError;

use crate::saml::SamlError;

/// A single sign-on error. Returned from handlers with `?`, it becomes an
/// error response (the module registers the mapping). Each refusal a user
/// can see carries its own `code`:
///
/// | Error | Status | `code` |
/// |---|---|---|
/// | `UnknownConnection` | 404 | `sso_connection_not_found` |
/// | `ConnectionDisabled` | 403 | `sso_connection_disabled` |
/// | `InvalidState` | 400 | `invalid_state` |
/// | `Saml` | 400 | `invalid_saml_response` |
/// | `Saml` (an error status from the identity provider) | 403 | `sso_refused` |
/// | `Replay` | 400 | `assertion_replayed` |
/// | `Provider` | 400 | `sso_provider_refused` |
/// | `InvalidIdToken` | 400 | `invalid_id_token` |
/// | `ProviderUnavailable` | 502 | `sso_provider_unavailable` |
/// | `MissingEmail` | 400 | `missing_email` |
/// | `UnverifiedDomain` | 403 | `unverified_domain` |
/// | `EmailTaken` | 409 | `email_taken` |
/// | `ProvisioningDisabled` | 403 | `sso_provisioning_disabled` |
/// | `NotAssigned` | 403 | `sso_not_assigned` |
/// | `InvalidCode` | 400 | `invalid_sso_code` |
/// | `InvalidReturnTo` | 400 | `invalid_return_to` |
/// | `DomainNotFound` | 404 | `sso_domain_not_found` |
/// | `DomainNotVerified` | 400 | `domain_not_verified` |
/// | `DomainTaken` | 409 | `domain_taken` |
/// | `Invalid` | 400 | `validation_failed` (with `errors`) |
///
/// Anything internal is a 500 whose details are logged, never sent.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SsoError {
    /// No connection has this id or slug.
    #[error("unknown single sign-on connection `{0}`")]
    UnknownConnection(String),
    /// The connection is turned off.
    #[error("the single sign-on connection is disabled")]
    ConnectionDisabled,
    /// The sign-in state is unknown, expired, used or for another
    /// connection.
    #[error("invalid or expired sign-in state")]
    InvalidState,
    /// The SAML response failed a check.
    #[error(transparent)]
    Saml(#[from] SamlError),
    /// The assertion was already used.
    #[error("the assertion was already used")]
    Replay,
    /// The OpenID Connect provider refused the code or the sign-in.
    #[error("the identity provider refused the sign-in: {0}")]
    Provider(String),
    /// The ID token failed verification.
    #[error("invalid ID token: {0}")]
    InvalidIdToken(String),
    /// The identity provider (metadata, discovery, keys, token endpoint) or
    /// the DNS resolver could not be reached.
    #[error("the identity provider is unavailable: {0}")]
    ProviderUnavailable(String),
    /// The identity provider sent no email address.
    #[error("the identity provider sent no email address")]
    MissingEmail,
    /// The email's domain is not a verified domain of the connection.
    #[error("`{0}` is not a verified domain of this connection")]
    UnverifiedDomain(String),
    /// A user with this email exists and cannot be linked to the identity
    /// (its domain is not verified for the connection).
    #[error("an account with this email already exists")]
    EmailTaken,
    /// No user is linked and just-in-time provisioning is off.
    #[error("just-in-time provisioning is off for this connection")]
    ProvisioningDisabled,
    /// The user's groups map to no role and the connection has no default
    /// role.
    #[error("the user has no role in this connection's organization or workspace")]
    NotAssigned,
    /// The one-time code of a finished sign-in is unknown, expired or used.
    #[error("invalid or expired sign-in code")]
    InvalidCode,
    /// `return_to` is not a relative path.
    #[error("return_to must be a relative path")]
    InvalidReturnTo,
    /// No domain has this id.
    #[error("unknown domain")]
    DomainNotFound,
    /// The domain's TXT record is missing.
    #[error("the TXT record of `{0}` was not found")]
    DomainNotVerified(String),
    /// Another connection verified this domain first.
    #[error("`{0}` is verified for another connection")]
    DomainTaken(String),
    /// Invalid input: field errors.
    #[error("invalid single sign-on input")]
    Invalid(BTreeMap<String, Vec<String>>),
    /// A misconfiguration found while signing in (no base URL, say).
    #[error("single sign-on configuration error: {0}")]
    Config(String),
    /// An auth error (the user table, issuing tokens).
    #[error(transparent)]
    Auth(#[from] AuthError),
    /// A workspace error (memberships of provisioned users).
    #[error(transparent)]
    Workspace(#[from] WorkspaceError),
    /// An outgoing request failed.
    #[error("identity provider request failed: {0}")]
    Http(#[from] HttpClientError),
    /// Encrypting or decrypting a client secret failed.
    #[error("single sign-on crypto error: {0}")]
    Crypto(#[from] CryptoError),
    /// The state, replay or code cache failed.
    #[error("single sign-on cache error: {0}")]
    Cache(#[from] CacheError),
    /// A database error (navree-db's statements, transactions and units of
    /// work).
    #[error("single sign-on database error: {0}")]
    Db(#[from] DbError),
    /// An ORM error (the module's models and their queries).
    #[error("single sign-on database error: {0}")]
    Orm(#[from] OrmError),
    /// A provider the service needs could not be resolved.
    #[error("single sign-on provider error: {0}")]
    Kernel(#[from] KernelError),
}

impl SsoError {
    /// A one-field validation error.
    pub fn invalid(field: &str, message: impl Into<String>) -> SsoError {
        SsoError::Invalid(BTreeMap::from([(field.to_owned(), vec![message.into()])]))
    }

    /// The error response for this error, `None` for errors that stay a 500
    /// (their details are logged, never sent).
    pub fn to_app_error(&self) -> Option<AppError> {
        let error = match self {
            SsoError::UnknownConnection(key) => AppError::not_found()
                .with_code("sso_connection_not_found")
                .with_detail(format!("Unknown single sign-on connection `{key}`")),
            SsoError::ConnectionDisabled => AppError::forbidden()
                .with_code("sso_connection_disabled")
                .with_detail("Single sign-on is turned off for this connection"),
            SsoError::InvalidState => {
                AppError::bad_request("Invalid or expired sign-in state").with_code("invalid_state")
            }
            SsoError::Saml(SamlError::Status { .. }) => AppError::forbidden()
                .with_code("sso_refused")
                .with_detail("The identity provider refused the sign-in"),
            SsoError::Saml(_) => {
                AppError::bad_request("The identity provider's response is invalid")
                    .with_code("invalid_saml_response")
            }
            SsoError::Replay => AppError::bad_request("This sign-in response was already used")
                .with_code("assertion_replayed"),
            SsoError::Provider(message) => {
                AppError::bad_request(format!("The identity provider refused the sign-in: {message}"))
                    .with_code("sso_provider_refused")
            }
            SsoError::InvalidIdToken(_) => {
                AppError::bad_request("The identity provider sent an invalid ID token")
                    .with_code("invalid_id_token")
            }
            SsoError::ProviderUnavailable(_) | SsoError::Http(_) => {
                AppError::from_status(http::StatusCode::BAD_GATEWAY)
                    .with_code("sso_provider_unavailable")
                    .with_detail("The identity provider is unavailable. Please try again.")
            }
            SsoError::MissingEmail => {
                AppError::bad_request("The identity provider did not share an email address")
                    .with_code("missing_email")
            }
            SsoError::UnverifiedDomain(domain) => AppError::forbidden()
                .with_code("unverified_domain")
                .with_detail(format!(
                    "{domain} is not a verified domain of this single sign-on connection"
                )),
            SsoError::EmailTaken => AppError::conflict(
                "An account with this email already exists. Sign in with your password, or ask an administrator to verify your domain.",
            )
            .with_code("email_taken"),
            SsoError::ProvisioningDisabled => AppError::forbidden()
                .with_code("sso_provisioning_disabled")
                .with_detail("Your account has not been set up yet. Ask an administrator for access."),
            SsoError::NotAssigned => AppError::forbidden()
                .with_code("sso_not_assigned")
                .with_detail("Your identity provider gives you no role here. Ask an administrator for access."),
            SsoError::InvalidCode => {
                AppError::bad_request("Invalid or expired sign-in code").with_code("invalid_sso_code")
            }
            SsoError::InvalidReturnTo => AppError::bad_request("return_to must be a relative path")
                .with_code("invalid_return_to"),
            SsoError::DomainNotFound => AppError::not_found()
                .with_code("sso_domain_not_found")
                .with_detail("Domain not found"),
            SsoError::DomainNotVerified(domain) => AppError::bad_request(format!(
                "The TXT record of {domain} was not found. DNS changes can take a while to appear; try again later."
            ))
            .with_code("domain_not_verified"),
            SsoError::DomainTaken(domain) => {
                AppError::conflict(format!("{domain} is verified for another connection"))
                    .with_code("domain_taken")
            }
            SsoError::Invalid(errors) => AppError::validation(errors.clone()),
            SsoError::Auth(error) => return error.to_app_error(),
            SsoError::Workspace(error) => error.to_app_error(),
            SsoError::Db(error) => return navree_db::map_db_error(error),
            SsoError::Orm(error) => return error.db_error().and_then(navree_db::map_db_error),
            SsoError::Config(_)
            | SsoError::Crypto(_)
            | SsoError::Cache(_)
            | SsoError::Kernel(_) => return None,
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

/// The mapping registered with `map_error`.
pub(crate) fn map_sso_error(error: &SsoError) -> Option<AppError> {
    error.to_app_error()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_refusal_has_a_valid_code() {
        let errors = [
            SsoError::UnknownConnection("acme".to_owned()),
            SsoError::ConnectionDisabled,
            SsoError::InvalidState,
            SsoError::Saml(SamlError::Expired),
            SsoError::Saml(SamlError::Status {
                code: "x".to_owned(),
                message: None,
            }),
            SsoError::Replay,
            SsoError::Provider("access_denied".to_owned()),
            SsoError::InvalidIdToken("bad".to_owned()),
            SsoError::ProviderUnavailable("down".to_owned()),
            SsoError::MissingEmail,
            SsoError::UnverifiedDomain("acme.test".to_owned()),
            SsoError::EmailTaken,
            SsoError::ProvisioningDisabled,
            SsoError::NotAssigned,
            SsoError::InvalidCode,
            SsoError::InvalidReturnTo,
            SsoError::DomainNotFound,
            SsoError::DomainNotVerified("acme.test".to_owned()),
            SsoError::DomainTaken("acme.test".to_owned()),
            SsoError::invalid("slug", "slug has already been taken"),
        ];
        for error in errors {
            let app = error.to_app_error().unwrap();
            assert!(navree_http::codes::is_valid(&app.code), "{}", app.code);
        }
        assert!(SsoError::Config("x".to_owned()).to_app_error().is_none());
    }

    #[test]
    fn saml_status_refusals_are_403_and_other_saml_errors_400() {
        let refused = SsoError::Saml(SamlError::Status {
            code: "urn:x".to_owned(),
            message: Some("no".to_owned()),
        })
        .into_app_error();
        assert_eq!(refused.status.as_u16(), 403);
        assert_eq!(refused.code, "sso_refused");
        let invalid = SsoError::Saml(SamlError::WrongAudience).into_app_error();
        assert_eq!(invalid.status.as_u16(), 400);
        // The reason stays in the logs.
        assert!(!invalid.body().to_string().contains("audience"));
    }
}
