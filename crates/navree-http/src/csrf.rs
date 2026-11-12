//! The names CSRF protection goes by, in one place: the session layer
//! (navree-session) sets and checks the token under them, and pages and
//! forms (navree-ui) carry it under them, so the two cannot drift apart.

/// The cookie holding the CSRF token for scripts (not `HttpOnly`). A script
/// sends its URL-decoded value back as [`XSRF_HEADER`].
pub const XSRF_COOKIE: &str = "XSRF-TOKEN";

/// The header carrying the URL-encoded [`XSRF_COOKIE`] value, as Axios sends
/// it.
pub const XSRF_HEADER: &str = "x-xsrf-token";

/// The header carrying the CSRF token as it is.
pub const CSRF_HEADER: &str = "x-csrf-token";

/// The field a plain HTML form carries the CSRF token in.
pub const CSRF_FIELD: &str = "_csrf";

/// Whether a browser's `Origin` names the host the request was sent to: the request's
/// `Host`, else its URI's authority (HTTP/2 and HTTP/3 send `:authority` and may send no
/// `Host`). The hosts are compared without case and the ports as numbers, a scheme's
/// default port (80 for `http`, 443 for `https`) written or not. An `Origin` that is
/// `null`, has another scheme, a path, a user or a port that isn't one, never matches.
///
/// ```
/// use http::{HeaderMap, Uri};
/// use navree_http::csrf::origin_is_host;
///
/// let mut headers = HeaderMap::new();
/// headers.insert("host", "App.Example:443".parse().unwrap());
/// let uri = Uri::from_static("/x");
/// assert!(origin_is_host("https://app.example", &headers, &uri));
/// assert!(!origin_is_host("https://app.example.evil.example", &headers, &uri));
/// // HTTP/2: no Host, the URI's authority.
/// let h2 = Uri::from_static("https://app.example/x");
/// assert!(origin_is_host("https://app.example", &HeaderMap::new(), &h2));
/// ```
pub fn origin_is_host(origin: &str, headers: &http::HeaderMap, uri: &http::Uri) -> bool {
    let Some((scheme, authority)) = origin.trim().split_once("://") else {
        return false;
    };
    let default = match scheme.to_ascii_lowercase().as_str() {
        "http" => 80,
        "https" => 443,
        _ => return false,
    };
    if authority.is_empty() || authority.contains(['/', '?', '#', '@']) {
        return false;
    }
    let host = headers
        .get(http::header::HOST)
        .and_then(|host| host.to_str().ok())
        .map(str::to_owned)
        .or_else(|| {
            uri.authority()
                .map(|authority| authority.as_str().to_owned())
        });
    let (Some(origin), Some(target)) = (
        host_and_port(authority, default),
        host.as_deref()
            .and_then(|host| host_and_port(host.trim(), default)),
    ) else {
        return false;
    };
    origin.0.eq_ignore_ascii_case(target.0) && origin.1 == target.1
}

/// `authority` as its host and its port, `default` when none is written; `None` for one
/// that doesn't read as a host and a port.
fn host_and_port(authority: &str, default: u16) -> Option<(&str, u16)> {
    if authority.contains('@') || authority.is_empty() {
        return None;
    }
    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        let end = rest.find(']')?;
        let host = &authority[..end + 2];
        match &rest[end + 1..] {
            "" => (host, None),
            port => (host, Some(port.strip_prefix(':')?)),
        }
    } else {
        match authority.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        }
    };
    if host.is_empty() {
        return None;
    }
    let port = match port {
        None => default,
        Some(port) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => {
            port.parse().ok()?
        }
        Some(_) => return None,
    };
    Some((host, port))
}

/// A path whose posts the session layer checks by their origin instead of the token: a
/// route that changes nothing on the server and nothing of the user's, only a preference
/// the browser keeps for itself (a theme cookie), so that a page cached for everyone,
/// which carries no token, can post to it. A module contributes one with
/// `m.contribute(SameOriginOnly("/_navree/ui/appearance"))`; navree-ui's appearance route
/// is one.
///
/// A request with a state-changing method (any of them, not only `POST`) to the path
/// passes when the browser says it came from this origin (`Sec-Fetch-Site: same-origin`),
/// or, from a browser that doesn't say, when its `Origin` is this host
/// ([`origin_is_host`]); one from another site, a sibling subdomain included, is refused
/// with a 403 as a token mismatch is. A request that sends neither header passes only
/// without a session cookie (a script, which has no session to ride on); with one it is
/// refused. The path is matched exactly, and the route should refuse requests from other
/// sites itself as well.
///
/// The check is weaker than the token: it trusts the browser's headers, and so any page
/// of this origin (and any script that can make the browser post from it). Use it only
/// for a route whose worst outcome is a changed preference; the session layer logs every
/// such path on the `navree::security` target when it starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SameOriginOnly(pub &'static str);

#[cfg(test)]
mod tests {
    use super::*;

    /// Header names are lower case: `HeaderName::from_static` takes no other.
    #[test]
    fn the_header_names_are_valid_static_header_names() {
        for name in [XSRF_HEADER, CSRF_HEADER] {
            assert_eq!(http::HeaderName::from_static(name).as_str(), name);
        }
    }
}
