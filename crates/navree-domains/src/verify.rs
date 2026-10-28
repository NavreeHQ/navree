//! One DNS check of a domain: its TXT record, and whether it points at the
//! app.

use std::net::IpAddr;

use crate::DomainsError;
use crate::dns::{DnsLookup, normal_name};

/// What one check found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Check {
    /// The TXT record holds the token.
    pub(crate) txt_found: bool,
    /// The domain points at the app (`None`: nothing to compare with).
    pub(crate) points_to_app: Option<bool>,
    /// What is missing, for the domain's `last_error`.
    pub(crate) missing: Vec<String>,
}

impl Check {
    /// Whether the domain passes: its TXT record, and its routing unless
    /// that is not required.
    pub(crate) fn passes(&self, require_routing: bool) -> bool {
        self.txt_found && !(require_routing && self.points_to_app == Some(false))
    }

    /// The missing records as one message.
    pub(crate) fn error(&self) -> Option<String> {
        (!self.missing.is_empty()).then(|| self.missing.join("; "))
    }
}

/// Where a domain must point.
#[derive(Clone, Debug, Default)]
pub(crate) struct Routing<'a> {
    pub(crate) target: Option<&'a str>,
    pub(crate) addresses: &'a [IpAddr],
}

impl Routing<'_> {
    fn is_set(&self) -> bool {
        self.target.is_some() || !self.addresses.is_empty()
    }

    fn describe(&self) -> String {
        let mut parts = Vec::new();
        if let Some(target) = self.target {
            parts.push(target.to_owned());
        }
        parts.extend(self.addresses.iter().map(IpAddr::to_string));
        parts.join(", ")
    }
}

/// The name of the TXT record of `host`.
pub(crate) fn txt_name(prefix: &str, host: &str) -> String {
    format!("{prefix}.{host}")
}

/// Checks `host`'s TXT record (`{prefix}.{host}` holding `token`) and,
/// when `routing` is set, that it points there: a CNAME to the target, or
/// an address the target or the configured addresses have.
pub(crate) async fn check(
    dns: &dyn DnsLookup,
    host: &str,
    prefix: &str,
    token: &str,
    routing: &Routing<'_>,
    check_txt: bool,
) -> Result<Check, DomainsError> {
    let mut missing = Vec::new();
    let txt_found = if check_txt {
        let name = txt_name(prefix, host);
        let found = dns
            .txt(&name)
            .await?
            .iter()
            .any(|value| value.trim().trim_matches('"') == token);
        if !found {
            missing.push(format!("TXT record {name} with the value {token} not found"));
        }
        found
    } else {
        true
    };
    let points_to_app = if routing.is_set() {
        let points = points_at(dns, host, routing).await?;
        if !points {
            missing.push(format!("{host} does not point at {}", routing.describe()));
        }
        Some(points)
    } else {
        None
    };
    Ok(Check {
        txt_found,
        points_to_app,
        missing,
    })
}

async fn points_at(
    dns: &dyn DnsLookup,
    host: &str,
    routing: &Routing<'_>,
) -> Result<bool, DomainsError> {
    if let Some(target) = routing.target
        && dns
            .cname(host)
            .await?
            .is_some_and(|cname| cname == normal_name(target))
    {
        return Ok(true);
    }
    let found = dns.addresses(host).await?;
    if found.is_empty() {
        return Ok(false);
    }
    let mut allowed: Vec<IpAddr> = routing.addresses.to_vec();
    if let Some(target) = routing.target {
        allowed.extend(dns.addresses(target).await?);
    }
    Ok(found.iter().any(|ip| allowed.contains(ip)))
}

#[cfg(test)]
mod tests {
    use crate::DnsFake;

    use super::*;

    const PREFIX: &str = "_domain-verification";

    #[tokio::test]
    async fn the_txt_record_must_hold_the_token() {
        let dns = DnsFake::new().with_txt("_domain-verification.shop.acme.test", "other");
        let routing = Routing::default();
        let found = check(&dns, "shop.acme.test", PREFIX, "tok3n", &routing, true)
            .await
            .unwrap();
        assert!(!found.txt_found);
        assert_eq!(found.points_to_app, None);
        assert_eq!(
            found.error().as_deref(),
            Some("TXT record _domain-verification.shop.acme.test with the value tok3n not found")
        );
        assert!(!found.passes(true));
        dns.set_txt("_domain-verification.shop.acme.test", "\"tok3n\"");
        let found = check(&dns, "shop.acme.test", PREFIX, "tok3n", &routing, true)
            .await
            .unwrap();
        assert!(found.txt_found && found.passes(true));
        assert_eq!(found.error(), None);
    }

    #[tokio::test]
    async fn routing_accepts_a_cname_or_a_shared_address() {
        let ip: IpAddr = "203.0.113.10".parse().unwrap();
        let addresses = [ip];
        let routing = Routing {
            target: Some("domains.example.com"),
            addresses: &addresses,
        };
        let dns = DnsFake::new()
            .with_txt("_domain-verification.shop.acme.test", "t")
            .with_address("domains.example.com", "203.0.113.10");
        // Nothing yet.
        let found = check(&dns, "shop.acme.test", PREFIX, "t", &routing, true)
            .await
            .unwrap();
        assert_eq!(found.points_to_app, Some(false));
        assert!(!found.passes(true));
        assert!(found.passes(false));
        assert_eq!(
            found.error().as_deref(),
            Some("shop.acme.test does not point at domains.example.com, 203.0.113.10")
        );
        // A CNAME to the target.
        dns.set_cname("shop.acme.test", "domains.example.com.");
        let found = check(&dns, "shop.acme.test", PREFIX, "t", &routing, true)
            .await
            .unwrap();
        assert_eq!(found.points_to_app, Some(true));
        // An apex domain with an A record of the target's address.
        dns.set_address("acme.test", "203.0.113.10");
        let found = check(&dns, "acme.test", PREFIX, "t", &routing, false)
            .await
            .unwrap();
        assert_eq!(found.points_to_app, Some(true));
        // An address elsewhere.
        dns.set_address("elsewhere.test", "198.51.100.7");
        let found = check(&dns, "elsewhere.test", PREFIX, "t", &routing, false)
            .await
            .unwrap();
        assert_eq!(found.points_to_app, Some(false));
    }
}
