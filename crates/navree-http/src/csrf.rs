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

/// A path whose posts the session layer checks by their origin instead of the token: a
/// route that changes nothing on the server and nothing of the user's, only a preference
/// the browser keeps for itself (a theme cookie), so that a page cached for everyone,
/// which carries no token, can post to it. A module contributes one with
/// `m.contribute(SameOriginOnly("/_navree/ui/appearance"))`; navree-ui's appearance route
/// is one.
///
/// A post to it passes when the browser says it came from this origin
/// (`Sec-Fetch-Site: same-origin`), or, from a browser that doesn't say, when its `Origin`
/// is this host; a post from another site, a sibling subdomain included, is refused with a
/// 403 as a token mismatch is. A client that sends neither header (an old browser, a
/// script) passes: what it can change is its own preference. The path is matched exactly,
/// and the route should refuse posts from other sites itself as well.
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
