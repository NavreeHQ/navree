//! [`DomainsError`] and its HTTP mapping.

use http::StatusCode;
use navree_crypto::CryptoError;
use navree_db::sea_orm::sqlx::error::ErrorKind;
use navree_db::sea_orm::{DbErr, RuntimeErr, SqlErr};
use navree_http::AppError;
use navree_kernel::KernelError;
use navree_orm::OrmError;

/// A domains error. Returned from handlers with `?`, it becomes an error
/// response (the module registers the mapping): a domain that is not
/// valid or not allowed is 400, a missing one 404, a taken one, a full
/// workspace or a domain that is not verified yet 409, a DNS, DNS
/// provider or CA failure 502, and anything internal a 500 whose details
/// are logged, never sent.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DomainsError {
    /// No domain has this id or host (or it belongs to another
    /// workspace).
    #[error("domain not found")]
    DomainNotFound,
    /// The input is not a domain name.
    #[error("{0}")]
    InvalidDomain(String),
    /// The domain is reserved (the app's own domains and their
    /// subdomains).
    #[error("`{0}` can't be added")]
    NotAllowed(String),
    /// The domain is verified for another workspace (it moves only
    /// through `Domains::transfer`), or this workspace already has it.
    /// Pending and failed claims of other workspaces never cause it.
    #[error("`{0}` is already in use")]
    Taken(String),
    /// The workspace has `domains.max_per_workspace` domains.
    #[error("this workspace already has {0} domains")]
    LimitReached(u32),
    /// The action needs a verified domain.
    #[error("`{0}` is not verified yet")]
    NotVerified(String),
    /// A DNS lookup failed (not a missing record: a resolver error).
    #[error("DNS lookup of {name} failed: {reason}")]
    Dns {
        /// The name looked up.
        name: String,
        /// What went wrong.
        reason: String,
    },
    /// The DNS provider refused a change.
    #[error("the DNS provider failed: {0}")]
    DnsProvider(String),
    /// The ACME CA did not issue the certificate.
    #[error("the certificate for {host} could not be issued: {reason}")]
    Acme {
        /// The host.
        host: String,
        /// What the CA answered.
        reason: String,
    },
    /// A certificate could not be read (a probe or a stored PEM).
    #[error("certificate error: {0}")]
    Certificate(String),
    /// The [`Domains`](crate::Domains) service is not part of a running app.
    #[error("the Domains service is not bound to an app")]
    NotBound,
    /// Something the module needs is missing or misconfigured.
    #[error("domains configuration error: {0}")]
    Config(String),
    /// Sealing or opening a private key failed (no application key).
    #[error("domains crypto error: {0}")]
    Crypto(#[from] CryptoError),
    /// A database error.
    #[error("domains database error: {0}")]
    Db(#[from] DbErr),
    /// A provider the service needs could not be resolved.
    #[error("domains provider error: {0}")]
    Kernel(#[from] KernelError),
}

impl DomainsError {
    /// A DNS lookup failure.
    pub fn dns(name: impl Into<String>, reason: impl Into<String>) -> Self {
        DomainsError::Dns {
            name: name.into(),
            reason: reason.into(),
        }
    }

    /// An ACME failure.
    pub fn acme(host: impl Into<String>, reason: impl Into<String>) -> Self {
        DomainsError::Acme {
            host: host.into(),
            reason: reason.into(),
        }
    }

    /// Whether the database refused a write because it repeats a value of
    /// a unique index (a second claim on a host, a second row of a
    /// workspace for one host).
    pub(crate) fn is_unique_violation(&self) -> bool {
        let DomainsError::Db(error) = self else {
            return false;
        };
        if matches!(error.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) {
            return true;
        }
        match error {
            DbErr::Exec(RuntimeErr::SqlxError(error))
            | DbErr::Query(RuntimeErr::SqlxError(error)) => error
                .as_database_error()
                .is_some_and(|error| matches!(error.kind(), ErrorKind::UniqueViolation)),
            _ => false,
        }
    }

    /// The error response for this error, `None` for errors that stay a 500
    /// (their details are logged, never sent).
    pub fn to_app_error(&self) -> Option<AppError> {
        let error = match self {
            DomainsError::DomainNotFound => AppError::not_found()
                .with_code("domain_not_found")
                .with_detail("Domain not found"),
            DomainsError::InvalidDomain(reason) => {
                AppError::bad_request(capitalize(reason)).with_code("invalid_domain")
            }
            DomainsError::NotAllowed(host) => {
                AppError::bad_request(format!("`{host}` can't be added"))
                    .with_code("domain_not_allowed")
            }
            DomainsError::Taken(host) => {
                AppError::conflict(format!("`{host}` is already in use")).with_code("domain_taken")
            }
            DomainsError::LimitReached(limit) => {
                AppError::conflict(format!("This workspace already has {limit} domains"))
                    .with_code("domain_limit_reached")
            }
            DomainsError::NotVerified(host) => {
                AppError::conflict(format!("`{host}` is not verified yet"))
                    .with_code("domain_not_verified")
            }
            DomainsError::Dns { name, .. } => AppError::from_status(StatusCode::BAD_GATEWAY)
                .with_code("dns_lookup_failed")
                .with_detail(format!("The DNS lookup of {name} failed")),
            DomainsError::DnsProvider(message) => AppError::from_status(StatusCode::BAD_GATEWAY)
                .with_code("dns_provider_failed")
                .with_detail(format!("The DNS provider failed: {message}")),
            DomainsError::Acme { host, reason } => AppError::from_status(StatusCode::BAD_GATEWAY)
                .with_code("certificate_not_issued")
                .with_detail(format!(
                    "The certificate for {host} could not be issued: {reason}"
                )),
            DomainsError::Db(error) => return navree_db::map_db_error(error),
            DomainsError::Certificate(_)
            | DomainsError::NotBound
            | DomainsError::Config(_)
            | DomainsError::Crypto(_)
            | DomainsError::Kernel(_) => return None,
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

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The ORM's errors are database errors (`Db`), mapped the same way.
impl From<OrmError> for DomainsError {
    fn from(error: OrmError) -> Self {
        DomainsError::Db(DbErr::from(error))
    }
}

/// The mapping registered with `map_error`.
pub(crate) fn map_domains_error(error: &DomainsError) -> Option<AppError> {
    error.to_app_error()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn body(error: DomainsError) -> serde_json::Value {
        error.into_app_error().body()
    }

    #[test]
    fn refusals_map_to_the_documented_answers() {
        let cases = [
            (
                DomainsError::DomainNotFound,
                404,
                "domain_not_found",
                "Domain not found",
            ),
            (
                DomainsError::InvalidDomain("give the domain without a port".into()),
                400,
                "invalid_domain",
                "Give the domain without a port",
            ),
            (
                DomainsError::NotAllowed("app.example.com".into()),
                400,
                "domain_not_allowed",
                "`app.example.com` can't be added",
            ),
            (
                DomainsError::Taken("crm.acme-corp.com".into()),
                409,
                "domain_taken",
                "`crm.acme-corp.com` is already in use",
            ),
            (
                DomainsError::LimitReached(10),
                409,
                "domain_limit_reached",
                "This workspace already has 10 domains",
            ),
            (
                DomainsError::NotVerified("crm.acme-corp.com".into()),
                409,
                "domain_not_verified",
                "`crm.acme-corp.com` is not verified yet",
            ),
            (
                DomainsError::dns("crm.acme-corp.com", "timed out"),
                502,
                "dns_lookup_failed",
                "The DNS lookup of crm.acme-corp.com failed",
            ),
            (
                DomainsError::DnsProvider("Authentication error".into()),
                502,
                "dns_provider_failed",
                "The DNS provider failed: Authentication error",
            ),
            (
                DomainsError::acme("crm.acme-corp.com", "rate limited"),
                502,
                "certificate_not_issued",
                "The certificate for crm.acme-corp.com could not be issued: rate limited",
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
        let body = body(DomainsError::Config(
            "no application key for the private keys".into(),
        ));
        assert_eq!(body["statusCode"], json!(500));
        assert!(!body.to_string().contains("private"));
        assert!(DomainsError::NotBound.to_app_error().is_none());
        assert!(
            DomainsError::Certificate("bad PEM".into())
                .to_app_error()
                .is_none()
        );
    }
}
