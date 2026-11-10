//! [`ClientIp`]: the client's address, behind trusted proxies.

use std::convert::Infallible;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use http::HeaderMap;
use http::request::Parts;

use super::FromRequestParts;
use crate::serve::ConnectInfo;

/// The client IP address. The pipeline computes it once per request (from
/// the connection, or from `X-Forwarded-For` / `X-Real-IP` when the peer is
/// a trusted proxy: `http.trusted_proxies`, or any peer with
/// `http.trust_proxy`) and stores it in the request extensions.
/// `0.0.0.0` when unknown (requests not served through a socket).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClientIp(pub IpAddr);

impl ClientIp {
    /// The client IP of a request the way the pipeline computes it, with the
    /// proxy settings of `options` (`http.trusted_proxies`,
    /// `http.trust_proxy`): for protocols that bypass the HTTP pipeline on
    /// the same port (navree-grpc).
    pub fn of(
        headers: &HeaderMap,
        extensions: &http::Extensions,
        options: &crate::HttpOptions,
    ) -> Self {
        let trust = ProxyTrust::new(options.trust_proxy, &options.trusted_proxies);
        Self::resolve(headers, extensions, &trust)
    }

    /// The client IP of a request: the proxy headers when `trust` trusts
    /// the peer, else the peer address, else `0.0.0.0`.
    pub(crate) fn resolve(
        headers: &HeaderMap,
        extensions: &http::Extensions,
        trust: &ProxyTrust,
    ) -> Self {
        let peer = extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ConnectInfo(addr)| addr.ip().to_canonical());
        if trust.trusts(peer)
            && let Some(ip) = forwarded_ip(headers, trust)
        {
            return Self(ip);
        }
        Self(peer.unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED)))
    }
}

/// Which peers' `X-Forwarded-For` / `X-Real-IP` headers are believed.
#[derive(Clone, Debug, Default)]
pub(crate) struct ProxyTrust {
    /// Every peer (`http.trust_proxy` without `http.trusted_proxies`).
    any: bool,
    /// The trusted proxies (`http.trusted_proxies`).
    networks: Vec<IpNetwork>,
}

impl ProxyTrust {
    /// The trust `http.trusted_proxies` and `http.trust_proxy` configure:
    /// the listed proxies when there are any (entries that do not parse
    /// are rejected at boot), else every peer when `trust_proxy` is on,
    /// else none.
    pub(crate) fn new(trust_proxy: bool, trusted_proxies: &[String]) -> Self {
        let networks: Vec<IpNetwork> = trusted_proxies
            .iter()
            .filter_map(|entry| IpNetwork::parse(entry))
            .collect();
        Self {
            any: trust_proxy && networks.is_empty(),
            networks,
        }
    }

    /// No proxy is trusted.
    pub(crate) fn none() -> Self {
        Self::default()
    }

    /// Whether the headers of a request from `peer` are believed.
    pub(crate) fn trusts(&self, peer: Option<IpAddr>) -> bool {
        self.any || peer.is_some_and(|peer| self.is_proxy(peer))
    }

    fn is_proxy(&self, ip: IpAddr) -> bool {
        let ip = ip.to_canonical();
        self.networks.iter().any(|network| network.contains(ip))
    }
}

/// An IP address or a CIDR block (`10.0.0.0/8`, `::1`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IpNetwork {
    addr: IpAddr,
    prefix: u8,
}

impl IpNetwork {
    /// `ip` or `ip/prefix`; `None` when it is neither.
    pub(crate) fn parse(entry: &str) -> Option<Self> {
        let entry = entry.trim();
        let (addr, prefix) = match entry.split_once('/') {
            Some((addr, prefix)) => (addr, Some(prefix)),
            None => (entry, None),
        };
        let addr = addr.parse::<IpAddr>().ok()?.to_canonical();
        let max = if addr.is_ipv4() { 32 } else { 128 };
        let prefix = match prefix {
            Some(prefix) => prefix.parse::<u8>().ok().filter(|p| *p <= max)?,
            None => max,
        };
        Some(Self { addr, prefix })
    }

    fn contains(&self, ip: IpAddr) -> bool {
        match (self.addr, ip) {
            (IpAddr::V4(net), IpAddr::V4(ip)) => {
                let mask = u32::MAX
                    .checked_shl(32 - u32::from(self.prefix))
                    .unwrap_or(0);
                u32::from(net) & mask == u32::from(ip) & mask
            }
            (IpAddr::V6(net), IpAddr::V6(ip)) => {
                let mask = u128::MAX
                    .checked_shl(128 - u32::from(self.prefix))
                    .unwrap_or(0);
                u128::from(net) & mask == u128::from(ip) & mask
            }
            _ => false,
        }
    }
}

/// The client in `X-Forwarded-For`: walking from the right (the entry our
/// proxy added), the first address that is not itself a trusted proxy (the
/// left-most when all are; the right-most when every peer is trusted).
/// Else `X-Real-IP`. An entry may carry a port (`203.0.113.7:51234`); an
/// entry that is no address ends the walk, so nothing the client wrote left
/// of it is believed.
fn forwarded_ip(headers: &HeaderMap, trust: &ProxyTrust) -> Option<IpAddr> {
    let mut chain: Vec<IpAddr> = Vec::new();
    let entries: Vec<&str> = headers
        .get_all("x-forwarded-for")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .collect();
    for entry in entries.iter().rev() {
        let Some(ip) = entry
            .parse::<IpAddr>()
            .ok()
            .or_else(|| entry.parse::<SocketAddr>().ok().map(|addr| addr.ip()))
        else {
            break;
        };
        chain.push(ip.to_canonical());
    }
    chain.reverse();
    let forwarded = if trust.any {
        chain.last().copied()
    } else {
        chain
            .iter()
            .rev()
            .find(|ip| !trust.is_proxy(**ip))
            .or(chain.first())
            .copied()
    };
    if forwarded.is_none() && !entries.is_empty() {
        // The proxy's own entry is unreadable: the client is unknown.
        return None;
    }
    forwarded.or_else(|| {
        headers
            .get("x-real-ip")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<IpAddr>().ok())
            .map(|ip| ip.to_canonical())
    })
}

impl ClientIp {
    /// The client IP the pipeline recorded for the request of `parts`; for
    /// a request outside the pipeline, the peer address (no proxy is
    /// trusted), else `0.0.0.0`.
    pub fn from_parts(parts: &Parts) -> Self {
        match parts.extensions.get::<ClientIp>() {
            Some(ip) => *ip,
            None => ClientIp::resolve(&parts.headers, &parts.extensions, &ProxyTrust::none()),
        }
    }
}

impl FromRequestParts for ClientIp {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        Ok(ClientIp::from_parts(parts))
    }
}

#[cfg(test)]
mod tests {
    use http::HeaderValue;

    use super::*;

    fn peer(ip: [u8; 4]) -> http::Extensions {
        let mut extensions = http::Extensions::new();
        extensions.insert(ConnectInfo(SocketAddr::from((ip, 4000))));
        extensions
    }

    fn forwarded_for(value: &'static str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static(value));
        headers
    }

    #[test]
    fn proxy_headers_are_ignored_unless_trusted() {
        let headers = forwarded_for("6.6.6.6, 10.0.0.1");
        let extensions = peer([192, 168, 1, 2]);
        let any = ProxyTrust::new(true, &[]);

        let untrusted = ClientIp::resolve(&headers, &extensions, &ProxyTrust::none());
        assert_eq!(untrusted.0, IpAddr::from([192, 168, 1, 2]));

        let trusted = ClientIp::resolve(&headers, &extensions, &any);
        assert_eq!(trusted.0, IpAddr::from([10, 0, 0, 1]), "right-most entry");

        let mut headers = HeaderMap::new();
        headers.insert("x-real-ip", HeaderValue::from_static("8.8.8.8"));
        assert_eq!(
            ClientIp::resolve(&headers, &extensions, &any).0,
            IpAddr::from([8, 8, 8, 8])
        );
        assert_eq!(
            ClientIp::resolve(&HeaderMap::new(), &http::Extensions::new(), &any).0,
            IpAddr::V4(Ipv4Addr::UNSPECIFIED)
        );
    }

    /// Behind a BFF or reverse proxy every request comes from the proxy;
    /// only the listed proxies may name the client, so a client reaching
    /// the app directly cannot pick its own address.
    #[test]
    fn only_listed_proxies_name_the_client() {
        let trust = ProxyTrust::new(false, &["127.0.0.1".to_owned(), "10.0.0.0/8".to_owned()]);
        let headers = forwarded_for("203.0.113.7");

        let via_bff = ClientIp::resolve(&headers, &peer([127, 0, 0, 1]), &trust);
        assert_eq!(via_bff.0, IpAddr::from([203, 0, 113, 7]));
        let via_lb = ClientIp::resolve(&headers, &peer([10, 1, 2, 3]), &trust);
        assert_eq!(via_lb.0, IpAddr::from([203, 0, 113, 7]));
        let direct = ClientIp::resolve(&headers, &peer([198, 51, 100, 9]), &trust);
        assert_eq!(
            direct.0,
            IpAddr::from([198, 51, 100, 9]),
            "spoofed header ignored"
        );

        // A client-supplied entry left of the proxies' own is skipped over:
        // the first untrusted address from the right is the client.
        let chained = forwarded_for("6.6.6.6, 203.0.113.7, 10.0.0.2");
        assert_eq!(
            ClientIp::resolve(&chained, &peer([127, 0, 0, 1]), &trust).0,
            IpAddr::from([203, 0, 113, 7])
        );
        // No header: the proxy's own address.
        assert_eq!(
            ClientIp::resolve(&HeaderMap::new(), &peer([127, 0, 0, 1]), &trust).0,
            IpAddr::from([127, 0, 0, 1])
        );
        // The list wins over `trust_proxy`: other peers stay untrusted.
        let both = ProxyTrust::new(true, &["127.0.0.1".to_owned()]);
        assert_eq!(
            ClientIp::resolve(&headers, &peer([198, 51, 100, 9]), &both).0,
            IpAddr::from([198, 51, 100, 9])
        );
    }

    /// Some proxies write the client with its port. The entry still names
    /// the client, and an entry that is no address at all ends the walk:
    /// what the client wrote left of it is never believed.
    #[test]
    fn entries_with_a_port_or_no_address_never_hand_the_client_its_choice() {
        let trust = ProxyTrust::new(false, &["10.0.0.0/8".to_owned()]);
        let via = |value: &'static str| {
            ClientIp::resolve(&forwarded_for(value), &peer([10, 0, 0, 1]), &trust).0
        };
        assert_eq!(
            via("6.6.6.6, 203.0.113.7:51234"),
            IpAddr::from([203, 0, 113, 7])
        );
        assert_eq!(
            via("6.6.6.6, [2001:db8::7]:443, 10.0.0.2"),
            "2001:db8::7".parse::<IpAddr>().unwrap()
        );
        assert_eq!(via("6.6.6.6, unknown"), IpAddr::from([10, 0, 0, 1]));
        let any = ProxyTrust::new(true, &[]);
        assert_eq!(
            ClientIp::resolve(
                &forwarded_for("6.6.6.6, 203.0.113.7:51234"),
                &peer([10, 0, 0, 1]),
                &any
            )
            .0,
            IpAddr::from([203, 0, 113, 7])
        );
        assert_eq!(
            ClientIp::resolve(
                &forwarded_for("6.6.6.6, unknown"),
                &peer([10, 0, 0, 1]),
                &any
            )
            .0,
            IpAddr::from([10, 0, 0, 1])
        );
    }

    #[test]
    fn networks_parse_and_match() {
        let net = |s| IpNetwork::parse(s).expect(s);
        assert!(net("10.0.0.0/8").contains(IpAddr::from([10, 200, 0, 1])));
        assert!(!net("10.0.0.0/8").contains(IpAddr::from([11, 0, 0, 1])));
        assert!(net("127.0.0.1").contains(IpAddr::from([127, 0, 0, 1])));
        assert!(!net("127.0.0.1").contains(IpAddr::from([127, 0, 0, 2])));
        assert!(net("0.0.0.0/0").contains(IpAddr::from([1, 2, 3, 4])));
        assert!(net("::1").contains("::1".parse().unwrap()));
        assert!(net("fd00::/8").contains("fd12::3".parse().unwrap()));
        assert!(!net("fd00::/8").contains(IpAddr::from([10, 0, 0, 1])));
        // IPv4-mapped IPv6 addresses count as IPv4.
        let mapped: IpAddr = "::ffff:127.0.0.1".parse().unwrap();
        assert!(net("127.0.0.1").contains(mapped.to_canonical()));
        assert!(net("::ffff:127.0.0.1").contains(IpAddr::from([127, 0, 0, 1])));
        for bad in ["", "localhost", "10.0.0.0/33", "::1/129", "1.2.3.4/x"] {
            assert_eq!(IpNetwork::parse(bad), None, "{bad}");
        }
    }
}
