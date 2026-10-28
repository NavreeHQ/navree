//! Rate limits of the protocol endpoints that take client credentials
//! (`oauth.rate_limits`): per client IP before anything is checked, per
//! client once it has authenticated. Navree's GCRA limiter
//! ([`navree_http::KeyedRateLimiter`]), one window a minute, kept per instance.

use std::net::IpAddr;
use std::time::Duration;

use navree_contracts::{AuditEntry, AuditKind};
use navree_http::KeyedRateLimiter;
use serde_json::json;
use uuid::Uuid;

use crate::config::{EndpointLimit, RateLimitOptions};
use crate::core::Core;
use crate::error::OAuthError;

/// A throttled protocol endpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Endpoint {
    Token,
    Introspect,
    Revoke,
    DeviceAuthorization,
}

impl Endpoint {
    fn name(self) -> &'static str {
        match self {
            Endpoint::Token => "token",
            Endpoint::Introspect => "introspect",
            Endpoint::Revoke => "revoke",
            Endpoint::DeviceAuthorization => "device_authorization",
        }
    }
}

/// The limiters of one endpoint (`None`: that limit is off).
struct Limits {
    per_client: Option<KeyedRateLimiter<Uuid>>,
    per_ip: Option<KeyedRateLimiter<IpAddr>>,
}

impl Limits {
    fn new(limit: EndpointLimit) -> Self {
        let minute = Duration::from_secs(60);
        Self {
            per_client: (limit.per_client > 0)
                .then(|| KeyedRateLimiter::new(limit.per_client, minute)),
            per_ip: (limit.per_ip > 0).then(|| KeyedRateLimiter::new(limit.per_ip, minute)),
        }
    }
}

/// The limiters of every throttled endpoint.
pub(crate) struct Throttles {
    token: Limits,
    introspect: Limits,
    revoke: Limits,
    device_authorization: Limits,
}

impl Throttles {
    pub(crate) fn new(options: &RateLimitOptions) -> Self {
        Self {
            token: Limits::new(options.token),
            introspect: Limits::new(options.introspect),
            revoke: Limits::new(options.revoke),
            device_authorization: Limits::new(options.device_authorization),
        }
    }

    fn limits(&self, endpoint: Endpoint) -> &Limits {
        match endpoint {
            Endpoint::Token => &self.token,
            Endpoint::Introspect => &self.introspect,
            Endpoint::Revoke => &self.revoke,
            Endpoint::DeviceAuthorization => &self.device_authorization,
        }
    }

    /// Counts a request to `endpoint` from `ip`, before the client is
    /// checked.
    pub(crate) async fn per_ip(
        &self,
        core: &Core,
        endpoint: Endpoint,
        ip: IpAddr,
    ) -> Result<(), OAuthError> {
        let Some(limiter) = &self.limits(endpoint).per_ip else {
            return Ok(());
        };
        match limiter.check(&ip) {
            Ok(()) => Ok(()),
            Err(limited) => {
                refuse(core, endpoint, "per_ip", None, ip, limited.report).await;
                Err(OAuthError::TooManyRequests {
                    retry_after: limited.retry_after,
                })
            }
        }
    }

    /// Counts a request to `endpoint` of `client`, once it has
    /// authenticated.
    pub(crate) async fn per_client(
        &self,
        core: &Core,
        endpoint: Endpoint,
        client: Uuid,
        ip: IpAddr,
    ) -> Result<(), OAuthError> {
        let Some(limiter) = &self.limits(endpoint).per_client else {
            return Ok(());
        };
        match limiter.check(&client) {
            Ok(()) => Ok(()),
            Err(limited) => {
                refuse(
                    core,
                    endpoint,
                    "per_client",
                    Some(client),
                    ip,
                    limited.report,
                )
                .await;
                Err(OAuthError::TooManyRequests {
                    retry_after: limited.retry_after,
                })
            }
        }
    }
}

/// Logs a refusal and, the first time in a window (`report`), audits it as
/// `oauth.rate_limited`.
async fn refuse(
    core: &Core,
    endpoint: Endpoint,
    limit: &'static str,
    client: Option<Uuid>,
    ip: IpAddr,
    report: bool,
) {
    tracing::warn!(
        target: "navree::security",
        endpoint = endpoint.name(),
        limit,
        client_id = client.map(tracing::field::display),
        %ip,
        "OAuth endpoint rate limit reached"
    );
    if !report {
        return;
    }
    let mut metadata = json!({ "endpoint": endpoint.name(), "limit": limit });
    if let Some(client) = client {
        metadata["client_id"] = json!(client.to_string());
    }
    core.audit(
        AuditEntry::new(AuditKind::Security, "oauth.rate_limited")
            .ip(ip.to_string())
            .metadata(metadata),
    )
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_turns_a_limit_off() {
        let limits = Limits::new(EndpointLimit::new(0, 5));
        assert!(limits.per_client.is_none());
        assert!(limits.per_ip.is_some());
    }
}
