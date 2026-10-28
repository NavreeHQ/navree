//! DNS lookups for verification: [`DnsLookup`], with [`SystemDns`]
//! (hickory-resolver) and [`DnsFake`] for tests.

use std::collections::BTreeMap;
use std::fmt;
use std::net::IpAddr;
use std::sync::{Arc, Mutex, PoisonError};

use hickory_resolver::proto::rr::{RData, RecordType};
use navree_kernel::BoxFuture;
use tokio::sync::OnceCell;

use crate::DomainsError;

/// Looks up the DNS records verification needs. The module registers a
/// [`SystemDns`]; replace it with `override_value::<dyn DnsLookup>(..)`,
/// for example with a [`DnsFake`] in tests.
///
/// A name or record that does not exist is an empty answer, never an
/// error; errors are resolver failures (a timeout, a refused query).
pub trait DnsLookup: Send + Sync + 'static {
    /// The TXT records at `name`, each record's strings joined.
    fn txt<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Vec<String>, DomainsError>>;

    /// The CNAME target of `name` (lower-case, without the trailing dot),
    /// if it has one.
    fn cname<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Option<String>, DomainsError>>;

    /// The IPv4 and IPv6 addresses `name` resolves to, following CNAMEs.
    fn addresses<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Vec<IpAddr>, DomainsError>>;
}

/// Asks the system's DNS resolvers (hickory-resolver with the system
/// configuration; `/etc/resolv.conf` or the Windows settings), created on
/// first use.
#[derive(Default)]
pub struct SystemDns {
    inner: OnceCell<hickory_resolver::TokioResolver>,
}

impl SystemDns {
    /// A resolver using the system configuration.
    pub fn new() -> SystemDns {
        SystemDns::default()
    }

    async fn resolver(&self, name: &str) -> Result<&hickory_resolver::TokioResolver, DomainsError> {
        self.inner
            .get_or_try_init(|| async {
                hickory_resolver::TokioResolver::builder_tokio()
                    .and_then(|builder| builder.build())
                    .map_err(|error| DomainsError::dns(name, error.to_string()))
            })
            .await
    }
}

impl fmt::Debug for SystemDns {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SystemDns")
    }
}

/// The fully qualified form of `name`, so no search domain is tried.
fn fqdn(name: &str) -> String {
    format!("{}.", name.trim_end_matches('.'))
}

impl DnsLookup for SystemDns {
    fn txt<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Vec<String>, DomainsError>> {
        Box::pin(async move {
            let resolver = self.resolver(name).await?;
            match resolver.txt_lookup(fqdn(name).as_str()).await {
                Ok(lookup) => Ok(lookup
                    .answers()
                    .iter()
                    .filter_map(|record| match &record.data {
                        RData::TXT(txt) => Some(txt.to_string()),
                        _ => None,
                    })
                    .collect()),
                Err(error) if error.is_no_records_found() || error.is_nx_domain() => Ok(Vec::new()),
                Err(error) => Err(DomainsError::dns(name, error.to_string())),
            }
        })
    }

    fn cname<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Option<String>, DomainsError>> {
        Box::pin(async move {
            let resolver = self.resolver(name).await?;
            match resolver.lookup(fqdn(name).as_str(), RecordType::CNAME).await {
                Ok(lookup) => Ok(lookup.answers().iter().find_map(|record| match &record.data {
                    RData::CNAME(target) => Some(normal_name(&target.to_string())),
                    _ => None,
                })),
                Err(error) if error.is_no_records_found() || error.is_nx_domain() => Ok(None),
                Err(error) => Err(DomainsError::dns(name, error.to_string())),
            }
        })
    }

    fn addresses<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Vec<IpAddr>, DomainsError>> {
        Box::pin(async move {
            let resolver = self.resolver(name).await?;
            match resolver.lookup_ip(fqdn(name).as_str()).await {
                Ok(lookup) => Ok(lookup.iter().collect()),
                Err(error) if error.is_no_records_found() || error.is_nx_domain() => Ok(Vec::new()),
                Err(error) => Err(DomainsError::dns(name, error.to_string())),
            }
        })
    }
}

/// A name lower-cased, without its trailing dot.
pub(crate) fn normal_name(name: &str) -> String {
    name.trim().trim_end_matches('.').to_ascii_lowercase()
}

/// DNS in memory for tests: TXT, CNAME and address records you set, and
/// names whose lookups fail. Clones share the records, so a test changes
/// DNS after the app has booted.
///
/// ```
/// use navree_domains::{DnsFake, DnsLookup};
///
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() {
/// let dns = DnsFake::new()
///     .with_txt("_domain-verification.crm.acme-corp.com", "4f9c2b7d")
///     .with_cname("crm.acme-corp.com", "domains.example.com")
///     .with_address("domains.example.com", "203.0.113.10");
/// assert_eq!(dns.txt("_domain-verification.CRM.acme-corp.com.").await.unwrap(), ["4f9c2b7d"]);
/// // Addresses follow CNAMEs, as a resolver does.
/// assert_eq!(dns.addresses("crm.acme-corp.com").await.unwrap(), ["203.0.113.10".parse::<std::net::IpAddr>().unwrap()]);
/// # }
/// ```
#[derive(Clone, Debug, Default)]
pub struct DnsFake {
    state: Arc<Mutex<FakeRecords>>,
}

#[derive(Debug, Default)]
struct FakeRecords {
    txt: BTreeMap<String, Vec<String>>,
    cname: BTreeMap<String, String>,
    addresses: BTreeMap<String, Vec<IpAddr>>,
    failing: BTreeMap<String, String>,
    lookups: Vec<String>,
}

impl DnsFake {
    /// No records.
    pub fn new() -> DnsFake {
        DnsFake::default()
    }

    fn with<T>(&self, f: impl FnOnce(&mut FakeRecords) -> T) -> T {
        f(&mut self.state.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// With a TXT record `value` at `name`.
    #[must_use]
    pub fn with_txt(self, name: &str, value: &str) -> DnsFake {
        self.set_txt(name, value);
        self
    }

    /// Adds a TXT record `value` at `name` (after boot).
    pub fn set_txt(&self, name: &str, value: &str) {
        self.with(|records| {
            records
                .txt
                .entry(normal_name(name))
                .or_default()
                .push(value.to_owned());
        });
    }

    /// Points `name` at `target` with a CNAME record.
    #[must_use]
    pub fn with_cname(self, name: &str, target: &str) -> DnsFake {
        self.set_cname(name, target);
        self
    }

    /// Points `name` at `target` with a CNAME record (after boot).
    pub fn set_cname(&self, name: &str, target: &str) {
        self.with(|records| {
            records.cname.insert(normal_name(name), normal_name(target));
        });
    }

    /// Adds an A or AAAA record (`address` is an IP address).
    ///
    /// # Panics
    ///
    /// If `address` is not an IP address.
    #[must_use]
    pub fn with_address(self, name: &str, address: &str) -> DnsFake {
        self.set_address(name, address);
        self
    }

    /// Adds an A or AAAA record (after boot).
    ///
    /// # Panics
    ///
    /// If `address` is not an IP address.
    pub fn set_address(&self, name: &str, address: &str) {
        let ip: IpAddr = address.parse().expect("an IP address");
        self.with(|records| {
            records
                .addresses
                .entry(normal_name(name))
                .or_default()
                .push(ip);
        });
    }

    /// Removes every record at `name`.
    pub fn remove(&self, name: &str) {
        let name = normal_name(name);
        self.with(|records| {
            records.txt.remove(&name);
            records.cname.remove(&name);
            records.addresses.remove(&name);
        });
    }

    /// Lookups of `name` fail with `reason` (a resolver error).
    pub fn fail(&self, name: &str, reason: &str) {
        self.with(|records| {
            records.failing.insert(normal_name(name), reason.to_owned());
        });
    }

    /// Every name looked up, in order (`txt:`, `cname:` or `addresses:`
    /// before it).
    pub fn lookups(&self) -> Vec<String> {
        self.with(|records| records.lookups.clone())
    }

    fn answer<T>(
        &self,
        kind: &str,
        name: &str,
        f: impl FnOnce(&FakeRecords, &str) -> T,
    ) -> Result<T, DomainsError> {
        let name = normal_name(name);
        self.with(|records| {
            records.lookups.push(format!("{kind}:{name}"));
            if let Some(reason) = records.failing.get(&name) {
                return Err(DomainsError::dns(name.clone(), reason.clone()));
            }
            Ok(f(records, &name))
        })
    }
}

impl DnsLookup for DnsFake {
    fn txt<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Vec<String>, DomainsError>> {
        let answer = self.answer("txt", name, |records, name| {
            records.txt.get(name).cloned().unwrap_or_default()
        });
        Box::pin(async move { answer })
    }

    fn cname<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Option<String>, DomainsError>> {
        let answer = self.answer("cname", name, |records, name| {
            records.cname.get(name).cloned()
        });
        Box::pin(async move { answer })
    }

    fn addresses<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Vec<IpAddr>, DomainsError>> {
        let answer = self.answer("addresses", name, |records, name| {
            // Follow CNAMEs (at most 8), as a resolver does.
            let mut current = name.to_owned();
            for _ in 0..8 {
                match records.cname.get(&current) {
                    Some(target) => current = target.clone(),
                    None => break,
                }
            }
            records.addresses.get(&current).cloned().unwrap_or_default()
        });
        Box::pin(async move { answer })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_fake_answers_from_its_records() {
        let dns = DnsFake::new()
            .with_txt("_v.shop.acme.test", "one")
            .with_txt("_v.shop.acme.test.", "two")
            .with_cname("shop.acme.test", "Domains.Example.com.")
            .with_address("domains.example.com", "203.0.113.10")
            .with_address("apex.test", "2001:db8::10");
        assert_eq!(dns.txt("_V.shop.acme.test").await.unwrap(), ["one", "two"]);
        assert!(dns.txt("missing.test").await.unwrap().is_empty());
        assert_eq!(
            dns.cname("shop.acme.test").await.unwrap().as_deref(),
            Some("domains.example.com")
        );
        assert_eq!(dns.cname("apex.test").await.unwrap(), None);
        assert_eq!(
            dns.addresses("shop.acme.test").await.unwrap(),
            ["203.0.113.10".parse::<IpAddr>().unwrap()]
        );
        assert_eq!(
            dns.addresses("apex.test").await.unwrap(),
            ["2001:db8::10".parse::<IpAddr>().unwrap()]
        );
        dns.remove("shop.acme.test");
        assert!(dns.addresses("shop.acme.test").await.unwrap().is_empty());
        dns.fail("broken.test", "SERVFAIL");
        let error = dns.txt("broken.test").await.unwrap_err();
        assert!(matches!(error, DomainsError::Dns { .. }), "{error}");
        assert_eq!(dns.lookups()[0], "txt:_v.shop.acme.test");
    }

    #[test]
    fn names_are_compared_without_case_or_the_root_dot() {
        assert_eq!(normal_name("Domains.Example.COM."), "domains.example.com");
        assert_eq!(fqdn("example.com"), "example.com.");
        assert_eq!(fqdn("example.com."), "example.com.");
    }
}
