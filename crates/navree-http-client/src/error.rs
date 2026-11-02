//! [`HttpClientError`]: everything that can go wrong with an outgoing request.

use http::{Method, StatusCode};
use navree_kernel::BoxError;

/// An error raised while building, sending or decoding an outgoing request.
///
/// URLs in messages are shown without their query string and credentials
/// (both often carry secrets), and as scheme and host alone for a request made
/// with [`RequestBuilder::secret_url`](crate::RequestBuilder::secret_url);
/// header values and bodies never appear.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum HttpClientError {
    /// The request could not be built (bad URL, header, or body encoding).
    #[error("invalid request: {0}")]
    Build(String),
    /// The SSRF policy refused the destination (never retried).
    #[error("request to `{host}` blocked by the SSRF policy: {reason}")]
    Blocked {
        /// The refused host (of the request URL or of a redirect target).
        host: String,
        /// Why: the refused range and address.
        reason: String,
    },
    /// The connection could not be opened (refused, reset, DNS failure,
    /// connect timeout). Retried for idempotent requests.
    #[error("cannot connect to {url}: {source}")]
    Connect {
        /// The request URL (without query string).
        url: String,
        /// The underlying error.
        #[source]
        source: BoxError,
    },
    /// The attempt took longer than its timeout (not retried).
    #[error("request to {url} timed out")]
    Timeout {
        /// The request URL (without query string).
        url: String,
    },
    /// The response body is larger than
    /// [`HttpClientOptions::max_response_bytes`](crate::HttpClientOptions::max_response_bytes)
    /// (not retried).
    #[error("the response from {url} is larger than {limit} bytes")]
    BodyTooLarge {
        /// The request URL (without query string).
        url: String,
        /// The configured limit.
        limit: u64,
    },
    /// Any other transport failure (protocol error, body read failure, too
    /// many redirects).
    #[error("request to {url} failed: {source}")]
    Request {
        /// The request URL (without query string).
        url: String,
        /// The underlying error.
        #[source]
        source: BoxError,
    },
    /// A fake client received a request no stub matches.
    #[error("no fake response matches {method} {url}")]
    NoFakeResponse {
        /// The request method.
        method: Method,
        /// The request URL (without query string).
        url: String,
    },
    /// [`Response::error_for_status`](crate::Response::error_for_status) got
    /// a client or server error status.
    #[error("{url} answered with status {status}")]
    Status {
        /// The response status.
        status: StatusCode,
        /// The response URL (without query string).
        url: String,
    },
    /// The response body is not the expected JSON.
    #[error("cannot decode the response body: {0}")]
    Decode(#[source] serde_json::Error),
}

/// How messages and logs show `url`: [`display_url`], or scheme and host alone
/// when the whole URL is a secret
/// ([`RequestBuilder::secret_url`](crate::RequestBuilder::secret_url)).
pub(crate) fn shown_url(url: &url::Url, secret: bool) -> String {
    if !secret {
        return display_url(url);
    }
    let host = url.host_str().unwrap_or_default();
    match url.port() {
        Some(port) => format!("{}://{host}:{port}/[hidden]", url.scheme()),
        None => format!("{}://{host}/[hidden]", url.scheme()),
    }
}

/// `url` without query string, fragment or credentials, for messages and logs.
pub(crate) fn display_url(url: &url::Url) -> String {
    let mut shown = url.clone();
    shown.set_query(None);
    shown.set_fragment(None);
    // Only fails for URLs that cannot have credentials, which have none.
    let _ = shown.set_username("");
    let _ = shown.set_password(None);
    shown.to_string()
}
