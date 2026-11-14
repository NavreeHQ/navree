//! The origin a client talks to, and how an address a page hands it is read.
//!
//! Every address goes through [`Origin::resolve`] before anything is done with
//! it. Only what comes back as [`Target::Inside`] is ever fetched or shown in
//! the web view; another site opens in the system's browser, and what can't be
//! read is dropped.
//!
//! An address is read as it is, never repaired:
//! - Nothing is trimmed. Text that is empty, starts with `#`, has white space
//!   at either end, or holds a control character, a backslash or a space other
//!   than the ASCII one anywhere does not read. An ASCII space inside is kept.
//! - A host is ASCII letters, digits, `-` and `.`, or an IPv6 literal in
//!   brackets. A name in another script is written in its ASCII form. The
//!   origins an app is configured with are held to the same rules.
//! - An address with a name before its host (`user@host`) does not read.
//! - A path that starts with `//` once its dot segments are taken out does not
//!   read. An empty segment further inside stays, as a browser keeps it.
//!
//! What a client is given to fetch, to stream from or to show in the web view
//! is a whole address, `scheme://host[:port]/path[?query]`, built from the
//! parts that were read ([`Origin::absolute`]), on the app's own origin or one
//! it allows. [`Origin::ours`] asks the same of a whole address: it is the
//! question a client asks again before it uses one.

/// Where an address leads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    /// On the app's own origin: a path and query. On another origin the app
    /// allows: the whole address.
    Inside(String),
    /// Another site, or a scheme the system handles: it opens outside the
    /// app. A web address is written again from the parts that were read.
    Outside(String),
    /// Nothing a client opens: a script, a file, a broken address.
    Refused,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Site {
    scheme: String,
    host: String,
    port: u16,
}

impl Site {
    fn base(&self) -> String {
        if self.port == default_port(&self.scheme) {
            format!("{}://{}", self.scheme, self.host)
        } else {
            format!("{}://{}:{}", self.scheme, self.host, self.port)
        }
    }
}

/// The origin the app was built for, and the others it allows.
#[derive(Clone, Debug)]
pub(crate) struct Origin {
    home: Site,
    /// Other allowed origins. A host that starts with `*.` stands for every
    /// host under that name.
    also: Vec<Site>,
}

impl Default for Origin {
    /// An origin nothing is ever on: what a core with an unusable
    /// configuration holds.
    fn default() -> Self {
        Self {
            home: Site {
                scheme: "https".to_owned(),
                host: "invalid.invalid".to_owned(),
                port: 443,
            },
            also: Vec::new(),
        }
    }
}

/// A web address, read.
struct Parts {
    site: Site,
    /// Never empty, and never starting with `//`.
    path: String,
    query: Option<String>,
    fragment: Option<String>,
}

impl Parts {
    /// The whole address without its fragment: what a request is sent to,
    /// written as a browser sends it.
    fn whole(&self) -> String {
        let mut out = self.site.base();
        encode_into(&mut out, &self.path, PATH_SET);
        if let Some(query) = &self.query {
            out.push('?');
            encode_into(&mut out, query, QUERY_SET);
        }
        out
    }

    /// The whole address with its fragment: what opens outside the app.
    fn with_fragment(&self) -> String {
        let mut out = self.whole();
        if let Some(fragment) = &self.fragment {
            out.push('#');
            encode_into(&mut out, fragment, FRAGMENT_SET);
        }
        out
    }
}

/// What the URL standard percent-encodes in the path of an `http` or `https`
/// address beyond the C0 controls and everything above `~`: its path
/// percent-encode set.
const PATH_SET: &[u8] = b" \"#<>?^`{}";
/// The same for a query: its special-query percent-encode set. `[` and `]`
/// are not in it, and stay.
const QUERY_SET: &[u8] = b" \"#<>'";
/// The same for a fragment: its fragment percent-encode set.
const FRAGMENT_SET: &[u8] = b" \"<>`";

/// Writes `text` with every byte the URL standard's `set` names, every C0
/// control and every byte above `~` as `%` and two hex digits. An escape
/// already written (`%` and two hex digits) stays as it is; any other `%` is
/// written as `%25`. Written once, the text is written the same again.
fn encode_into(out: &mut String, text: &str, set: &[u8]) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let bytes = text.as_bytes();
    for (at, &byte) in bytes.iter().enumerate() {
        let escape = byte == b'%'
            && bytes.get(at + 1).is_some_and(u8::is_ascii_hexdigit)
            && bytes.get(at + 2).is_some_and(u8::is_ascii_hexdigit);
        if escape || ((0x20..=0x7e).contains(&byte) && byte != b'%' && !set.contains(&byte)) {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 0xf)]));
        }
    }
}

/// How an address was read.
enum Reading {
    /// On one of the app's origins.
    Ours(Parts),
    /// Another site, as the whole address that opens outside.
    Other(String),
    Refused,
}

fn default_port(scheme: &str) -> u16 {
    if scheme == "http" { 80 } else { 443 }
}

const HANDED_TO_SYSTEM: [&str; 4] = ["mailto", "tel", "sms", "geo"];
const LOOPBACK: [&str; 4] = ["localhost", "127.0.0.1", "10.0.2.2", "[::1]"];

/// Whether `text` may be read as an address at all: not empty, no white
/// space at either end, and no control character, backslash, or space other
/// than the ASCII one anywhere. A browser reads a backslash as a slash, so
/// `/\host` would leave the origin.
fn readable(text: &str) -> bool {
    let (Some(first), Some(last)) = (text.chars().next(), text.chars().next_back()) else {
        return false;
    };
    !first.is_whitespace()
        && !last.is_whitespace()
        && !text
            .chars()
            .any(|c| c == '\\' || c.is_control() || (c.is_whitespace() && c != ' '))
}

/// `text` without its fragment.
fn before_fragment(text: &str) -> &str {
    text.split('#').next().unwrap_or("")
}

/// The scheme an address starts with, in lower case, when it has one.
fn scheme_of(text: &str) -> Option<String> {
    let (scheme, _) = text.split_once(':')?;
    let mut chars = scheme.chars();
    let first = chars.next()?;
    let rest = |c: char| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-');
    (first.is_ascii_alphabetic() && chars.all(rest)).then(|| scheme.to_ascii_lowercase())
}

/// Whether `text` starts with `http:` or `https:`, in any case.
fn is_web(text: &str) -> bool {
    matches!(scheme_of(text).as_deref(), Some("http" | "https"))
}

/// Whether `host` is ASCII letters, digits, `-` and `.`, or an IPv6 literal
/// in brackets.
fn host_reads(host: &str) -> bool {
    match host.strip_prefix('[') {
        Some(rest) => rest.strip_suffix(']').is_some_and(|inner| {
            inner.contains(':')
                && inner
                    .chars()
                    .all(|c| c.is_ascii_hexdigit() || matches!(c, ':' | '.'))
        }),
        None => {
            !host.is_empty()
                && host
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.'))
        }
    }
}

/// Reads `scheme://host[:port][/path][?query][#fragment]`, for `http` and
/// `https` only, by the rules at the top of this file.
fn split(text: &str) -> Option<Parts> {
    // What is left once the fragment is cut is held to the same rules.
    if !readable(before_fragment(text)) {
        return None;
    }
    let (scheme, rest) = text.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);
    // A name before an `@` is not the host, and no address that holds one is read.
    if authority.contains('@') {
        return None;
    }
    let (host, port) = if authority.starts_with('[') {
        let close = authority.find(']')?;
        let (host, after) = authority.split_at(close + 1);
        match after.strip_prefix(':') {
            Some(port) => (host, Some(port)),
            None if after.is_empty() => (host, None),
            None => return None,
        }
    } else {
        match authority.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        }
    };
    if !host_reads(host) {
        return None;
    }
    let port = match port {
        Some("") | None => default_port(&scheme),
        Some(port) if port.bytes().all(|byte| byte.is_ascii_digit()) => port.parse().ok()?,
        Some(_) => return None,
    };
    let (tail, fragment) = match tail.split_once('#') {
        Some((tail, fragment)) => (tail, Some(fragment.to_owned())),
        None => (tail, None),
    };
    let (path, query) = match tail.split_once('?') {
        Some((path, query)) => (path, Some(query.to_owned())),
        None => (tail, None),
    };
    Some(Parts {
        site: Site {
            scheme,
            host: host.to_ascii_lowercase(),
            port,
        },
        path: clean(if path.is_empty() { "/" } else { path })?,
        query,
        fragment,
    })
}

/// A path with its `.` and `..` segments taken out. A path that climbs above
/// the root stays at the root. `None` when what is left starts with `//`: a
/// browser would read the rest as another host.
fn clean(path: &str) -> Option<String> {
    let mut kept: Vec<&str> = Vec::new();
    let mut open = false;
    for segment in path.split('/').skip(1) {
        let dots = segment.replace("%2e", ".").replace("%2E", ".");
        open = false;
        match dots.as_str() {
            "." => open = true,
            ".." => {
                kept.pop();
                open = true;
            }
            _ => kept.push(segment),
        }
    }
    let mut out = format!("/{}", kept.join("/"));
    if open && !out.ends_with('/') {
        out.push('/');
    }
    (!out.starts_with("//")).then_some(out)
}

/// The path of an address, without its query: of `/orders?page=2`, or of a
/// whole address on another allowed origin.
pub(crate) fn path_of(url: &str) -> String {
    if is_web(url) {
        return split(url).map_or_else(|| "/".to_owned(), |parts| parts.path);
    }
    let path = url.split(['?', '#']).next().unwrap_or("");
    if path.is_empty() {
        "/".to_owned()
    } else {
        path.to_owned()
    }
}

/// Whether the path of `url` fits `pattern`, where `*` stands for one segment.
pub(crate) fn route_matches(pattern: &str, url: &str) -> bool {
    let path = path_of(url);
    let wanted: Vec<&str> = pattern.split('/').filter(|part| !part.is_empty()).collect();
    let given: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    wanted.len() == given.len()
        && wanted
            .iter()
            .zip(&given)
            .all(|(want, have)| *want == "*" || want == have)
}

/// What may be opened outside the app: a whole `http` or `https` address that
/// reads, written again from its parts, or an address of a scheme the system
/// handles (mail, the dialler, messages, maps). Nothing else.
pub(crate) fn openable(address: &str) -> Option<String> {
    if !readable(address) {
        return None;
    }
    match scheme_of(address).as_deref() {
        Some("http" | "https") => split(address).map(|parts| parts.with_fragment()),
        Some(scheme) if HANDED_TO_SYSTEM.contains(&scheme) => Some(address.to_owned()),
        _ => None,
    }
}

/// A whole `https` address that reads, written again from its parts.
pub(crate) fn https_address(address: &str) -> Option<String> {
    if !readable(address) || scheme_of(address).as_deref() != Some("https") {
        return None;
    }
    split(address).map(|parts| parts.with_fragment())
}

/// Reads one origin of the app's configuration. In the list of others it
/// allows (`wildcard`), `*.` before a host stands for every host under it.
fn configured(text: &str, wildcard: bool, loopback_http: bool) -> Option<Site> {
    if !readable(text) {
        return None;
    }
    let under = match text.split_once("://") {
        Some((scheme, rest)) if wildcard => rest
            .strip_prefix("*.")
            .map(|rest| format!("{scheme}://{rest}")),
        _ => None,
    };
    let parts = split(under.as_deref().unwrap_or(text))?;
    let allowed = parts.site.scheme == "https"
        || (loopback_http && LOOPBACK.contains(&parts.site.host.as_str()));
    let mut site = parts.site;
    if under.is_some() {
        site.host = format!("*.{}", site.host);
    }
    allowed.then_some(site)
}

impl Origin {
    /// Reads the app's origin and the others it allows. It must be `https`;
    /// plain `http` is taken only for the machine itself (and the emulator's
    /// name for its host), and only when `loopback_http` says so, which a
    /// release build never does. An origin of the list that does not read is
    /// left out; the app's own that does not read leaves the core unusable.
    pub(crate) fn parse(text: &str, also: &[String], loopback_http: bool) -> Option<Self> {
        Some(Self {
            home: configured(text, false, loopback_http)?,
            also: also
                .iter()
                .filter_map(|text| configured(text, true, loopback_http))
                .collect(),
        })
    }

    /// Whether `site` is the app's own origin or one it allows.
    fn holds(&self, site: &Site) -> bool {
        *site == self.home
            || self.also.iter().any(|allowed| {
                allowed.scheme == site.scheme
                    && allowed.port == site.port
                    && match allowed.host.strip_prefix("*.") {
                        // Under the name: one label or more, none of them empty.
                        Some(under) => site
                            .host
                            .strip_suffix(under)
                            .and_then(|left| left.strip_suffix('.'))
                            .is_some_and(|name| {
                                !name.is_empty() && name.split('.').all(|label| !label.is_empty())
                            }),
                        None => allowed.host == site.host,
                    }
            })
    }

    /// Whether `address` is a whole address on one of the app's origins: an
    /// `http` or `https` address with no white space at all (what leaves the
    /// core is percent-encoded) that reads by the rules at the top of this
    /// file, on the app's own origin or one it allows. A client asks the same
    /// of what its platform's own parser reads, before it fetches an address,
    /// streams from it or shows it: `vectors/addresses.json`, `ours`.
    pub(crate) fn ours(&self, address: &str) -> bool {
        readable(address)
            && !address.contains(' ')
            && is_web(address)
            && split(address).is_some_and(|parts| self.holds(&parts.site))
    }

    /// The whole address of something inside, built from its parts: what a
    /// request, a stream or the web view is given. `None` for anything that
    /// is not on one of the app's origins: it never leaves the core. What it
    /// gives is asked [`Origin::ours`] once more, the question a client asks.
    pub(crate) fn absolute(&self, url: &str) -> Option<String> {
        match self.read(url, "/") {
            Reading::Ours(parts) => Some(parts.whole()).filter(|whole| self.ours(whole)),
            Reading::Other(_) | Reading::Refused => None,
        }
    }

    /// Reads `href` as found on the page at `from`. A fragment is dropped: a
    /// tree has no anchors.
    pub(crate) fn resolve(&self, href: &str, from: &str) -> Target {
        match self.read(href, from) {
            Reading::Ours(parts) => {
                // The app's own origin goes by path; another it allows, whole.
                let mut out = if parts.site == self.home {
                    String::new()
                } else {
                    parts.site.base()
                };
                out.push_str(&parts.path);
                if let Some(query) = &parts.query {
                    out.push('?');
                    out.push_str(query);
                }
                Target::Inside(out)
            }
            Reading::Other(address) => Target::Outside(address),
            Reading::Refused => Target::Refused,
        }
    }

    fn read(&self, href: &str, from: &str) -> Reading {
        // The address, what is left of it once its fragment is cut, and the
        // page it stands on are all held to the same rules.
        if !readable(href)
            || href.starts_with('#')
            || !readable(before_fragment(href))
            || !readable(from)
        {
            return Reading::Refused;
        }
        // The page the address stands on: a path on the app's own origin, or
        // a whole address on another it allows.
        let base = if is_web(from) {
            match split(from) {
                Some(parts) if self.holds(&parts.site) => parts,
                _ => return Reading::Refused,
            }
        } else {
            Parts {
                site: self.home.clone(),
                path: path_of(from),
                query: None,
                fragment: None,
            }
        };
        if href.starts_with("//") {
            return self.web(&format!("{}:{href}", base.site.scheme));
        }
        match scheme_of(href).as_deref() {
            Some("http" | "https") => self.web(href),
            Some(scheme) if HANDED_TO_SYSTEM.contains(&scheme) => Reading::Other(href.to_owned()),
            Some(_) => Reading::Refused,
            None => {
                let text = href.split('#').next().unwrap_or("");
                let (path, query) = match text.split_once('?') {
                    Some((path, query)) => (path, Some(query.to_owned())),
                    None => (text, None),
                };
                let path = if path.is_empty() {
                    clean(&base.path)
                } else if path.starts_with('/') {
                    clean(path)
                } else {
                    let dir = base.path.rsplit_once('/').map_or("", |(dir, _)| dir);
                    clean(&format!("{dir}/{path}"))
                };
                let Some(path) = path else {
                    return Reading::Refused;
                };
                self.sort(Parts {
                    site: base.site,
                    path,
                    query,
                    fragment: None,
                })
            }
        }
    }

    fn web(&self, text: &str) -> Reading {
        split(text).map_or(Reading::Refused, |parts| self.sort(parts))
    }

    fn sort(&self, parts: Parts) -> Reading {
        if self.holds(&parts.site) {
            Reading::Ours(parts)
        } else {
            Reading::Other(parts.with_fragment())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin() -> Origin {
        Origin::parse(
            "https://shop.example",
            &[
                "https://*.shop.example".to_owned(),
                "https://pay.example:8443".to_owned(),
            ],
            false,
        )
        .unwrap()
    }

    fn inside(url: &str) -> Target {
        Target::Inside(url.to_owned())
    }

    fn outside(url: &str) -> Target {
        Target::Outside(url.to_owned())
    }

    /// The whole address of something inside, as a request is given it.
    fn whole(origin: &Origin, url: &str) -> Option<String> {
        origin.absolute(url)
    }

    #[test]
    fn an_address_is_inside_outside_or_refused() {
        let origin = origin();
        assert_eq!(
            origin.resolve("/orders/7?tab=items#notes", "/"),
            inside("/orders/7?tab=items")
        );
        assert_eq!(
            origin.resolve("https://shop.example/orders", "/"),
            inside("/orders")
        );
        assert_eq!(origin.resolve("HTTPS://Shop.Example:443", "/"), inside("/"));
        assert_eq!(origin.resolve("//shop.example/a", "/"), inside("/a"));
        assert_eq!(
            origin.resolve("https://other.example/a", "/"),
            outside("https://other.example/a")
        );
        // The same name on another scheme or port is another site.
        assert_eq!(
            origin.resolve("http://shop.example/a", "/"),
            outside("http://shop.example/a")
        );
        assert_eq!(
            origin.resolve("https://shop.example:8443/a", "/"),
            outside("https://shop.example:8443/a")
        );
        assert_eq!(
            origin.resolve("mailto:help@shop.example", "/"),
            outside("mailto:help@shop.example")
        );
        assert_eq!(
            origin.resolve("tel:+911234567890", "/"),
            outside("tel:+911234567890")
        );
        for refused in [
            "",
            "#top",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "data:text/html,x",
            "intent://x",
            "/\\evil.example",
            "/a\nb",
            "https:orders",
        ] {
            assert_eq!(origin.resolve(refused, "/"), Target::Refused, "{refused:?}");
        }
    }

    #[test]
    fn nothing_is_trimmed() {
        let origin = origin();
        for refused in [
            " /orders",
            "/orders ",
            "\u{3000}//evil.example/x",
            "\u{a0}/orders",
            "/orders\u{2003}",
            "\t/orders",
            "/orders\n",
        ] {
            assert_eq!(origin.resolve(refused, "/"), Target::Refused, "{refused:?}");
        }
    }

    #[test]
    fn a_space_of_another_kind_a_control_or_a_backslash_does_not_read_anywhere() {
        let origin = origin();
        for refused in [
            "/a\tb",
            "/a\u{3000}b",
            "/a\u{2028}b",
            "/a\u{85}b",
            "/a\u{7f}b",
            "/a?q=\\",
            "https://shop.example/a\u{a0}b",
        ] {
            assert_eq!(origin.resolve(refused, "/"), Target::Refused, "{refused:?}");
        }
        // An ASCII space inside an address is left as it is.
        assert_eq!(origin.resolve("/a b?q=c d", "/"), inside("/a b?q=c d"));
    }

    #[test]
    fn a_host_is_ascii_letters_digits_dashes_and_dots_or_a_bracketed_literal() {
        let origin = origin();
        for refused in [
            "https://shöp.example/",
            "https://evil.example%2fshop.example/",
            "https://shop%2eexample/",
            "https://shop_example/",
            "https://[evil.example]/",
            "https://[]/",
            "https://shop.example:+443/",
            "https://shop.example:99999/",
            "https://shop.example:4a/",
            "https:///evil.example",
            "https://shop.example]/",
        ] {
            assert_eq!(origin.resolve(refused, "/"), Target::Refused, "{refused:?}");
        }
        assert_eq!(
            origin.resolve("https://[::1]:8080/x", "/"),
            outside("https://[::1]:8080/x")
        );
        assert_eq!(
            origin.resolve("https://xn--shp-sna.example/", "/"),
            outside("https://xn--shp-sna.example/")
        );
        // The configured origins are held to the same rules.
        for config in [
            "https://shöp.example",
            " https://shop.example",
            "https://shop.example ",
            "https://shop.example%2f",
        ] {
            assert!(Origin::parse(config, &[], false).is_none(), "{config:?}");
        }
        let only = Origin::parse(
            "https://shop.example",
            &[
                "https://pāy.example".to_owned(),
                "https://*.shop.example".to_owned(),
            ],
            false,
        )
        .unwrap();
        assert_eq!(only.also.len(), 1);
    }

    #[test]
    fn an_address_with_a_name_before_its_host_does_not_read() {
        let origin = origin();
        for refused in [
            "https://shop.example@evil.example/",
            "https://user@shop.example/",
            "https://user:secret@shop.example/",
            "//user@evil.example/",
            "https://@shop.example/",
        ] {
            assert_eq!(origin.resolve(refused, "/"), Target::Refused, "{refused:?}");
        }
    }

    #[test]
    fn a_path_that_starts_with_two_slashes_once_its_dots_are_taken_out_does_not_read() {
        let origin = origin();
        for refused in [
            "https://shop.example//evil.example/x",
            "https://shop.example//evil.example/x?a=b://c",
            "/.//evil.example/x",
            "/a/..//evil.example/x",
            "/%2e%2e//evil.example/x",
            "/a/%2E%2e//evil.example/x",
            ".//evil.example/x",
            "https://patiala.shop.example/.//evil.example/x",
        ] {
            assert_eq!(origin.resolve(refused, "/"), Target::Refused, "{refused:?}");
        }
        assert_eq!(
            origin.resolve("./x/..//evil.example", "/b"),
            Target::Refused
        );
        // Further inside, an empty segment stays, as a browser keeps it.
        assert_eq!(origin.resolve("/a//b", "/"), inside("/a//b"));
        assert_eq!(origin.resolve("/a/.//b", "/"), inside("/a//b"));
    }

    #[test]
    fn a_query_or_a_fragment_that_holds_a_scheme_changes_nothing() {
        let origin = origin();
        assert_eq!(origin.resolve("/x?a=b://c", "/"), inside("/x?a=b://c"));
        assert_eq!(
            whole(&origin, "/x?a=b://c").as_deref(),
            Some("https://shop.example/x?a=b://c")
        );
        assert_eq!(
            origin.resolve("?a=b://c", "/orders?page=1"),
            inside("/orders?a=b://c")
        );
        assert_eq!(
            origin.resolve("edit", "/orders/7?back=https://shop.example/x"),
            inside("/orders/edit")
        );
        assert_eq!(path_of("/x?a=b://c"), "/x");
        assert!(route_matches("/x", "/x?a=b://c"));
        assert_eq!(
            origin.resolve("//evil.example/x?a=b://c", "/"),
            outside("https://evil.example/x?a=b://c")
        );
    }

    #[test]
    fn what_leaves_for_another_site_is_built_from_what_was_read() {
        let origin = origin();
        assert_eq!(
            origin.resolve("HTTPS://Evil.Example:443", "/"),
            outside("https://evil.example/")
        );
        assert_eq!(
            origin.resolve("https://evil.example#@shop.example/", "/"),
            outside("https://evil.example/#@shop.example/")
        );
        assert_eq!(
            origin.resolve("https://evil.example?@shop.example/", "/"),
            outside("https://evil.example/?@shop.example/")
        );
        assert_eq!(
            origin.resolve("https://shop.example.evil.example/", "/"),
            outside("https://shop.example.evil.example/")
        );
    }

    #[test]
    fn a_relative_address_is_read_against_the_page() {
        let origin = origin();
        assert_eq!(origin.resolve("edit", "/orders/7"), inside("/orders/edit"));
        assert_eq!(
            origin.resolve("edit", "/orders/7/"),
            inside("/orders/7/edit")
        );
        assert_eq!(
            origin.resolve("../inventory", "/orders/7"),
            inside("/inventory")
        );
        assert_eq!(
            origin.resolve("?page=2", "/orders?page=1"),
            inside("/orders?page=2")
        );
        assert_eq!(origin.resolve("./", "/orders/7"), inside("/orders/"));
        // A path can't climb out of the origin.
        assert_eq!(origin.resolve("/../../etc", "/"), inside("/etc"));
        assert_eq!(origin.resolve("/a/%2e%2e/b", "/"), inside("/b"));
        assert_eq!(origin.resolve("x/..", "/a/b"), inside("/a/"));
        assert_eq!(origin.resolve("/उत्पाद/1", "/"), inside("/उत्पाद/1"));
    }

    #[test]
    fn another_allowed_origin_is_inside() {
        let origin = origin();
        assert_eq!(
            origin.resolve("https://patiala.shop.example/orders?x=1", "/"),
            inside("https://patiala.shop.example/orders?x=1")
        );
        assert_eq!(
            origin.resolve("https://pay.example:8443/pay", "/"),
            inside("https://pay.example:8443/pay")
        );
        // Under the name, not merely ending in it, and never the bare pattern.
        for other in [
            "https://evilshop.example/",
            "https://.shop.example/",
            "https://a..shop.example/",
            "https://pay.example/pay",
        ] {
            assert!(
                matches!(origin.resolve(other, "/"), Target::Outside(_)),
                "{other:?}"
            );
        }
        // A page on another allowed origin reads its own links against itself.
        assert_eq!(
            origin.resolve("/inventory", "https://patiala.shop.example/orders"),
            inside("https://patiala.shop.example/inventory")
        );
        assert_eq!(
            origin.resolve("https://shop.example/x", "https://patiala.shop.example/"),
            inside("/x")
        );
        assert_eq!(
            path_of("https://patiala.shop.example/orders?x=1"),
            "/orders"
        );
    }

    #[test]
    fn an_address_inside_leaves_whole_and_nothing_else_leaves() {
        let origin = origin();
        assert_eq!(
            whole(&origin, "/orders").as_deref(),
            Some("https://shop.example/orders")
        );
        assert_eq!(
            whole(&origin, "https://patiala.shop.example/orders").as_deref(),
            Some("https://patiala.shop.example/orders")
        );
        assert_eq!(
            whole(&origin, "/उत्पाद/1").as_deref(),
            Some("https://shop.example/%E0%A4%89%E0%A4%A4%E0%A5%8D%E0%A4%AA%E0%A4%BE%E0%A4%A6/1")
        );
        for not_ours in [
            "https://evil.example/x",
            "//evil.example/x",
            "https://shop.example//evil.example/x",
            "//evil.example/x?a=b://c",
            "\u{3000}//evil.example/x",
            "mailto:help@shop.example",
            "javascript:1",
        ] {
            assert_eq!(whole(&origin, not_ours), None, "{not_ours:?}");
        }
    }

    #[test]
    fn a_route_matches_by_segment() {
        assert!(route_matches("/orders/*/edit", "/orders/7/edit?x=1"));
        assert!(route_matches("/sign-in", "/sign-in"));
        assert!(route_matches("/", "/"));
        assert!(!route_matches("/orders/*", "/orders"));
        assert!(!route_matches("/orders/*", "/orders/7/edit"));
        assert!(!route_matches("/orders", "/inventory"));
    }

    #[test]
    fn plain_http_is_for_the_machine_itself_and_only_when_asked() {
        assert!(Origin::parse("http://shop.example", &[], true).is_none());
        assert!(Origin::parse("http://10.0.2.2:8095", &[], false).is_none());
        let local = Origin::parse("http://10.0.2.2:8095", &[], true).unwrap();
        assert_eq!(
            whole(&local, "/orders").as_deref(),
            Some("http://10.0.2.2:8095/orders")
        );
        assert!(Origin::parse("https://user@shop.example", &[], false).is_none());
        assert!(Origin::parse("shop.example", &[], false).is_none());
    }

    #[test]
    fn what_opens_outside_is_a_whole_web_address_or_a_scheme_the_system_handles() {
        assert_eq!(
            openable("HTTPS://Evil.Example/a?b#c").as_deref(),
            Some("https://evil.example/a?b#c")
        );
        assert_eq!(
            openable("http://evil.example").as_deref(),
            Some("http://evil.example/")
        );
        assert_eq!(
            openable("tel:+919876543210").as_deref(),
            Some("tel:+919876543210")
        );
        for refused in [
            "/orders",
            "//evil.example/x",
            "javascript:1",
            "intent://x",
            "file:///etc/hosts",
            "https://shop.example@evil.example/",
            " https://evil.example/",
            "https://evil.example/\u{3000}",
            "mailto:a\nb",
        ] {
            assert_eq!(openable(refused), None, "{refused:?}");
        }
        assert_eq!(
            https_address("https://store.example/app?id=1").as_deref(),
            Some("https://store.example/app?id=1")
        );
        for refused in [
            "http://store.example/app",
            "javascript:1",
            "market://details?id=1",
            "",
        ] {
            assert_eq!(https_address(refused), None, "{refused:?}");
        }
    }

    /// The URL standard's path, special-query and fragment percent-encode
    /// sets: C0 controls, everything above `~`, and the characters each names.
    #[test]
    fn a_whole_address_is_written_as_a_browser_sends_it() {
        let origin = origin();
        for (inside, sent) in [
            ("/a b", "https://shop.example/a%20b"),
            ("/a\"<>b", "https://shop.example/a%22%3C%3Eb"),
            ("/a^`{|}b", "https://shop.example/a%5E%60%7B|%7Db"),
            ("/x?a b\"<>'", "https://shop.example/x?a%20b%22%3C%3E%27"),
            // `[` and `]` in a query stay: apps write `filter[status]=won`.
            (
                "/x?filter[status]=won&sort=-total",
                "https://shop.example/x?filter[status]=won&sort=-total",
            ),
            // In a path, only what the standard's path set names is written.
            ("/a[1]?b^`{}", "https://shop.example/a[1]?b^`{}"),
            ("/ü?ü", "https://shop.example/%C3%BC?%C3%BC"),
            // An escape stays as it was; a `%` that is not one is written as one.
            ("/a%20b?c=%41", "https://shop.example/a%20b?c=%41"),
            ("/100%?a=%zz&b=%4", "https://shop.example/100%25?a=%25zz&b=%254"),
        ] {
            assert_eq!(whole(&origin, inside).as_deref(), Some(sent), "{inside:?}");
        }
        // The local form in the view state is not written again.
        assert_eq!(origin.resolve("/a b", "/"), inside("/a b"));
        // What leaves for another site is written the same way, its fragment
        // by the standard's fragment set.
        assert_eq!(
            origin.resolve("https://evil.example/ä b?ä b#ä b`", "/"),
            outside("https://evil.example/%C3%A4%20b?%C3%A4%20b#%C3%A4%20b%60")
        );
        assert_eq!(
            openable("https://evil.example/a b?c d").as_deref(),
            Some("https://evil.example/a%20b?c%20d")
        );
        // Written once, it is written the same again.
        let sent = whole(&origin, "/ü b?%zz").unwrap();
        assert_eq!(openable(&sent).as_deref(), Some(sent.as_str()));
    }

    #[test]
    fn what_is_left_once_the_fragment_is_cut_is_read_by_the_same_rules() {
        let origin = origin();
        for (href, from) in [
            ("a #x", "/orders/7"),
            ("?q #x", "/orders?page=1"),
            ("/a #x", "/"),
            ("/a?b #x", "/"),
            ("https://shop.example/a #x", "/"),
            ("//shop.example/a #x", "/"),
        ] {
            assert_eq!(origin.resolve(href, from), Target::Refused, "{href:?}");
        }
        // A space inside the fragment changes nothing: the fragment is dropped.
        assert_eq!(origin.resolve("/a#x y", "/"), inside("/a"));
        // The page an address stands on is held to the same rules.
        for from in [" /orders", "/orders\n", "/a\u{3000}b"] {
            assert_eq!(origin.resolve("edit", from), Target::Refused, "{from:?}");
        }
    }

    /// Whatever `resolve` calls inside, `absolute` gives a whole address for.
    #[test]
    fn every_address_read_as_inside_leaves_whole() {
        let text = include_str!("../vectors/addresses.json");
        let vectors: Vectors = serde_json::from_str(text).expect("the vectors read");
        let mut hrefs: Vec<(Origin, String, String)> = vectors
            .resolve
            .iter()
            .map(|case| {
                (
                    Origin::parse(&case.origin, &case.also, true).expect("an origin"),
                    case.href.clone(),
                    case.from.clone(),
                )
            })
            .collect();
        for href in [
            "a b#c d", "?a b", "/a b?c d", "x/../y z", "/%zz", "/a%2", "/ü#ü", "?#",
        ] {
            hrefs.push((origin(), href.to_owned(), "/orders/7".to_owned()));
        }
        for (origin, href, from) in &hrefs {
            if let Target::Inside(url) = origin.resolve(href, from) {
                assert!(
                    whole(origin, &url).is_some(),
                    "{href:?} on {from:?} is inside as {url:?}, and does not leave whole"
                );
            }
        }
    }

    #[derive(serde::Deserialize)]
    struct Vectors {
        resolve: Vec<Reads>,
        ours: Vec<Ours>,
    }

    #[derive(serde::Deserialize)]
    struct Reads {
        origin: String,
        also: Vec<String>,
        from: String,
        href: String,
        reads: String,
        #[serde(default)]
        address: Option<String>,
    }

    #[derive(serde::Deserialize)]
    struct Ours {
        origin: String,
        also: Vec<String>,
        address: String,
        ours: bool,
    }

    /// The addresses every client reads the same way: `vectors/addresses.json`.
    #[test]
    fn every_client_reads_the_shared_addresses_the_same_way() {
        let text = include_str!("../vectors/addresses.json");
        let vectors: Vectors = serde_json::from_str(text).expect("the vectors read");
        assert!(vectors.resolve.len() > 40 && vectors.ours.len() > 30);
        let mut wrong = Vec::new();
        for case in &vectors.resolve {
            let origin = Origin::parse(&case.origin, &case.also, true).expect("an origin");
            let got = match origin.resolve(&case.href, &case.from) {
                Target::Inside(url) => ("inside", whole(&origin, &url)),
                Target::Outside(url) => ("outside", Some(url)),
                Target::Refused => ("refused", None),
            };
            if (got.0, got.1.as_deref()) != (case.reads.as_str(), case.address.as_deref()) {
                wrong.push(format!(
                    "{:?} on {:?}: {got:?}, not {} {:?}",
                    case.href, case.from, case.reads, case.address
                ));
            }
        }
        for case in &vectors.ours {
            let origin = Origin::parse(&case.origin, &case.also, true).expect("an origin");
            if origin.ours(&case.address) != case.ours {
                wrong.push(format!("{:?} ours: not {}", case.address, case.ours));
            }
        }
        assert!(wrong.is_empty(), "\n{}", wrong.join("\n"));
    }
}
