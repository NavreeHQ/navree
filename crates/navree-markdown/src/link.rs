//! What an address may be: the rules for a link and for a picture.
//!
//! An address is checked on the text the parser decoded, so `&#106;avascript:` is
//! read as the `javascript:` it is.

use crate::{Link, Trust};

/// The schemes a link may have.
const SCHEMES: [&str; 4] = ["http", "https", "mailto", "tel"];

/// The link for a destination, or `None` when its words should stay and the link go.
pub(crate) fn link(dest: &str, title: &str, trust: Trust) -> Option<Link> {
    let dest = dest.trim();
    if dest.is_empty() || !is_href(dest) {
        return None;
    }
    let off_site = leaves_the_site(dest);
    let untrusted = trust == Trust::Untrusted;
    Some(Link {
        href: dest.to_owned(),
        title: (!title.is_empty()).then(|| title.to_owned()),
        off_site,
        host: (off_site && untrusted).then(|| host_of(dest)).flatten(),
        nofollow: off_site && untrusted,
    })
}

/// The address of a picture in trusted text: a link that stays on this site.
pub(crate) fn image_source(dest: &str) -> Option<String> {
    let dest = dest.trim();
    if dest.is_empty() || !is_href(dest) || has_host(dest) || has_scheme(dest) {
        return None;
    }
    Some(dest.to_owned())
}

/// Whether an address is safe in `href`: no control character, no space at the start
/// (browsers strip those before reading the scheme, so `java\tscript:` would still
/// run), and a scheme, when there is one, from [`SCHEMES`].
fn is_href(url: &str) -> bool {
    if url.starts_with(' ') || url.bytes().any(|byte| byte < 0x20 || byte == 0x7f) {
        return false;
    }
    match scheme_of(url) {
        Some(scheme) => SCHEMES.iter().any(|name| name.eq_ignore_ascii_case(scheme)),
        None => true,
    }
}

/// The scheme of an address: what comes before the first `:`, when no `/`, `?` or `#`
/// comes sooner.
fn scheme_of(url: &str) -> Option<&str> {
    let end = url.find([':', '/', '?', '#'])?;
    if url.as_bytes().get(end) == Some(&b':') {
        url.get(..end)
    } else {
        None
    }
}

fn has_scheme(url: &str) -> bool {
    scheme_of(url).is_some()
}

/// Whether the address names a host without a scheme: `//host`, `/\host` and `\host`.
/// A browser reads a backslash as a slash in an address of this kind.
fn has_host(url: &str) -> bool {
    let bytes = url.as_bytes();
    let first = bytes.first().copied();
    let second = bytes.get(1).copied();
    let slash = |byte: Option<u8>| matches!(byte, Some(b'/' | b'\\'));
    first == Some(b'\\') || (slash(first) && slash(second))
}

/// Whether following the link leaves this site: it names a host, with a scheme or with
/// `//`, `/\` or `\\`. A `mailto:` or `tel:` link does not.
fn leaves_the_site(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("mailto:") || lower.starts_with("tel:") {
        return false;
    }
    has_host(url) || has_scheme(url)
}

/// The host a link goes to, for the reader to see, read the way a browser reads it: no
/// scheme, no user name or password, no port, no path, in lower case. A backslash is a
/// slash, and any run of slashes follows the scheme (`https:/\host` and `https:///host` go
/// to `host`). Without a scheme the host follows two slashes or more, as in `//host` and
/// `/\host`.
///
/// `None` when no host can be read: nothing after the scheme but a name, a single slash
/// before a name that has no scheme, or an empty host. A link that leaves the site and
/// has no host to show is still off-site.
fn host_of(url: &str) -> Option<String> {
    let (rest, slashes_needed) = match scheme_of(url) {
        Some(scheme) => (url.get(scheme.len() + 1..)?, 1),
        None => (url, 2),
    };
    let after = rest.trim_start_matches(['/', '\\']);
    if rest.len() - after.len() < slashes_needed {
        return None;
    }
    let authority = after.split(['/', '\\', '?', '#']).next()?;
    // The user name and password end at the last `@`.
    let at_host = authority.rsplit('@').next()?;
    let host = if at_host.starts_with('[') {
        // An address in brackets keeps its colons: `[::1]:8080` is `[::1]`.
        &at_host[..=at_host.find(']')?]
    } else {
        at_host.split(':').next()?
    };
    (!host.is_empty()).then(|| {
        host.chars()
            .take(80)
            .collect::<String>()
            .to_ascii_lowercase()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_four_schemes_make_a_link() {
        for url in [
            "https://navree.com",
            "HTTP://navree.com",
            "mailto:a@example.com",
            "tel:+919876543210",
            "/path",
            "../up",
            "?q=1",
            "#frag",
            "plain-name",
        ] {
            assert!(is_href(url), "{url}");
        }
        for url in [
            "javascript:alert(1)",
            "JavaScript:alert(1)",
            "vbscript:x",
            "data:text/html,x",
            "file:///etc/passwd",
            "blob:x",
            "unknown:x",
            "java\tscript:x",
            "java\nscript:x",
            " javascript:x",
            "\u{1}javascript:x",
            "/ok\u{7f}",
        ] {
            assert!(!is_href(url), "{url:?}");
        }
    }

    #[test]
    fn a_colon_after_a_slash_is_not_a_scheme() {
        assert!(is_href("/a:b"));
        assert!(is_href("?a:b"));
        assert!(is_href("#a:b"));
        assert!(!leaves_the_site("/a:b"));
    }

    #[test]
    fn what_leaves_the_site() {
        for url in [
            "https://a.example",
            "//a.example",
            "/\\a.example",
            "\\\\a.example",
            "\\a.example",
            "HTTP://a.example",
        ] {
            assert!(leaves_the_site(url), "{url}");
        }
        for url in [
            "/a/b",
            "a/b",
            "../a",
            "?q",
            "#f",
            "mailto:a@example.com",
            "MAILTO:a@example.com",
            "tel:+91",
        ] {
            assert!(!leaves_the_site(url), "{url}");
        }
    }

    #[test]
    fn the_host_is_read_the_way_a_browser_reads_it() {
        for (url, host) in [
            ("https://evil.example/x", "evil.example"),
            ("https:/\\evil.example/x", "evil.example"),
            ("https:\\\\evil.example/x", "evil.example"),
            ("https:\\/evil.example", "evil.example"),
            ("https:///evil.example", "evil.example"),
            ("https:////evil.example/a?b#c", "evil.example"),
            ("HTTPS:/\\EVIL.Example", "evil.example"),
            ("https://user:pass@evil.example:8443/x", "evil.example"),
            ("https://good.example@evil.example", "evil.example"),
            ("https://evil.example:80@good.example/", "good.example"),
            ("https://evil.example\\@good.example/", "evil.example"),
            ("https://evil.example?x=1", "evil.example"),
            ("https://evil.example#frag", "evil.example"),
            ("https://[::1]:8080/x", "[::1]"),
            ("//evil.example/x", "evil.example"),
            ("/\\evil.example", "evil.example"),
            ("\\\\evil.example", "evil.example"),
            ("\\/evil.example", "evil.example"),
        ] {
            assert_eq!(host_of(url).as_deref(), Some(host), "{url}");
            assert!(leaves_the_site(url), "{url}");
        }
    }

    #[test]
    fn a_link_with_no_readable_host_is_still_off_site_and_shows_none() {
        for url in [
            "https:evil.example",
            "\\evil.example",
            "https://",
            "https:///",
            "https://@",
            "https://:80/x",
            "https://[::1/x",
        ] {
            assert_eq!(host_of(url), None, "{url}");
        }
        for url in ["https:evil.example", "\\evil.example", "https://"] {
            let found = link(url, "", Trust::Untrusted).expect("a link");
            assert!(found.off_site, "{url}");
            assert!(found.nofollow, "{url}");
            assert_eq!(found.host, None, "{url}");
        }
    }

    #[test]
    fn a_picture_comes_from_a_path_on_this_site_and_nowhere_else() {
        assert_eq!(image_source(" /a.png "), Some("/a.png".to_owned()));
        assert_eq!(image_source("a.png"), Some("a.png".to_owned()));
        for url in [
            "",
            "https://a.example/x.png",
            "//a.example/x.png",
            "/\\a.example/x.png",
            "data:image/png;base64,AA",
            "javascript:x",
            "mailto:a@example.com",
        ] {
            assert_eq!(image_source(url), None, "{url}");
        }
    }
}
