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
