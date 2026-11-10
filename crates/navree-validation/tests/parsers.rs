//! Property tests of the parsers navree-validation owns: the `email` and
//! `url` rules. Addresses and URLs built from the grammar pass; anything
//! that passes has the shape the grammar promises; and every URL that
//! passes also parses with the `url` crate, an independent parser.

use navree_validation::rules::{is_email, is_url};
use proptest::prelude::*;

/// A dot-atom of RFC 5322 `atext`.
fn local_part() -> impl Strategy<Value = String> {
    prop::collection::vec("[A-Za-z0-9!#$%&'*+/=?^_`{|}~-]{1,12}", 1..4)
        .prop_map(|atoms| atoms.join("."))
}

/// Host-name labels, the last with a letter.
fn host_name() -> impl Strategy<Value = String> {
    (
        prop::collection::vec("[a-z0-9]([a-z0-9-]{0,8}[a-z0-9])?", 0..3),
        "[a-z]([a-z0-9-]{0,8}[a-z0-9])?",
    )
        .prop_map(|(mut labels, last)| {
            labels.push(last);
            labels.join(".")
        })
}

/// Characters a path, query or fragment may hold besides `/` and `?`.
const PCHAR: &str = "([A-Za-z0-9._~!$&'()*+,;=:@-]|%[0-9A-Fa-f]{2})";

/// Text matching `pattern`.
fn matching(pattern: &str) -> impl Strategy<Value = String> + use<> {
    proptest::string::string_regex(pattern).expect("a valid pattern")
}

fn web_url() -> impl Strategy<Value = String> {
    (
        prop::sample::select(vec!["http", "https", "ws", "wss", "ftp", "HTTPS"]),
        prop::option::of(matching("[a-z0-9]{1,8}(:[a-z0-9]{0,8})?")),
        host_name(),
        prop::option::of(0u32..=65535),
        prop::collection::vec(matching(&format!("{PCHAR}{{0,6}}")), 0..4),
        prop::option::of(matching(&format!("({PCHAR}|[/?]){{0,10}}"))),
        prop::option::of(matching(&format!("({PCHAR}|[/?]){{0,10}}"))),
    )
        .prop_map(|(scheme, userinfo, host, port, path, query, fragment)| {
            let mut url = format!("{scheme}://");
            if let Some(userinfo) = userinfo {
                url.push_str(&userinfo);
                url.push('@');
            }
            url.push_str(&host);
            if let Some(port) = port {
                url.push_str(&format!(":{port}"));
            }
            for segment in path {
                url.push('/');
                url.push_str(&segment);
            }
            if let Some(query) = query {
                url.push('?');
                url.push_str(&query);
            }
            if let Some(fragment) = fragment {
                url.push('#');
                url.push_str(&fragment);
            }
            url
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2_000))]

    #[test]
    fn grammar_addresses_pass(local in local_part(), domain in host_name()) {
        let address = format!("{local}@{domain}");
        prop_assume!(local.len() <= 64 && address.len() <= 254);
        prop_assert!(is_email(&address), "{}", address);
    }

    #[test]
    fn a_forbidden_character_in_the_local_part_fails(
        local in local_part(),
        domain in host_name(),
        at in 0usize..64,
        bad in prop::sample::select(vec![' ', '"', '(', ')', ',', ':', ';', '<', '>', '@', '[', '\\', ']', '\n', '\u{0}', 'é']),
    ) {
        let at = at.min(local.len());
        let local = format!("{}{bad}{}", &local[..at], &local[at..]);
        let address = format!("{local}@{domain}");
        prop_assert!(!is_email(&address), "{}", address);
    }

    #[test]
    fn whatever_passes_as_an_email_is_well_formed(text in "[a-z0-9.@\\[\\]:_ -]{0,40}|.{0,40}") {
        if is_email(&text) {
            prop_assert!(text.is_ascii() && text.len() <= 254);
            let (local, domain) = text.split_once('@').unwrap();
            prop_assert!(!domain.contains('@'));
            prop_assert!(!local.is_empty() && local.len() <= 64);
            prop_assert!(!local.starts_with('.') && !local.ends_with('.') && !local.contains(".."));
            prop_assert!(!domain.is_empty() && !domain.ends_with('.'));
            prop_assert!(!text.contains(' ') && !text.contains('\n'));
        }
    }

    #[test]
    fn grammar_urls_pass_and_parse_with_the_url_crate(url in web_url()) {
        prop_assert!(is_url(&url), "{}", url);
        // The `url` crate also decodes punycode labels (`xn--`), which
        // this rule leaves to the host.
        if !url.to_ascii_lowercase().contains("xn--") {
            prop_assert!(url::Url::parse(&url).is_ok(), "{}", url);
        }
    }

    #[test]
    fn whatever_passes_as_a_url_parses_with_the_url_crate(
        text in "[a-zA-Z][a-zA-Z0-9+.-]{0,5}:(//)?[a-zA-Z0-9.:@%/?#!$&'()*+,;=_~\\[\\] -]{0,40}"
    ) {
        // The `url` crate also decodes punycode labels (`xn--`), which
        // this rule leaves to the host.
        if is_url(&text) && !text.to_ascii_lowercase().contains("xn--") {
            prop_assert!(url::Url::parse(&text).is_ok(), "{}", text);
            prop_assert!(text.is_ascii() && !text.contains(' '));
        }
    }

    #[test]
    fn the_parsers_never_panic(text in ".{0,300}") {
        let _ = is_email(&text);
        let _ = is_url(&text);
    }
}
