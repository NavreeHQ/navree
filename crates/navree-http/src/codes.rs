//! The framework's stable error codes: the `code` member of the JSON error
//! body.
//!
//! A front end maps `code` to a translation key, so a refusal reaches the
//! user in their own language whatever the English `message` says. These
//! values are part of Navree's public contract: they do not change between
//! releases, and new codes are only ever added.
//!
//! [`AppError::from_status`](crate::AppError::from_status) derives a code
//! from the status (the reason phrase in snake case, `unauthenticated` for
//! 401 and `internal_error` for 500); the constructors below set theirs.
//! Modules set more specific codes on their own errors (`invalid_credentials`,
//! `workspace_not_found`, ...), and apps set theirs with
//! [`AppError::with_code`](crate::AppError::with_code).
//!
//! | Code | Status | Sent by |
//! |---|---|---|
//! | `bad_request` | 400 | `AppError::bad_request`, malformed input |
//! | `validation_failed` | 400 | `AppError::validation`: field errors in `errors` |
//! | `invalid_filter` | 400 | `AppError::invalid_filter`: an unknown or malformed list filter |
//! | `unauthenticated` | 401 | `AppError::unauthenticated`: no user, or bad credentials |
//! | `two_factor_required` | 401 | `AppError::two_factor_required` |
//! | `invalid_signature` | 401 | a signed URL that does not verify |
//! | `forbidden` | 403 | `AppError::forbidden`: access control refuses |
//! | `csrf_token_mismatch` | 403 | the CSRF check fails |
//! | `demo_read_only` | 403 | `AppError::demo_read_only`: a write in a demo workspace |
//! | `not_found` | 404 | `AppError::not_found`, and a path no route matches |
//! | `method_not_allowed` | 405 | a path that exists for other methods |
//! | `conflict` | 409 | `AppError::conflict` |
//! | `limit_reached` | 409 | `AppError::limit_reached`: a plan or quota limit |
//! | `payload_too_large` | 413 | `AppError::payload_too_large` |
//! | `unsupported_media_type` | 415 | a JSON or form body of another content type |
//! | `page_expired` | 419 | `AppError::page_expired`: a page left open too long |
//! | `too_many_requests` | 429 | `AppError::too_many_requests`, rate limits |
//! | `internal_error` | 500 | `AppError::internal`, `?` on any error, panics |
//! | `service_unavailable` | 503 | `AppError::service_unavailable` |
//! | `maintenance` | 503 | maintenance mode |
//! | `request_timeout` | 503 | a request that ran out of time |

/// 400: malformed input ([`AppError::bad_request`](crate::AppError::bad_request)).
pub const BAD_REQUEST: &str = "bad_request";
/// 400 with field errors ([`AppError::validation`](crate::AppError::validation)).
pub const VALIDATION_FAILED: &str = "validation_failed";
/// 400: an unknown or malformed list filter
/// ([`AppError::invalid_filter`](crate::AppError::invalid_filter)).
pub const INVALID_FILTER: &str = "invalid_filter";
/// 401: the request is not authenticated
/// ([`AppError::unauthenticated`](crate::AppError::unauthenticated)).
pub const UNAUTHENTICATED: &str = "unauthenticated";
/// 401: the password was right but a second factor is needed
/// ([`AppError::two_factor_required`](crate::AppError::two_factor_required)).
pub const TWO_FACTOR_REQUIRED: &str = "two_factor_required";
/// 401: a signed URL whose signature is missing, wrong or expired.
pub const INVALID_SIGNATURE: &str = "invalid_signature";
/// 403: the user may not do this ([`AppError::forbidden`](crate::AppError::forbidden)).
pub const FORBIDDEN: &str = "forbidden";
/// 403: the CSRF token is missing or does not match the session
/// ([`AppError::csrf_mismatch`](crate::AppError::csrf_mismatch)).
pub const CSRF_TOKEN_MISMATCH: &str = "csrf_token_mismatch";
/// 403: a write in a demo workspace
/// ([`AppError::demo_read_only`](crate::AppError::demo_read_only)).
pub const DEMO_READ_ONLY: &str = "demo_read_only";
/// 404 ([`AppError::not_found`](crate::AppError::not_found)).
pub const NOT_FOUND: &str = "not_found";
/// 405: the path exists for other methods.
pub const METHOD_NOT_ALLOWED: &str = "method_not_allowed";
/// 409 ([`AppError::conflict`](crate::AppError::conflict)).
pub const CONFLICT: &str = "conflict";
/// 409: a plan or quota limit is reached
/// ([`AppError::limit_reached`](crate::AppError::limit_reached)).
pub const LIMIT_REACHED: &str = "limit_reached";
/// 413: the body is too large
/// ([`AppError::payload_too_large`](crate::AppError::payload_too_large)).
pub const PAYLOAD_TOO_LARGE: &str = "payload_too_large";
/// 415: a JSON or form body sent with another content type.
pub const UNSUPPORTED_MEDIA_TYPE: &str = "unsupported_media_type";
/// 419: a page that was open too long or whose session changed
/// ([`AppError::page_expired`](crate::AppError::page_expired)).
pub const PAGE_EXPIRED: &str = "page_expired";
/// 429: a rate limit is exceeded
/// ([`AppError::too_many_requests`](crate::AppError::too_many_requests)).
pub const TOO_MANY_REQUESTS: &str = "too_many_requests";
/// 500: an unexpected error or a panic ([`AppError::internal`](crate::AppError::internal)).
pub const INTERNAL_ERROR: &str = "internal_error";
/// 503 ([`AppError::service_unavailable`](crate::AppError::service_unavailable)).
pub const SERVICE_UNAVAILABLE: &str = "service_unavailable";
/// 503: the app is in maintenance mode.
pub const MAINTENANCE: &str = "maintenance";
/// 503: the request ran out of time.
pub const REQUEST_TIMEOUT: &str = "request_timeout";

/// Every code above, in the order of the table.
pub const ALL: &[&str] = &[
    BAD_REQUEST,
    VALIDATION_FAILED,
    INVALID_FILTER,
    UNAUTHENTICATED,
    TWO_FACTOR_REQUIRED,
    INVALID_SIGNATURE,
    FORBIDDEN,
    CSRF_TOKEN_MISMATCH,
    DEMO_READ_ONLY,
    NOT_FOUND,
    METHOD_NOT_ALLOWED,
    CONFLICT,
    LIMIT_REACHED,
    PAYLOAD_TOO_LARGE,
    UNSUPPORTED_MEDIA_TYPE,
    PAGE_EXPIRED,
    TOO_MANY_REQUESTS,
    INTERNAL_ERROR,
    SERVICE_UNAVAILABLE,
    MAINTENANCE,
    REQUEST_TIMEOUT,
];

/// Whether `code` is a well-formed error code: 1 to 64 characters of
/// lower-case ASCII letters, digits and `_`, starting with a letter.
pub fn is_valid(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 64
        && code.starts_with(|c: char| c.is_ascii_lowercase())
        && code
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn every_code_is_unique_and_snake_case() {
        let unique: HashSet<&str> = ALL.iter().copied().collect();
        assert_eq!(unique.len(), ALL.len(), "duplicate codes in {ALL:?}");
        for code in ALL {
            assert!(is_valid(code), "{code}");
        }
    }

    #[test]
    fn well_formed_codes_are_lower_snake_case() {
        assert!(is_valid("campaign_already_running"));
        assert!(is_valid("limit_reached_2"));
        assert!(!is_valid(""));
        assert!(!is_valid("Campaign"));
        assert!(!is_valid("2fa_required"));
        assert!(!is_valid("limit-reached"));
        assert!(!is_valid("limit reached"));
        assert!(!is_valid(&"a".repeat(65)));
    }
}
