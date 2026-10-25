//! The [`TestClient`](crate::TestClient)'s cookie jar: a small, browser-like
//! store keyed by cookie name and path.

use cookie::Cookie;
use cookie::time::OffsetDateTime;

/// One stored cookie.
#[derive(Clone, Debug)]
struct Stored {
    name: String,
    /// The value exactly as the server sent it (not percent-decoded).
    value: String,
    path: String,
    /// `None` for a session cookie.
    expires_at: Option<OffsetDateTime>,
}

/// Cookies set by responses, sent back on later requests whose path matches.
///
/// Domains are ignored (every request goes to the same in-process app) and
/// `Secure` cookies are sent too, so apps configured with secure cookies can
/// be tested. Expiry uses the wall clock, like a browser.
#[derive(Clone, Debug, Default)]
pub(crate) struct CookieJar {
    cookies: Vec<Stored>,
}

impl CookieJar {
    /// Stores (or, for an expired cookie, removes) the cookie of one
    /// `Set-Cookie` header received for a request to `request_path`.
    /// Malformed headers are ignored, as browsers do.
    pub(crate) fn store(&mut self, request_path: &str, set_cookie: &str) {
        let Ok(cookie) = Cookie::parse(set_cookie) else {
            return;
        };
        if cookie.name().is_empty() {
            return;
        }
        let path = match cookie.path() {
            Some(path) if path.starts_with('/') => path.to_owned(),
            _ => default_path(request_path),
        };
        let now = OffsetDateTime::now_utc();
        let expires_at = match (cookie.max_age(), cookie.expires_datetime()) {
            (Some(max_age), _) => Some(now.saturating_add(max_age)),
            (None, Some(expires)) => Some(expires),
            (None, None) => None,
        };
        self.cookies
            .retain(|c| !(c.name == cookie.name() && c.path == path));
        if expires_at.is_some_and(|at| at <= now) {
            return;
        }
        self.cookies.push(Stored {
            name: cookie.name().to_owned(),
            value: cookie.value().to_owned(),
            path,
            expires_at,
        });
    }

    /// The `Cookie` header for a request to `request_path`, if any cookie matches.
    pub(crate) fn header_for(&self, request_path: &str) -> Option<String> {
        let pairs: Vec<String> = self
            .matching(request_path)
            .map(|c| format!("{}={}", c.name, c.value))
            .collect();
        (!pairs.is_empty()).then(|| pairs.join("; "))
    }

    /// The raw value of cookie `name` that a request to `request_path` would carry.
    pub(crate) fn value_for(&self, name: &str, request_path: &str) -> Option<String> {
        self.matching(request_path)
            .find(|c| c.name == name)
            .map(|c| c.value.clone())
    }

    /// The raw value of the cookie `name` (with the longest path, if several).
    pub(crate) fn get(&self, name: &str) -> Option<String> {
        let now = OffsetDateTime::now_utc();
        self.cookies
            .iter()
            .filter(|c| c.name == name && c.is_live(now))
            .max_by_key(|c| c.path.len())
            .map(|c| c.value.clone())
    }

    /// Stores a session cookie `name=value` for path `/`.
    pub(crate) fn set(&mut self, name: &str, value: &str) {
        self.cookies.retain(|c| !(c.name == name && c.path == "/"));
        self.cookies.push(Stored {
            name: name.to_owned(),
            value: value.to_owned(),
            path: "/".to_owned(),
            expires_at: None,
        });
    }

    /// Removes every cookie named `name`.
    pub(crate) fn remove(&mut self, name: &str) {
        self.cookies.retain(|c| c.name != name);
    }

    /// Removes every cookie.
    pub(crate) fn clear(&mut self) {
        self.cookies.clear();
    }

    /// The live cookies a request to `request_path` carries: longer paths
    /// first, then in the order they were stored.
    fn matching(&self, request_path: &str) -> impl Iterator<Item = &Stored> {
        let now = OffsetDateTime::now_utc();
        let mut matching: Vec<&Stored> = self
            .cookies
            .iter()
            .filter(|c| c.is_live(now) && path_matches(&c.path, request_path))
            .collect();
        matching.sort_by_key(|c| std::cmp::Reverse(c.path.len()));
        matching.into_iter()
    }
}

impl Stored {
    fn is_live(&self, now: OffsetDateTime) -> bool {
        self.expires_at.is_none_or(|at| at > now)
    }
}

/// RFC 6265 §5.1.4: the directory of the request path (`/api/deals/1` →
/// `/api/deals`), or `/`.
fn default_path(request_path: &str) -> String {
    match request_path.rfind('/') {
        Some(0) | None => "/".to_owned(),
        Some(last) => request_path[..last].to_owned(),
    }
}

/// RFC 6265 §5.1.4: `cookie_path` matches `request_path` when they are equal,
/// or when it is a prefix ending at a `/` boundary.
fn path_matches(cookie_path: &str, request_path: &str) -> bool {
    match request_path.strip_prefix(cookie_path) {
        Some("") => true,
        Some(rest) => cookie_path.ends_with('/') || rest.starts_with('/'),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jar(set_cookies: &[(&str, &str)]) -> CookieJar {
        let mut jar = CookieJar::default();
        for (path, header) in set_cookies {
            jar.store(path, header);
        }
        jar
    }

    #[test]
    fn stores_raw_values_and_sends_them_back() {
        let jar = jar(&[
            (
                "/login",
                "session=abc%3D%3D; Path=/; HttpOnly; SameSite=Lax",
            ),
            ("/login", "theme=\"dark\"; Path=/"),
        ]);
        assert_eq!(jar.get("session").as_deref(), Some("abc%3D%3D"));
        assert_eq!(
            jar.header_for("/api/deals").as_deref(),
            Some("session=abc%3D%3D; theme=\"dark\"")
        );
    }

    #[test]
    fn a_later_cookie_with_the_same_name_and_path_replaces_the_first() {
        let jar = jar(&[("/", "a=1; Path=/"), ("/", "a=2; Path=/")]);
        assert_eq!(jar.header_for("/").as_deref(), Some("a=2"));
    }

    #[test]
    fn max_age_zero_and_past_expiry_delete() {
        let mut jar = jar(&[
            ("/", "a=1; Path=/"),
            ("/", "b=2; Path=/"),
            ("/", "c=3; Path=/"),
        ]);
        jar.store("/", "a=; Path=/; Max-Age=0");
        jar.store("/", "b=; Path=/; Expires=Thu, 01 Jan 1970 00:00:00 GMT");
        assert_eq!(jar.header_for("/").as_deref(), Some("c=3"));
        assert_eq!(jar.get("a"), None);
    }

    #[test]
    fn future_expiry_and_positive_max_age_keep_the_cookie() {
        let jar = jar(&[
            ("/", "a=1; Path=/; Max-Age=3600"),
            ("/", "b=2; Path=/; Expires=Fri, 01 Jan 2100 00:00:00 GMT"),
        ]);
        assert_eq!(jar.header_for("/").as_deref(), Some("a=1; b=2"));
    }

    #[test]
    fn paths_scope_cookies() {
        let jar = jar(&[
            ("/", "root=1; Path=/"),
            ("/", "api=2; Path=/api"),
            // No Path attribute: the default path is the request's directory.
            ("/api/deals/1", "deals=3"),
        ]);
        assert_eq!(
            jar.header_for("/api/deals/7").as_deref(),
            Some("deals=3; api=2; root=1"),
            "longer paths first"
        );
        assert_eq!(jar.header_for("/api").as_deref(), Some("api=2; root=1"));
        assert_eq!(
            jar.header_for("/apiary").as_deref(),
            Some("root=1"),
            "a path matches on segment boundaries only"
        );
        assert_eq!(jar.value_for("deals", "/api/other"), None);
        assert_eq!(jar.value_for("api", "/api/x").as_deref(), Some("2"));
    }

    #[test]
    fn same_name_on_two_paths_are_two_cookies() {
        let mut jar = jar(&[("/", "t=root; Path=/"), ("/", "t=api; Path=/api")]);
        assert_eq!(jar.header_for("/api/x").as_deref(), Some("t=api; t=root"));
        assert_eq!(jar.value_for("t", "/api/x").as_deref(), Some("api"));
        assert_eq!(jar.value_for("t", "/web").as_deref(), Some("root"));
        jar.remove("t");
        assert_eq!(jar.header_for("/api/x"), None);
    }

    #[test]
    fn malformed_headers_are_ignored() {
        let jar = jar(&[("/", "no-equals-sign"), ("/", "=nameless")]);
        assert_eq!(jar.header_for("/"), None);
    }

    #[test]
    fn set_and_clear() {
        let mut jar = CookieJar::default();
        jar.set("a", "1");
        assert_eq!(jar.header_for("/any/path").as_deref(), Some("a=1"));
        jar.clear();
        assert_eq!(jar.header_for("/"), None);
    }
}
