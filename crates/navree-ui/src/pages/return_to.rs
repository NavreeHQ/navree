//! [`ReturnTo`]: where to send a user after they sign in, checked.
//!
//! A sign-in link carries the page the user was on (`/sign-in?next=/deals/42`), and
//! the sign-in handler sends them back there. A `next` that comes from a request is
//! the classic open redirect, so this type accepts only an address on this site
//! that no browser can read as another: one path, starting with a single slash.

use std::fmt;

use http::Uri;

use crate::html::{Href, LocalHref};

/// The most bytes an address may have.
pub const MOST: usize = 2048;

/// An address on this site that is safe to send a user to.
///
/// It is a path with an optional query and fragment, written the way a browser sends
/// it: ASCII only, anything else percent-encoded. It starts with one `/` that is
/// not followed by another `/` or by `\`, has no control character, space or `\`,
/// no dot segment, and says none of that either once its `%xx` escapes are read.
///
/// ```
/// use navree_ui::pages::ReturnTo;
///
/// assert!(ReturnTo::parse("/deals/42?tab=notes").is_some());
/// assert!(ReturnTo::parse("//evil.example").is_none());
/// assert!(ReturnTo::parse("/\\evil.example").is_none());
/// assert!(ReturnTo::parse("https://evil.example").is_none());
/// assert_eq!(ReturnTo::resolve(Some("//evil.example"), "/").as_str(), "/");
/// ```
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ReturnTo(String);

impl ReturnTo {
    /// Checks `raw`. `None` when it could lead anywhere but this site.
    pub fn parse(raw: &str) -> Option<Self> {
        safe(raw).then(|| Self(raw.to_owned()))
    }

    /// Checks `raw` and also refuses the sign-in page itself, and anything below it, which
    /// would send the user round in a circle. `sign_in` is its address; the two are compared
    /// as a server reads them, with their escapes decoded (`/%73ign-in` is `/sign-in`). A
    /// `sign_in` that is not a path on this site (a provider's address) refuses nothing.
    pub fn parse_for(raw: &str, sign_in: &str) -> Option<Self> {
        let found = Self::parse(raw)?;
        (!is_at_or_below(path_of(raw), path_of(sign_in))).then_some(found)
    }

    /// The page a request asked for: its path and query. `None` when that could not be
    /// sent back safely, such as a path with a backslash in it.
    pub fn of_uri(uri: &Uri) -> Option<Self> {
        let target = uri.path_and_query().map_or_else(|| "/", |pq| pq.as_str());
        Self::parse(target)
    }

    /// The `next` of a query string (`a=1&next=%2Fdeals`), checked.
    pub fn from_query(query: &str) -> Option<Self> {
        let value = query.split('&').find_map(|pair| {
            let (name, value) = pair.split_once('=')?;
            (name == "next").then_some(value)
        })?;
        Self::parse(&decode_once(value)?)
    }

    /// Where to go after signing in: `next` when it is safe, else `fallback`, which
    /// is yours and must also be on this site (anything else becomes `#`).
    pub fn resolve(next: Option<&str>, fallback: &str) -> LocalHref {
        match next.and_then(Self::parse) {
            Some(found) => LocalHref::from(found.0),
            None => LocalHref::from(fallback),
        }
    }

    /// [`resolve`](Self::resolve) that also drops the sign-in page at `sign_in` (see
    /// [`parse_for`](Self::parse_for)): what a sign-in handler should use.
    ///
    /// ```
    /// use navree_ui::pages::ReturnTo;
    ///
    /// let after = ReturnTo::resolve_for(Some("/sign-in?next=/x"), "/sign-in", "/");
    /// assert_eq!(after.as_str(), "/");
    /// let after = ReturnTo::resolve_for(Some("/deals/42"), "/sign-in", "/");
    /// assert_eq!(after.as_str(), "/deals/42");
    /// ```
    pub fn resolve_for(next: Option<&str>, sign_in: &str, fallback: &str) -> LocalHref {
        match next.and_then(|raw| Self::parse_for(raw, sign_in)) {
            Some(found) => LocalHref::from(found.0),
            None => LocalHref::from(fallback),
        }
    }

    /// The address.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The address as a redirect target.
    pub fn into_local(self) -> LocalHref {
        LocalHref::from(self.0)
    }

    /// `sign_in` with this address as its `next`: `/sign-in?next=%2Fdeals%2F42`.
    pub fn sign_in_href(&self, sign_in: &str) -> Href {
        let mut out = String::with_capacity(sign_in.len() + self.0.len() * 3 + 8);
        out.push_str(sign_in);
        out.push(if sign_in.contains('?') { '&' } else { '?' });
        out.push_str("next=");
        for byte in self.0.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    out.push(char::from(byte));
                }
                _ => {
                    out.push('%');
                    out.push(char::from(b"0123456789ABCDEF"[usize::from(byte >> 4)]));
                    out.push(char::from(b"0123456789ABCDEF"[usize::from(byte & 15)]));
                }
            }
        }
        Href::from(out)
    }
}

impl fmt::Debug for ReturnTo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ReturnTo").field(&self.0).finish()
    }
}

impl From<ReturnTo> for LocalHref {
    fn from(value: ReturnTo) -> Self {
        value.into_local()
    }
}

/// The path of an address, without its query or fragment: cut at the first `?` or `#` as
/// written, so one that is escaped (`%3F`) stays in the path, as a browser reads it.
fn path_of(address: &str) -> &str {
    address.split(['?', '#']).next().unwrap_or(address)
}

/// Whether `path` is the page at `page`, or below it, read as a server reads them. `page` that
/// is not a path of this site is no page of it.
pub(crate) fn is_at_or_below(path: &str, page: &str) -> bool {
    if !page.starts_with('/') {
        return false;
    }
    let read = |text: &str| {
        let text = path_of(text);
        decode_once(text).unwrap_or_else(|| text.to_owned())
    };
    let (path, page) = (read(path), read(page));
    let page = page.trim_end_matches('/');
    if page.is_empty() {
        return path.trim_end_matches('/').is_empty();
    }
    path.trim_end_matches('/') == page
        || path
            .strip_prefix(page)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// Whether the path climbs: a `.` or `..` piece once its escapes are read, once and twice, so
/// `%2e%2e`, `%2f..` and a doubly escaped dot count. A path of two slashes is what a climb
/// can end in.
fn climbs(path: &str) -> bool {
    let Some(once) = decode_once(path) else {
        return true;
    };
    let twice = decode_once(&once);
    [Some(once), twice]
        .into_iter()
        .flatten()
        .any(|text| text.split('/').any(|piece| piece == "." || piece == ".."))
}

/// Whether `raw` is an address on this site that no browser reads as another.
fn safe(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    if bytes.is_empty() || bytes.len() > MOST || bytes[0] != b'/' {
        return false;
    }
    if matches!(bytes.get(1), Some(b'/' | b'\\')) {
        return false;
    }
    // Printable ASCII only, and no backslash: what a browser sends, so nothing is lost.
    if bytes
        .iter()
        .any(|&b| !(0x21..=0x7e).contains(&b) || b == b'\\')
    {
        return false;
    }
    let Some(decoded) = decode(raw) else {
        return false;
    };
    let decoded = decoded.as_bytes();
    // What the escapes say must be as plain as what they hide.
    if decoded.iter().any(|&b| b < 0x20 || b == 0x7f || b == b'\\') {
        return false;
    }
    if decoded.starts_with(b"//") {
        return false;
    }
    // A dot segment can climb to a path that starts with two slashes once it is resolved. The
    // path is cut at the `?` as written, before any escape is read: an escaped `?` is text in a
    // segment, and the dot segments after it still count.
    !climbs(path_of(raw))
}

/// `raw` with its `%xx` escapes read as UTF-8; `None` for a broken escape or bytes that
/// are not text. Done twice so a doubly encoded slash is seen too.
fn decode(raw: &str) -> Option<String> {
    let once = decode_once(raw)?;
    let twice = decode_once(&once).unwrap_or_else(|| once.clone());
    // The strictest of the two readings is the one that counts.
    if twice.bytes().any(|b| b < 0x20 || b == 0x7f || b == b'\\') || twice.starts_with("//") {
        return Some(twice);
    }
    Some(once)
}

fn decode_once(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' {
            let high = hex(*bytes.get(at + 1)?)?;
            let low = hex(*bytes.get(at + 2)?)?;
            out.push(high << 4 | low);
            at += 3;
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8(out).ok()
}

const fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REFUSED: &[&str] = &[
        "",
        "deals",
        "?next=/",
        "#top",
        "//evil.example",
        "///evil.example",
        "////evil.example",
        "/\\evil.example",
        "\\\\evil.example",
        "\\/evil.example",
        "\\evil.example",
        "/\\/evil.example",
        "https://evil.example",
        "HTTPS://evil.example",
        "http:evil.example",
        "javascript:alert(1)",
        "JaVaScRiPt:alert(1)",
        "data:text/html,x",
        "vbscript:x",
        " //evil.example",
        " /deals",
        "/ deals",
        "/deals ",
        "/\t/evil.example",
        "/\n/evil.example",
        "/\r\n/evil.example",
        "/%09/evil.example",
        "/%0a/evil.example",
        "/%0d%0aSet-Cookie:%20x=1",
        "/%2f/evil.example",
        "/%2F/evil.example",
        "/%5cevil.example",
        "/%5Cevil.example",
        "/%252f/evil.example",
        "/%255cevil.example",
        "/%2e%2e/evil",
        "/%2E/evil",
        "/.//evil.example",
        "/./evil",
        "/../evil",
        "/a/../../evil",
        "/a/./b",
        "/a/%2e%2e/b",
        "/a\\b",
        "/a%5Cb",
        "/\u{0}",
        "/a\u{7f}",
        "/\u{e9}",
        "/\u{ff0f}evil",
        "/%",
        "/%2",
        "/%zz",
        "/%ff",
        "/%c0%af",
        // A `?` or `#` that is only text in a segment does not end the path: the dot
        // segments after it still climb to a path of two slashes.
        "/a%3F/..//evil.example",
        "/a%3f/..//evil.example",
        "/a%23/..//evil.example",
        "/x%3F/../../..//evil.example/path",
        "/a%3F/../",
        "/a%253F/..//evil.example",
        "/%2e%2e%2f%2e%2e//evil.example",
        "/a/%2e%2e%2f..",
        "/a%2F..",
    ];

    #[test]
    fn nothing_that_leaves_the_site_or_breaks_a_header_is_accepted() {
        for raw in REFUSED {
            assert!(ReturnTo::parse(raw).is_none(), "{raw:?} must be refused");
            assert_eq!(
                ReturnTo::resolve(Some(raw), "/home").as_str(),
                "/home",
                "{raw:?}"
            );
        }
    }

    #[test]
    fn a_long_address_is_refused() {
        let long = format!("/{}", "a".repeat(MOST));
        assert!(ReturnTo::parse(&long).is_none());
        let fits = format!("/{}", "a".repeat(MOST - 1));
        assert!(ReturnTo::parse(&fits).is_some());
        assert!(ReturnTo::parse(&format!("/{}", "a".repeat(5000))).is_none());
    }

    #[test]
    fn an_address_on_this_site_is_accepted_as_it_is() {
        for raw in [
            "/",
            "/deals",
            "/deals/42",
            "/deals?per=50&sort=-created",
            "/deals/42?tab=notes#history",
            "/a//b",
            "/a/b/",
            "/a:b",
            "/search?q=%C3%A9",
            "/search?q=a%20b",
            "/files/report.v2.pdf",
            "/%7Euser",
            "/a/.hidden",
            "/a/...",
            "/?",
            // The dot segments are in the query, which is not a path.
            "/a?b/../..//c",
            "/a#/../..//c",
            "/a%3Fb",
        ] {
            let found = ReturnTo::parse(raw).unwrap_or_else(|| panic!("{raw:?} is on this site"));
            assert_eq!(found.as_str(), raw);
            assert_eq!(ReturnTo::resolve(Some(raw), "/home").as_str(), raw);
        }
    }

    #[test]
    fn no_next_means_the_fallback() {
        assert_eq!(ReturnTo::resolve(None, "/home").as_str(), "/home");
        // A fallback that leaves the site goes nowhere, like any redirect.
        assert_eq!(ReturnTo::resolve(None, "//evil.example").as_str(), "#");
    }

    #[test]
    fn the_sign_in_page_is_not_a_place_to_return_to() {
        assert!(ReturnTo::parse_for("/sign-in", "/sign-in").is_none());
        assert!(ReturnTo::parse_for("/sign-in?next=/x", "/sign-in").is_none());
        assert!(ReturnTo::parse_for("/sign-in/two-factor", "/sign-in").is_none());
        assert!(ReturnTo::parse_for("/sign-in#x", "/sign-in").is_none());
        assert!(ReturnTo::parse_for("/sign-inside", "/sign-in").is_some());
        assert!(ReturnTo::parse_for("/deals", "/sign-in?x=1").is_some());
        assert!(ReturnTo::parse_for("/sign-in", "/sign-in?x=1").is_none());
    }

    #[test]
    fn the_sign_in_page_is_dropped_however_it_is_spelled() {
        for raw in [
            "/sign-in",
            "/sign-in/",
            "/sign-in?next=/x",
            "/%73ign-in",
            "/sign%2Din",
            "/sign-in/two-factor",
            "/sign-in#top",
        ] {
            assert!(ReturnTo::parse_for(raw, "/sign-in").is_none(), "{raw}");
            assert_eq!(
                ReturnTo::resolve_for(Some(raw), "/sign-in", "/home").as_str(),
                "/home",
                "{raw}"
            );
        }
        // Not the sign-in page: a sibling, and a page when sign-in is another site's.
        for raw in ["/sign-inside", "/sign", "/deals"] {
            assert!(ReturnTo::parse_for(raw, "/sign-in").is_some(), "{raw}");
        }
        assert!(ReturnTo::parse_for("/sign-in", "https://sso.example/sign-in").is_some());
        assert_eq!(
            ReturnTo::resolve_for(Some("/deals?x=1"), "/sign-in", "/home").as_str(),
            "/deals?x=1"
        );
        assert_eq!(
            ReturnTo::resolve_for(None, "/sign-in", "/home").as_str(),
            "/home"
        );
    }

    #[test]
    fn a_request_comes_back_to_its_path_and_query() {
        let uri: Uri = "/deals/42?tab=notes".parse().unwrap();
        assert_eq!(
            ReturnTo::of_uri(&uri).unwrap().as_str(),
            "/deals/42?tab=notes"
        );
        let uri: Uri = "/".parse().unwrap();
        assert_eq!(ReturnTo::of_uri(&uri).unwrap().as_str(), "/");
        // A request line can carry what a redirect must not.
        let uri: Uri = "/a%5Cb".parse().unwrap();
        assert!(ReturnTo::of_uri(&uri).is_none());
    }

    #[test]
    fn the_sign_in_link_carries_the_address_encoded() {
        let found = ReturnTo::parse("/deals?per=50&tab=a b".replace(' ', "%20").as_str()).unwrap();
        let link = found.sign_in_href("/sign-in");
        assert_eq!(
            link.as_str(),
            "/sign-in?next=%2Fdeals%3Fper%3D50%26tab%3Da%2520b"
        );
        let with_query = found.sign_in_href("/sign-in?reason=expired");
        assert!(
            with_query
                .as_str()
                .starts_with("/sign-in?reason=expired&next=")
        );
        // And back again through the query.
        let query = link.as_str().split_once('?').unwrap().1;
        assert_eq!(ReturnTo::from_query(query).unwrap(), found);
    }

    #[test]
    fn a_query_with_a_hostile_next_gives_nothing() {
        for query in [
            "next=%2F%2Fevil.example",
            "next=%2F%5Cevil.example",
            "next=https%3A%2F%2Fevil.example",
            "next=%2F%252Fevil.example",
            "next=%2F%09%2Fevil.example",
            "next=",
            "x=1",
            "",
            "next",
            "next=%",
            "next=%ff",
        ] {
            assert!(ReturnTo::from_query(query).is_none(), "{query:?}");
        }
        assert_eq!(
            ReturnTo::from_query("a=1&next=%2Fdeals&next=%2F%2Fevil")
                .unwrap()
                .as_str(),
            "/deals",
            "the first next counts"
        );
    }

    /// The sign-in page as a server reads it: escapes decoded once, a trailing slash ignored.
    /// Written apart from the code it checks, and the same in the fuzz target.
    fn names_sign_in(address: &str) -> bool {
        let path = address.split(['?', '#']).next().unwrap_or(address);
        let (raw, mut bytes, mut at) = (path.as_bytes(), Vec::new(), 0);
        while at < raw.len() {
            let digit = |i: usize| raw.get(i).and_then(|b| (*b as char).to_digit(16));
            if raw[at] == b'%'
                && let (Some(high), Some(low)) = (digit(at + 1), digit(at + 2))
            {
                bytes.push((high * 16 + low) as u8);
                at += 3;
            } else {
                bytes.push(raw[at]);
                at += 1;
            }
        }
        let path = String::from_utf8_lossy(&bytes);
        let path = path.trim_end_matches('/');
        path == "/sign-in" || path.starts_with("/sign-in/")
    }

    /// What a browser does with a redirect `Location` is what the URL standard says, and
    /// `url` follows it: whatever is accepted stays on the site it was resolved against, for
    /// the addresses above, the generated ones below, and a page deep in the site.
    #[test]
    fn an_independent_url_parser_keeps_every_accepted_address_on_the_site() {
        let bases = [
            "https://site.example/",
            "https://site.example/a/b/c?x=1#y",
            "http://site.example:8080/deep/page/",
        ];
        let mut samples: Vec<String> = [
            "/",
            "/deals",
            "/deals?per=50&tab=a%20b",
            "/a//b",
            "/a:b",
            "/a/.hidden",
            "/%7Euser",
            "/?",
            "/#x",
            "/search?q=%C3%A9",
            "/files/report.v2.pdf",
            "/sign-in",
            "/sign-in/",
            "/sign-in/two",
            "/sign-inx",
            "/%73ign-in",
            "/sign%2din",
            "/sign-in?next=/x",
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
        let alphabet = [
            "/", "\\", "%2f", "%5c", "%09", ".", "..", "%2e", "a", "?", "#", ":", "@", "%25", "b",
            "%3f", "%3F", "%23", "%2e%2e",
        ];
        let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for _ in 0..20_000 {
            let mut raw = String::from("/");
            for _ in 0..(next() % 8) {
                raw.push_str(alphabet[(next() as usize) % alphabet.len()]);
            }
            samples.push(raw);
        }
        let mut accepted = 0;
        for raw in &samples {
            let Some(found) = ReturnTo::parse(raw) else {
                continue;
            };
            accepted += 1;
            assert_eq!(
                ReturnTo::parse_for(raw, "/sign-in").is_some(),
                !names_sign_in(raw),
                "{raw:?}"
            );
            for base in bases {
                let base = url::Url::parse(base).unwrap();
                let joined = base
                    .join(found.as_str())
                    .unwrap_or_else(|e| panic!("{raw:?}: {e}"));
                assert_eq!(joined.scheme(), base.scheme(), "{raw:?}");
                assert_eq!(joined.host_str(), base.host_str(), "{raw:?} -> {joined}");
                assert_eq!(joined.port(), base.port(), "{raw:?} -> {joined}");
                assert!(joined.username().is_empty(), "{raw:?} -> {joined}");
                // Once its dot segments are resolved the path is still one path of this site, and
                // not two slashes that a later hop could read as a host.
                assert!(
                    !joined.path().starts_with("//"),
                    "{raw:?} resolves to a path of two slashes: {joined}"
                );
            }
        }
        assert!(
            accepted > 1000,
            "the generator reaches accepted addresses: {accepted}"
        );
    }

    /// The same check from a different angle: whatever string comes in, what goes out is
    /// on this site, or is nothing.
    #[test]
    fn nothing_generated_gets_through_unsafe() {
        let alphabet: Vec<&str> = vec![
            "/", "\\", "%2f", "%2F", "%5c", "%5C", "%09", "%0a", "%00", ".", "..", "%2e", "a", "?",
            "%3f", "%23", "%2e%2e", "#", ":", " ", "\t", "@", "%25", "2f", "%", "\u{e9}", "\u{0}",
        ];
        let mut seed = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for _ in 0..20_000 {
            let length = (next() % 9) as usize;
            let mut raw = String::from(if next() % 3 == 0 { "" } else { "/" });
            for _ in 0..length {
                raw.push_str(alphabet[(next() as usize) % alphabet.len()]);
            }
            let Some(found) = ReturnTo::parse(&raw) else {
                continue;
            };
            let text = found.as_str();
            assert!(text.starts_with('/'), "{raw:?}");
            assert!(
                !text.starts_with("//") && !text.starts_with("/\\"),
                "{raw:?}"
            );
            assert!(
                text.bytes()
                    .all(|b| (0x21..=0x7e).contains(&b) && b != b'\\'),
                "{raw:?}"
            );
            let decoded = decode(text).expect("accepted text decodes");
            let path = decode_once(path_of(text)).expect("the path decodes");
            assert!(
                path.split('/').all(|piece| piece != "." && piece != ".."),
                "{raw:?}"
            );
            assert!(!decoded.starts_with("//"), "{raw:?}");
            assert!(!decoded.contains('\\'), "{raw:?}");
            assert!(decoded.bytes().all(|b| b >= 0x20 && b != 0x7f), "{raw:?}");
            let local = LocalHref::parse(text);
            assert!(local.is_ok(), "{raw:?} is local");
        }
    }
}
