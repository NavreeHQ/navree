//! The network a client's rate limit is counted against.
//!
//! One IPv6 allocation (a /64 is the smallest a provider normally hands out)
//! holds billions of addresses, so a limit that counts single addresses
//! limits nobody who has one. Every per-IP limit in Navree (the per-IP
//! limit and route throttles of this crate, the sign-in throttles of
//! navree-auth and navree-social, the endpoint limits of navree-oauth) keys
//! its counters with [`rate_limit_key`], and the prefix is one setting:
//! `http.rate_limit_ipv6_prefix`.

use std::net::{IpAddr, Ipv6Addr};

/// The default IPv6 prefix length of a rate-limit key: a /64.
pub const DEFAULT_IPV6_PREFIX: u8 = 64;

/// The key `ip` is counted under: the address itself for IPv4 (and for
/// IPv4-mapped IPv6 addresses, which are IPv4 clients behind a dual-stack
/// socket), else the address with every bit after the first `ipv6_prefix`
/// cleared, so the addresses of one network share one budget. A prefix of
/// 0 is read as 1 and one above 128 as 128.
///
/// ```
/// use navree_http::rate_limit_key;
///
/// let a = "2001:db8:1:2::1".parse().unwrap();
/// let b = "2001:db8:1:2:ffff::9".parse().unwrap();
/// assert_eq!(rate_limit_key(a, 64), rate_limit_key(b, 64));
/// assert_ne!(rate_limit_key(a, 128), rate_limit_key(b, 128));
/// ```
pub fn rate_limit_key(ip: IpAddr, ipv6_prefix: u8) -> IpAddr {
    match ip.to_canonical() {
        IpAddr::V6(v6) => {
            let prefix = u32::from(ipv6_prefix.clamp(1, 128));
            let mask = u128::MAX << (128 - prefix);
            IpAddr::V6(Ipv6Addr::from(u128::from(v6) & mask))
        }
        v4 => v4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    #[test]
    fn addresses_of_one_slash_64_share_a_key() {
        let key = rate_limit_key(ip("2001:db8:1:2:aaaa:bbbb:cccc:dddd"), 64);
        assert_eq!(key, ip("2001:db8:1:2::"));
        assert_eq!(rate_limit_key(ip("2001:db8:1:2::1"), 64), key);
        assert_ne!(rate_limit_key(ip("2001:db8:1:3::1"), 64), key);
    }

    #[test]
    fn the_prefix_sets_the_network_size() {
        let a = ip("2001:db8:1:2::1");
        let b = ip("2001:db8:1:9::1");
        assert_eq!(rate_limit_key(a, 48), rate_limit_key(b, 48));
        assert_ne!(rate_limit_key(a, 64), rate_limit_key(b, 64));
        assert_eq!(rate_limit_key(a, 128), a);
        assert_eq!(rate_limit_key(a, 200), a, "clamped to a full address");
        // 56 is not a multiple of 16: the mask cuts inside a group.
        assert_eq!(
            rate_limit_key(ip("2001:db8:1:ff7f::1"), 56),
            ip("2001:db8:1:ff00::")
        );
    }

    #[test]
    fn ipv4_is_counted_per_address_and_mapped_addresses_are_ipv4() {
        assert_eq!(rate_limit_key(ip("192.0.2.7"), 64), ip("192.0.2.7"));
        assert_eq!(rate_limit_key(ip("::ffff:192.0.2.7"), 64), ip("192.0.2.7"));
        assert_ne!(
            rate_limit_key(ip("192.0.2.7"), 64),
            rate_limit_key(ip("192.0.2.8"), 64)
        );
    }
}
