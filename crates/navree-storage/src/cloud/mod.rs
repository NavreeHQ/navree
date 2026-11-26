//! What the cloud stores share: one HTTP client, one way of trying again,
//! one encoding and one check of object keys, one cache of credentials and
//! one way of reading an XML answer.
//!
//! A driver of a cloud store is a [`Store`](crate::store::Store) that puts
//! these together, in this order for every request:
//!
//! 1. [`keys::object_url`] for the address of the object: it checks the key
//!    ([`keys::check_key`]), encodes it ([`keys::encode_key`]) and makes
//!    sure the address stays under the bucket. A driver that puts a key
//!    anywhere else (a query, a body) checks it first.
//! 2. [`retry::with_backoff`] around one attempt. Inside the attempt, so
//!    that each one has a credential that is still good:
//!    [`credentials::Provider::credential`], then the request is built and
//!    signed, then [`http::Client::send`] and [`http::Response::success`].
//! 3. [`http::Response::bytes_up_to`] and [`xml::parse`] for an answer, or
//!    [`http::Response::resuming_stream`] for a file that is opened: it
//!    asks for the rest when the body stops before the file does.
//!
//! Every part fails with [`Error`], which becomes a [`StoreError`] at the
//! store's edge (a 404 is "not found"). Neither its text nor its `Debug`
//! form holds a credential, the query of a URL or the body of an answer.
//!
//! ```no_run
//! use navree_storage::cloud::credentials::Provider;
//! use navree_storage::cloud::http::{Client, Request, Response};
//! use navree_storage::cloud::retry::{Replay, RetryPolicy, with_backoff};
//! use navree_storage::cloud::{keys, xml};
//! use navree_storage::store::{Method, StoreResult, Url};
//!
//! /// The size of one object of a bucket that answers in XML.
//! async fn size_of(
//!     client: &Client,
//!     tokens: &Provider<String>,
//!     bucket: &Url,
//!     key: &str,
//! ) -> StoreResult<u64> {
//!     let url = keys::object_url(bucket, key)?;
//!     let policy = RetryPolicy::default();
//!     let answer: Response = with_backoff(&policy, Replay::Safe, || async {
//!         let token = tokens.credential().await?;
//!         let request = Request::new(Method::GET, url.clone())
//!             .secret("authorization", &format!("Bearer {}", token.secret))?;
//!         client.send(request).await?.success().await
//!     })
//!     .await?;
//!     let body = answer.bytes_up_to(64 * 1024).await?;
//!     let size = xml::parse(&body)?.value("Size").unwrap_or("0").parse();
//!     Ok(size.unwrap_or(0))
//! }
//! ```

#[cfg(feature = "azure")]
pub mod azure;
pub mod credentials;
#[cfg(feature = "gcs")]
pub mod gcs;
pub mod http;
pub mod keys;
pub mod retry;
#[cfg(feature = "s3")]
pub mod s3;
pub mod xml;

use std::borrow::Cow;
use std::fmt;

use ::http::StatusCode;
use navree_kernel::BoxError;

use crate::store::StoreError;

/// What a part of the cloud core gives.
pub type Result<T> = std::result::Result<T, Error>;

/// Maximum number of pages a single `Store::list` call follows before refusing
/// an unbounded pagination stream.
pub(crate) const MAX_LIST_PAGES: usize = 100_000;

/// Maximum number of object entries a single `Store::list` call collects in
/// memory.
pub(crate) const MAX_LIST_ENTRIES: usize = 10_000_000;

/// What went wrong, as far as trying again and reporting care.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// No connection could be made (the name did not resolve, the port was
    /// closed, TLS failed): nothing was sent.
    Connect,
    /// The time allowed for the request ran out.
    Timeout,
    /// The connection dropped before the answer was whole.
    Interrupted,
    /// The answer's status is not a success: see [`Error::status`].
    Status,
    /// The provider said, in the body of an answer, to try again.
    Busy,
    /// The request was not sent: plain HTTP where it is not allowed, a
    /// header that cannot be sent, a URL that is not `http` or `https`.
    Refused,
    /// An answer that cannot be used: longer than its limit, or not the
    /// XML it should be.
    Answer,
    /// An object key no store is asked for: empty, with a `.` or `..`
    /// segment, or with a control character.
    Key,
    /// No credential could be had.
    Credential,
    /// Anything else.
    Other,
}

/// How a part of the cloud core fails.
///
/// Its text says what failed and, for an answer that is not a success, the
/// status and the provider's [code](Error::with_code) once a driver has
/// given it. It never holds a credential or the query of a URL, and
/// neither its text nor its `Debug` form shows the [body](Error::body) of
/// an answer.
///
/// ```
/// use http::StatusCode;
/// use navree_storage::cloud::retry::Replay;
/// use navree_storage::cloud::{Error, ErrorKind};
/// use navree_storage::store::StoreError;
///
/// let busy = Error::answered(StatusCode::SERVICE_UNAVAILABLE, None);
/// assert_eq!(busy.kind(), ErrorKind::Status);
/// assert_eq!(busy.to_string(), "the store answered 503 Service Unavailable");
/// assert!(busy.retryable(Replay::IfNotSent));
///
/// let body = "<Error><Code>AccessDenied</Code></Error>".to_owned();
/// let denied = Error::answered(StatusCode::FORBIDDEN, Some(body));
/// assert!(!denied.retryable(Replay::Safe));
/// assert!(!format!("{denied} {denied:?}").contains("AccessDenied"));
/// // The provider's code, once a driver has taken it out of the body.
/// let denied = denied.with_code("AccessDenied");
/// assert_eq!(denied.to_string(), "the store answered 403 Forbidden (AccessDenied)");
///
/// let missing = Error::answered(StatusCode::NOT_FOUND, None);
/// assert!(matches!(StoreError::from(missing), StoreError::NotFound));
/// ```
pub struct Error {
    kind: ErrorKind,
    message: Cow<'static, str>,
    status: Option<StatusCode>,
    code: Option<String>,
    body: Option<String>,
    source: Option<BoxError>,
}

impl Error {
    /// A failure of `kind` that says `message`. The message is shown in
    /// errors and logs: it holds no credential, no token and no query of a
    /// URL.
    pub fn new(kind: ErrorKind, message: impl Into<Cow<'static, str>>) -> Self {
        Self {
            kind,
            message: message.into(),
            status: None,
            code: None,
            body: None,
            source: None,
        }
    }

    /// An answer whose `status` is not a success, with what could be read
    /// of its `body`.
    pub fn answered(status: StatusCode, body: Option<String>) -> Self {
        Self {
            kind: ErrorKind::Status,
            message: format!("the store answered {status}").into(),
            status: Some(status),
            code: None,
            body,
            source: None,
        }
    }

    /// An answer that says, in its body, to try again: S3 can answer a
    /// request that completes an upload with `200 OK` and an `InternalError`
    /// or `SlowDown` inside. [`with_backoff`](retry::with_backoff) runs the
    /// attempt again. `message` names what the provider said.
    pub fn busy(message: impl Into<Cow<'static, str>>) -> Self {
        Self::new(ErrorKind::Busy, message)
    }

    /// This failure, caused by `source`. The cause is shown with the error,
    /// so it holds no credential either.
    #[must_use]
    pub fn with_source(mut self, source: impl Into<BoxError>) -> Self {
        self.source = Some(source.into());
        self
    }

    /// This failure, named as the provider names it (`AccessDenied`,
    /// `NoSuchBucket`, `AuthenticationFailed`): the code a driver takes out
    /// of the [body](Self::body). The text of the error shows it after the
    /// message. Only a short name is taken (letters, digits, `.`, `_` and
    /// `-`, at most 64 of them); anything else is left out, so nothing
    /// else of an answer reaches the text of an error.
    #[must_use]
    pub fn with_code(mut self, code: &str) -> Self {
        let is_a_name = !code.is_empty()
            && code.len() <= 64
            && code
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'));
        self.code = is_a_name.then(|| code.to_owned());
        self
    }

    /// What went wrong.
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// The provider's name for the failure, when a driver
    /// [gave](Self::with_code) one.
    pub fn code(&self) -> Option<&str> {
        self.code.as_deref()
    }

    /// The status of the answer, when the failure is one.
    pub fn status(&self) -> Option<StatusCode> {
        self.status
    }

    /// What the provider said in an answer that is not a success: at most
    /// the first 64 KiB. A driver takes the provider's error code out of
    /// it. It can repeat what was signed (a session token among it), so it
    /// is never logged or put into an error whole.
    pub fn body(&self) -> Option<&str> {
        self.body.as_deref()
    }

    /// Whether the attempt that failed this way is worth running again:
    ///
    /// - always when nothing was sent ([`ErrorKind::Connect`]), when the
    ///   provider said to try again ([`ErrorKind::Busy`]) and when it
    ///   answered `408`, `429` or a `5xx` status;
    /// - when the time ran out or the connection dropped
    ///   ([`ErrorKind::Timeout`], [`ErrorKind::Interrupted`]), only if
    ///   `replay` says a second arrival does no harm;
    /// - never otherwise: another `4xx` status, a refused request, an
    ///   answer that cannot be used, a key, a credential.
    pub fn retryable(&self, replay: retry::Replay) -> bool {
        match self.kind {
            ErrorKind::Connect | ErrorKind::Busy => true,
            ErrorKind::Status => self.status.is_some_and(|status| {
                status.is_server_error()
                    || status == StatusCode::TOO_MANY_REQUESTS
                    || status == StatusCode::REQUEST_TIMEOUT
            }),
            ErrorKind::Timeout | ErrorKind::Interrupted => replay == retry::Replay::Safe,
            ErrorKind::Refused
            | ErrorKind::Answer
            | ErrorKind::Key
            | ErrorKind::Credential
            | ErrorKind::Other => false,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)?;
        match &self.code {
            Some(code) => write!(f, " ({code})"),
            None => Ok(()),
        }
    }
}

/// Everything but the body of the answer.
impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Error")
            .field("kind", &self.kind)
            .field("message", &self.message)
            .field("status", &self.status)
            .field("code", &self.code)
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn std::error::Error + 'static))
    }
}

/// A `404` is [`StoreError::NotFound`]; everything else is
/// [`StoreError::Failed`] with this error inside.
impl From<Error> for StoreError {
    fn from(error: Error) -> Self {
        if error.status == Some(StatusCode::NOT_FOUND) {
            StoreError::NotFound
        } else {
            StoreError::Failed(Box::new(error))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::retry::Replay;
    use super::*;

    #[test]
    fn what_is_run_again_is_what_did_not_arrive_or_was_asked_again() {
        let kind = |kind| Error::new(kind, "x");
        for replay in [Replay::Safe, Replay::IfNotSent] {
            assert!(kind(ErrorKind::Connect).retryable(replay));
            assert!(Error::busy("SlowDown").retryable(replay));
            for status in [408, 429, 500, 502, 503, 504, 599] {
                let status = StatusCode::from_u16(status).unwrap();
                assert!(Error::answered(status, None).retryable(replay), "{status}");
            }
            for status in [301, 304, 400, 401, 403, 404, 409, 412, 416] {
                let status = StatusCode::from_u16(status).unwrap();
                assert!(!Error::answered(status, None).retryable(replay), "{status}");
            }
            for never in [
                ErrorKind::Refused,
                ErrorKind::Answer,
                ErrorKind::Key,
                ErrorKind::Credential,
                ErrorKind::Other,
            ] {
                assert!(!kind(never).retryable(replay), "{never:?}");
            }
        }
        for unsure in [ErrorKind::Timeout, ErrorKind::Interrupted] {
            assert!(kind(unsure).retryable(Replay::Safe), "{unsure:?}");
            assert!(!kind(unsure).retryable(Replay::IfNotSent), "{unsure:?}");
        }
    }

    #[test]
    fn the_body_of_an_answer_is_kept_and_never_shown() {
        let body = "<Error><Code>SignatureDoesNotMatch</Code>\
                    <CanonicalRequest>x-amz-security-token:s3ss10n</CanonicalRequest></Error>";
        let error = Error::answered(StatusCode::FORBIDDEN, Some(body.to_owned()))
            .with_source(std::io::Error::other("a cause"));
        assert_eq!(error.body(), Some(body));
        assert_eq!(error.status(), Some(StatusCode::FORBIDDEN));
        assert_eq!(error.to_string(), "the store answered 403 Forbidden");
        let shown = format!("{error} {error:?}");
        assert!(!shown.contains("s3ss10n"), "{shown}");
        assert!(!shown.contains("SignatureDoesNotMatch"), "{shown}");
        assert!(shown.contains("a cause"), "{shown}");
        assert_eq!(
            std::error::Error::source(&error).unwrap().to_string(),
            "a cause"
        );
    }

    #[test]
    fn a_provider_s_code_is_shown_and_nothing_else_of_an_answer_can_be() {
        let body = "<Error><Code>AccessDenied</Code><Message>s3cret</Message></Error>";
        let denied =
            Error::answered(StatusCode::FORBIDDEN, Some(body.to_owned())).with_code("AccessDenied");
        assert_eq!(denied.code(), Some("AccessDenied"));
        assert_eq!(
            denied.to_string(),
            "the store answered 403 Forbidden (AccessDenied)"
        );
        assert!(format!("{denied:?}").contains("AccessDenied"));
        assert!(!format!("{denied} {denied:?}").contains("s3cret"));
        for code in ["Request.Timeout_1-a", "InternalError", &"x".repeat(64)] {
            let error = Error::busy("try again").with_code(code);
            assert_eq!(error.code(), Some(code));
            assert_eq!(error.to_string(), format!("try again ({code})"));
        }
        // What is not a short name is no code: not the body, not a line of it.
        for not_a_code in [
            "",
            body,
            "Access Denied",
            "a\nb",
            "token=s3cret",
            "ਨਾਮ",
            &"x".repeat(65),
        ] {
            let error = Error::answered(StatusCode::FORBIDDEN, None)
                .with_code("AccessDenied")
                .with_code(not_a_code);
            assert_eq!(error.code(), None, "{not_a_code:?}");
            assert_eq!(error.to_string(), "the store answered 403 Forbidden");
        }
    }

    #[test]
    fn only_a_404_is_not_found() {
        let not_found = Error::answered(StatusCode::NOT_FOUND, None);
        assert!(matches!(StoreError::from(not_found), StoreError::NotFound));
        for error in [
            Error::answered(StatusCode::FORBIDDEN, None),
            Error::answered(StatusCode::GONE, None),
            Error::new(ErrorKind::Key, "the key is empty"),
        ] {
            let text = error.to_string();
            match StoreError::from(error) {
                StoreError::Failed(source) => assert_eq!(source.to_string(), text),
                other => panic!("{other:?}"),
            }
        }
    }
}
