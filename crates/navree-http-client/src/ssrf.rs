//! The server-side request forgery guard.
//!
//! Requests may not reach internal addresses: loopback, private, link-local,
//! carrier-grade NAT, unique-local and other special-purpose ranges, nor the
//! cloud metadata endpoints. IPv6 addresses that embed an IPv4 address
//! (IPv4-mapped, IPv4-compatible, NAT64, 6to4) are judged by that address.
//! Hosts and networks on the allow list bypass the guard.

use std::error::Error;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;

use ipnet::{IpNet, Ipv4Net, Ipv6Net};
use navree_glob::Glob;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use url::{Host, Url};

use crate::{HttpClientError, SsrfPolicy};

/// Cloud metadata endpoints, refused even when `block_private` is off: AWS,
/// GCP and Azure (`169.254.169.254`), AWS ECS task metadata, Alibaba Cloud
/// and Oracle Cloud.
const METADATA_V4: [Ipv4Addr; 4] = [
    Ipv4Addr::new(169, 254, 169, 254),
    Ipv4Addr::new(169, 254, 170, 2),
    Ipv4Addr::new(100, 100, 100, 200),
    Ipv4Addr::new(192, 0, 0, 192),
];

/// The AWS IPv6 metadata endpoint.
const METADATA_V6: [Ipv6Addr; 1] = [Ipv6Addr::new(0xfd00, 0x0ec2, 0, 0, 0, 0, 0, 0x0254)];

/// IPv4 ranges refused while `block_private` is on.
const BLOCKED_V4: [(Ipv4Net, &str); 14] = [
    (v4(0, 0, 0, 0, 8), "'this network' address"),
    (v4(10, 0, 0, 0, 8), "private address"),
    (v4(100, 64, 0, 0, 10), "carrier-grade NAT address"),
    (v4(127, 0, 0, 0, 8), "loopback address"),
    (v4(169, 254, 0, 0, 16), "link-local address"),
    (v4(172, 16, 0, 0, 12), "private address"),
    (v4(192, 0, 0, 0, 24), "IETF protocol assignment"),
    (v4(192, 0, 2, 0, 24), "documentation address"),
    (v4(192, 168, 0, 0, 16), "private address"),
    (v4(198, 18, 0, 0, 15), "benchmarking address"),
    (v4(198, 51, 100, 0, 24), "documentation address"),
    (v4(203, 0, 113, 0, 24), "documentation address"),
    (v4(224, 0, 0, 0, 4), "multicast address"),
    (v4(240, 0, 0, 0, 4), "reserved address"),
];

/// IPv6 ranges refused while `block_private` is on (embedded IPv4 forms are
/// judged by their IPv4 address before these apply).
const BLOCKED_V6: [(Ipv6Net, &str); 10] = [
    (v6([0, 0, 0, 0, 0, 0, 0, 0], 128), "unspecified address"),
    (v6([0, 0, 0, 0, 0, 0, 0, 1], 128), "loopback address"),
    (
        v6([0x64, 0xff9b, 1, 0, 0, 0, 0, 0], 48),
        "local-use NAT64 address",
    ),
    (v6([0x100, 0, 0, 0, 0, 0, 0, 0], 64), "discard-only address"),
    (
        v6([0x2001, 0, 0, 0, 0, 0, 0, 0], 32),
        "Teredo tunnel address",
    ),
    (
        v6([0x2001, 0xdb8, 0, 0, 0, 0, 0, 0], 32),
        "documentation address",
    ),
    (v6([0xfc00, 0, 0, 0, 0, 0, 0, 0], 7), "unique-local address"),
    (v6([0xfe80, 0, 0, 0, 0, 0, 0, 0], 10), "link-local address"),
    (v6([0xfec0, 0, 0, 0, 0, 0, 0, 0], 10), "site-local address"),
    (v6([0xff00, 0, 0, 0, 0, 0, 0, 0], 8), "multicast address"),
];

/// An IPv4 network literal.
const fn v4(a: u8, b: u8, c: u8, d: u8, prefix: u8) -> Ipv4Net {
    Ipv4Net::new_assert(Ipv4Addr::new(a, b, c, d), prefix)
}

/// An IPv6 network literal.
const fn v6(s: [u16; 8], prefix: u8) -> Ipv6Net {
    Ipv6Net::new_assert(
        Ipv6Addr::new(s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]),
        prefix,
    )
}

/// An [`SsrfPolicy`], parsed once.
#[derive(Debug)]
pub(crate) struct SsrfGuard {
    block_private: bool,
    /// Allow-list entries that are IP addresses or CIDR networks.
    allow_nets: Vec<IpNet>,
    /// Allow-list entries that are host names or globs (lower-cased).
    allow_hosts: Vec<Glob>,
}

impl SsrfGuard {
    /// Parses `policy`: each allow-list entry is an IP address, a CIDR
    /// network, or a host name glob (`*.internal.example`).
    pub(crate) fn new(policy: &SsrfPolicy) -> Self {
        let mut allow_nets = Vec::new();
        let mut allow_hosts = Vec::new();
        for entry in &policy.allow_hosts {
            let entry = normalize_host(entry);
            if entry.is_empty() {
                continue;
            }
            if let Ok(net) = entry.parse::<IpNet>() {
                allow_nets.push(net);
            } else if let Ok(ip) = entry.parse::<IpAddr>() {
                allow_nets.push(IpNet::from(ip));
            } else {
                allow_hosts.push(Glob::new(&entry));
            }
        }
        Self {
            block_private: policy.block_private,
            allow_nets,
            allow_hosts,
        }
    }

    /// `true` when `host` matches an allow-list name or glob: every address
    /// it resolves to is then accepted.
    pub(crate) fn host_allowed(&self, host: &str) -> bool {
        let host = normalize_host(host);
        self.allow_hosts.iter().any(|glob| glob.matches(&host))
    }

    /// Accepts `ip` unless it lies in a refused range and no allow-list
    /// network contains it; the error names the range and the address.
    pub(crate) fn check_ip(&self, ip: IpAddr) -> Result<(), String> {
        let canonical = ip.to_canonical();
        if self
            .allow_nets
            .iter()
            .any(|net| net.contains(&ip) || net.contains(&canonical))
        {
            return Ok(());
        }
        match classify(ip, self.block_private) {
            Some(range) => Err(format!("{ip} is a {range}")),
            None => Ok(()),
        }
    }

    /// Checks what can be checked from the URL alone: the scheme
    /// (`http`/`https`), and the address when the host is an IP literal
    /// (literal addresses never reach the DNS resolver).
    pub(crate) fn check_url(&self, url: &Url) -> Result<(), HttpClientError> {
        if !matches!(url.scheme(), "http" | "https") {
            return Err(HttpClientError::Build(format!(
                "unsupported URL scheme `{}` (only http and https)",
                url.scheme()
            )));
        }
        self.check_url_host(url).map_err(Refused::into_error)
    }

    /// The host part of [`check_url`](Self::check_url).
    fn check_url_host(&self, url: &Url) -> Result<(), Refused> {
        let ip = match url.host() {
            None => {
                return Err(Refused {
                    host: String::new(),
                    reason: "the URL has no host".to_owned(),
                });
            }
            Some(Host::Domain(_)) => return Ok(()),
            Some(Host::Ipv4(ip)) => IpAddr::V4(ip),
            Some(Host::Ipv6(ip)) => IpAddr::V6(ip),
        };
        let host = url.host_str().unwrap_or_default();
        if self.host_allowed(host) {
            return Ok(());
        }
        self.check_ip(ip).map_err(|reason| Refused {
            host: host.to_owned(),
            reason,
        })
    }

    /// The reqwest redirect policy: at most [`MAX_REDIRECTS`] hops, each to
    /// an `http`/`https` URL the guard accepts (named hosts are checked again
    /// when they are resolved).
    pub(crate) fn redirect_policy(self: &Arc<Self>) -> reqwest::redirect::Policy {
        let guard = self.clone();
        reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() > MAX_REDIRECTS {
                return attempt.error("too many redirects");
            }
            let url = attempt.url();
            if !matches!(url.scheme(), "http" | "https") {
                let message = format!("redirect to unsupported scheme `{}`", url.scheme());
                return attempt.error(message);
            }
            match guard.check_url_host(url) {
                Ok(()) => attempt.follow(),
                Err(refused) => attempt.error(refused),
            }
        })
    }
}

/// The most redirects followed for one request.
const MAX_REDIRECTS: usize = 10;

/// A destination the guard refused; travels through reqwest's error chain
/// and comes back out as [`HttpClientError::Blocked`].
#[derive(Clone, Debug)]
pub(crate) struct Refused {
    host: String,
    reason: String,
}

impl Refused {
    /// The public error.
    pub(crate) fn into_error(self) -> HttpClientError {
        HttpClientError::Blocked {
            host: self.host,
            reason: self.reason,
        }
    }

    /// The refusal somewhere in `error`'s source chain, if any.
    pub(crate) fn find<'a>(error: &'a (dyn Error + 'static)) -> Option<&'a Refused> {
        let mut current = Some(error);
        while let Some(error) = current {
            if let Some(refused) = error.downcast_ref::<Refused>() {
                return Some(refused);
            }
            current = error.source();
        }
        None
    }
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`{}` blocked by the SSRF policy: {}",
            self.host, self.reason
        )
    }
}

impl Error for Refused {}

/// The DNS resolver of real clients: resolves with the system resolver,
/// then refuses the host if any address it resolves to is refused (unless
/// the host name itself is allow-listed). Connections only ever go to the
/// addresses checked here, so DNS rebinding cannot slip past the check.
pub(crate) struct SsrfResolver {
    guard: Arc<SsrfGuard>,
}

impl SsrfResolver {
    /// A resolver enforcing `guard`.
    pub(crate) fn new(guard: Arc<SsrfGuard>) -> Self {
        Self { guard }
    }
}

impl Resolve for SsrfResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let guard = self.guard.clone();
        let host = name.as_str().to_owned();
        Box::pin(async move {
            let addrs: Vec<SocketAddr> =
                tokio::net::lookup_host((host.as_str(), 0)).await?.collect();
            if !guard.host_allowed(&host) {
                for addr in &addrs {
                    guard.check_ip(addr.ip()).map_err(|reason| Refused {
                        host: host.clone(),
                        reason,
                    })?;
                }
            }
            Ok(Box::new(addrs.into_iter()) as Addrs)
        })
    }
}

/// Lower-cased, without IPv6 brackets or a trailing dot.
fn normalize_host(host: &str) -> String {
    let host = host.trim().trim_end_matches('.');
    let host = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host);
    host.to_ascii_lowercase()
}

/// The refused range `ip` falls in, if any.
fn classify(ip: IpAddr, block_private: bool) -> Option<&'static str> {
    match ip {
        IpAddr::V4(v4) => classify_v4(v4, block_private),
        IpAddr::V6(v6) => match embedded_ipv4(v6) {
            Some(v4) => classify_v4(v4, block_private),
            None => classify_v6(v6, block_private),
        },
    }
}

/// See [`classify`].
fn classify_v4(ip: Ipv4Addr, block_private: bool) -> Option<&'static str> {
    if METADATA_V4.contains(&ip) {
        return Some("cloud metadata address");
    }
    if !block_private {
        return None;
    }
    BLOCKED_V4
        .iter()
        .find(|(net, _)| net.contains(&ip))
        .map(|(_, range)| *range)
}

/// See [`classify`].
fn classify_v6(ip: Ipv6Addr, block_private: bool) -> Option<&'static str> {
    if METADATA_V6.contains(&ip) {
        return Some("cloud metadata address");
    }
    if !block_private {
        return None;
    }
    BLOCKED_V6
        .iter()
        .find(|(net, _)| net.contains(&ip))
        .map(|(_, range)| *range)
}

/// The IPv4 address an IPv6 address stands for: IPv4-mapped
/// (`::ffff:a.b.c.d`), IPv4-compatible (`::a.b.c.d`, except `::` and
/// `::1`), NAT64 (`64:ff9b::a.b.c.d`) or 6to4 (`2002:aabb:ccdd::/48`).
fn embedded_ipv4(ip: Ipv6Addr) -> Option<Ipv4Addr> {
    let s = ip.segments();
    let o = ip.octets();
    let tail = Ipv4Addr::new(o[12], o[13], o[14], o[15]);
    if s[..5] == [0; 5] && s[5] == 0xffff {
        return Some(tail);
    }
    if s[..6] == [0; 6] && !(s[6] == 0 && s[7] <= 1) {
        return Some(tail);
    }
    if s[0] == 0x64 && s[1] == 0xff9b && s[2..6] == [0; 4] {
        return Some(tail);
    }
    if s[0] == 0x2002 {
        return Some(Ipv4Addr::new(o[2], o[3], o[4], o[5]));
    }
    None
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use super::*;
    use crate::SsrfPolicy;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    fn guard(block_private: bool, allow: &[&str]) -> SsrfGuard {
        SsrfGuard::new(&SsrfPolicy {
            block_private,
            allow_hosts: allow.iter().map(|s| s.to_string()).collect(),
        })
    }

    #[test]
    fn blocks_internal_ipv4_ranges() {
        let g = guard(true, &[]);
        for blocked in [
            "127.0.0.1",
            "127.255.255.254",
            "10.1.2.3",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.169.254",
            "169.254.1.1",
            "100.64.0.1",
            "100.127.255.255",
            "100.100.100.200",
            "0.0.0.0",
            "192.0.0.192",
            "198.18.0.1",
            "224.0.0.1",
            "255.255.255.255",
        ] {
            assert!(
                g.check_ip(ip(blocked)).is_err(),
                "{blocked} must be blocked"
            );
        }
    }

    #[test]
    fn allows_public_ipv4() {
        let g = guard(true, &[]);
        for public in [
            "8.8.8.8",
            "1.1.1.1",
            "172.32.0.1",
            "100.128.0.1",
            "93.184.216.34",
            "192.169.0.1",
        ] {
            assert!(g.check_ip(ip(public)).is_ok(), "{public} must be allowed");
        }
    }

    #[test]
    fn blocks_internal_ipv6_ranges_and_embedded_ipv4() {
        let g = guard(true, &[]);
        for blocked in [
            "::1",
            "::",
            "fe80::1",
            "fc00::1",
            "fd12:3456::1",
            "fd00:ec2::254",
            "ff02::1",
            "::ffff:127.0.0.1",
            "::ffff:169.254.169.254",
            "::127.0.0.1",
            "64:ff9b::a9fe:a9fe",
            "2002:7f00:1::",
            "2001:0:4136:e378:8000:63bf:3fff:fdd2",
        ] {
            assert!(
                g.check_ip(ip(blocked)).is_err(),
                "{blocked} must be blocked"
            );
        }
    }

    #[test]
    fn allows_public_ipv6() {
        let g = guard(true, &[]);
        for public in [
            "2606:4700:4700::1111",
            "2001:4860:4860::8888",
            "::ffff:8.8.8.8",
            "2002:808:808::",
        ] {
            assert!(g.check_ip(ip(public)).is_ok(), "{public} must be allowed");
        }
    }

    #[test]
    fn metadata_stays_blocked_when_private_ranges_are_allowed() {
        let g = guard(false, &[]);
        for metadata in [
            "169.254.169.254",
            "169.254.170.2",
            "100.100.100.200",
            "192.0.0.192",
            "fd00:ec2::254",
            "::ffff:169.254.169.254",
        ] {
            assert!(
                g.check_ip(ip(metadata)).is_err(),
                "{metadata} must be blocked"
            );
        }
        for internal in ["127.0.0.1", "10.0.0.1", "::1", "fd12::1"] {
            assert!(
                g.check_ip(ip(internal)).is_ok(),
                "{internal} must be allowed"
            );
        }
    }

    #[test]
    fn allow_list_accepts_ips_networks_and_host_globs() {
        let g = guard(
            true,
            &["127.0.0.1", "10.0.0.0/8", "*.internal.example", "LocalHost"],
        );
        assert!(g.check_ip(ip("127.0.0.1")).is_ok());
        assert!(g.check_ip(ip("::ffff:127.0.0.1")).is_ok());
        assert!(g.check_ip(ip("10.2.3.4")).is_ok());
        assert!(g.check_ip(ip("127.0.0.2")).is_err());
        assert!(g.check_ip(ip("192.168.0.1")).is_err());
        assert!(g.host_allowed("api.internal.example"));
        assert!(g.host_allowed("API.Internal.Example."));
        assert!(g.host_allowed("localhost"));
        assert!(!g.host_allowed("internal.example"));
        assert!(!g.host_allowed("evil.example"));
    }

    #[test]
    fn host_globs_know_only_star_and_question_mark() {
        let g = guard(
            true,
            &["db-?.internal", "*.corp.*", "a*b*c.example", "[ab].x", ""],
        );
        // `?` is exactly one character.
        assert!(g.host_allowed("db-1.internal"));
        assert!(!g.host_allowed("db-.internal"));
        assert!(!g.host_allowed("db-12.internal"));
        // `*` crosses dots and may match nothing.
        assert!(g.host_allowed("x.y.corp.example.com"));
        assert!(g.host_allowed("x.corp.y"));
        assert!(!g.host_allowed("corp.example.com"));
        assert!(g.host_allowed("abc.example"));
        assert!(g.host_allowed("a-long-b-longer-c.example"));
        assert!(!g.host_allowed("a-long-b-longer.example"));
        // Brackets are literal; hosts are lower-cased before matching.
        assert!(!g.host_allowed("a.x"));
        assert!(g.host_allowed("[ab].x"));
        assert!(g.host_allowed("DB-9.INTERNAL"));
        // An empty entry is skipped, so the empty host is not allowed.
        assert!(!g.host_allowed(""));
        // A run of `*` between letters still matches in linear time.
        let g = guard(true, &["*a*a*a*a*a*a*a*b"]);
        assert!(!g.host_allowed(&"a".repeat(4000)));
    }

    #[test]
    fn a_host_glob_matches_names_and_never_addresses() {
        // A glob is the text of a name: `*` crosses dots, so an entry that
        // ends in a star fits names under any domain, and a network written
        // as a glob is a name pattern. (The allow list is trusted with every
        // address an allow-listed name resolves to; the docs say to end a
        // glob in a domain the operator owns and to write networks as CIDR.)
        let g = guard(true, &["api.internal.*", "10.20.*", "10.30.0.0/16"]);
        assert!(g.host_allowed("api.internal.example"));
        assert!(g.host_allowed("api.internal.attacker.example"));
        assert!(g.host_allowed("10.20.attacker.example"));
        assert!(!g.host_allowed("internal.example"));
        // The glob does not make a network: only the CIDR entry does.
        assert!(g.check_ip(ip("10.20.1.1")).is_err());
        assert!(g.check_ip(ip("10.30.1.1")).is_ok());
        assert!(g.check_ip(ip("10.31.1.1")).is_err());
        // Without an allow-listed name, the metadata endpoints stay refused.
        assert!(guard(false, &[]).check_ip(ip("169.254.169.254")).is_err());
    }

    #[test]
    fn blocked_reason_names_the_range_and_address() {
        let error = guard(true, &[])
            .check_ip(ip("169.254.169.254"))
            .unwrap_err();
        assert!(error.contains("169.254.169.254"), "{error}");
        assert!(error.contains("metadata"), "{error}");
    }
}
