//! Host names: what a custom domain may be, and the form it is kept in.

use http::header::HOST;
use http::request::Parts;

/// The normal form of `input` as a custom domain: lower-case ASCII (an
/// international name in its `xn--` form), without a trailing dot, at
/// least two labels of letters, digits and inner hyphens, at most 253
/// characters. URLs, ports, paths, wildcards and IP addresses are
/// refused; the error says why.
///
/// ```
/// use navree_domains::normalize_host;
///
/// assert_eq!(normalize_host(" CRM.Acme-Corp.com. ").unwrap(), "crm.acme-corp.com");
/// assert_eq!(normalize_host("bücher.example").unwrap(), "xn--bcher-kva.example");
/// assert!(normalize_host("https://crm.acme-corp.com").is_err());
/// assert!(normalize_host("*.acme-corp.com").is_err());
/// ```
pub fn normalize_host(input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("the domain is empty".to_owned());
    }
    if trimmed.contains("://") {
        return Err("give the domain alone, without `https://`".to_owned());
    }
    if trimmed.contains(['/', '?', '#', '@']) {
        return Err("give the domain alone, without a path".to_owned());
    }
    if trimmed.contains('*') {
        return Err("wildcard domains are not supported".to_owned());
    }
    if trimmed.contains(':') || trimmed.starts_with('[') {
        return Err("give the domain without a port".to_owned());
    }
    if trimmed.contains(char::is_whitespace) {
        return Err("a domain has no spaces".to_owned());
    }
    let without_dot = trimmed.strip_suffix('.').unwrap_or(trimmed);
    if without_dot.parse::<std::net::IpAddr>().is_ok() {
        return Err("give a domain name, not an IP address".to_owned());
    }
    if without_dot.is_ascii() {
        // The precise reason, before the IDNA processing.
        check_labels(&without_dot.to_ascii_lowercase())?;
    }
    let host = match url::Host::parse(without_dot) {
        Ok(url::Host::Domain(domain)) => domain,
        Ok(url::Host::Ipv4(_) | url::Host::Ipv6(_)) => {
            return Err("give a domain name, not an IP address".to_owned());
        }
        Err(_) => return Err("this is not a valid domain name".to_owned()),
    };
    check_labels(&host)?;
    Ok(host)
}

/// Letters, digits and inner hyphens in each label; two labels at least; a
/// top-level label that is not all digits.
fn check_labels(host: &str) -> Result<(), String> {
    if host.len() > 253 {
        return Err("a domain is at most 253 characters".to_owned());
    }
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 {
        return Err("give a full domain such as `crm.acme-corp.com`".to_owned());
    }
    for label in &labels {
        let valid = !label.is_empty()
            && label.len() <= 63
            && label
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            && !label.starts_with('-')
            && !label.ends_with('-');
        if !valid {
            return Err(format!(
                "`{label}` is not a valid part of a domain (letters, digits and inner hyphens, at most 63)"
            ));
        }
    }
    if labels
        .last()
        .is_some_and(|tld| tld.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err("this is not a valid domain name".to_owned());
    }
    Ok(())
}

/// Whether `host` is `zone` or one of its subdomains (both normalized).
pub(crate) fn is_within(host: &str, zone: &str) -> bool {
    host == zone
        || host
            .strip_suffix(zone)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

/// The host a request was sent to: the `Host` header, else the URI's
/// authority; lower-case, without the port or a trailing dot.
pub fn request_host(parts: &Parts) -> Option<String> {
    let raw = parts
        .headers
        .get(HOST)
        .and_then(|value| value.to_str().ok())
        .or_else(|| parts.uri.authority().map(|authority| authority.as_str()))?;
    host_without_port(raw)
}

/// `raw` (a `Host` value) lower-cased, without its port or trailing dot;
/// `None` for IP literals, empty values and values that are no host name:
/// a second trailing dot (`example.com..`) or a `:` that is no port
/// separator (`a:b`).
pub(crate) fn host_without_port(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.starts_with('[') {
        return None;
    }
    let name = match raw.rsplit_once(':') {
        Some((name, port)) if port.bytes().all(|b| b.is_ascii_digit()) => name,
        _ => raw,
    };
    let name = name.strip_suffix('.').unwrap_or(name).to_ascii_lowercase();
    (!name.is_empty()
        && !name.ends_with('.')
        && !name.contains(':')
        && name.parse::<std::net::IpAddr>().is_err())
    .then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_are_kept_lower_case_without_a_trailing_dot() {
        assert_eq!(
            normalize_host("crm.acme-corp.com").unwrap(),
            "crm.acme-corp.com"
        );
        assert_eq!(
            normalize_host("CRM.ACME-corp.COM.").unwrap(),
            "crm.acme-corp.com"
        );
        assert_eq!(normalize_host("acme.co.uk").unwrap(), "acme.co.uk");
        assert_eq!(
            normalize_host("xn--bcher-kva.example").unwrap(),
            "xn--bcher-kva.example"
        );
        assert_eq!(
            normalize_host("Bücher.Example").unwrap(),
            "xn--bcher-kva.example"
        );
    }

    #[test]
    fn urls_ports_wildcards_and_addresses_are_refused() {
        for (input, reason) in [
            ("", "empty"),
            ("https://crm.acme-corp.com", "https://"),
            ("crm.acme-corp.com/login", "path"),
            ("crm.acme-corp.com:8443", "port"),
            ("*.acme-corp.com", "wildcard"),
            ("192.0.2.10", "IP address"),
            ("[2001:db8::1]", "port"),
            ("localhost", "full domain"),
            ("crm acme.com", "spaces"),
            ("-crm.acme.com", "valid part"),
            ("crm-.acme.com", "valid part"),
            ("crm..acme.com", "valid"),
            ("under_score.acme.com", "valid"),
            ("acme.123", "valid domain"),
        ] {
            let error = normalize_host(input).unwrap_err();
            assert!(error.contains(reason), "{input}: {error}");
        }
        let long = format!("{}.com", "a".repeat(64));
        assert!(normalize_host(&long).unwrap_err().contains("at most 63"));
        let too_long = format!("{}com", "abcdefghi.".repeat(26));
        assert!(normalize_host(&too_long).unwrap_err().contains("253"));
    }

    #[test]
    fn zones_contain_themselves_and_their_subdomains() {
        assert!(is_within("example.com", "example.com"));
        assert!(is_within("app.example.com", "example.com"));
        assert!(is_within("a.b.example.com", "example.com"));
        assert!(!is_within("badexample.com", "example.com"));
        assert!(!is_within("example.com.evil.test", "example.com"));
    }

    #[test]
    fn the_request_host_comes_without_its_port() {
        let parts = |host: Option<&str>, uri: &str| {
            let mut builder = http::Request::builder().uri(uri);
            if let Some(host) = host {
                builder = builder.header("host", host);
            }
            builder.body(()).unwrap().into_parts().0
        };
        assert_eq!(
            request_host(&parts(Some("CRM.Acme-Corp.com:8443"), "/")).as_deref(),
            Some("crm.acme-corp.com")
        );
        assert_eq!(
            request_host(&parts(Some("crm.acme-corp.com."), "/")).as_deref(),
            Some("crm.acme-corp.com")
        );
        assert_eq!(
            request_host(&parts(None, "https://shop.acme.test/x")).as_deref(),
            Some("shop.acme.test")
        );
        // Found by fuzzing: a second dot or a stray colon is no host name.
        for malformed in [
            "ez..",
            "crm.acme-corp.com..",
            ".",
            "..",
            "a:b",
            "a:b:8080",
            "a.b:c",
        ] {
            assert_eq!(
                request_host(&parts(Some(malformed), "/")),
                None,
                "{malformed}"
            );
        }
        assert_eq!(request_host(&parts(Some("127.0.0.1:8080"), "/")), None);
        assert_eq!(request_host(&parts(Some("[::1]:8080"), "/")), None);
        assert_eq!(request_host(&parts(None, "/")), None);
    }
}
