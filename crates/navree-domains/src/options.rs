//! [`DomainsOptions`]: the `domains` configuration section.

use std::net::{IpAddr, SocketAddr};

use navree_config::ConfigSection;
use serde::{Deserialize, Serialize};

use crate::host::normalize_host;

/// Let's Encrypt's production directory, the default ACME directory.
pub const LETS_ENCRYPT: &str = "https://acme-v02.api.letsencrypt.org/directory";

/// Let's Encrypt's staging directory: untrusted certificates, generous
/// rate limits, for trying the setup out.
pub const LETS_ENCRYPT_STAGING: &str = "https://acme-staging-v02.api.letsencrypt.org/directory";

/// How custom domains get their certificates (`domains.tls`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TlsMode {
    /// A proxy in front of the app (Caddy, which Harbor sets up) gets a
    /// certificate when a domain is first visited, after asking the app's
    /// ask endpoint (`domains.ask_path`) whether the domain is one of
    /// yours. The default.
    #[default]
    OnDemand,
    /// The app gets certificates from an ACME CA itself (HTTP-01
    /// challenges) and serves them with a rustls certificate resolver,
    /// on `domains.acme.https_addr`.
    Acme,
    /// No certificates: something else terminates TLS.
    Off,
}

impl TlsMode {
    /// `on_demand`, `acme` or `off`.
    pub fn as_str(self) -> &'static str {
        match self {
            TlsMode::OnDemand => "on_demand",
            TlsMode::Acme => "acme",
            TlsMode::Off => "off",
        }
    }
}

/// Options of the domains module (configuration section `domains`).
///
/// ```toml
/// [domains]
/// target = "domains.example.com"     # the CNAME customers create
/// addresses = ["203.0.113.10"]       # A records for apex domains
/// tls = "on_demand"                  # on_demand | acme | off
///
/// [domains.verification]
/// prefix = "_example-verification"   # TXT record at <prefix>.<domain>
/// interval_mins = 5
///
/// [domains.acme]                     # tls = "acme"
/// contact = ["ops@example.com"]
/// terms_agreed = true
/// https_addr = "0.0.0.0:443"
/// ```
///
/// Every key also comes from the environment:
/// `APP_DOMAINS__TLS=acme`, `APP_DOMAINS__DNS__CLOUDFLARE__API_TOKEN=...`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct DomainsOptions {
    /// Mount the `{http.api_prefix}/domains` routes (default true;
    /// permission `domains.manage`).
    pub routes: bool,
    /// How domains get certificates (default `on_demand`).
    pub tls: TlsMode,
    /// The ask endpoint an on-demand TLS proxy calls with `?domain=`
    /// (default `/_domains/allow`; mounted when `tls = "on_demand"`).
    pub ask_path: String,
    /// The host name customers point their domain at with a CNAME record
    /// (`domains.example.com`). With `addresses`, what a domain must point
    /// at to pass the routing check.
    pub target: Option<String>,
    /// The IP addresses an apex domain (which can't have a CNAME) points at
    /// with A and AAAA records (a list, or a comma-separated string).
    #[serde(deserialize_with = "list_or_csv")]
    pub addresses: Vec<String>,
    /// Domains no workspace may add, with their subdomains
    /// (`["example.com"]`). The host of `app.url`, `workspace.base_domain`
    /// and `target` are reserved too.
    #[serde(deserialize_with = "list_or_csv")]
    pub reserved: Vec<String>,
    /// The most domains one workspace may have (default 10; 0 for no
    /// limit).
    pub max_per_workspace: u32,
    /// How long a host lookup is kept in memory, in seconds (default 60;
    /// 0 asks the database every time).
    pub cache_secs: u64,
    /// DNS verification.
    pub verification: VerificationOptions,
    /// Certificate checks and expiry reports.
    pub certificates: CertificateOptions,
    /// In-process certificates (`tls = "acme"`).
    pub acme: AcmeOptions,
    /// A DNS provider that manages a zone of your own.
    pub dns: DnsOptions,
}

impl Default for DomainsOptions {
    fn default() -> Self {
        Self {
            routes: true,
            tls: TlsMode::OnDemand,
            ask_path: "/_domains/allow".to_owned(),
            target: None,
            addresses: Vec::new(),
            reserved: Vec::new(),
            max_per_workspace: 10,
            cache_secs: 60,
            verification: VerificationOptions::default(),
            certificates: CertificateOptions::default(),
            acme: AcmeOptions::default(),
            dns: DnsOptions::default(),
        }
    }
}

/// DNS verification (`domains.verification`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct VerificationOptions {
    /// The label in front of the domain that holds the TXT record
    /// (default `_domain-verification`: the record for `crm.acme-corp.com`
    /// is `_domain-verification.crm.acme-corp.com`). Name it after your
    /// product.
    pub prefix: String,
    /// Minutes between checks of a pending domain (default 5).
    pub interval_mins: u32,
    /// A domain still pending this many hours after it was added fails
    /// (default 72); verifying it by hand still works.
    pub give_up_after_hours: u32,
    /// A failed domain (one that gave up, or lost its host to the
    /// workspace that proved control) is deleted this many hours after it
    /// last changed (default 168, a week; 0 keeps it until its workspace
    /// removes it). A failed domain never blocks another workspace.
    pub failed_retention_hours: u32,
    /// Hours between checks that a verified domain still points at the app
    /// (default 24; 0 never checks again).
    pub recheck_hours: u32,
    /// A domain must point at `target` or `addresses` to be verified
    /// (default true; nothing is checked while both are unset).
    pub require_routing: bool,
}

impl Default for VerificationOptions {
    fn default() -> Self {
        Self {
            prefix: "_domain-verification".to_owned(),
            interval_mins: 5,
            give_up_after_hours: 72,
            failed_retention_hours: 168,
            recheck_hours: 24,
            require_routing: true,
        }
    }
}

/// Certificate checks and expiry reports (`domains.certificates`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct CertificateOptions {
    /// Report certificates that expire within this many days (default 14):
    /// a warning in the log, a `DomainCertificateExpiring` event and, with
    /// navree-lighthouse, an issue.
    pub warn_days: u32,
    /// Hours between certificate checks of one domain (default 24).
    pub check_hours: u32,
    /// With `tls = "on_demand"`, read each verified domain's certificate
    /// over TLS on port 443 to learn when it expires (default true).
    pub probe: bool,
}

impl Default for CertificateOptions {
    fn default() -> Self {
        Self {
            warn_days: 14,
            check_hours: 24,
            probe: true,
        }
    }
}

/// In-process certificates from an ACME CA (`domains.acme`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct AcmeOptions {
    /// The CA's ACME directory (default Let's Encrypt's production
    /// directory, [`LETS_ENCRYPT`]).
    pub directory_url: String,
    /// Contact addresses for the ACME account (`ops@example.com`).
    #[serde(deserialize_with = "list_or_csv")]
    pub contact: Vec<String>,
    /// You agree to the CA's terms of service (required with
    /// `tls = "acme"`).
    pub terms_agreed: bool,
    /// Where `serve` also listens for HTTPS with the certificates
    /// (`0.0.0.0:443`); unset, the app serves no HTTPS itself and you hand
    /// [`DomainCertificates`](crate::DomainCertificates) to your own
    /// rustls server.
    pub https_addr: Option<String>,
    /// Renew a certificate this many days before it expires (default 30).
    pub renew_days: u32,
    /// Your own hosts that get certificates too (`["app.example.com"]`).
    #[serde(deserialize_with = "list_or_csv")]
    pub hosts: Vec<String>,
    /// A PEM file with an extra root certificate for the directory's HTTPS
    /// (a private CA, or a test CA such as Pebble's).
    pub ca_file: Option<String>,
    /// Seconds between reloads of the certificates from the database by
    /// the HTTPS listener (default 60), so every server sees renewals.
    pub reload_secs: u64,
}

impl Default for AcmeOptions {
    fn default() -> Self {
        Self {
            directory_url: LETS_ENCRYPT.to_owned(),
            contact: Vec::new(),
            terms_agreed: false,
            https_addr: None,
            renew_days: 30,
            hosts: Vec::new(),
            ca_file: None,
            reload_secs: 60,
        }
    }
}

/// A DNS provider that manages a zone of your own (`domains.dns`): a
/// domain added inside `zone` gets its CNAME from the provider and is
/// verified at once.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct DnsOptions {
    /// `cloudflare`, or unset for none.
    pub provider: Option<String>,
    /// The zone the provider manages (`links.example.com`).
    pub zone: Option<String>,
    /// The Cloudflare driver.
    pub cloudflare: CloudflareOptions,
}

/// The Cloudflare driver (`domains.dns.cloudflare`).
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct CloudflareOptions {
    /// An API token with `Zone.DNS:Edit` on the zone.
    pub api_token: Option<String>,
    /// The zone's id (Overview page of the zone).
    pub zone_id: Option<String>,
    /// The API's base URL (default `https://api.cloudflare.com/client/v4`).
    pub api_url: String,
    /// Proxy the records through Cloudflare (default false: DNS only).
    pub proxied: bool,
    /// The records' TTL in seconds (default 1, automatic).
    pub ttl: u32,
}

impl Default for CloudflareOptions {
    fn default() -> Self {
        Self {
            api_token: None,
            zone_id: None,
            api_url: "https://api.cloudflare.com/client/v4".to_owned(),
            proxied: false,
            ttl: 1,
        }
    }
}

impl std::fmt::Debug for CloudflareOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CloudflareOptions")
            .field("api_token", &self.api_token.as_ref().map(|_| "[redacted]"))
            .field("zone_id", &self.zone_id)
            .field("api_url", &self.api_url)
            .field("proxied", &self.proxied)
            .field("ttl", &self.ttl)
            .finish()
    }
}

impl DomainsOptions {
    /// Every problem with the options, naming the configuration keys.
    pub fn check(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        if !self.ask_path.starts_with('/') || self.ask_path.len() < 2 {
            errors.push("domains.ask_path must start with `/` and name a path".to_owned());
        }
        if let Some(target) = &self.target
            && normalize_host(target).is_err()
        {
            errors.push(format!(
                "domains.target must be a host name such as `domains.example.com`, not `{target}`"
            ));
        }
        for address in &self.addresses {
            if address.trim().parse::<IpAddr>().is_err() {
                errors.push(format!(
                    "domains.addresses: `{address}` is not an IP address"
                ));
            }
        }
        for host in &self.reserved {
            if normalize_host(host).is_err() {
                errors.push(format!("domains.reserved: `{host}` is not a host name"));
            }
        }
        let verification = &self.verification;
        if !is_label(&verification.prefix) {
            errors.push(
                "domains.verification.prefix must be one DNS label of letters, digits, `-` and `_` (`_domain-verification`)"
                    .to_owned(),
            );
        }
        if verification.interval_mins == 0 {
            errors.push("domains.verification.interval_mins must be at least 1".to_owned());
        }
        if verification.give_up_after_hours == 0 {
            errors.push("domains.verification.give_up_after_hours must be at least 1".to_owned());
        }
        if self.certificates.check_hours == 0 {
            errors.push("domains.certificates.check_hours must be at least 1".to_owned());
        }
        self.check_acme(&mut errors);
        self.check_dns(&mut errors);
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    fn check_acme(&self, errors: &mut Vec<String>) {
        let acme = &self.acme;
        let url = acme.directory_url.trim();
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            errors.push("domains.acme.directory_url must be an http(s) URL".to_owned());
        }
        if acme.renew_days == 0 {
            errors.push("domains.acme.renew_days must be at least 1".to_owned());
        }
        if let Some(addr) = &acme.https_addr
            && addr.parse::<SocketAddr>().is_err()
        {
            errors.push(format!(
                "domains.acme.https_addr must be an address and port such as `0.0.0.0:443`, not `{addr}`"
            ));
        }
        for host in &acme.hosts {
            if normalize_host(host).is_err() {
                errors.push(format!("domains.acme.hosts: `{host}` is not a host name"));
            }
        }
        for contact in &acme.contact {
            let address = contact.trim().trim_start_matches("mailto:");
            if !address.contains('@') {
                errors.push(format!(
                    "domains.acme.contact: `{contact}` is not an email address"
                ));
            }
        }
        if self.tls == TlsMode::Acme && !acme.terms_agreed {
            errors.push(
                "domains.acme.terms_agreed must be true to use domains.tls = \"acme\" (you agree to the CA's terms of service)"
                    .to_owned(),
            );
        }
        if acme.reload_secs == 0 {
            errors.push("domains.acme.reload_secs must be at least 1".to_owned());
        }
    }

    fn check_dns(&self, errors: &mut Vec<String>) {
        let dns = &self.dns;
        if let Some(zone) = &dns.zone
            && normalize_host(zone).is_err()
        {
            errors.push(format!("domains.dns.zone: `{zone}` is not a host name"));
        }
        match dns.provider.as_deref().map(str::trim) {
            None | Some("") => {
                if dns.zone.is_some() {
                    errors.push("domains.dns.zone needs domains.dns.provider".to_owned());
                }
            }
            Some("cloudflare") => {
                if dns.zone.is_none() {
                    errors.push("domains.dns.zone is required with a DNS provider".to_owned());
                }
                let cloudflare = &dns.cloudflare;
                if cloudflare.api_token.as_deref().is_none_or(str::is_empty) {
                    errors.push(
                        "domains.dns.cloudflare.api_token is required with provider = \"cloudflare\""
                            .to_owned(),
                    );
                }
                if cloudflare.zone_id.as_deref().is_none_or(str::is_empty) {
                    errors.push(
                        "domains.dns.cloudflare.zone_id is required with provider = \"cloudflare\""
                            .to_owned(),
                    );
                }
                if !cloudflare.api_url.starts_with("http") {
                    errors.push("domains.dns.cloudflare.api_url must be an http(s) URL".to_owned());
                }
            }
            Some(other) => errors.push(format!(
                "domains.dns.provider: unknown provider `{other}` (use cloudflare)"
            )),
        }
    }

    /// The configured addresses, parsed.
    pub(crate) fn ip_addresses(&self) -> Vec<IpAddr> {
        self.addresses
            .iter()
            .filter_map(|address| address.trim().parse().ok())
            .collect()
    }
}

impl ConfigSection for DomainsOptions {
    const KEY: &'static str = "domains";

    fn validate(&self) -> Result<(), String> {
        self.check().map_err(|errors| errors.join("; "))
    }
}

/// One DNS label: letters, digits, `-` and `_`, 1 to 63 characters.
fn is_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 63
        && label
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// A list, or one comma-separated string (what an environment variable
/// holds).
fn list_or_csv<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum ListOrCsv {
        List(Vec<String>),
        Csv(String),
    }
    Ok(match ListOrCsv::deserialize(deserializer)? {
        ListOrCsv::List(items) => items,
        ListOrCsv::Csv(text) => text
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_owned)
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use navree_config::{Config, ConfigSources};
    use serde_json::json;

    use super::*;

    fn load(sources: ConfigSources) -> Result<DomainsOptions, navree_config::ConfigError> {
        Config::load(&sources).unwrap().typed::<DomainsOptions>()
    }

    #[test]
    fn the_defaults_need_no_configuration() {
        let options = load(ConfigSources::new()).unwrap();
        assert_eq!(options, DomainsOptions::default());
        assert!(options.routes);
        assert_eq!(options.tls, TlsMode::OnDemand);
        assert_eq!(options.ask_path, "/_domains/allow");
        assert_eq!(options.verification.prefix, "_domain-verification");
        assert_eq!(options.acme.directory_url, LETS_ENCRYPT);
        assert!(options.check().is_ok());
    }

    #[test]
    fn options_come_from_the_environment() {
        let options = load(ConfigSources::new().env_vars(
            "APP_",
            [
                ("APP_DOMAINS__TLS", "acme"),
                ("APP_DOMAINS__TARGET", "domains.example.com"),
                ("APP_DOMAINS__ADDRESSES", "203.0.113.10, 2001:db8::10"),
                ("APP_DOMAINS__ACME__TERMS_AGREED", "true"),
                ("APP_DOMAINS__ACME__CONTACT", "ops@example.com"),
                ("APP_DOMAINS__ACME__HTTPS_ADDR", "0.0.0.0:443"),
                ("APP_DOMAINS__VERIFICATION__PREFIX", "_acme-crm"),
                ("APP_DOMAINS__DNS__PROVIDER", "cloudflare"),
                ("APP_DOMAINS__DNS__ZONE", "links.example.com"),
                ("APP_DOMAINS__DNS__CLOUDFLARE__API_TOKEN", "token"),
                ("APP_DOMAINS__DNS__CLOUDFLARE__ZONE_ID", "zone-1"),
            ],
        ))
        .unwrap();
        assert_eq!(options.tls, TlsMode::Acme);
        assert_eq!(options.target.as_deref(), Some("domains.example.com"));
        assert_eq!(options.addresses, ["203.0.113.10", "2001:db8::10"]);
        assert_eq!(options.ip_addresses().len(), 2);
        assert_eq!(options.acme.contact, ["ops@example.com"]);
        assert_eq!(options.verification.prefix, "_acme-crm");
        assert_eq!(options.dns.provider.as_deref(), Some("cloudflare"));
        assert!(!format!("{:?}", options.dns.cloudflare).contains("token\""));
    }

    #[test]
    fn invalid_options_name_their_keys() {
        for (key, value, message) in [
            ("domains.ask_path", json!("allow"), "domains.ask_path"),
            ("domains.target", json!("https://x.test"), "domains.target"),
            ("domains.addresses", json!(["nope"]), "domains.addresses"),
            ("domains.reserved", json!(["*.x.test"]), "domains.reserved"),
            (
                "domains.verification.prefix",
                json!("a.b"),
                "domains.verification.prefix",
            ),
            (
                "domains.verification.interval_mins",
                json!(0),
                "interval_mins",
            ),
            (
                "domains.verification.give_up_after_hours",
                json!(0),
                "give_up_after_hours",
            ),
            ("domains.certificates.check_hours", json!(0), "check_hours"),
            (
                "domains.acme.directory_url",
                json!("acme.test"),
                "directory_url",
            ),
            ("domains.acme.renew_days", json!(0), "renew_days"),
            ("domains.acme.https_addr", json!("443"), "https_addr"),
            ("domains.acme.hosts", json!(["x"]), "domains.acme.hosts"),
            (
                "domains.acme.contact",
                json!(["ops"]),
                "domains.acme.contact",
            ),
            ("domains.acme.reload_secs", json!(0), "reload_secs"),
            ("domains.tls", json!("acme"), "terms_agreed"),
            (
                "domains.dns.zone",
                json!("links.example.com"),
                "needs domains.dns.provider",
            ),
            ("domains.dns.provider", json!("route53"), "unknown provider"),
            ("domains.dns.provider", json!("cloudflare"), "api_token"),
        ] {
            let error = load(ConfigSources::new().set(key, value.clone()))
                .unwrap_err()
                .to_string();
            assert!(error.contains(message), "{key} = {value}: {error}");
        }
    }

    #[test]
    fn the_section_never_shows_the_api_token_in_debug_output() {
        let options = load(ConfigSources::new().env_vars(
            "APP_",
            [
                ("APP_DOMAINS__DNS__PROVIDER", "cloudflare"),
                ("APP_DOMAINS__DNS__ZONE", "links.example.com"),
                (
                    "APP_DOMAINS__DNS__CLOUDFLARE__API_TOKEN",
                    "cf-api-t0ken-s3cret",
                ),
                ("APP_DOMAINS__DNS__CLOUDFLARE__ZONE_ID", "zone-1"),
            ],
        ))
        .unwrap();
        let printed = format!("{options:?}");
        assert!(!printed.contains("cf-api-t0ken-s3cret"), "{printed}");
        assert!(printed.contains("zone-1"), "{printed}");
    }
}
