//! The `email` rule's parser: an address as people type it in a form, in
//! the RFC 5321 form mail servers accept.
//!
//! - `local@domain`, with exactly one `@`, 254 bytes at most (the longest
//!   path RFC 5321 allows), ASCII only.
//! - The local part is a dot-atom of at most 64 bytes: letters, digits and
//!   ``!#$%&'*+-/=?^_`{|}~``, in runs separated by single dots (no dot at
//!   either end, no two in a row). Quoted local parts (`"a b"@x.com`) are
//!   refused: they are legal but almost always a typo or an attack.
//! - The domain is a host name of at most 253 bytes, labels of 1 to 63
//!   letters, digits and hyphens with no hyphen at either end and no
//!   trailing dot, whose last label isn't a number (all digits, or `0x`
//!   and hex digits: `a@127.0.0.1` is refused); or an address literal:
//!   `[192.0.2.1]` or `[IPv6:2001:db8::1]`.
//!   A single label (`admin@localhost`) is a host name too.
//! - Internationalized domains are accepted in their ASCII form
//!   (`user@xn--bcher-kva.de`); clients convert `bücher.de` before sending.

use std::net::{Ipv4Addr, Ipv6Addr};

/// The longest address: RFC 5321's 256-byte path less its angle brackets.
const MAX_ADDRESS: usize = 254;
const MAX_LOCAL: usize = 64;
const MAX_DOMAIN: usize = 253;
const MAX_LABEL: usize = 63;

/// Whether `text` is an email address (see the [module](self)).
pub fn is_email(text: &str) -> bool {
    if text.len() > MAX_ADDRESS || !text.is_ascii() {
        return false;
    }
    let Some((local, domain)) = text.split_once('@') else {
        return false;
    };
    is_local_part(local) && is_domain(domain)
}

/// A dot-atom of at most 64 bytes.
fn is_local_part(local: &str) -> bool {
    local.len() <= MAX_LOCAL && local.split('.').all(is_atom)
}

/// One or more `atext` characters.
fn is_atom(atom: &str) -> bool {
    !atom.is_empty() && atom.bytes().all(is_atext)
}

/// RFC 5322 `atext`.
fn is_atext(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"!#$%&'*+-/=?^_`{|}~".contains(&byte)
}

/// A host name or an address literal.
fn is_domain(domain: &str) -> bool {
    match domain.strip_prefix('[').and_then(|d| d.strip_suffix(']')) {
        Some(literal) => match literal.strip_prefix("IPv6:") {
            Some(v6) => v6.parse::<Ipv6Addr>().is_ok(),
            None => literal.parse::<Ipv4Addr>().is_ok(),
        },
        None => is_host_name(domain),
    }
}

/// Labels of letters, digits and hyphens; the last not a number.
pub(crate) fn is_host_name(host: &str) -> bool {
    if host.is_empty() || host.len() > MAX_DOMAIN {
        return false;
    }
    let mut last = "";
    for label in host.split('.') {
        if !is_label(label) {
            return false;
        }
        last = label;
    }
    !last.bytes().all(|byte| byte.is_ascii_digit()) && !is_hex_number(last)
}

/// `0x` and hex digits: URL parsers read such a last label as an IPv4
/// number, so the name is no host name to them.
fn is_hex_number(label: &str) -> bool {
    label
        .get(..2)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("0x"))
        && label[2..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// 1 to 63 letters, digits and hyphens, no hyphen at either end.
fn is_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= MAX_LABEL
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_people_use_pass() {
        for address in [
            "ada@example.com",
            "Ada.Lovelace@Example.COM",
            "first.last+tag@sub.example.co.uk",
            "!def!xyz%abc@example.com",
            "o'brien@example.ie",
            "user@xn--bcher-kva.de",
            "admin@localhost",
            "x@a.b",
            "email@[192.0.2.1]",
            "email@[IPv6:2001:db8::1]",
            "example@valid-----hyphens.com",
            "1234567890@example.com",
            "a@1.example.com",
            &format!("{}@example.com", "a".repeat(64)),
            &format!("a@{}.com", "b".repeat(63)),
        ] {
            assert!(is_email(address), "{address}");
        }
    }

    #[test]
    fn malformed_addresses_fail() {
        for address in [
            "",
            "ada",
            "ada@",
            "@example.com",
            "ada@@example.com",
            "a@b@example.com",
            "a b@example.com",
            "ada@exa mple.com",
            ".ada@example.com",
            "ada.@example.com",
            "a..b@example.com",
            "\"ada\"@example.com",
            "ada@example.com.",
            "ada@.example.com",
            "ada@example..com",
            "ada@-example.com",
            "ada@example-.com",
            "ada@exam_ple.com",
            "ada@127.0.0.1",
            "ada@example.123",
            "ada@[127.0.0.256]",
            "ada@[2001:db8::1]",
            "ada@[IPv6:2001:db8::12345]",
            "ada@[IPv6:127.0.0.1]",
            "ada@example.com\n",
            "ada\n@example.com",
            "ada@example.com<script>",
            "zoë@example.com",
            "ada@bücher.de",
            &format!("{}@example.com", "a".repeat(65)),
            &format!("a@{}.com", "b".repeat(64)),
            &format!("a@{}.com", vec!["b".repeat(60); 5].join(".")),
        ] {
            assert!(!is_email(address), "{address:?}");
        }
    }

    #[test]
    fn the_whole_address_is_at_most_254_bytes() {
        let domain = vec!["d".repeat(61); 4].join(".");
        assert_eq!(domain.len(), 247);
        let fits = format!("abcdef@{domain}");
        assert_eq!(fits.len(), 254);
        assert!(is_email(&fits));
        assert!(!is_email(&format!("abcdefg@{domain}")));
    }
}
