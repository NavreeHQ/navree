//! `navree_http::ws`'s handshake on untrusted input. The input is a request
//! head as text: the first line is the method and the version (`1`, `2`, or
//! anything else for HTTP/1.0), and the rest are `name: value` lines. Lines
//! that are not headers are skipped: hyper refuses those before a handler.
//!
//! Invariants:
//! - nothing panics, and every rejection is a 4xx with a message;
//! - an HTTP/1.1 handshake that passes has one key that is the base64 of 16
//!   bytes, and an accept key of the 28 characters of a SHA-1;
//! - the same headers in another case read the same;
//! - offered subprotocols are never empty and hold no comma, nor space at
//!   either end, so a chosen protocol is one item of the client's list;
//! - `origin_allowed` lets through no origin that is not listed, unless it
//!   is the request's own host.
#![no_main]

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use http::{HeaderName, HeaderValue, Method, Request, Version};
use libfuzzer_sys::fuzz_target;
use navree_fuzz::text;
use navree_http::ws::{Handshake, origin_allowed};

const ALLOWED: [&str; 1] = ["https://app.example.com"];

fn parts(text: &str, swap_case: bool) -> Option<http::request::Parts> {
    let mut lines = text.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let method = Method::from_bytes(first.next()?.as_bytes()).ok()?;
    let version = match first.next() {
        Some("1") => Version::HTTP_11,
        Some("2") => Version::HTTP_2,
        _ => Version::HTTP_10,
    };
    let mut request = Request::builder().method(method).version(version).uri("/");
    for line in lines {
        let Some((name, value)) = line.split_once(": ") else {
            continue;
        };
        let name = if swap_case {
            name.to_ascii_uppercase()
        } else {
            name.to_owned()
        };
        let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(value),
        ) else {
            continue;
        };
        request = request.header(name, value);
    }
    let mut request = request.body(()).ok()?;
    if version == Version::HTTP_2 {
        request
            .extensions_mut()
            .insert(hyper::ext::Protocol::from_static("websocket"));
    }
    Some(request.into_parts().0)
}

fuzz_target!(|data: &[u8]| {
    let Some(input) = text(data) else {
        return;
    };
    let Some(head) = parts(input, false) else {
        return;
    };
    let read = Handshake::read(&head);

    if let Some(other) = parts(input, true) {
        assert_eq!(
            read.as_ref().err(),
            Handshake::read(&other).as_ref().err(),
            "the case of a header name changed the verdict"
        );
    }

    let handshake = match read {
        Ok(handshake) => handshake,
        Err(rejection) => {
            assert!(rejection.status().is_client_error());
            assert!(!rejection.body_text().is_empty());
            return;
        }
    };

    match handshake.accept_key() {
        Some(accept) => {
            assert_eq!(head.version, Version::HTTP_11);
            assert_eq!(accept.len(), 28);
            let key = head.headers.get("sec-websocket-key").expect("a key");
            assert_eq!(head.headers.get_all("sec-websocket-key").iter().count(), 1);
            let decoded = STANDARD.decode(key.as_bytes()).expect("base64");
            assert_eq!(decoded.len(), 16);
        }
        None => assert_ne!(head.version, Version::HTTP_11),
    }

    for protocol in handshake.requested_protocols() {
        let protocol = protocol.as_bytes();
        assert!(!protocol.is_empty());
        assert!(!protocol.contains(&b','));
        assert_eq!(protocol, protocol.trim_ascii());
    }

    let origin = handshake.origin();
    if origin_allowed(origin, handshake.host(), &ALLOWED)
        && let Some(origin) = origin
    {
        let origin = origin.trim().trim_end_matches('/');
        let listed = origin.eq_ignore_ascii_case(ALLOWED[0]);
        let own = origin.split_once("://").is_some_and(|(_, authority)| {
            handshake
                .host()
                .is_some_and(|host| authority.eq_ignore_ascii_case(host.trim()))
        });
        assert!(listed || own, "{origin} passed");
    }
});
