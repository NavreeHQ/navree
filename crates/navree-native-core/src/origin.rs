//! The origin a client talks to, and how an address a page hands it is read.
//!
//! Every address goes through [`Origin::resolve`] before anything is done with
//! it. Only what comes back as [`Target::Inside`] is ever fetched or shown in
//! the web view; another site opens in the system's browser, and what can't be
//! read is dropped.

/// Where an address leads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    /// On the app's own origin: a path and query. On another origin the app
    /// allows: the whole address.
    Inside(String),
    /// Another site, or a scheme the system handles: it opens outside the app.
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

struct Parts {
    site: Site,
    user: bool,
    path: String,
    query: Option<String>,
}

fn default_port(scheme: &str) -> u16 {
    if scheme == "http" { 80 } else { 443 }
}

const HANDED_TO_SYSTEM: [&str; 4] = ["mailto", "tel", "sms", "geo"];
const LOOPBACK: [&str; 4] = ["localhost", "127.0.0.1", "10.0.2.2", "[::1]"];

/// The scheme an address starts with, in lower case, when it has one.
fn scheme_of(text: &str) -> Option<String> {
    let (scheme, _) = text.split_once(':')?;
    let mut chars = scheme.chars();
    let first = chars.next()?;
    let rest = |c: char| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-');
    (first.is_ascii_alphabetic() && chars.all(rest)).then(|| scheme.to_ascii_lowercase())
}

/// Reads `scheme://host[:port][/path][?query][#fragment]`.
fn split(text: &str) -> Option<Parts> {
    let (scheme, rest) = text.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);
    let (user, hostport) = match authority.rsplit_once('@') {
        Some((_, hostport)) => (true, hostport),
        None => (false, authority),
    };
    let (host, port) = if hostport.starts_with('[') {
        let close = hostport.find(']')?;
        let (host, after) = hostport.split_at(close + 1);
        match after.strip_prefix(':') {
            Some(port) => (host, Some(port)),
            None if after.is_empty() => (host, None),
            None => return None,
        }
    } else {
        match hostport.rsplit_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (hostport, None),
        }
    };
    if host.is_empty() {
        return None;
    }
    let port = match port {
        Some("") | None => default_port(&scheme),
        Some(port) => port.parse().ok()?,
    };
    let tail = tail.split('#').next().unwrap_or("");
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
        user,
        path: clean(path),
        query,
    })
}

/// A path with its `.` and `..` segments taken out. A path that climbs above
/// the root stays at the root.
fn clean(path: &str) -> String {
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
    out
}

/// The path of an address, without its query: of `/orders?page=2`, or of a
/// whole address on another allowed origin.
pub(crate) fn path_of(url: &str) -> String {
    if url.contains("://") {
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

impl Origin {
    /// Reads the app's origin and the others it allows. It must be `https`;
    /// plain `http` is taken only for the machine itself (and the emulator's
    /// name for its host), and only when `loopback_http` says so, which a
    /// release build never does.
    pub(crate) fn parse(text: &str, also: &[String], loopback_http: bool) -> Option<Self> {
        let site = |text: &str| {
            let parts = split(text.trim())?;
            let allowed = parts.site.scheme == "https"
                || (loopback_http && LOOPBACK.contains(&parts.site.host.as_str()));
            (allowed && !parts.user).then_some(parts.site)
        };
        Some(Self {
            home: site(text)?,
            also: also.iter().filter_map(|text| site(text)).collect(),
        })
    }

    fn allows(&self, site: &Site) -> bool {
        self.also.iter().any(|allowed| {
            allowed.scheme == site.scheme
                && allowed.port == site.port
                && match allowed.host.strip_prefix("*.") {
                    Some(under) => site
                        .host
                        .strip_suffix(under)
                        .is_some_and(|left| left.len() > 1 && left.ends_with('.')),
                    None => allowed.host == site.host,
                }
        })
    }

    fn render(&self, site: &Site, path: &str, query: Option<&str>) -> String {
        let mut out = if *site == self.home {
            String::new()
        } else {
            site.base()
        };
        out.push_str(path);
        if let Some(query) = query {
            out.push('?');
            out.push_str(query);
        }
        out
    }

    /// The whole address of something inside: what a request or the web view
    /// is given.
    pub(crate) fn absolute(&self, url: &str) -> String {
        if url.contains("://") {
            url.to_owned()
        } else if url.starts_with('/') {
            format!("{}{url}", self.home.base())
        } else {
            format!("{}/{url}", self.home.base())
        }
    }

    /// Reads `href` as found on the page at `from`. A fragment is dropped: a
    /// tree has no anchors.
    pub(crate) fn resolve(&self, href: &str, from: &str) -> Target {
        let text = href.trim();
        if text.is_empty() || text.starts_with('#') {
            return Target::Refused;
        }
        // A browser reads a backslash as a slash; `/\host` would leave the origin.
        if text.chars().any(|c| c == '\\' || c.is_control()) {
            return Target::Refused;
        }
        let base = if from.contains("://") {
            split(from)
        } else {
            None
        };
        let base_site = base.as_ref().map_or(&self.home, |parts| &parts.site);
        if text.starts_with("//") {
            return self.web(&format!("{}:{text}", base_site.scheme));
        }
        match scheme_of(text).as_deref() {
            Some("http" | "https") => self.web(text),
            Some(scheme) if HANDED_TO_SYSTEM.contains(&scheme) => Target::Outside(text.to_owned()),
            Some(_) => Target::Refused,
            None => {
                let base_path = base.as_ref().map_or_else(|| path_of(from), |parts| parts.path.clone());
                let text = text.split('#').next().unwrap_or("");
                let (path, query) = match text.split_once('?') {
                    Some((path, query)) => (path, Some(query)),
                    None => (text, None),
                };
                let path = if path.is_empty() {
                    base_path
                } else if path.starts_with('/') {
                    clean(path)
                } else {
                    let dir = base_path.rsplit_once('/').map_or("", |(dir, _)| dir);
                    clean(&format!("{dir}/{path}"))
                };
                Target::Inside(self.render(base_site, &path, query))
            }
        }
    }

    fn web(&self, text: &str) -> Target {
        let Some(parts) = split(text) else {
            return Target::Refused;
        };
        if !parts.user && (parts.site == self.home || self.allows(&parts.site)) {
            Target::Inside(self.render(&parts.site, &parts.path, parts.query.as_deref()))
        } else {
            Target::Outside(text.to_owned())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin() -> Origin {
        Origin::parse(
            "https://shop.example",
            &["https://*.shop.example".to_owned(), "https://pay.example:8443".to_owned()],
            false,
        )
        .unwrap()
    }

    fn inside(url: &str) -> Target {
        Target::Inside(url.to_owned())
    }

    #[test]
    fn an_address_is_inside_outside_or_refused() {
        let origin = origin();
        assert_eq!(origin.resolve("/orders/7?tab=items#notes", "/"), inside("/orders/7?tab=items"));
        assert_eq!(origin.resolve("https://shop.example/orders", "/"), inside("/orders"));
        assert_eq!(origin.resolve("HTTPS://Shop.Example:443", "/"), inside("/"));
        assert_eq!(origin.resolve("//shop.example/a", "/"), inside("/a"));
        assert_eq!(
            origin.resolve("https://other.example/a", "/"),
            Target::Outside("https://other.example/a".to_owned())
        );
        // The same name on another scheme or port is another site.
        assert!(matches!(origin.resolve("http://shop.example/a", "/"), Target::Outside(_)));
        assert!(matches!(origin.resolve("https://shop.example:8443/a", "/"), Target::Outside(_)));
        // A name before an `@` is not the host.
        assert!(matches!(origin.resolve("https://shop.example@evil.example/", "/"), Target::Outside(_)));
        assert!(matches!(origin.resolve("https://user@shop.example/", "/"), Target::Outside(_)));
        assert_eq!(
            origin.resolve("mailto:help@shop.example", "/"),
            Target::Outside("mailto:help@shop.example".to_owned())
        );
        assert_eq!(origin.resolve("tel:+911234567890", "/"), Target::Outside("tel:+911234567890".to_owned()));
        for refused in ["", "#top", "javascript:alert(1)", "file:///etc/passwd", "data:text/html,x", "/\\evil.example", "/a\nb", "https:orders"] {
            assert_eq!(origin.resolve(refused, "/"), Target::Refused, "{refused:?}");
        }
    }

    #[test]
    fn a_relative_address_is_read_against_the_page() {
        let origin = origin();
        assert_eq!(origin.resolve("edit", "/orders/7"), inside("/orders/edit"));
        assert_eq!(origin.resolve("edit", "/orders/7/"), inside("/orders/7/edit"));
        assert_eq!(origin.resolve("../inventory", "/orders/7"), inside("/inventory"));
        assert_eq!(origin.resolve("?page=2", "/orders?page=1"), inside("/orders?page=2"));
        assert_eq!(origin.resolve("./", "/orders/7"), inside("/orders/"));
        // A path can't climb out of the origin.
        assert_eq!(origin.resolve("/../../etc", "/"), inside("/etc"));
        assert_eq!(origin.resolve("/a/%2e%2e/b", "/"), inside("/b"));
        assert_eq!(origin.resolve("x/..", "/a/b"), inside("/a/"));
    }

    #[test]
    fn another_allowed_origin_is_inside() {
        let origin = origin();
        assert_eq!(
            origin.resolve("https://patiala.shop.example/orders?x=1", "/"),
            inside("https://patiala.shop.example/orders?x=1")
        );
        assert_eq!(origin.resolve("https://pay.example:8443/pay", "/"), inside("https://pay.example:8443/pay"));
        // Under the name, not merely ending in it, and never the bare pattern.
        assert!(matches!(origin.resolve("https://evilshop.example/", "/"), Target::Outside(_)));
        assert!(matches!(origin.resolve("https://.shop.example/", "/"), Target::Outside(_)));
        assert!(matches!(origin.resolve("https://pay.example/pay", "/"), Target::Outside(_)));
        // A page on another allowed origin reads its own links against itself.
        assert_eq!(
            origin.resolve("/inventory", "https://patiala.shop.example/orders"),
            inside("https://patiala.shop.example/inventory")
        );
        assert_eq!(origin.resolve("https://shop.example/x", "https://patiala.shop.example/"), inside("/x"));
        assert_eq!(origin.absolute("/orders"), "https://shop.example/orders");
        assert_eq!(origin.absolute("https://patiala.shop.example/orders"), "https://patiala.shop.example/orders");
        assert_eq!(path_of("https://patiala.shop.example/orders?x=1"), "/orders");
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
        assert_eq!(local.absolute("/orders"), "http://10.0.2.2:8095/orders");
        assert!(Origin::parse("https://user@shop.example", &[], false).is_none());
        assert!(Origin::parse("shop.example", &[], false).is_none());
    }
}
