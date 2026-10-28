//! `navree_http`: client IP resolution (`X-Forwarded-For`, `X-Real-IP`, the
//! trusted-proxy list) and the rate-limit key.
//!
//! Invariants: a peer that is not a trusted proxy is the client whatever
//! the headers say; the rate-limit key is idempotent, keeps IPv4 addresses
//! whole and clears every IPv6 bit after the prefix.
#![no_main]

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use arbitrary::Arbitrary;
use axum::extract::ConnectInfo;
use http::{HeaderMap, HeaderName, HeaderValue};
use libfuzzer_sys::fuzz_target;
use navree_http::{ClientIp, HttpOptions, rate_limit_key};

#[derive(Debug, Arbitrary)]
struct Input {
    peer: Option<([u8; 16], bool, u16)>,
    trust_proxy: bool,
    trusted: Vec<String>,
    forwarded: Vec<Vec<u8>>,
    real_ip: Option<Vec<u8>>,
    prefix: u8,
    probe: [u8; 16],
}

fn ip(bytes: [u8; 16], v4: bool) -> IpAddr {
    if v4 {
        IpAddr::V4(Ipv4Addr::new(bytes[0], bytes[1], bytes[2], bytes[3]))
    } else {
        IpAddr::V6(Ipv6Addr::from(bytes))
    }
}

fuzz_target!(|input: Input| {
    // The rate-limit key.
    for v4 in [true, false] {
        let address = ip(input.probe, v4);
        let key = rate_limit_key(address, input.prefix);
        assert_eq!(rate_limit_key(key, input.prefix), key, "idempotent");
        match (address.to_canonical(), key) {
            (IpAddr::V4(before), IpAddr::V4(after)) => assert_eq!(before, after),
            (IpAddr::V6(before), IpAddr::V6(after)) => {
                let prefix = u32::from(input.prefix.clamp(1, 128));
                let kept = u128::MAX << (128 - prefix);
                assert_eq!(u128::from(after), u128::from(before) & kept);
            }
            (before, after) => panic!("{before} became {after}"),
        }
    }

    // The client IP.
    let mut headers = HeaderMap::new();
    for value in &input.forwarded {
        if let Ok(value) = HeaderValue::from_bytes(value) {
            headers.append(HeaderName::from_static("x-forwarded-for"), value);
        }
    }
    if let Some(value) = input
        .real_ip
        .as_deref()
        .and_then(|value| HeaderValue::from_bytes(value).ok())
    {
        headers.insert(HeaderName::from_static("x-real-ip"), value);
    }
    let mut extensions = http::Extensions::new();
    let peer = input.peer.map(|(bytes, v4, port)| {
        let peer = SocketAddr::new(ip(bytes, v4), port);
        extensions.insert(ConnectInfo(peer));
        peer.ip().to_canonical()
    });
    let mut options = HttpOptions::default();
    options.trust_proxy = input.trust_proxy;
    options.trusted_proxies = input.trusted;
    let client = ClientIp::of(&headers, &extensions, &options).0;
    let nobody = options.trusted_proxies.is_empty() && !options.trust_proxy;
    if nobody || (peer.is_none() && !options.trust_proxy) {
        assert_eq!(
            client,
            peer.unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED)),
            "proxy headers were believed from a peer that is no proxy"
        );
    }
    assert_eq!(client, client.to_canonical(), "the client IP is canonical");
});
