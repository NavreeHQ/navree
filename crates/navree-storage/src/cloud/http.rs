//! [`Client`]: the HTTP client of the cloud stores, on reqwest with rustls
//! and ring, with [`Request`] and [`Response`].
//!
//! - **HTTPS, unless an endpoint says `http://`.** A client sends plain
//!   HTTP only to the host and port of the endpoint it was made for, and
//!   only when that endpoint is an `http://` URL (MinIO, Azurite and other
//!   stores on a developer's machine). A client for the credential services
//!   inside a machine or its network ([`Client::for_metadata`]) may send
//!   plain HTTP anywhere, because those services speak nothing else.
//! - **No redirects.** An answer that points elsewhere is given to the
//!   driver as it is: a signed request is never sent again to another
//!   address.
//! - **The times a request may take** are the ones the cloud disks have
//!   always had: [`REQUEST_TIMEOUT`] for the whole request, body included,
//!   and [`CONNECT_TIMEOUT`] to connect.
//! - **HTTP/1.1, and no compression**, so `Content-Length` is the size of
//!   the object.
//! - **Nothing secret is shown.** The `Debug` form of a [`Request`] has no
//!   query and no header value, and a failure has no URL.

use std::fmt;
use std::ops::Range;
use std::time::Duration;

use ::http::header::{CONTENT_LENGTH, HeaderMap, HeaderName, HeaderValue};
use ::http::{Method, StatusCode};
use bytes::{Bytes, BytesMut};
use futures_util::StreamExt;
use futures_util::stream::BoxStream;
use url::Url;

use super::{Error, ErrorKind, Result};

/// The longest one request may take, from connecting to the last byte of
/// the answer's body.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// The longest connecting to a store may take.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// The longest connecting to a credential service inside the machine or
/// its network may take: where there is none, the next way of signing in
/// is tried soon.
pub const METADATA_CONNECT_TIMEOUT: Duration = Duration::from_secs(1);

/// The most that is kept of the body of an answer that is not a success.
pub const ERROR_BODY_LIMIT: usize = 64 * 1024;

/// What the client calls itself.
const USER_AGENT: &str = concat!("navree-storage/", env!("CARGO_PKG_VERSION"));

/// Where a client may send plain HTTP.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Plain {
    /// Nowhere.
    Never,
    /// To one host and port: the endpoint the disk is configured with.
    To { host: String, port: u16 },
    /// Anywhere: the credential services inside a machine or its network.
    Anywhere,
}

/// The HTTP client of a cloud store. Cheap to clone: the clones share one
/// pool of connections.
///
/// ```no_run
/// use navree_storage::cloud::http::{Client, Request};
/// use navree_storage::store::{Method, Url};
///
/// # async fn example() -> navree_storage::cloud::Result<()> {
/// // MinIO on this machine: plain HTTP to 127.0.0.1:9000 and nowhere else.
/// let client = Client::for_endpoint(Some("http://127.0.0.1:9000"))?;
/// let url = Url::parse("http://127.0.0.1:9000/ludhiana-invoices/INV-1101.pdf").unwrap();
/// let answer = client.send(Request::new(Method::GET, url)).await?.success().await?;
/// let pdf = answer.bytes().await?;
/// # drop(pdf);
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct Client {
    inner: reqwest::Client,
    plain: Plain,
}

impl Client {
    /// A client for a store at `endpoint`, or at the provider's own address
    /// when there is none. Requests go over HTTPS; plain HTTP is sent only
    /// when `endpoint` is an `http://` URL, and then only to its host and
    /// port. An endpoint that is not an `http` or `https` URL is refused,
    /// without being repeated in the error.
    pub fn for_endpoint(endpoint: Option<&str>) -> Result<Client> {
        let plain = match endpoint {
            None => Plain::Never,
            Some(endpoint) => plain_to(endpoint)?,
        };
        Self::build(plain, CONNECT_TIMEOUT, true)
    }

    /// A client for the credential services inside a machine or its
    /// network: the instance metadata services of AWS, Google Cloud and
    /// Azure, and the credential endpoints of containers. They speak plain
    /// HTTP, so this client sends it anywhere; it waits only
    /// [`METADATA_CONNECT_TIMEOUT`] to connect, and never goes through a
    /// proxy. Use it for nothing else: a driver decides which addresses it
    /// asks for credentials.
    pub fn for_metadata() -> Result<Client> {
        Self::build(Plain::Anywhere, METADATA_CONNECT_TIMEOUT, false)
    }

    fn build(plain: Plain, connect_timeout: Duration, proxy: bool) -> Result<Client> {
        // reqwest is built with rustls and no crypto provider of its own:
        // install ring. An error only means the process already has one.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let mut builder = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(REQUEST_TIMEOUT)
            .connect_timeout(connect_timeout)
            .http1_only()
            .no_gzip()
            .no_brotli()
            .no_zstd()
            .no_deflate()
            .redirect(reqwest::redirect::Policy::none())
            .https_only(plain == Plain::Never);
        if !proxy {
            builder = builder.no_proxy();
        }
        let inner = builder.build().map_err(|error| {
            Error::new(ErrorKind::Other, "the HTTP client could not be built")
                .with_source(error.without_url())
        })?;
        Ok(Client { inner, plain })
    }

    /// Sends `request` and gives the answer, whatever its status: see
    /// [`Response::success`]. A request this client may not send (plain
    /// HTTP to another address than its endpoint) fails with
    /// [`ErrorKind::Refused`] and nothing leaves the machine.
    pub async fn send(&self, request: Request) -> Result<Response> {
        self.allows(&request.url)?;
        let Request {
            method,
            url,
            mut headers,
            body,
            timeout,
        } = request;
        // The length is said outright for a request that has a body or
        // writes one: an empty file is written with `Content-Length: 0`,
        // which the providers ask for and nothing below adds. A request
        // that only asks says none.
        let writes = matches!(method, Method::PUT | Method::POST | Method::PATCH);
        if writes || !body.is_empty() {
            headers.insert(CONTENT_LENGTH, HeaderValue::from(body.len() as u64));
        }
        let mut builder = self.inner.request(method, url).headers(headers);
        if !body.is_empty() {
            builder = builder.body(body);
        }
        if let Some(timeout) = timeout {
            builder = builder.timeout(timeout);
        }
        match builder.send().await {
            Ok(inner) => Ok(Response { inner }),
            Err(error) => Err(transport_error(error, "the request could not be sent")),
        }
    }

    /// Whether this client may send a request to `url`.
    fn allows(&self, url: &Url) -> Result<()> {
        match url.scheme() {
            "https" => Ok(()),
            "http" => match &self.plain {
                Plain::Anywhere => Ok(()),
                Plain::To { host, port }
                    if url
                        .host_str()
                        .is_some_and(|asked| asked.eq_ignore_ascii_case(host))
                        && url.port_or_known_default() == Some(*port) =>
                {
                    Ok(())
                }
                Plain::To { .. } | Plain::Never => Err(Error::new(
                    ErrorKind::Refused,
                    "plain HTTP is sent only to an endpoint that is configured with http://",
                )),
            },
            _ => Err(Error::new(
                ErrorKind::Refused,
                "only http and https addresses are asked",
            )),
        }
    }
}

/// Where a client made for `endpoint` may send plain HTTP.
fn plain_to(endpoint: &str) -> Result<Plain> {
    let refused = || {
        Error::new(
            ErrorKind::Refused,
            "an endpoint is an absolute http or https URL",
        )
    };
    let url = Url::parse(endpoint).map_err(|_| refused())?;
    match (url.scheme(), url.host_str()) {
        ("https", Some(_)) => Ok(Plain::Never),
        ("http", Some(host)) => Ok(Plain::To {
            host: host.to_ascii_lowercase(),
            port: url.port_or_known_default().unwrap_or(80),
        }),
        _ => Err(refused()),
    }
}

/// Whether it sends plain HTTP, never a header or a credential.
impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("plain_http", &self.plain)
            .finish_non_exhaustive()
    }
}

/// One request to a store. Cheap to clone (the body is shared), so an
/// attempt that is run again can send it again.
///
/// The fields are public so a signer sees exactly what is sent: sign last,
/// after every header is in place.
#[derive(Clone)]
pub struct Request {
    /// The method.
    pub method: Method,
    /// The address, with its query.
    pub url: Url,
    /// The headers. `Host` and `User-Agent` are added when the request is
    /// sent, and so is `Content-Length`, from the body, for a request that
    /// has one or is a `PUT`, a `POST` or a `PATCH` (`0` for an empty one).
    pub headers: HeaderMap,
    /// The body; empty for a request that has none.
    pub body: Bytes,
    /// The longest this request may take, when not [`REQUEST_TIMEOUT`].
    pub timeout: Option<Duration>,
}

impl Request {
    /// A `method` request to `url`, with no header and no body.
    pub fn new(method: Method, url: Url) -> Self {
        Self {
            method,
            url,
            headers: HeaderMap::new(),
            body: Bytes::new(),
            timeout: None,
        }
    }

    /// This request with the header `name: value`, replacing any header of
    /// that name. A name or a value that cannot be sent (a line break in
    /// it, say) fails with [`ErrorKind::Refused`]; the error names the
    /// header and never repeats the value.
    pub fn header(mut self, name: &str, value: &str) -> Result<Self> {
        let (name, value) = header(name, value)?;
        self.headers.insert(name, value);
        Ok(self)
    }

    /// Like [`header`](Self::header), for a value that is a credential or
    /// is made from one (`authorization`, a session token, a key): it is
    /// marked so that nothing that prints headers prints it.
    pub fn secret(mut self, name: &str, value: &str) -> Result<Self> {
        let (name, mut value) = header(name, value)?;
        value.set_sensitive(true);
        self.headers.insert(name, value);
        Ok(self)
    }

    /// This request with `body`.
    #[must_use]
    pub fn with_body(mut self, body: impl Into<Bytes>) -> Self {
        self.body = body.into();
        self
    }

    /// This request, allowed `timeout` from connecting to the last byte of
    /// the answer, in place of [`REQUEST_TIMEOUT`].
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }
}

/// A header's name and value, or which header cannot be sent.
fn header(name: &str, value: &str) -> Result<(HeaderName, HeaderValue)> {
    let parsed = HeaderName::from_bytes(name.as_bytes())
        .map_err(|_| Error::new(ErrorKind::Refused, "a header's name cannot be sent"))?;
    let value = HeaderValue::from_str(value).map_err(|_| {
        Error::new(
            ErrorKind::Refused,
            format!("the value of the `{parsed}` header cannot be sent"),
        )
    })?;
    Ok((parsed, value))
}

/// The method, the address without its query, the names of the headers
/// (sorted) and the size of the body: a signature, a token or a key in the
/// query or in a header is never shown.
impl fmt::Debug for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Request")
            .field("method", &self.method)
            .field("url", &redacted(&self.url))
            .field("headers", &header_names(&self.headers))
            .field("body_bytes", &self.body.len())
            .finish()
    }
}

/// The names of `headers`, each once, sorted.
fn header_names(headers: &HeaderMap) -> Vec<&str> {
    let mut names: Vec<&str> = headers.keys().map(HeaderName::as_str).collect();
    names.sort_unstable();
    names
}

/// `url` as it may be shown in an error or a log: the scheme, the host, the
/// port and the path, without the user, the password, the query (where a
/// signed link keeps its signature and a SAS its token) and the fragment.
///
/// ```
/// use navree_storage::cloud::http::redacted;
/// use navree_storage::store::Url;
///
/// let link = "https://agra.blob.core.windows.net/docs/q1.csv?sp=r&sig=s3cret";
/// let link = Url::parse(link).unwrap();
/// assert_eq!(redacted(&link), "https://agra.blob.core.windows.net/docs/q1.csv");
/// ```
pub fn redacted(url: &Url) -> String {
    format!("{}{}", url.origin().ascii_serialization(), url.path())
}

/// The answer to a [`Request`].
pub struct Response {
    inner: reqwest::Response,
}

impl Response {
    /// The status.
    pub fn status(&self) -> StatusCode {
        self.inner.status()
    }

    /// The headers.
    pub fn headers(&self) -> &HeaderMap {
        self.inner.headers()
    }

    /// The value of the header `name`, when the answer has one that is
    /// text.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.inner.headers().get(name)?.to_str().ok()
    }

    /// The `Content-Length` the answer declares: the size of the object in
    /// the answer to a `HEAD`, which has no body.
    pub fn declared_length(&self) -> Option<u64> {
        number(self.header("content-length")?)
    }

    /// The `Content-Range` of the answer to a request for a range (`206`):
    /// the bytes its body holds and, when the provider says it, the size of
    /// the whole file. `None` when the answer has none, or one that is not
    /// a range of bytes.
    pub fn content_range(&self) -> Option<(Range<u64>, Option<u64>)> {
        content_range(self.header("content-range")?)
    }

    /// This answer when its status is a success (`2xx`). Any other status
    /// is a failure of [`ErrorKind::Status`] that holds the status and at
    /// most [`ERROR_BODY_LIMIT`] of the body. A driver that expects another
    /// status (`404` for a file that may be missing, `416` for a range past
    /// the end, `308` for an upload that is not finished) looks at
    /// [`status`](Self::status) first.
    pub async fn success(self) -> Result<Response> {
        let status = self.status();
        if status.is_success() {
            return Ok(self);
        }
        let body = self.prefix(ERROR_BODY_LIMIT).await;
        Err(Error::answered(status, body))
    }

    /// The first `limit` bytes of the body as text, as far as they can be
    /// read; `None` when there is none.
    async fn prefix(mut self, limit: usize) -> Option<String> {
        let mut body = BytesMut::new();
        while body.len() < limit {
            match self.inner.chunk().await {
                Ok(Some(chunk)) => {
                    let take = chunk.len().min(limit - body.len());
                    body.extend_from_slice(&chunk[..take]);
                }
                Ok(None) | Err(_) => break,
            }
        }
        (!body.is_empty()).then(|| String::from_utf8_lossy(&body).into_owned())
    }

    /// The whole body: the contents of a file that was asked for whole.
    pub async fn bytes(self) -> Result<Bytes> {
        self.inner
            .bytes()
            .await
            .map_err(|error| transport_error(error, "the answer could not be read to its end"))
    }

    /// The whole body of an answer that is not a file (XML, JSON), which
    /// must fit in `limit` bytes: one that declares or sends more fails
    /// with [`ErrorKind::Answer`], and no more than `limit` bytes are kept
    /// in memory.
    pub async fn bytes_up_to(mut self, limit: usize) -> Result<Bytes> {
        let too_long = || {
            Error::new(
                ErrorKind::Answer,
                format!("the answer is longer than {limit} bytes"),
            )
        };
        if self
            .declared_length()
            .is_some_and(|length| usize::try_from(length).map_or(true, |length| length > limit))
        {
            return Err(too_long());
        }
        let mut body = BytesMut::new();
        loop {
            let chunk = self.inner.chunk().await.map_err(|error| {
                transport_error(error, "the answer could not be read to its end")
            })?;
            let Some(chunk) = chunk else {
                return Ok(body.freeze());
            };
            if chunk.len() > limit - body.len() {
                return Err(too_long());
            }
            body.extend_from_slice(&chunk);
        }
    }

    /// The body in chunks, as they arrive: the contents of a file that is
    /// streamed. [`REQUEST_TIMEOUT`] counts from the start of the request,
    /// so a body still arriving when it runs out ends with a failure of
    /// [`ErrorKind::Timeout`]; a driver then asks for the rest with a
    /// `Range` (and `If-Match`, so it is the rest of the same file).
    pub fn stream(self) -> BoxStream<'static, Result<Bytes>> {
        self.inner
            .bytes_stream()
            .map(|chunk| {
                chunk.map_err(|error| {
                    transport_error(error, "the answer could not be read to its end")
                })
            })
            .boxed()
    }
}

/// A number written in digits only, with white space around it or not.
fn number(text: &str) -> Option<u64> {
    let text = text.trim();
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// `bytes <first>-<last>/<size>` (or `/*`) as the bytes from `first` up to
/// and including `last`, and the size.
fn content_range(value: &str) -> Option<(Range<u64>, Option<u64>)> {
    let (range, size) = value.trim().strip_prefix("bytes ")?.split_once('/')?;
    let (first, last) = range.split_once('-')?;
    let (first, last) = (number(first)?, number(last)?);
    if last < first {
        return None;
    }
    let size = match size.trim() {
        "*" => None,
        size => Some(number(size)?),
    };
    Some((first..last.checked_add(1)?, size))
}

/// The status and the names of the headers.
impl fmt::Debug for Response {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Response")
            .field("status", &self.status())
            .field("headers", &header_names(self.headers()))
            .finish()
    }
}

/// reqwest's failure as an [`Error`], without the URL it names (a query
/// can hold a signature).
fn transport_error(error: reqwest::Error, message: &'static str) -> Error {
    let kind = transport_kind(&error);
    Error::new(kind, message).with_source(error.without_url())
}

/// What kind of failure reqwest reports: it says whether the time ran out,
/// whether no connection was made and whether the request was one it would
/// not send; the errors under it say whether the connection dropped.
fn transport_kind(error: &reqwest::Error) -> ErrorKind {
    if error.is_timeout() {
        return ErrorKind::Timeout;
    }
    if error.is_connect() {
        return ErrorKind::Connect;
    }
    if error.is_builder() || error.is_redirect() {
        return ErrorKind::Refused;
    }
    let mut cause = std::error::Error::source(error);
    while let Some(under) = cause {
        if let Some(protocol) = under.downcast_ref::<hyper::Error>() {
            if protocol.is_timeout() {
                return ErrorKind::Timeout;
            }
            if protocol.is_incomplete_message()
                || protocol.is_closed()
                || protocol.is_canceled()
                || protocol.is_body_write_aborted()
            {
                return ErrorKind::Interrupted;
            }
        }
        if let Some(socket) = under.downcast_ref::<std::io::Error>() {
            match socket.kind() {
                std::io::ErrorKind::TimedOut => return ErrorKind::Timeout,
                std::io::ErrorKind::ConnectionAborted
                | std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::BrokenPipe
                | std::io::ErrorKind::UnexpectedEof => return ErrorKind::Interrupted,
                _ => {}
            }
        }
        cause = under.source();
    }
    ErrorKind::Other
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(text: &str) -> Url {
        Url::parse(text).unwrap()
    }

    fn refused(result: Result<()>) -> bool {
        result.is_err_and(|error| error.kind() == ErrorKind::Refused)
    }

    #[test]
    fn https_is_always_allowed_and_plain_http_only_to_the_endpoint() {
        let provider = Client::for_endpoint(None).unwrap();
        assert_eq!(provider.plain, Plain::Never);
        assert!(
            provider
                .allows(&url("https://s3.ap-south-1.amazonaws.com/a"))
                .is_ok()
        );
        assert!(refused(provider.allows(&url("http://127.0.0.1:9000/a"))));

        let secure = Client::for_endpoint(Some("https://minio.jaipur.example:9000")).unwrap();
        assert_eq!(secure.plain, Plain::Never);
        assert!(refused(
            secure.allows(&url("http://minio.jaipur.example:9000/a"))
        ));

        let minio = Client::for_endpoint(Some("http://MinIO.jaipur.example:9000/")).unwrap();
        assert!(
            minio
                .allows(&url("http://minio.jaipur.example:9000/bucket/a"))
                .is_ok()
        );
        assert!(
            minio
                .allows(&url("https://sts.ap-south-1.amazonaws.com/"))
                .is_ok()
        );
        for elsewhere in [
            "http://minio.jaipur.example/bucket/a",
            "http://minio.jaipur.example:9001/bucket/a",
            "http://other.jaipur.example:9000/bucket/a",
            "http://bucket.minio.jaipur.example:9000/a",
            "http://169.254.169.254/latest/meta-data/",
        ] {
            assert!(refused(minio.allows(&url(elsewhere))), "{elsewhere}");
        }

        // The default port of an endpoint is the port asked without one.
        let plain = Client::for_endpoint(Some("http://files.kanpur.example")).unwrap();
        assert!(
            plain
                .allows(&url("http://files.kanpur.example:80/a"))
                .is_ok()
        );
        assert!(refused(
            plain.allows(&url("http://files.kanpur.example:8080/a"))
        ));
    }

    #[test]
    fn the_metadata_client_sends_plain_http_and_no_other_scheme_is_asked() {
        let metadata = Client::for_metadata().unwrap();
        assert!(
            metadata
                .allows(&url("http://169.254.169.254/latest/api/token"))
                .is_ok()
        );
        for client in [metadata, Client::for_endpoint(None).unwrap()] {
            assert!(refused(client.allows(&url("ftp://files.kanpur.example/a"))));
            assert!(refused(client.allows(&url("file:///etc/passwd"))));
        }
    }

    #[test]
    fn an_endpoint_that_is_not_an_http_url_is_refused_without_being_repeated() {
        for endpoint in [
            "ftp://user:s3cret@files.kanpur.example",
            "s3cret-not-a-url",
            "file:///s3cret",
            "",
        ] {
            let error = Client::for_endpoint(Some(endpoint)).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::Refused, "{endpoint}");
            assert!(
                !format!("{error} {error:?}").contains("s3cret"),
                "{error:?}"
            );
        }
    }

    #[test]
    fn a_request_shows_no_query_and_no_header_value() {
        let request = Request::new(
            Method::PUT,
            url("https://user:pa55word@agra.blob.core.windows.net/docs/q1.csv?sp=w&sig=s1gnature"),
        )
        .header("x-ms-blob-type", "BlockBlob")
        .unwrap()
        .secret("authorization", "Bearer t0ken")
        .unwrap()
        .with_body("a,b\n")
        .with_timeout(Duration::from_secs(5));
        let shown = format!("{request:?}");
        assert_eq!(
            shown,
            "Request { method: PUT, url: \"https://agra.blob.core.windows.net/docs/q1.csv\", \
             headers: [\"authorization\", \"x-ms-blob-type\"], body_bytes: 4 }"
        );
        // The value is marked, so the header map does not print it either.
        assert!(request.headers["authorization"].is_sensitive());
        assert!(!format!("{:?}", request.headers).contains("t0ken"));
        assert_eq!(request.headers["x-ms-blob-type"], "BlockBlob");
        assert_eq!(request.timeout, Some(Duration::from_secs(5)));
    }

    #[test]
    fn a_header_that_cannot_be_sent_is_refused_without_its_value() {
        let request = || Request::new(Method::GET, url("https://files.kanpur.example/a"));
        let error = request()
            .secret("authorization", "Bearer t0ken\r\nx-injected: 1")
            .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Refused);
        assert_eq!(
            error.to_string(),
            "the value of the `authorization` header cannot be sent"
        );
        assert!(!format!("{error:?}").contains("t0ken"));
        let error = request().header("bad name", "1").unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Refused);
        // A header given twice is sent once, with the last value.
        let twice = request()
            .header("x-amz-date", "20261125T060000Z")
            .unwrap()
            .header("X-Amz-Date", "20261125T060001Z")
            .unwrap();
        assert_eq!(twice.headers.len(), 1);
        assert_eq!(twice.headers["x-amz-date"], "20261125T060001Z");
    }

    #[test]
    fn a_content_range_is_the_bytes_held_and_the_size_of_the_file() {
        assert_eq!(
            content_range("bytes 0-499/1234"),
            Some((0..500, Some(1234)))
        );
        assert_eq!(content_range("bytes 500-1233/*"), Some((500..1234, None)));
        assert_eq!(content_range(" bytes 5-5/6 "), Some((5..6, Some(6))));
        for not_one in [
            "bytes */1234",
            "items 0-1/2",
            "bytes 5-4/10",
            "bytes 0-499",
            "bytes -5/10",
            "bytes +1-2/3",
            "bytes 1-2/+3",
            "bytes 0-18446744073709551615/*",
            "bytes 0-99999999999999999999/*",
            "",
        ] {
            assert_eq!(content_range(not_one), None, "{not_one}");
        }
        assert_eq!(number(" 1000 "), Some(1000));
        assert_eq!(number("+1000"), None);
        assert_eq!(number("1e3"), None);
        assert_eq!(number(""), None);
    }

    #[test]
    fn a_shown_url_has_no_user_no_query_and_no_fragment() {
        assert_eq!(
            redacted(&url(
                "http://user:pa55word@127.0.0.1:10000/devstoreaccount1/q1%20final.csv?sig=abc#top"
            )),
            "http://127.0.0.1:10000/devstoreaccount1/q1%20final.csv"
        );
        assert_eq!(
            redacted(&url(
                "https://storage.googleapis.com:443/agra-media/a.png?X-Goog-Signature=ff"
            )),
            "https://storage.googleapis.com/agra-media/a.png"
        );
    }
}
