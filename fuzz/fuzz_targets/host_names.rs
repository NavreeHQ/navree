//! Host and domain names from users and from requests:
//! `navree_auth::email_domain` (sign-in policies and SSO domains) and
//! `navree_domains` (`normalize_host` for custom domains, `request_host`
//! for the `Host` header).
//!
//! Invariants: a normalized host is lower-case ASCII with valid labels and
//! normalizes to itself; an email domain is its own canonical spelling
//! (reading it again changes nothing) and never ends with a dot.
#![no_main]

use http::{Request, header::HOST};
use libfuzzer_sys::fuzz_target;
use navree_auth::email_domain;
use navree_domains::{normalize_host, request_host};

fn check_host(host: &str) {
    assert!(host.is_ascii(), "{host:?} is not ASCII");
    assert_eq!(host, host.to_ascii_lowercase());
    assert!(host.len() <= 253);
    assert!(!host.ends_with('.'));
    let labels: Vec<&str> = host.split('.').collect();
    assert!(labels.len() >= 2, "{host:?} has one label");
    for label in labels {
        assert!(!label.is_empty() && label.len() <= 63, "{host:?}");
        assert!(
            label
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
            "{host:?}"
        );
        assert!(!label.starts_with('-') && !label.ends_with('-'), "{host:?}");
    }
}

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        if let Ok(host) = normalize_host(text) {
            check_host(&host);
            assert_eq!(
                normalize_host(&host).as_deref(),
                Ok(host.as_str()),
                "normalizing twice changes the host"
            );
        }

        if let Some(domain) = email_domain(text) {
            assert!(!domain.is_empty() && !domain.ends_with('.'));
            // The domain of an address built from it is itself.
            let again = email_domain(&format!("someone@{domain}"));
            assert_eq!(
                again.as_deref(),
                Some(domain.as_str()),
                "the email domain is not canonical (from {text:?})"
            );
            if let Ok(host) = normalize_host(&domain) {
                check_host(&host);
            }
        }
    }

    // The `Host` header, byte for byte.
    if let Ok(request) = Request::builder()
        .header(HOST, data)
        .body(())
    {
        let (parts, ()) = request.into_parts();
        if let Some(host) = request_host(&parts) {
            assert!(!host.is_empty() && !host.ends_with('.'));
            assert!(!host.contains(':'));
            assert_eq!(host, host.to_ascii_lowercase());
            assert!(!host.starts_with('['));
        }
    }
});
