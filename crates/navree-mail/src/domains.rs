//! Sending-domain readiness: SPF, DKIM and DMARC records in DNS
//! ([`SendingDomains`]).

use std::collections::BTreeMap;
use std::fmt;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hickory_resolver::config::{ConnectionConfig, NameServerConfig, ResolverConfig};
use hickory_resolver::net::runtime::TokioRuntimeProvider;
use navree_kernel::BoxFuture;
use navree_openapi::ToSchema;
use serde::Serialize;
use tokio::sync::OnceCell;

use crate::MailError;

/// Looks up DNS TXT records. [`SystemResolver`] asks the system's
/// resolvers; [`StaticResolver`] answers from a table (tests). Override
/// `dyn DnsResolver` in the app to replace it.
pub trait DnsResolver: Send + Sync + 'static {
    /// The TXT records at `name`, each record's strings joined; empty when
    /// the name or the record does not exist.
    fn txt<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Vec<String>, MailError>>;
}

/// Asks the system's DNS resolvers (hickory-resolver with the system
/// configuration), created on first use, or the one name server given to
/// [`with_nameserver`](SystemResolver::with_nameserver).
#[derive(Default)]
pub struct SystemResolver {
    inner: OnceCell<hickory_resolver::TokioResolver>,
    server: Option<SocketAddr>,
}

impl SystemResolver {
    /// A resolver using the system configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// A resolver asking only the name server at `address` (UDP, and TCP for
    /// answers that do not fit), whatever the system's configuration says:
    /// for a private resolver on a port other than 53, or a test server.
    pub fn with_nameserver(address: SocketAddr) -> Self {
        Self {
            inner: OnceCell::new(),
            server: Some(address),
        }
    }
}

impl fmt::Debug for SystemResolver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SystemResolver")
    }
}

impl DnsResolver for SystemResolver {
    fn txt<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Vec<String>, MailError>> {
        Box::pin(async move {
            let dns_error = |reason: String| MailError::Dns {
                name: name.to_owned(),
                reason,
            };
            let resolver = self
                .inner
                .get_or_try_init(|| async {
                    let built = match self.server {
                        None => hickory_resolver::TokioResolver::builder_tokio()
                            .and_then(|builder| builder.build()),
                        Some(address) => {
                            let mut udp = ConnectionConfig::udp();
                            udp.port = address.port();
                            let mut tcp = ConnectionConfig::tcp();
                            tcp.port = address.port();
                            let server = NameServerConfig::new(address.ip(), true, vec![udp, tcp]);
                            hickory_resolver::TokioResolver::builder_with_config(
                                ResolverConfig::from_name_servers(vec![server]),
                                TokioRuntimeProvider::default(),
                            )
                            .build()
                        }
                    };
                    built.map_err(|error| dns_error(error.to_string()))
                })
                .await?;
            let fqdn = format!("{}.", name.trim_end_matches('.'));
            match resolver.txt_lookup(fqdn.as_str()).await {
                Ok(lookup) => Ok(lookup
                    .answers()
                    .iter()
                    .filter_map(|record| match &record.data {
                        hickory_resolver::proto::rr::RData::TXT(txt) => Some(txt.to_string()),
                        _ => None,
                    })
                    .collect()),
                Err(error) if error.is_no_records_found() || error.is_nx_domain() => Ok(Vec::new()),
                Err(error) => Err(dns_error(error.to_string())),
            }
        })
    }
}

/// Answers TXT lookups from a table: a fake for tests.
///
/// ```
/// use navree_mail::{DnsResolver, StaticResolver};
///
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() {
/// let dns = StaticResolver::new().record("acme.test", "v=spf1 include:amazonses.com -all");
/// assert_eq!(dns.txt("ACME.test.").await.unwrap(), ["v=spf1 include:amazonses.com -all"]);
/// assert!(dns.txt("other.test").await.unwrap().is_empty());
/// # }
/// ```
#[derive(Clone, Debug, Default)]
pub struct StaticResolver {
    records: Arc<Mutex<BTreeMap<String, Vec<String>>>>,
}

impl StaticResolver {
    /// An empty table.
    pub fn new() -> Self {
        Self::default()
    }

    /// This table with a TXT record `value` at `name`.
    #[must_use]
    pub fn record(self, name: &str, value: &str) -> Self {
        self.records
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(normalize(name))
            .or_default()
            .push(value.to_owned());
        self
    }
}

impl DnsResolver for StaticResolver {
    fn txt<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Vec<String>, MailError>> {
        let records = self
            .records
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&normalize(name))
            .cloned()
            .unwrap_or_default();
        Box::pin(async move { Ok(records) })
    }
}

fn normalize(name: &str) -> String {
    name.trim().trim_end_matches('.').to_ascii_lowercase()
}

/// What a domain is expected to publish.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DomainExpectations {
    /// DKIM selectors whose keys must be published
    /// (`<selector>._domainkey.<domain>`).
    pub dkim_selectors: Vec<String>,
    /// Domains the SPF record must include (`include:amazonses.com`,
    /// `include:mailgun.org`).
    pub spf_includes: Vec<String>,
}

impl DomainExpectations {
    /// Nothing expected beyond valid records.
    pub fn new() -> Self {
        Self::default()
    }

    /// Expects a DKIM key at `selector`.
    #[must_use]
    pub fn selector(mut self, selector: impl Into<String>) -> Self {
        self.dkim_selectors.push(selector.into());
        self
    }

    /// Expects `include:<domain>` in the SPF record.
    #[must_use]
    pub fn spf_include(mut self, domain: impl Into<String>) -> Self {
        self.spf_includes.push(domain.into());
        self
    }
}

/// The verdict on one record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CheckStatus {
    /// The record is there and does what it should.
    Pass,
    /// It works, but should be tightened (`p=none`, `?all`, a short key).
    Warn,
    /// Missing or broken: mail from the domain may be rejected or land in
    /// spam.
    Fail,
}

/// One record's check.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct RecordCheck {
    /// The verdict.
    pub status: CheckStatus,
    /// The DNS name looked up.
    pub name: String,
    /// The record found, if any.
    pub record: Option<String>,
    /// What is wrong, worst first; empty when it passes.
    pub problems: Vec<String>,
}

impl RecordCheck {
    fn new(name: String, record: Option<String>) -> Self {
        Self {
            status: CheckStatus::Pass,
            name,
            record,
            problems: Vec::new(),
        }
    }

    fn fail(&mut self, problem: impl Into<String>) {
        self.status = CheckStatus::Fail;
        self.problems.push(problem.into());
    }

    fn warn(&mut self, problem: impl Into<String>) {
        if self.status == CheckStatus::Pass {
            self.status = CheckStatus::Warn;
        }
        self.problems.push(problem.into());
    }
}

/// A DKIM key check.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct DkimCheck {
    /// The selector.
    pub selector: String,
    /// The record's check.
    #[serde(flatten)]
    pub check: RecordCheck,
}

/// Whether a domain is ready to send: its SPF, DKIM and DMARC records.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct DomainReport {
    /// The domain checked.
    pub domain: String,
    /// The SPF record (`v=spf1`) at the domain.
    pub spf: RecordCheck,
    /// Each expected DKIM key.
    pub dkim: Vec<DkimCheck>,
    /// The DMARC record at `_dmarc.<domain>`.
    pub dmarc: RecordCheck,
}

impl DomainReport {
    /// No record fails.
    pub fn ready(&self) -> bool {
        self.spf.status != CheckStatus::Fail
            && self.dmarc.status != CheckStatus::Fail
            && self
                .dkim
                .iter()
                .all(|d| d.check.status != CheckStatus::Fail)
    }
}

/// Checks that sending domains publish SPF, DKIM and DMARC records.
///
/// Provided by [`MailModule`](crate::MailModule) with the app's
/// `dyn DnsResolver` ([`SystemResolver`] unless overridden); the command
/// `mail:domains <domain> [--selector s] [--spf-include d]` prints a report.
///
/// ```
/// use std::sync::Arc;
/// use navree_mail::{CheckStatus, DomainExpectations, SendingDomains, StaticResolver};
///
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() -> Result<(), navree_mail::MailError> {
/// let dns = StaticResolver::new()
///     .record("acme.test", "v=spf1 include:amazonses.com -all")
///     .record("_dmarc.acme.test", "v=DMARC1; p=quarantine; rua=mailto:dmarc@acme.test");
/// let domains = SendingDomains::new(Arc::new(dns));
///
/// let report = domains
///     .check("acme.test", &DomainExpectations::new().spf_include("amazonses.com"))
///     .await?;
/// assert_eq!(report.spf.status, CheckStatus::Pass);
/// assert_eq!(report.dmarc.status, CheckStatus::Pass);
/// assert!(report.ready());
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct SendingDomains {
    resolver: Arc<dyn DnsResolver>,
}

impl fmt::Debug for SendingDomains {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SendingDomains")
    }
}

/// SPF mechanisms and modifiers that cost a DNS lookup (RFC 7208 §4.6.4
/// allows ten).
const SPF_LOOKUPS: &[&str] = &[
    "include:",
    "a",
    "a:",
    "a/",
    "mx",
    "mx:",
    "mx/",
    "ptr",
    "exists:",
    "redirect=",
];

impl SendingDomains {
    /// Checks with `resolver`.
    pub fn new(resolver: Arc<dyn DnsResolver>) -> Self {
        Self { resolver }
    }

    /// Checks `domain`'s SPF and DMARC records and the DKIM keys
    /// `expect` names.
    pub async fn check(
        &self,
        domain: &str,
        expect: &DomainExpectations,
    ) -> Result<DomainReport, MailError> {
        let domain = normalize(domain);
        if domain.is_empty() || !domain.contains('.') {
            return Err(MailError::Config(format!(
                "`{domain}` is not a domain name"
            )));
        }
        let spf = self.spf(&domain, expect).await?;
        let mut dkim = Vec::new();
        for selector in &expect.dkim_selectors {
            dkim.push(DkimCheck {
                selector: selector.clone(),
                check: self.dkim(&domain, selector).await?,
            });
        }
        let dmarc = self.dmarc(&domain).await?;
        Ok(DomainReport {
            domain,
            spf,
            dkim,
            dmarc,
        })
    }

    async fn spf(
        &self,
        domain: &str,
        expect: &DomainExpectations,
    ) -> Result<RecordCheck, MailError> {
        let records: Vec<String> = self
            .resolver
            .txt(domain)
            .await?
            .into_iter()
            .filter(|r| {
                let lower = r.trim().to_ascii_lowercase();
                lower == "v=spf1" || lower.starts_with("v=spf1 ")
            })
            .collect();
        let mut check = RecordCheck::new(domain.to_owned(), records.first().cloned());
        let Some(record) = records.first() else {
            check.fail("no SPF record (`v=spf1 ...`)");
            return Ok(check);
        };
        if records.len() > 1 {
            check.fail("more than one SPF record: receivers treat that as an error");
        }
        let terms: Vec<String> = record
            .split_whitespace()
            .skip(1)
            .map(str::to_ascii_lowercase)
            .collect();
        for include in &expect.spf_includes {
            let wanted = format!("include:{}", include.to_ascii_lowercase());
            if !terms
                .iter()
                .any(|t| t.trim_start_matches(['+', '~', '?']) == wanted)
            {
                check.fail(format!("the record does not include `{wanted}`"));
            }
        }
        match terms
            .iter()
            .find(|t| t.ends_with("all") && t.len() <= 4)
            .map(String::as_str)
        {
            Some("+all" | "all") => check.fail("`+all` lets anyone send as this domain"),
            Some("?all") => check.warn("`?all` is neutral: use `~all` or `-all`"),
            Some(_) => {}
            None if terms.iter().any(|t| t.starts_with("redirect=")) => {}
            None => check.warn("no `all` mechanism: end the record with `~all` or `-all`"),
        }
        let lookups = terms
            .iter()
            .filter(|t| {
                let t = t.trim_start_matches(['+', '-', '~', '?']);
                SPF_LOOKUPS.iter().any(|m| {
                    if m.ends_with([':', '/', '=']) {
                        t.starts_with(m)
                    } else {
                        t == *m
                    }
                })
            })
            .count();
        if lookups > 10 {
            check.fail(format!("{lookups} DNS lookups: SPF allows ten"));
        }
        Ok(check)
    }

    async fn dkim(&self, domain: &str, selector: &str) -> Result<RecordCheck, MailError> {
        let name = format!("{selector}._domainkey.{domain}");
        let records = self.resolver.txt(&name).await?;
        let record = records
            .iter()
            .find(|r| r.contains("p="))
            .or(records.first())
            .cloned();
        let mut check = RecordCheck::new(name.clone(), record.clone());
        let Some(record) = record else {
            check.fail(format!("no DKIM key at `{name}`"));
            return Ok(check);
        };
        let tags = tags(&record);
        if let Some(version) = tags.get("v")
            && version != "DKIM1"
        {
            check.fail(format!("`v={version}` should be `v=DKIM1`"));
        }
        let key_type = tags.get("k").map_or("rsa", String::as_str);
        if key_type != "rsa" && key_type != "ed25519" {
            check.fail(format!("unknown key type `k={key_type}`"));
        }
        match tags.get("p").map(String::as_str) {
            None => check.fail("the record has no public key (`p=`)"),
            Some("") => check.fail("the key is revoked (empty `p=`)"),
            Some(p) => match STANDARD.decode(p.replace(' ', "")) {
                Err(_) => check.fail("the public key (`p=`) is not valid base64"),
                Ok(der) if key_type == "rsa" && der.len() < 270 => {
                    check.warn("the RSA key is shorter than 2048 bits");
                }
                Ok(_) => {}
            },
        }
        Ok(check)
    }

    async fn dmarc(&self, domain: &str) -> Result<RecordCheck, MailError> {
        let name = format!("_dmarc.{domain}");
        let records: Vec<String> = self
            .resolver
            .txt(&name)
            .await?
            .into_iter()
            .filter(|r| r.trim().to_ascii_lowercase().starts_with("v=dmarc1"))
            .collect();
        let mut check = RecordCheck::new(name.clone(), records.first().cloned());
        let Some(record) = records.first() else {
            check.fail(format!("no DMARC record at `{name}`"));
            return Ok(check);
        };
        if records.len() > 1 {
            check.fail("more than one DMARC record: receivers ignore them all");
        }
        let tags = tags(record);
        match tags.get("p").map(|p| p.to_ascii_lowercase()).as_deref() {
            None => check.fail("the record has no policy (`p=`)"),
            Some("none") => check.warn("`p=none` only monitors: move to `quarantine` or `reject`"),
            Some("quarantine" | "reject") => {}
            Some(other) => check.fail(format!("unknown policy `p={other}`")),
        }
        if let Some(pct) = tags.get("pct")
            && pct != "100"
        {
            check.warn(format!(
                "`pct={pct}` applies the policy to part of the mail only"
            ));
        }
        if !tags.contains_key("rua") {
            check.warn("no `rua=` address: you get no aggregate reports");
        }
        Ok(check)
    }
}

/// `k=v; k2=v2` tag lists (DKIM, DMARC), keys lower-cased.
fn tags(record: &str) -> BTreeMap<String, String> {
    record
        .split(';')
        .filter_map(|part| part.split_once('='))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect()
}
