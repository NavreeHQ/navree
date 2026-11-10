//! The `url` rule's parser: an absolute URI as RFC 3986 writes it,
//! `scheme:[//authority]path[?query][#fragment]`, in ASCII.
//!
//! - The scheme is a letter, then letters, digits, `+`, `-` and `.`.
//! - Every other character is unreserved, a sub-delimiter, one the part
//!   allows (`:` and `@` in paths, `/` and `?` in queries and fragments)
//!   or a `%` followed by two hex digits. Spaces, quotes, `<`, `>`, `\`,
//!   `{`, `}`, `|`, `^`, `` ` ``, control characters and non-ASCII text
//!   fail: clients percent-encode them.
//! - An authority is `[userinfo@]host[:port]`, the port 0 to 65535 when it
//!   is given (`http://example.com:` fails).
//! - For the web's schemes (`http`, `https`, `ws`, `wss`, `ftp`, `file`)
//!   the host is a host name (labels of letters, digits and hyphens, the
//!   last not a number: all digits, or `0x` and hex digits), an IPv4
//!   address or a bracketed IPv6 address, and it is required except for
//!   `file` (`file:///etc/hosts`), which takes no credentials or port. Other
//!   schemes take any RFC 3986 host (`urn:isbn:0451450523`,
//!   `mailto:anu@example.com` and `custom://anything` pass).
//! - Internationalized host names are accepted in their ASCII form
//!   (`https://xn--bcher-kva.de`).

use std::net::{Ipv4Addr, Ipv6Addr};

use super::email::is_host_name;

/// Schemes whose host is a host name or an IP address.
const WEB_SCHEMES: &[&str] = &["http", "https", "ws", "wss", "ftp", "file"];

/// Whether `text` is an absolute URL (see the module).
pub fn is_url(text: &str) -> bool {
    if !text.is_ascii() {
        return false;
    }
    let Some((scheme, rest)) = text.split_once(':') else {
        return false;
    };
    if !is_scheme(scheme) {
        return false;
    }
    let (rest, fragment) = match rest.split_once('#') {
        Some((rest, fragment)) => (rest, Some(fragment)),
        None => (rest, None),
    };
    let (hier, query) = match rest.split_once('?') {
        Some((hier, query)) => (hier, Some(query)),
        None => (rest, None),
    };
    if !fragment.is_none_or(is_query) || !query.is_none_or(is_query) {
        return false;
    }
    let web = WEB_SCHEMES
        .iter()
        .any(|web| web.eq_ignore_ascii_case(scheme));
    match hier.strip_prefix("//") {
        Some(rest) => {
            let (authority, path) = match rest.find('/') {
                Some(slash) => rest.split_at(slash),
                None => (rest, ""),
            };
            is_authority(authority, web, scheme.eq_ignore_ascii_case("file")) && is_path(path)
        }
        // A web URL names its host (`http:/x` and `http:x` fail), `file`
        // aside.
        None if web && !scheme.eq_ignore_ascii_case("file") => false,
        None => is_path(hier),
    }
}

/// A letter, then letters, digits, `+`, `-` and `.`.
fn is_scheme(scheme: &str) -> bool {
    let mut bytes = scheme.bytes();
    bytes
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
}

/// `[userinfo@]host[:port]`.
fn is_authority(authority: &str, web: bool, file: bool) -> bool {
    let (userinfo, host_port) = match authority.rsplit_once('@') {
        Some((userinfo, host_port)) => (Some(userinfo), host_port),
        None => (None, authority),
    };
    if !userinfo.is_none_or(|userinfo| all_allowed(userinfo, b":")) {
        return false;
    }
    let (host, port) = split_port(host_port);
    if !port.is_none_or(is_port) {
        return false;
    }
    // Credentials or a port for no host (`a://@`, `a://:80`) name nothing, and
    // a `file` URL has neither.
    if (host.is_empty() || file) && (userinfo.is_some() || port.is_some()) {
        return false;
    }
    if let Some(literal) = host.strip_prefix('[') {
        return literal
            .strip_suffix(']')
            .is_some_and(|v6| v6.parse::<Ipv6Addr>().is_ok());
    }
    if web {
        if host.is_empty() {
            return file;
        }
        if host
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.')
        {
            return host.parse::<Ipv4Addr>().is_ok();
        }
        return is_host_name(host);
    }
    all_allowed(host, b"")
}

/// The host and the port after its last `:` (outside an IPv6 literal).
fn split_port(host_port: &str) -> (&str, Option<&str>) {
    let colon = match host_port.rfind(']') {
        Some(end) => host_port[end..].find(':').map(|i| end + i),
        None => host_port.rfind(':'),
    };
    match colon {
        Some(colon) => (&host_port[..colon], Some(&host_port[colon + 1..])),
        None => (host_port, None),
    }
}

/// 1 to 5 digits, at most 65535.
fn is_port(port: &str) -> bool {
    !port.is_empty()
        && port.len() <= 5
        && port.bytes().all(|byte| byte.is_ascii_digit())
        && port.parse::<u16>().is_ok()
}

/// Path characters and `/`.
fn is_path(path: &str) -> bool {
    all_allowed(path, b":@/")
}

/// Path characters, `/` and `?`.
fn is_query(query: &str) -> bool {
    all_allowed(query, b":@/?")
}

/// Every character is unreserved, a sub-delimiter, one of `extra`, or a
/// percent-encoded byte.
fn all_allowed(text: &str, extra: &[u8]) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let byte = bytes[i];
        if byte == b'%' {
            let encoded = bytes.get(i + 1..i + 3);
            if !encoded.is_some_and(|hex| hex.iter().all(u8::is_ascii_hexdigit)) {
                return false;
            }
            i += 3;
            continue;
        }
        let allowed = byte.is_ascii_alphanumeric()
            || b"-._~".contains(&byte)
            || b"!$&'()*+,;=".contains(&byte)
            || extra.contains(&byte);
        if !allowed {
            return false;
        }
        i += 1;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_pass() {
        for url in [
            "https://example.com",
            "https://example.com/",
            "HTTPS://Example.COM/a/b?c=d&e=f#top",
            "http://localhost:8080/health",
            "http://user:pass@example.com:443/path;x=1",
            "http://192.0.2.1/",
            "http://[2001:db8::1]:8080/",
            "https://xn--bcher-kva.de/b%C3%BCcher",
            "https://example.com/?q=a/b?c#frag/ment?",
            "https://example.com/~anu/(1)/it's,ok;*+=!$&",
            "ftp://ftp.example.com/pub",
            "ws://example.com/socket",
            "file:///etc/hosts",
            "file://localhost/etc/hosts",
            "https://0x.example.com",
            "https://example.0xg",
            "file://server/share",
            "mailto:anu@example.com",
            "urn:isbn:0451450523",
            "tel:+44-20-7946-0000",
            "data:text/plain,hello",
            "custom+scheme.v1://any.thing_here/x",
            "a:",
        ] {
            assert!(is_url(url), "{url}");
        }
    }

    #[test]
    fn malformed_urls_fail() {
        for url in [
            "",
            "example.com",
            "//example.com",
            "/relative/path",
            "1http://example.com",
            "ht tp://example.com",
            "http://",
            "http:///path",
            "http:/example.com",
            "http:example.com",
            "https://exa mple.com",
            "https://example.com/a b",
            "https://example.com/<script>",
            "https://example.com/\\",
            "https://example.com/{x}",
            "https://example.com/%",
            "https://example.com/%zz",
            "https://example.com:",
            "https://example.com:65536",
            "https://example.com:-1",
            "https://example.com:8o",
            "https://exam_ple.com",
            "https://-example.com",
            "https://example.com./",
            "http://999.0.0.1/",
            "http://1.2.3/",
            "http://[2001:db8::12345]/",
            "http://[192.0.2.1]/",
            "https://example.0xa",
            "https://example.0Xa/path",
            "https://example.0x",
            "http://0x7f000001/",
            "http://user@:80/",
            "https://bücher.de",
            "https://example.com/\n",
            "https://example.com#a#b",
            "file://user@/x",
            "file://user@host/x",
            "file://a+@a/p?",
            "file://host:80/x",
            "a://@?",
            "a://:80/",
        ] {
            assert!(!is_url(url), "{url:?}");
        }
    }
}
