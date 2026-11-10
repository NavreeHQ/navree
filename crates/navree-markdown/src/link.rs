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
        host: (off_site && untrusted)
            .then(|| host_of(dest))
            .filter(|host| !host.is_empty()),
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

/// The host a link goes to, for the reader to see: no scheme, no user name, no path.
fn host_of(url: &str) -> String {
    let after = match url.split_once("://") {
        Some((_, rest)) => rest,
        None => url.trim_start_matches(['/', '\\']),
    };
    let authority = after
        .split(['/', '\\', '?', '#'])
        .next()
        .unwrap_or_default();
    let host = authority.rsplit('@').next().unwrap_or_default();
    host.chars()
        .take(80)
        .collect::<String>()
        .to_ascii_lowercase()
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
